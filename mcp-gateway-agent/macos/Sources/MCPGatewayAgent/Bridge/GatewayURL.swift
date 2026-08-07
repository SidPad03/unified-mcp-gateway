import Foundation

/// Turning what someone types into the two URLs the app needs.
///
/// Mirrors `normalize_gateway_url` and `api_base_url` in
/// `mcp-gateway-agent-core/src/config.rs`. Two implementations of the same rule
/// is not ideal, but the alternative is a round trip through FFI before the app
/// can even show a sign-in button.
enum GatewayURL {
    /// Where the gateway serves the agent WebSocket — top level, beside
    /// `/api/v1` rather than under it.
    static let agentPath = "/agent/ws"

    /// `https://gw.example.com` → `wss://gw.example.com/agent/ws`
    static func websocket(from input: String) -> String {
        let scheme = withScheme(input, secure: "wss", insecure: "ws")
        guard !scheme.isEmpty else { return "" }
        if scheme.hasSuffix(agentPath) { return scheme }
        return trimmingAPIPrefix(scheme) + agentPath
    }

    /// `wss://gw.example.com/agent/ws` → `https://gw.example.com/api/v1`
    static func apiBase(from input: String) -> URL? {
        let scheme = withScheme(input, secure: "https", insecure: "http")
        guard !scheme.isEmpty else { return nil }
        if let range = scheme.range(of: "/api/v1") {
            return URL(string: String(scheme[scheme.startIndex..<range.upperBound]))
        }
        // Everything up to the first path separator after the scheme.
        guard let separator = scheme.range(of: "://") else { return nil }
        let afterScheme = scheme[separator.upperBound...]
        let origin =
            if let slash = afterScheme.firstIndex(of: "/") {
                String(scheme[scheme.startIndex..<slash])
            } else {
                scheme
            }
        return URL(string: origin + "/api/v1")
    }

    // ── Internals ───────────────────────────────────────────────────────

    private static func withScheme(_ input: String, secure: String, insecure: String) -> String {
        var trimmed = input.trimmingCharacters(in: .whitespacesAndNewlines)
        while trimmed.hasSuffix("/") { trimmed.removeLast() }
        guard !trimmed.isEmpty else { return "" }

        for (prefix, replacement) in [
            ("https://", secure), ("wss://", secure),
            ("http://", insecure), ("ws://", insecure),
        ] where trimmed.hasPrefix(prefix) {
            return replacement + "://" + trimmed.dropFirst(prefix.count)
        }

        // No scheme. Assume TLS, because anything on the open internet needs
        // it — unless it is obviously a machine on this desk.
        let local = ["localhost", "127.0.0.1", "[::1]"].contains { trimmed.hasPrefix($0) }
        return (local ? insecure : secure) + "://" + trimmed
    }

    private static func trimmingAPIPrefix(_ url: String) -> String {
        var trimmed = url
        while trimmed.hasSuffix("/") { trimmed.removeLast() }
        if trimmed.hasSuffix("/api/v1") { trimmed.removeLast("/api/v1".count) }
        return trimmed
    }
}
