import AppKit
import Foundation
import Observation

/// The app's model. One instance, owned by the `App`, shared through the
/// environment.
///
/// Everything the UI renders comes from here, and everything here comes from the
/// Rust core — the model holds no state the core does not also hold, apart from
/// the signed-in account and whatever a sheet is halfway through editing. That
/// is deliberate: two sources of truth for "is this backend running" is how you
/// get a UI that disagrees with itself.
@MainActor
@Observable
final class AgentModel {
    // ── Published state ─────────────────────────────────────────────────

    private(set) var snapshot: Snapshot?
    private(set) var logLines: [LogLine] = []
    private(set) var logLinesDropped = 0
    private(set) var toolCalls: [ToolCall] = []
    private(set) var account: SignedInAccount?

    /// Bumped whenever `logLines` / `toolCalls` change.
    ///
    /// Views memoize their filtered, sorted, bucketed derivations against these
    /// rather than recomputing inside `body`. `body` runs on every observed
    /// change — with a 10 Hz event tick, filtering five thousand log lines in
    /// there is fifty thousand predicate evaluations a second to redraw a
    /// screenful of rows.
    private(set) var logRevision = 0
    private(set) var callRevision = 0

    /// Set when the core could not start at all — the one failure the UI cannot
    /// work around.
    private(set) var launchFailure: String?
    var signingIn = false
    /// Surfaced as a transient banner.
    var lastError: String?

    let updater = Updater()

    // ── Internals ───────────────────────────────────────────────────────

    private let bridge = AgentBridge()
    private let auth = AgentAuth()
    /// request_id → index into `toolCalls`, so a completion is an in-place
    /// update rather than a scan.
    private var callIndex: [String: Int] = [:]
    private var apiKey: String?
    /// The six-hourly update poll. Unstructured on purpose: it has to outlive
    /// the window, which the user can close without quitting the app.
    private var updateChecks: Task<Void, Never>?

    /// Matches `MAX_LOG_LINES` in the core. The core is the source of truth;
    /// this only stops the array growing past what it would ever send.
    private let maxLogLines = 5_000
    private let maxCalls = 1_000

    // ── Derived ─────────────────────────────────────────────────────────

    var connection: ConnectionStatus? { snapshot?.connection }
    var backends: [BackendView] { snapshot?.backends ?? [] }
    var stats: Stats? { snapshot?.stats }
    var config: AgentConfigView? { snapshot?.config }
    var isSignedIn: Bool { account != nil && (snapshot?.config.hasApiKey ?? false) }
    var coreVersion: String { snapshot?.version ?? bridge.coreVersion }

    /// The gateway's id for this machine — the `backend` filter for Audit and
    /// Usage.
    var backendId: String? { snapshot?.connection.backendId }

    var apiBaseURL: URL? {
        guard let base = snapshot?.config.apiBaseUrl else { return nil }
        return URL(string: base)
    }

    // ── Lifecycle ───────────────────────────────────────────────────────

    func launch() async {
        do {
            try bridge.start(configPath: nil) { [weak self] tick in
                Task { @MainActor in self?.apply(tick) }
            }
        } catch {
            launchFailure = error.localizedDescription
            return
        }

        account = SignedInAccount.load()

        // One read, not two. On an ad-hoc signed build every read of this item
        // can raise "MCP Gateway Agent wants to use your confidential
        // information", and reading once here and again inside the migration
        // check asked twice on every single launch.
        var key = Keychain.read()
        if key == nil { key = await migrateLegacyKeyIfNeeded() }

        if let key, !key.isEmpty {
            apiKey = key
            try? await bridge.send(.setApiKey(key))
        }

        await refresh()
        updateChecks = Task { [updater] in await updater.checkPeriodically() }
    }

    /// Move a plaintext key out of a `config.toml` written by the old terminal
    /// agent, then rewrite the file without it.
    ///
    /// Only ever called when the Keychain has nothing — an existing item always
    /// wins, so this cannot clobber a key the user signed in for. The caller
    /// does that check, so this does not re-read the Keychain itself.
    ///
    /// Returns the migrated key, or nil if there was nothing to migrate.
    private func migrateLegacyKeyIfNeeded() async -> String? {
        guard let legacy = try? await bridge.send(.takeLegacyApiKey, as: LegacyKey.self),
            let key = legacy.key, !key.isEmpty
        else { return nil }

        do {
            try Keychain.write(key)
            return key
        } catch {
            lastError = "Could not move the existing API key into the Keychain: \(error.localizedDescription)"
            return nil
        }
    }

    /// The `mcp-gateway-agent://auth/callback` redirect, from the app's URL
    /// handler. Sign-in waits on this now that the page opens in the user's own
    /// browser rather than in a window this app presents.
    func handleCallbackURL(_ url: URL) {
        auth.resume(with: url)
    }

    func shutdown() {
        updateChecks?.cancel()
        bridge.shutdown()
    }

    func refresh() async {
        await refreshSnapshot()
        if let logs = try? await bridge.send(.logsSnapshot, as: LogsSnapshot.self) {
            logLines = Array(logs.lines.suffix(maxLogLines))
            logLinesDropped = logs.dropped
            logRevision &+= 1
        }
        if let calls = try? await bridge.send(.callsSnapshot, as: CallsSnapshot.self) {
            toolCalls = calls.calls
            reindexCalls()
            callRevision &+= 1
        }
    }

    func refreshSnapshot() async {
        snapshot = try? await bridge.send(.snapshot, as: Snapshot.self)
    }

    // ── Streaming ───────────────────────────────────────────────────────

    private func apply(_ tick: Tick) {
        if let snapshot = tick.snapshot {
            self.snapshot = snapshot
        }
        if let lines = tick.logs, !lines.isEmpty {
            logLines.append(contentsOf: lines)
            if logLines.count > maxLogLines {
                logLines.removeFirst(logLines.count - maxLogLines)
            }
            logRevision &+= 1
        }
        if let calls = tick.calls, !calls.isEmpty {
            merge(calls)
            callRevision &+= 1
        }
    }

    /// A completed call arrives again with its duration filled in, so records
    /// are merged by `request_id` rather than appended.
    private func merge(_ incoming: [ToolCall]) {
        for call in incoming {
            if let index = callIndex[call.requestId] {
                toolCalls[index] = call
            } else {
                callIndex[call.requestId] = toolCalls.count
                toolCalls.append(call)
            }
        }
        if toolCalls.count > maxCalls {
            toolCalls.removeFirst(toolCalls.count - maxCalls)
            reindexCalls()
        }
    }

    private func reindexCalls() {
        callIndex = Dictionary(
            uniqueKeysWithValues: toolCalls.enumerated().map { ($1.requestId, $0) }
        )
    }

    // ── Sign-in ─────────────────────────────────────────────────────────

    func signIn(gateway: String, agentId: String, allowInsecureTLS: Bool) async {
        signingIn = true
        defer { signingIn = false }

        let agentId = agentId.trimmingCharacters(in: .whitespacesAndNewlines)
        let name = agentId.isEmpty ? Self.defaultAgentID() : agentId

        do {
            let result = try await auth.signIn(
                gateway: gateway,
                agentId: name,
                allowInsecureTLS: allowInsecureTLS
            )
            try Keychain.write(result.apiKey)
            apiKey = result.apiKey
            result.account.save()
            account = result.account

            try await bridge.send(
                .applySettings(
                    agentId: result.account.agentId,
                    gatewayUrl: result.account.gatewayUrl,
                    dashboardUrl: nil,
                    tlsSkipVerify: allowInsecureTLS
                )
            )
            try await bridge.send(.setApiKey(result.apiKey))
            await refreshSnapshot()

            // Come back to the front. The browser took focus to run the sign-in
            // and does not hand it back, and the activation policy is synced
            // from window visibility — so with no window key, the app can be
            // left in `.accessory` with its Dock icon gone, which is
            // indistinguishable from having quit. Ask for both explicitly
            // rather than hoping the window notifications land in a useful
            // order.
            NSApp.setActivationPolicy(.regular)
            NSApp.activate(ignoringOtherApps: true)
        } catch AgentAuth.Failure.cancelled {
            // Closing the sign-in window is not an error worth a banner.
        } catch {
            lastError = error.localizedDescription
        }
    }

    func signOut() async {
        Keychain.delete()
        SignedInAccount.clear()
        account = nil
        apiKey = nil
        try? await bridge.send(.setApiKey(""))
        await refreshSnapshot()
    }

    /// A machine name that is legible in the dashboard and legal as a tool
    /// namespace: `Sid's MacBook Pro` → `sids-macbook-pro`.
    static func defaultAgentID() -> String {
        let raw = Host.current().localizedName ?? ProcessInfo.processInfo.hostName
        let slug = raw.lowercased()
            .replacingOccurrences(of: "’", with: "")
            .replacingOccurrences(of: "'", with: "")
            .map { $0.isLetter || $0.isNumber ? String($0) : "-" }
            .joined()
        let collapsed = slug.split(separator: "-", omittingEmptySubsequences: true).joined(separator: "-")
        return collapsed.isEmpty ? "mac" : String(collapsed.prefix(64))
    }

    // ── Actions ─────────────────────────────────────────────────────────

    func reconnect() async { await run(.reconnect) }
    func reregister() async { await run(.reregister) }
    func clearLogs() async {
        await run(.clearLogs)
        logLines.removeAll()
        logLinesDropped = 0
        logRevision &+= 1
    }

    func addBackend(_ backend: BackendConfig) async throws {
        try await bridge.send(.addBackend(backend))
        await refreshSnapshot()
    }

    func updateBackend(name: String, to backend: BackendConfig) async throws {
        try await bridge.send(.updateBackend(name: name, backend: backend))
        await refreshSnapshot()
    }

    func removeBackend(_ name: String) async {
        await run(.removeBackend(name))
    }

    func restartBackend(_ name: String) async {
        await run(.restartBackend(name))
    }

    func setBackendEnabled(_ name: String, enabled: Bool) async {
        await run(.setBackendEnabled(name, enabled: enabled))
    }

    func testBackend(_ backend: BackendConfig) async throws -> BackendTestResult {
        try await bridge.send(.testBackend(backend), as: BackendTestResult.self)
    }

    func applySettings(
        agentId: String,
        gatewayUrl: String,
        dashboardUrl: String?,
        tlsSkipVerify: Bool
    ) async {
        await run(
            .applySettings(
                agentId: agentId,
                gatewayUrl: gatewayUrl,
                dashboardUrl: dashboardUrl?.isEmpty == true ? nil : dashboardUrl,
                tlsSkipVerify: tlsSkipVerify
            )
        )
    }

    /// Open the gateway's web dashboard.
    ///
    /// `apiBaseUrl` ends in `/api/v1`; the dashboard is served from the origin.
    func openDashboard() {
        guard let api = apiBaseURL,
            var components = URLComponents(url: api, resolvingAgainstBaseURL: false)
        else { return }
        components.path = ""
        components.query = nil
        guard let url = components.url else { return }
        NSWorkspace.shared.open(url)
    }

    private func run(_ request: CommandRequest) async {
        do {
            try await bridge.send(request)
            await refreshSnapshot()
        } catch {
            lastError = error.localizedDescription
        }
    }

    // ── Gateway REST ────────────────────────────────────────────────────

    /// Audit and Usage, scoped to this machine. `nil` until the agent has
    /// registered, which is what having a `backend_id` at all proves.
    ///
    /// The id proves registration; it is the *name* that does the filtering, and
    /// sending the wrong one of the two is why both pages read zero.
    /// `backend_id` is a UUID the gateway mints for the `backends` row, while
    /// every `backend=` filter on the server compares against
    /// `audit_events.backend_name`, which holds the agent's own name — the same
    /// string that namespaces its tools as `sids-macbook-pro__blender__…`. A
    /// UUID never equals a name, so the filter matched nothing and Audit and
    /// Usage both reported "0 of 0" on a machine that was routing calls.
    func gatewayAPI() -> GatewayAPI? {
        guard let base = apiBaseURL, let key = apiKey, backendId != nil,
              let backendName = snapshot?.connection.agentId
        else { return nil }
        return GatewayAPI(
            baseURL: base,
            apiKey: key,
            backendName: backendName,
            allowInsecureTLS: snapshot?.config.tlsSkipVerify ?? false
        )
    }
}
