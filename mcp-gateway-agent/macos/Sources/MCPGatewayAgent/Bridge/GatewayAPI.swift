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
    ///
    /// `before` pages backwards. Deliberately a timestamp rather than the
    /// `offset` the endpoint also accepts: events keep arriving at the top while
    /// the page is open, and each one shifts an offset-based window down by a
    /// row, so paging that way hands back a duplicate for every event that
    /// landed in between. Both bounds are inclusive on the server, so the row
    /// they are taken from always comes back and the caller dedupes by id.
    func auditEvents(since: Date? = nil, before: Date? = nil, limit: Int = 100) async throws
        -> AuditPage
    {
        var items = [
            URLQueryItem(name: "backend", value: backendName),
            // The endpoint clamps to 500; asking for more is not an error, but
            // it is a lie about what comes back.
            URLQueryItem(name: "limit", value: String(min(max(limit, 1), 500))),
        ]
        if let since {
            items.append(URLQueryItem(name: "from", value: Self.rfc3339.string(from: since)))
        }
        if let before {
            items.append(URLQueryItem(name: "to", value: Self.rfc3339.string(from: before)))
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
