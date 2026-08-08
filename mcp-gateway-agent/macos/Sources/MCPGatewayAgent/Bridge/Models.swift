import Foundation

// Swift mirrors of the types in `mcp-gateway-agent-core`.
//
// Rust serializes snake_case; the decoder is configured with
// `.convertFromSnakeCase`, so the property names here are the camelCase of the
// Rust field names and nothing needs a CodingKeys block. Fields the app never
// reads are deliberately not mirrored at all — the decoder skips unknown JSON
// for free, and an unread property is a lie about what the app depends on.

// ── Enumerations ────────────────────────────────────────────────────────

enum ConnState: String, Codable, Sendable {
    case idle, connecting, connected, reconnecting, error

    var label: String {
        switch self {
        case .idle: "Not configured"
        case .connecting: "Connecting"
        case .connected: "Connected"
        case .reconnecting: "Reconnecting"
        case .error: "Disconnected"
        }
    }
}

enum BackendStatus: String, Codable, Sendable {
    case disabled, starting, ready, failed, crashed, stopped

    var label: String {
        switch self {
        case .disabled: "Disabled"
        case .starting: "Starting"
        case .ready: "Ready"
        case .failed: "Failed"
        case .crashed: "Crashed"
        case .stopped: "Stopped"
        }
    }
}

enum LogLevel: String, Codable, Sendable, CaseIterable, Identifiable {
    case trace, debug, info, warn, error

    var id: String { rawValue }

    var label: String {
        switch self {
        case .trace: "Trace"
        case .debug: "Debug"
        case .info: "Info"
        case .warn: "Warn"
        case .error: "Error"
        }
    }

    /// Ordering for the "level and above" filter.
    var severity: Int {
        switch self {
        case .trace: 0
        case .debug: 1
        case .info: 2
        case .warn: 3
        case .error: 4
        }
    }
}

enum CallStatus: String, Codable, Sendable {
    case running, ok, error
}

// ── Snapshot ────────────────────────────────────────────────────────────

struct ConnectionStatus: Codable, Sendable {
    var state: ConnState
    var gatewayUrl: String
    var agentId: String
    var backendId: String?
    var attempt: Int
    var lastError: String?
    var retryInMs: Int?

    /// `lastError`, rewritten into something you can act on.
    ///
    /// The core hands the transport error through verbatim, which is right for
    /// the log and wrong for the one line on Overview. "IO error: invalid peer
    /// certificate: UnknownIssuer" names the library that raised it rather than
    /// the thing that is wrong, and the fix is two clicks away in Settings. The
    /// raw text stays in the log for anyone debugging; this is what the hero
    /// shows. Anything unrecognised falls through unchanged, so a new failure
    /// mode is never swallowed.
    var readableError: String? {
        guard let lastError else { return nil }
        let text = lastError.lowercased()

        if text.contains("unknownissuer") || text.contains("invalid peer certificate")
            || text.contains("certificate verify failed")
        {
            return """
                This Mac does not trust the gateway's TLS certificate. If it is self-signed, \
                turn on Skip TLS certificate verification in Settings → Gateway, or add the \
                issuing CA to your login keychain.
                """
        }
        if text.contains("certificate") && text.contains("expired") {
            return "The gateway's TLS certificate has expired. Renew it on the server."
        }
        if text.contains("connection refused") {
            return "Nothing is listening at \(gatewayUrl). Check the address and that the gateway is running."
        }
        if text.contains("dns") || text.contains("nodename nor servname")
            || text.contains("failed to lookup")
        {
            return "That host could not be resolved. Check the gateway address in Settings → Gateway."
        }
        if text.contains("401") || text.contains("unauthorized") {
            return "The gateway rejected this Mac's API key. Sign out and sign in again."
        }
        return lastError
    }
}

struct Stats: Codable, Sendable {
    var toolsRegistered: Int
    var backendsReady: Int
    var backendsTotal: Int
    var callsTotal: Int
    var callsErrors: Int
}

struct AgentConfigView: Codable, Sendable {
    var agentId: String
    var gatewayUrl: String
    var dashboardUrl: String?
    var apiBaseUrl: String?
    var tlsSkipVerify: Bool
    var hasApiKey: Bool
    var configured: Bool
    var configPath: String
}

struct ToolSummary: Codable, Sendable, Hashable, Identifiable {
    var name: String
    var description: String

    var id: String { name }

    /// The tool name without its `backend__` namespace.
    var bareName: String {
        guard let range = name.range(of: "__") else { return name }
        return String(name[range.upperBound...])
    }
}

struct BackendView: Codable, Sendable, Identifiable, Equatable {
    var name: String
    var transport: String
    var enabled: Bool
    var status: BackendStatus
    var error: String?
    var pid: Int?
    var startedAt: Date?
    var uptimeSecs: Int?
    var restarts: Int
    var toolCount: Int
    var command: String?
    var args: [String]
    var url: String?
    var envKeys: [String]
    var headerKeys: [String]
    var tools: [ToolSummary]

    var id: String { name }

    var isStdio: Bool { transport == "stdio" }

    /// What the row shows under the name.
    var subtitle: String {
        if isStdio {
            let arguments = args.isEmpty ? "" : " " + args.joined(separator: " ")
            return (command ?? "—") + arguments
        }
        return url ?? "—"
    }
}

struct Snapshot: Codable, Sendable {
    var connection: ConnectionStatus
    var backends: [BackendView]
    var config: AgentConfigView
    var stats: Stats
    var version: String
    var uptimeSecs: Int
}

// ── Streamed records ────────────────────────────────────────────────────

struct LogLine: Codable, Sendable, Identifiable, Equatable {
    var seq: Int
    var ts: Date
    var level: LogLevel
    var source: String
    var message: String

    var id: Int { seq }
}

struct ToolCall: Codable, Sendable, Identifiable, Equatable {
    var seq: Int
    var requestId: String
    var tool: String
    var backend: String?
    var startedAt: Date
    var durationMs: Int?
    var status: CallStatus
    var error: String?

    var id: String { requestId }

    var bareTool: String {
        guard let range = tool.range(of: "__") else { return tool }
        return String(tool[range.upperBound...])
    }
}

/// One batch from the emitter task. Everything is optional — a tick that only
/// carries log lines omits the rest.
struct Tick: Codable, Sendable {
    var snapshot: Snapshot?
    var logs: [LogLine]?
    var calls: [ToolCall]?
}

// ── Command payloads ────────────────────────────────────────────────────

/// A local MCP server, as the config file stores it.
///
/// This is the one type that travels *into* Rust, so it is `Encodable` as well.
struct BackendConfig: Codable, Sendable, Equatable {
    var name: String = ""
    var transport: String = "stdio"
    var command: String?
    var args: [String] = []
    var env: [String: String] = [:]
    var url: String?
    var headers: [String: String] = [:]
    var enabled: Bool = true

    var isStdio: Bool { transport == "stdio" }

    init(
        name: String = "",
        transport: String = "stdio",
        command: String? = nil,
        args: [String] = [],
        env: [String: String] = [:],
        url: String? = nil,
        headers: [String: String] = [:],
        enabled: Bool = true
    ) {
        self.name = name
        self.transport = transport
        self.command = command
        self.args = args
        self.env = env
        self.url = url
        self.headers = headers
        self.enabled = enabled
    }

    /// Prefill the editor from a running backend.
    ///
    /// Environment *values* are deliberately not available here — the agent
    /// never hands them back out — so editing a backend that has secrets in its
    /// environment shows the keys with empty values and leaves them untouched
    /// unless the user types a new one.
    init(existing: BackendView) {
        self.init(
            name: existing.name,
            transport: existing.transport,
            command: existing.command,
            args: existing.args,
            env: Dictionary(uniqueKeysWithValues: existing.envKeys.map { ($0, "") }),
            url: existing.url,
            headers: Dictionary(uniqueKeysWithValues: existing.headerKeys.map { ($0, "") }),
            enabled: existing.enabled
        )
    }
}

struct BackendTestResult: Codable, Sendable, Equatable {
    var toolCount: Int
    var tools: [String]
    var tookMs: Int
}

struct LogsSnapshot: Codable, Sendable {
    var lines: [LogLine]
    var dropped: Int
}

struct CallsSnapshot: Codable, Sendable {
    var calls: [ToolCall]
}

struct LegacyKey: Codable, Sendable {
    var key: String?
}
