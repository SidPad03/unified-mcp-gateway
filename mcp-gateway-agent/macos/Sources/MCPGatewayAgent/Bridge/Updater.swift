import AppKit
import CryptoKit
import Foundation
import Observation

/// Self-update from GitHub Releases, with an Ed25519 signature over the archive.
///
/// **Why not `releases/latest/download/…`.** This repository interleaves
/// `gateway-v*` and `agent-v*` tags, so "the latest release" is regularly a
/// gateway release with no app in it. CI instead force-updates a permanent,
/// non-"latest" release tagged `agent-latest` whose only asset is
/// `appcast.json`. That URL is stable forever and no gateway server is involved.
///
/// **Why a signature and not just a hash.** A hash published beside the file
/// proves the download was not corrupted, and nothing else — whoever can replace
/// the archive can replace the hash. The signature is over authorship: the
/// private key lives only in a repository secret, and the public key is baked
/// into this app. Without one configured the updater will tell you an update
/// exists and refuse to install it, which is the right way round.
@MainActor
@Observable
final class Updater {
    enum State: Equatable {
        case idle
        case checking
        case upToDate
        case available(Release)
        case downloading(Double)
        case readyToRelaunch
        case failed(String)
    }

    struct Release: Codable, Equatable, Sendable {
        var version: String
        var url: String
        var signature: String
        var notes: String?
        var pubDate: Date?
    }

    private(set) var state: State = .idle
    private(set) var lastChecked: Date?

    /// Where CI publishes the manifest. Overridable in Info.plist so a fork does
    /// not have to patch source.
    private var appcastURL: URL? {
        let configured = Bundle.main.object(forInfoDictionaryKey: "MCPGAAppcastURL") as? String
        return URL(
            string: configured?.isEmpty == false
                ? configured!
                : "https://github.com/SidPad03/unified-mcp-gateway/releases/download/agent-latest/appcast.json"
        )
    }

    /// Base64 Ed25519 public key, injected at build time.
    private var publicKey: Curve25519.Signing.PublicKey? {
        guard let encoded = Bundle.main.object(forInfoDictionaryKey: "MCPGAUpdatePublicKey") as? String,
            !encoded.isEmpty,
            let raw = Data(base64Encoded: encoded)
        else { return nil }
        return try? Curve25519.Signing.PublicKey(rawRepresentation: raw)
    }

    var currentVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0.0.0"
    }

    var canInstall: Bool { publicKey != nil }

    // ── Checking ────────────────────────────────────────────────────────

    private static let checkInterval: TimeInterval = 6 * 60 * 60

    /// Launch, then every six hours, for as long as the app runs.
    ///
    /// This is driven from the model rather than a `.task` on the window,
    /// because closing the window drops the app to the menu bar without quitting
    /// it. A machine left running for a week would otherwise never check again.
    func checkPeriodically() async {
        while !Task.isCancelled {
            await checkInBackground()
            try? await Task.sleep(for: .seconds(Self.checkInterval))
        }
    }

    /// Failures are silent: a check that cannot reach GitHub is not something to
    /// interrupt anyone about.
    ///
    /// Re-checking from `.upToDate` and `.failed` is the whole point of the
    /// periodic call. Guarding on `.idle` alone meant the second check and every
    /// one after it returned immediately, so in practice the app only ever
    /// checked once, at launch.
    func checkInBackground() async {
        switch state {
        case .idle, .upToDate, .failed:
            await check(announceFailure: false)
        case .checking, .downloading, .available, .readyToRelaunch:
            // A check is already running, or there is a release in hand and
            // re-checking would only risk clearing it.
            break
        }
    }

    func check(announceFailure: Bool = true) async {
        guard let appcastURL else { return }
        state = .checking
        do {
            var request = URLRequest(url: appcastURL)
            request.cachePolicy = .reloadIgnoringLocalCacheData
            request.timeoutInterval = 20
            let (data, _) = try await URLSession(configuration: .ephemeral).data(for: request)

            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            decoder.dateDecodingStrategy = .custom { decoder in
                let text = try decoder.singleValueContainer().decode(String.self)
                return JSON.parseTimestamp(text) ?? Date()
            }
            let release = try decoder.decode(Release.self, from: data)

            lastChecked = Date()
            state =
                Self.isNewer(release.version, than: currentVersion)
                ? .available(release) : .upToDate
        } catch {
            state = announceFailure ? .failed(error.localizedDescription) : .idle
        }
    }

    // ── Installing ──────────────────────────────────────────────────────

    func install(_ release: Release) async {
        guard let publicKey else {
            state = .failed(
                "This build has no update signing key, so it cannot install updates. "
                    + "Download the new version from GitHub instead."
            )
            return
        }
        guard let url = URL(string: release.url),
            let signature = Data(base64Encoded: release.signature)
        else {
            state = .failed("The update manifest is malformed.")
            return
        }

        state = .downloading(0)
        do {
            let (archive, _) = try await URLSession(configuration: .ephemeral).data(from: url)

            guard publicKey.isValidSignature(signature, for: archive) else {
                // Not a download error. Either the archive was tampered with or
                // it was signed by a different key, and both mean stop.
                state = .failed("The update's signature did not verify. It has not been installed.")
                return
            }

            state = .downloading(0.9)
            try await Self.swapBundle(archive: archive)
            state = .readyToRelaunch
            // The swap script is already waiting on this PID. Quitting here is
            // what makes updating one click rather than two, and matches what
            // every other Mac app does once you have asked it to update.
            relaunch()
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    /// Unpack next to the installed app, then hand the swap to a detached
    /// script.
    ///
    /// A process cannot reliably replace its own bundle while running, so the
    /// script waits for this PID to exit, `ditto`s the new bundle over the old
    /// one, and opens it. `ditto` rather than `mv` because it preserves the
    /// signature and extended attributes that make the bundle launchable.
    private static func swapBundle(archive: Data) async throws {
        let installed = Bundle.main.bundleURL
        let staging = FileManager.default.temporaryDirectory
            .appendingPathComponent("mcp-gateway-agent-update-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: staging, withIntermediateDirectories: true)

        let archiveURL = staging.appendingPathComponent("update.tar.gz")
        try archive.write(to: archiveURL)

        try run("/usr/bin/tar", ["-xzf", archiveURL.path, "-C", staging.path])

        let unpacked = try FileManager.default
            .contentsOfDirectory(at: staging, includingPropertiesForKeys: nil)
            .first { $0.pathExtension == "app" }
        guard let unpacked else {
            throw Updater.InstallError.noAppInArchive
        }

        let script = """
            #!/bin/sh
            # Wait for the running app to exit, then swap the bundle and relaunch.
            while kill -0 \(ProcessInfo.processInfo.processIdentifier) 2>/dev/null; do sleep 0.2; done
            /usr/bin/ditto "\(unpacked.path)" "\(installed.path)" || exit 1
            /usr/bin/xattr -dr com.apple.quarantine "\(installed.path)" 2>/dev/null
            /usr/bin/open "\(installed.path)"
            /bin/rm -rf "\(staging.path)"
            """
        let scriptURL = staging.appendingPathComponent("install.sh")
        try script.write(to: scriptURL, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: scriptURL.path)

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/sh")
        process.arguments = [scriptURL.path]
        try process.run()
    }

    enum InstallError: LocalizedError {
        case noAppInArchive

        var errorDescription: String? {
            switch self {
            case .noAppInArchive: "The update archive did not contain an application."
            }
        }
    }

    private static func run(_ path: String, _ arguments: [String]) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = arguments
        try process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            throw InstallError.noAppInArchive
        }
    }

    /// Quit, so the staged script can replace the bundle and reopen it.
    ///
    /// The quit confirmation is suppressed for this one path. It warns that
    /// quitting stops this Mac's MCP servers, which is true of a normal ⌘Q and
    /// misleading here, where the app is coming straight back.
    func relaunch() {
        AppDelegate.shared?.isRelaunchingForUpdate = true
        NSApplication.shared.terminate(nil)
    }

    // ── Versions ────────────────────────────────────────────────────────

    /// Numeric compare, component by component. `1.0.10` is newer than `1.0.9`,
    /// which a string compare gets backwards.
    static func isNewer(_ candidate: String, than current: String) -> Bool {
        let a = parse(candidate)
        let b = parse(current)
        for index in 0..<max(a.count, b.count) {
            let left = index < a.count ? a[index] : 0
            let right = index < b.count ? b[index] : 0
            if left != right { return left > right }
        }
        return false
    }

    private static func parse(_ version: String) -> [Int] {
        version
            .split(separator: "-").first
            .map(String.init)?
            .split(separator: ".")
            .map { Int($0) ?? 0 } ?? []
    }
}
