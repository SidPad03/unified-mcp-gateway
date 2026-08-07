import Foundation

/// The gateway's REST API, scoped to this machine.
///
/// The app authenticates with the agent's own `mcpgw_` key, so it sees exactly
/// what that account would see in the dashboard, narrowed by `backend=<agent
/// id>`. If the key belongs to an owner that is every user's calls to this
/// machine; if not, only that user's. The pages say which — decision D4.
struct GatewayAPI: Sendable {
    let baseURL: URL
    let apiKey: String
    /// The gateway's id for this machine, used as the `backend` filter.
    let backendId: String
    let allowInsecureTLS: Bool

    enum Failure: LocalizedError {
        case http(Int, String?)
        case transport(String)

        var errorDescription: String? {
            switch self {
            case .http(401, _):
                "The gateway rejected this machine's API key. Sign in again."
            case let .http(code, detail):
                detail ?? "The gateway returned HTTP \(code)."
            case let .transport(message):
                message
            }
        }
    }

    // ── Audit ───────────────────────────────────────────────────────────

    /// Newest events first.
    ///
    /// `since` makes the poll incremental: the Audit page passes the newest
    /// timestamp it already has, so a quiet gateway costs an empty array rather
    /// than fifty rows it already drew.
    func auditEvents(since: Date? = nil, limit: Int = 100) async throws -> AuditPage {
        var items = [
            URLQueryItem(name: "backend", value: backendId),
            URLQueryItem(name: "limit", value: String(limit)),
        ]
        if let since {
            items.append(URLQueryItem(name: "from", value: Self.rfc3339.string(from: since)))
        }
        return try await get("audit", query: items)
    }

    func auditStats() async throws -> AuditStats {
        try await get("audit/stats", query: [URLQueryItem(name: "backend", value: backendId)])
    }

    // ── Usage ───────────────────────────────────────────────────────────

    func usageGraph(range: String) async throws -> UsageGraph {
        try await get(
            "usage/graph",
            query: [
                URLQueryItem(name: "backend", value: backendId),
                URLQueryItem(name: "range", value: range),
                // Owners default to "just me" on this endpoint; the app wants
                // everything that reached this machine.
                URLQueryItem(name: "user_id", value: "all"),
            ]
        )
    }

    // ── Transport ───────────────────────────────────────────────────────

    private func get<T: Decodable>(_ path: String, query: [URLQueryItem]) async throws -> T {
        var components = URLComponents(
            url: baseURL.appendingPathComponent(path),
            resolvingAgainstBaseURL: false
        )
        components?.queryItems = query
        guard let url = components?.url else {
            throw Failure.transport("Could not build a request URL for \(path).")
        }

        var request = URLRequest(url: url)
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.timeoutInterval = 20

        let session = URLSession.make(allowInsecureTLS: allowInsecureTLS)
        do {
            let (data, response) = try await session.data(for: request)
            guard let http = response as? HTTPURLResponse else {
                throw Failure.transport("Unexpected response from the gateway.")
            }
            guard (200..<300).contains(http.statusCode) else {
                let detail = try? JSONDecoder().decode(ServerError.self, from: data).error
                throw Failure.http(http.statusCode, detail)
            }
            return try JSON.decoder().decode(T.self, from: data)
        } catch let failure as Failure {
            throw failure
        } catch let error as URLError where error.code == .cancelled {
            // The page went away mid-request; not worth surfacing.
            throw error
        } catch {
            throw Failure.transport(error.localizedDescription)
        }
    }

    private struct ServerError: Decodable {
        let error: String
    }

    private nonisolated(unsafe) static let rfc3339: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        return formatter
    }()
}

// ── Audit shapes ────────────────────────────────────────────────────────

struct AuditPage: Decodable, Sendable {
    var events: [AuditEvent]
    var total: Int
}

struct AuditEvent: Decodable, Sendable, Identifiable, Equatable {
    var eventId: String
    var timestamp: Date
    var traceId: String
    var userId: String?
    var toolName: String
    var backendName: String
    var riskCategory: String?
    var durationMs: Double?
    var status: String
    var errorMessage: String?
    var policyDecision: String?
    var application: String?

    var id: String { eventId }

    var isError: Bool { status == "error" || status == "tool_error" }
    var isDenied: Bool { status == "denied" }
}

struct AuditStats: Decodable, Sendable {
    var totalEvents: Int
    var events24h: Int
    var successCount: Int
    var errorCount: Int
    var deniedCount: Int
    var avgDurationMs: Double
    var topTools: [ToolStat]
    var statusBreakdown: [StatusStat]
    var hourlyVolume: [HourlyStat]

    struct ToolStat: Decodable, Sendable, Identifiable, Equatable {
        var toolName: String
        var count: Int
        var id: String { toolName }
    }

    struct StatusStat: Decodable, Sendable, Identifiable, Equatable {
        var status: String
        var count: Int
        var id: String { status }
    }

    struct HourlyStat: Decodable, Sendable, Identifiable, Equatable {
        var hour: Date
        var count: Int
        var id: Date { hour }
    }

    var errorRate: Double {
        let total = successCount + errorCount + deniedCount
        return total == 0 ? 0 : Double(errorCount) / Double(total)
    }
}

// ── Usage shapes ────────────────────────────────────────────────────────

struct UsageGraph: Decodable, Sendable {
    var users: [UserNode]
    var applications: [AppNode]
    var backends: [BackendNode]
    var tools: [ToolNode]
    var userToApp: [GraphEdge]
    var appToBackend: [GraphEdge]
    var backendToTool: [GraphEdge]

    struct UserNode: Decodable, Sendable, Identifiable, Equatable {
        var userId: String
        var username: String
        var callCount: Int
        var lastSeen: Date?
        var id: String { userId }
    }

    struct AppNode: Decodable, Sendable, Identifiable, Equatable {
        var application: String
        var isConnected: Bool
        var lastSeen: Date?
        var callCount: Int
        var id: String { application }
    }

    struct BackendNode: Decodable, Sendable, Identifiable, Equatable {
        var backendName: String
        var transport: String
        var healthStatus: String
        var toolCount: Int
        var id: String { backendName }
    }

    struct ToolNode: Decodable, Sendable, Identifiable, Equatable {
        var toolName: String
        var backendName: String
        var riskCategory: String?
        var callCount: Int
        var lastCall: Date?
        var id: String { toolName }
    }

    struct GraphEdge: Decodable, Sendable, Equatable {
        var source: String
        var target: String
        var callCount: Int
        var lastCall: Date?
    }
}
