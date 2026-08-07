import Foundation

// The shapes of the gateway's REST responses, kept apart from the transport
// that fetches them so the views that draw them can be rendered on their own —
// `GatewayAPI` pulls in the Keychain, the FFI bridge and a URLSession, and none
// of that is needed to lay out a table row.

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

    /// For fixtures and previews; the wire always goes through `init(from:)`.
    init(
        eventId: String,
        timestamp: Date,
        toolName: String,
        status: String,
        traceId: String = "",
        backendName: String = "",
        userId: String? = nil,
        riskCategory: String? = nil,
        durationMs: Double? = nil,
        errorMessage: String? = nil,
        policyDecision: String? = nil,
        application: String? = nil
    ) {
        self.eventId = eventId
        self.timestamp = timestamp
        self.toolName = toolName
        self.status = status
        self.traceId = traceId
        self.backendName = backendName
        self.userId = userId
        self.riskCategory = riskCategory
        self.durationMs = durationMs
        self.errorMessage = errorMessage
        self.policyDecision = policyDecision
        self.application = application
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
