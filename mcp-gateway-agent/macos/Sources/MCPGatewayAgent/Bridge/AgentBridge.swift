import Foundation
import MCPGatewayAgentFFI

/// The Swift side of the C ABI.
///
/// Two directions, and they behave differently on purpose:
///
/// * **Commands** block. `mcpga_command` runs the request to completion on the
///   calling thread, and a `test_backend` can spend thirty seconds waiting on a
///   process that will never answer. Every call therefore goes out on a detached
///   task and comes back through `async`.
/// * **Events** arrive on Rust's emitter thread, one at a time, about ten times
///   a second. They are decoded there — off the main thread, which is the whole
///   point of doing it here rather than in the view — and only the decoded value
///   is handed to the main actor.
final class AgentBridge: @unchecked Sendable {
    enum Failure: LocalizedError {
        case startFailed(Int32)
        case command(String)
        /// The detail stays in the associated value for the debugger; it is
        /// deliberately not shown. This is a raw dump of whatever came over the
        /// FFI, and a generic "echo the payload into the UI" path is one
        /// refactor away from echoing a response that carries a secret.
        case malformedResponse(String)

        var errorDescription: String? {
            switch self {
            case let .startFailed(code):
                "The agent core could not start (code \(code))."
            case let .command(message):
                message
            case .malformedResponse:
                "The agent core sent a response this app could not read."
            }
        }
    }

    /// Called on Rust's emitter thread with each decoded batch.
    private var onTick: (@Sendable (Tick) -> Void)?
    private var started = false

    /// Owned by the emitter thread alone. Events arrive one at a time on that
    /// one thread, ten times a second — building a decoder (and its custom
    /// date-parsing closure) per tick was pure allocation churn. Commands keep
    /// making their own; they genuinely run concurrently.
    private let tickDecoder = JSON.decoder()

    // ── Lifecycle ───────────────────────────────────────────────────────

    func start(configPath: String?, onTick: @escaping @Sendable (Tick) -> Void) throws {
        guard !started else { return }
        self.onTick = onTick

        // Retained deliberately and never released: the bridge lives for the
        // lifetime of the process, and Rust holds this pointer until
        // `mcpga_shutdown` returns. Balancing it would mean tracking a release
        // that can only ever happen as the process exits.
        let ctx = Unmanaged.passRetained(self).toOpaque()

        let code: Int32 = if let configPath {
            configPath.withCString { mcpga_start($0, agentEventTrampoline, ctx) }
        } else {
            mcpga_start(nil, agentEventTrampoline, ctx)
        }

        guard code == 0 else { throw Failure.startFailed(code) }
        started = true
    }

    func shutdown() {
        guard started else { return }
        mcpga_shutdown()
    }

    var coreVersion: String {
        guard let pointer = mcpga_version() else { return "unknown" }
        defer { mcpga_string_free(pointer) }
        return String(cString: pointer)
    }

    // ── Events ──────────────────────────────────────────────────────────

    fileprivate func handleEvent(_ json: String) {
        guard let onTick else { return }
        do {
            let tick = try tickDecoder.decode(Tick.self, from: Data(json.utf8))
            onTick(tick)
        } catch {
            // A tick we cannot read is a bug on our side, not the user's
            // problem; drop it rather than tearing anything down.
            NSLog("mcp-gateway-agent: could not decode an event: \(error)")
        }
    }

    // ── Commands ────────────────────────────────────────────────────────

    @discardableResult
    func send(_ request: CommandRequest) async throws -> Bool {
        let response = try await raw(request)
        let status = try decodeStatus(response)
        guard status.ok else { throw Failure.command(status.error ?? "Unknown error") }
        return true
    }

    func send<T: Decodable>(_ request: CommandRequest, as type: T.Type) async throws -> T {
        let response = try await raw(request)
        let status = try decodeStatus(response)
        guard status.ok else { throw Failure.command(status.error ?? "Unknown error") }
        do {
            return try JSON.decoder().decode(Envelope<T>.self, from: response).data
        } catch {
            throw Failure.malformedResponse("\(error)")
        }
    }

    private func raw(_ request: CommandRequest) async throws -> Data {
        let encoded = try JSON.encoder().encode(request)
        let json = String(decoding: encoded, as: UTF8.self)
        return await Task.detached(priority: .userInitiated) {
            json.withCString { pointer -> Data in
                guard let response = mcpga_command(pointer) else {
                    return Data(#"{"ok":false,"error":"No response from the agent core"}"#.utf8)
                }
                defer { mcpga_string_free(response) }
                return Data(String(cString: response).utf8)
            }
        }.value
    }

    private func decodeStatus(_ data: Data) throws -> Status {
        do {
            return try JSON.decoder().decode(Status.self, from: data)
        } catch {
            throw Failure.malformedResponse(String(decoding: data.prefix(200), as: UTF8.self))
        }
    }

    private struct Status: Decodable {
        let ok: Bool
        let error: String?
    }

    private struct Envelope<T: Decodable>: Decodable {
        let data: T
    }
}

/// The C entry point. A free function so it converts to `@convention(c)`.
///
/// Rust guarantees this is never called concurrently with itself, so there is
/// no locking here; it is, however, always on a background thread.
private func agentEventTrampoline(
    _ ctx: UnsafeMutableRawPointer?,
    _ json: UnsafePointer<CChar>?
) {
    guard let ctx, let json else { return }
    let bridge = Unmanaged<AgentBridge>.fromOpaque(ctx).takeUnretainedValue()
    bridge.handleEvent(String(cString: json))
}

// ── Requests ────────────────────────────────────────────────────────────

/// One command, flattened.
///
/// Rust's `Command` is an internally tagged enum, so every request is
/// `{"cmd": …}` plus that variant's fields. Synthesized `Encodable` omits nil
/// optionals, which is exactly the shape serde wants.
struct CommandRequest: Encodable, Sendable {
    let cmd: String
    var key: String?
    var name: String?
    var enabled: Bool?
    var backend: BackendConfig?
    var agentId: String?
    var gatewayUrl: String?
    var dashboardUrl: String?
    var tlsSkipVerify: Bool?

    static let snapshot = CommandRequest(cmd: "snapshot")
    static let logsSnapshot = CommandRequest(cmd: "logs_snapshot")
    static let callsSnapshot = CommandRequest(cmd: "calls_snapshot")
    static let clearLogs = CommandRequest(cmd: "clear_logs")
    static let reconnect = CommandRequest(cmd: "reconnect")
    static let reregister = CommandRequest(cmd: "reregister")
    static let takeLegacyApiKey = CommandRequest(cmd: "take_legacy_api_key")

    static func setApiKey(_ key: String) -> Self {
        CommandRequest(cmd: "set_api_key", key: key)
    }

    static func applySettings(
        agentId: String,
        gatewayUrl: String,
        dashboardUrl: String?,
        tlsSkipVerify: Bool
    ) -> Self {
        CommandRequest(
            cmd: "apply_settings",
            agentId: agentId,
            gatewayUrl: gatewayUrl,
            dashboardUrl: dashboardUrl,
            tlsSkipVerify: tlsSkipVerify
        )
    }

    static func addBackend(_ backend: BackendConfig) -> Self {
        CommandRequest(cmd: "add_backend", backend: backend)
    }

    static func updateBackend(name: String, backend: BackendConfig) -> Self {
        CommandRequest(cmd: "update_backend", name: name, backend: backend)
    }

    static func removeBackend(_ name: String) -> Self {
        CommandRequest(cmd: "remove_backend", name: name)
    }

    static func restartBackend(_ name: String) -> Self {
        CommandRequest(cmd: "restart_backend", name: name)
    }

    static func setBackendEnabled(_ name: String, enabled: Bool) -> Self {
        CommandRequest(cmd: "set_backend_enabled", name: name, enabled: enabled)
    }

    static func testBackend(_ backend: BackendConfig) -> Self {
        CommandRequest(cmd: "test_backend", backend: backend)
    }
}

// ── JSON ────────────────────────────────────────────────────────────────

/// Coders configured to match the Rust side.
///
/// A fresh instance per call rather than a shared one: `JSONDecoder` is not
/// `Sendable`, decoding happens from several tasks at once, and allocating one
/// is far cheaper than the FFI call it accompanies.
enum JSON {
    static func decoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .custom { decoder in
            let text = try decoder.singleValueContainer().decode(String.self)
            guard let date = parseTimestamp(text) else {
                throw DecodingError.dataCorruptedError(
                    in: try decoder.singleValueContainer(),
                    debugDescription: "'\(text)' is not an RFC 3339 timestamp"
                )
            }
            return date
        }
        return decoder
    }

    static func encoder() -> JSONEncoder {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        return encoder
    }

    /// Parse the several shapes of RFC 3339 this app actually receives.
    ///
    /// The agent core emits millisecond precision with a `Z`. The gateway's REST
    /// API hands back whatever Postgres gave chrono, which is microseconds with
    /// a `+00:00` offset. Both have to work, and neither is worth a hand-rolled
    /// parser.
    static func parseTimestamp(_ text: String) -> Date? {
        if let date = fractionalFormatter.date(from: text) { return date }
        if let date = plainFormatter.date(from: text) { return date }
        return nil
    }

    // Configured once and only ever asked to parse. `ISO8601DateFormatter` is
    // safe to share once it is no longer being mutated.
    private nonisolated(unsafe) static let fractionalFormatter: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter
    }()

    private nonisolated(unsafe) static let plainFormatter: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        return formatter
    }()
}
