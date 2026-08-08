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
    var toolName: String
    var backendName: String
    var riskCategory: String?
    var durationMs: Double?
    var status: String
    var errorMessage: String?
    var policyDecision: String?
    var application: String?

    private enum CodingKeys: String, CodingKey {
        case eventId, timestamp, traceId, toolName, backendName
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
/// Only the four figures the summary band draws. The endpoint also sends
/// `events_24h`, `top_tools`, `status_breakdown` and `hourly_volume`; the cards
/// that showed them were removed from the Audit page (see the notes there), and
/// the decoder skips what nothing reads.
struct AuditStats: Decodable, Sendable {
    var totalEvents: Int
    var successCount: Int
    var errorCount: Int
    var deniedCount: Int
    var avgDurationMs: Double

    private enum CodingKeys: String, CodingKey {
        case totalEvents, successCount, errorCount, deniedCount, avgDurationMs
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        totalEvents = try c.decodeIfPresent(Int.self, forKey: .totalEvents) ?? 0
        successCount = try c.decodeIfPresent(Int.self, forKey: .successCount) ?? 0
        errorCount = try c.decodeIfPresent(Int.self, forKey: .errorCount) ?? 0
        deniedCount = try c.decodeIfPresent(Int.self, forKey: .deniedCount) ?? 0
        avgDurationMs = try c.decodeIfPresent(Double.self, forKey: .avgDurationMs) ?? 0
    }

    var errorRate: Double {
        let total = successCount + errorCount + deniedCount
        return total == 0 ? 0 : Double(errorCount) / Double(total)
    }
}

// ── Usage shapes ────────────────────────────────────────────────────────

/// The three collections the flow board actually draws. The endpoint also
/// sends `users`, `backends` and three other edge lists; the board derives its
/// backend column by splitting tool namespaces (see `ToolName.split`), so
/// decoding the rest was work done to be thrown away on every poll.
struct UsageGraph: Decodable, Sendable {
    var applications: [AppNode]
    var tools: [ToolNode]
    /// application → *tool*, which is what the flow board turns into
    /// application → backend once it has split the namespace.
    ///
    /// Optional on the wire on purpose: a gateway older than this app does not
    /// send it, and the right answer there is a board without those lines rather
    /// than a page that fails to load. Everything else on Usage works without
    /// it.
    var appToTool: [GraphEdge] = []

    private enum CodingKeys: String, CodingKey {
        case applications, tools, appToTool
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        applications = try c.decodeIfPresent([AppNode].self, forKey: .applications) ?? []
        tools = try c.decodeIfPresent([ToolNode].self, forKey: .tools) ?? []
        appToTool = try c.decodeIfPresent([GraphEdge].self, forKey: .appToTool) ?? []
    }

    struct AppNode: Decodable, Sendable, Identifiable, Equatable {
        var application: String
        var isConnected: Bool
        var callCount: Int
        var id: String { application }
    }

    struct ToolNode: Decodable, Sendable, Identifiable, Equatable {
        var toolName: String
        var riskCategory: String?
        var callCount: Int
        var id: String { toolName }
    }

    struct GraphEdge: Decodable, Sendable, Equatable {
        var source: String
        var target: String
        var callCount: Int
    }
}
