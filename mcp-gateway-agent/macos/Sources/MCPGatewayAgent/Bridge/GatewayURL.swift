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

        for (prefix, wantsCleartext) in [
            ("https://", false), ("wss://", false),
            ("http://", true), ("ws://", true),
        ] where trimmed.hasPrefix(prefix) {
            let rest = String(trimmed.dropFirst(prefix.count))
            // An explicit cleartext scheme is honoured only where cleartext
            // cannot cross the open internet — loopback, RFC 1918 space, and
            // LAN names. `http://` in front of a public host would put the API
            // key on the wire in the clear on every reconnect; ATS blocks that
            // on the Swift side, but the Rust tunnel has no ATS, so the rule
            // has to live here, where the URL is made.
            let cleartext = wantsCleartext && isPrivateHost(of: rest)
            return (cleartext ? insecure : secure) + "://" + rest
        }

        // No scheme. Assume TLS, because anything on the open internet needs
        // it — unless it is obviously a machine on this desk.
        let local = ["localhost", "127.0.0.1", "[::1]"].contains { trimmed.hasPrefix($0) }
        return (local ? insecure : secure) + "://" + trimmed
    }

    /// Whether the authority at the front of `rest` names something that never
    /// routes over the public internet: loopback, RFC 1918 / link-local
    /// addresses, mDNS `.local`, `.home.arpa`, or a bare single-label LAN name.
    private static func isPrivateHost(of rest: String) -> Bool {
        var host = rest
        if let slash = host.firstIndex(of: "/") { host = String(host[..<slash]) }
        if host.hasPrefix("[") {
            // A bracketed IPv6 literal; the port lives outside the brackets.
            host = String(host.dropFirst().prefix { $0 != "]" })
        } else if let colon = host.lastIndex(of: ":"), !host[host.index(after: colon)...].isEmpty,
            host[host.index(after: colon)...].allSatisfy(\.isNumber)
        {
            host = String(host[..<colon])
        }
        host = host.lowercased()
        guard !host.isEmpty else { return false }

        if host == "localhost" || host.hasSuffix(".local") || host.hasSuffix(".home.arpa") {
            return true
        }
        if host.contains(":") {
            // IPv6: loopback, link-local, unique-local.
            return host == "::1" || host.hasPrefix("fe80:") || host.hasPrefix("fd")
                || host.hasPrefix("fc")
        }
        let octets = host.split(separator: ".").compactMap { Int($0) }
        if octets.count == 4 {
            if octets[0] == 10 || octets[0] == 127 { return true }
            if octets[0] == 192, octets[1] == 168 { return true }
            if octets[0] == 172, (16...31).contains(octets[1]) { return true }
            if octets[0] == 169, octets[1] == 254 { return true }
            return false
        }
        // A single-label name resolves on the local network or not at all.
        return !host.contains(".")
    }

    private static func trimmingAPIPrefix(_ url: String) -> String {
        var trimmed = url
        while trimmed.hasSuffix("/") { trimmed.removeLast() }
        if trimmed.hasSuffix("/api/v1") { trimmed.removeLast("/api/v1".count) }
        return trimmed
    }
}
