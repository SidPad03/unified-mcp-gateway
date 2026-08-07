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
    /// This machine's name on the gateway, which is what every `backend=`
    /// filter is compared against: the server matches it to
    /// `audit_events.backend_name`, not to the `backends` primary key. Named for
    /// what it is, because holding the UUID here instead read as correct and
    /// silently matched nothing.
    let backendName: String
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
            URLQueryItem(name: "backend", value: backendName),
            URLQueryItem(name: "limit", value: String(limit)),
        ]
        if let since {
            items.append(URLQueryItem(name: "from", value: Self.rfc3339.string(from: since)))
        }
        return try await get("audit", query: items)
    }

    func auditStats() async throws -> AuditStats {
        try await get("audit/stats", query: [URLQueryItem(name: "backend", value: backendName)])
    }

    // ── Usage ───────────────────────────────────────────────────────────

    func usageGraph(range: String) async throws -> UsageGraph {
        try await get(
            "usage/graph",
            query: [
                URLQueryItem(name: "backend", value: backendName),
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
        } catch let error as DecodingError {
            throw Failure.transport(Self.describe(error, path: path))
        } catch {
            throw Failure.transport(error.localizedDescription)
        }
    }

    /// Foundation renders every `DecodingError` as one of two sentences, and
    /// neither names the field, the request, or anything you could act on. "The
    /// data couldn't be read because it is missing." is what a user saw when a
    /// gateway a few versions behind left one number out of one response, and it
    /// gives whoever reads it no way to tell that from a broken connection.
    private static func describe(_ error: DecodingError, path: String) -> String {
        func field(_ context: DecodingError.Context) -> String {
            let steps = context.codingPath.map(\.stringValue).filter { !$0.isEmpty }
            return steps.isEmpty ? "the response" : "'\(steps.joined(separator: "."))'"
        }
        let older = "This usually means the gateway is older than the app; updating it should fix it."

        switch error {
        case let .keyNotFound(key, context):
            let container = context.codingPath.isEmpty ? "" : " in \(field(context))"
            return "The gateway's /\(path) response has no '\(key.stringValue)'\(container). \(older)"
        case let .valueNotFound(_, context):
            return "The gateway sent no value for \(field(context)) in /\(path). \(older)"
        case let .typeMismatch(type, context):
            return "The gateway sent \(field(context)) in /\(path) as the wrong type; "
                + "this app expects \(type). \(older)"
        case let .dataCorrupted(context):
            return "The gateway's /\(path) response could not be read at \(field(context)): "
                + context.debugDescription
        @unknown default:
            return "The gateway's /\(path) response could not be read."
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

    private enum CodingKeys: String, CodingKey {
        case events, total
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        events = try c.decodeIfPresent([AuditEvent].self, forKey: .events) ?? []
        // A count, so an absent one is zero. It only drives the "showing n of m"
        // line; losing the whole page over it would be a poor trade.
        total = try c.decodeIfPresent(Int.self, forKey: .total) ?? 0
    }
}

/// What a row cannot be drawn without stays required: without an id, a time, a
/// tool and an outcome there is no event here to show. Everything else is
/// context, and a row missing its trace id is still worth a line in the table.
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

    private enum CodingKeys: String, CodingKey {
        case eventId, timestamp, traceId, userId, toolName, backendName
        case riskCategory, durationMs, status, errorMessage, policyDecision, application
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        eventId = try c.decode(String.self, forKey: .eventId)
        timestamp = try c.decode(Date.self, forKey: .timestamp)
        toolName = try c.decode(String.self, forKey: .toolName)
        status = try c.decode(String.self, forKey: .status)
        traceId = try c.decodeIfPresent(String.self, forKey: .traceId) ?? ""
        backendName = try c.decodeIfPresent(String.self, forKey: .backendName) ?? ""
        userId = try c.decodeIfPresent(String.self, forKey: .userId)
        riskCategory = try c.decodeIfPresent(String.self, forKey: .riskCategory)
        durationMs = try c.decodeIfPresent(Double.self, forKey: .durationMs)
        errorMessage = try c.decodeIfPresent(String.self, forKey: .errorMessage)
        policyDecision = try c.decodeIfPresent(String.self, forKey: .policyDecision)
        application = try c.decodeIfPresent(String.self, forKey: .application)
    }

    var id: String { eventId }

    var isError: Bool { status == "error" || status == "tool_error" }
    var isDenied: Bool { status == "denied" }
}

/// Every field here is optional on the wire, and that is deliberate.
///
/// These are counts and averages: a gateway that omits one, or sends null for
/// it, is telling us zero. Declaring them required meant the opposite — one
/// absent number failed the whole decode, the page lost its summary *and* its
/// event list, and all the user got was "The data couldn't be read because it is
/// missing." The most likely field to go missing is exactly the one that does so
/// hardest to notice: `avg_duration_ms` is `AVG(duration_ms)`, which SQL returns
/// as NULL when there is nothing to average, so a gateway build without the
/// coalesce breaks this page precisely when there are no events to show.
///
/// A summary is worth showing with a hole in it. It is not worth taking the rest
/// of the page down for.
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

    private enum CodingKeys: String, CodingKey {
        // `events24H`, with the capital H, is not a typo and is the whole reason
        // this page failed.
        //
        // `.convertFromSnakeCase` splits on underscores and capitalises each
        // following component, and `24h` capitalises to `24H` — the digits are
        // skipped and the first *letter* is raised. So the gateway's
        // `events_24h` arrives as `events24H`, the synthesised key for a
        // property named `events24h` did not match it, and the decode failed
        // with `keyNotFound`. Foundation renders that as "The data couldn't be
        // read because it is missing.", which is what the Audit page showed
        // instead of a summary — against every gateway, healthy or not, since
        // this was written.
        //
        // Spelled out rather than left to the strategy, because the strategy is
        // the thing that got it wrong.
        case events24h = "events24H"
        case totalEvents, successCount, errorCount, deniedCount
        case avgDurationMs, topTools, statusBreakdown, hourlyVolume
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        totalEvents = try c.decodeIfPresent(Int.self, forKey: .totalEvents) ?? 0
        events24h = try c.decodeIfPresent(Int.self, forKey: .events24h) ?? 0
        successCount = try c.decodeIfPresent(Int.self, forKey: .successCount) ?? 0
        errorCount = try c.decodeIfPresent(Int.self, forKey: .errorCount) ?? 0
        deniedCount = try c.decodeIfPresent(Int.self, forKey: .deniedCount) ?? 0
        avgDurationMs = try c.decodeIfPresent(Double.self, forKey: .avgDurationMs) ?? 0
        topTools = try c.decodeIfPresent([ToolStat].self, forKey: .topTools) ?? []
        statusBreakdown = try c.decodeIfPresent([StatusStat].self, forKey: .statusBreakdown) ?? []
        hourlyVolume = try c.decodeIfPresent([HourlyStat].self, forKey: .hourlyVolume) ?? []
    }

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
