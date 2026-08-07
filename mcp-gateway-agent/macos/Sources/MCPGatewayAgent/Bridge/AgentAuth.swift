import AppKit
import CryptoKit
import Foundation

/// Who this Mac is signed in as. Not secret — the credential itself lives in the
/// Keychain — so `UserDefaults` is the right home for it.
struct SignedInAccount: Codable, Sendable, Equatable {
    var username: String
    var userId: String
    /// Whether this account sees every user's calls to this machine, or only its
    /// own. The Audit and Usage pages say which (decision D4).
    var isOwner: Bool
    var agentId: String
    var gatewayUrl: String

    var scopeDescription: String {
        isOwner
            ? "Showing all users' calls to this machine"
            : "Showing your calls to this machine"
    }

    private static let key = "com.mcpgateway.agent.account"

    static func load() -> SignedInAccount? {
        guard let data = UserDefaults.standard.data(forKey: key) else { return nil }
        return try? JSONDecoder().decode(SignedInAccount.self, from: data)
    }

    func save() {
        guard let data = try? JSONEncoder().encode(self) else { return }
        UserDefaults.standard.set(data, forKey: Self.key)
    }

    static func clear() {
        UserDefaults.standard.removeObject(forKey: key)
    }
}

/// Browser sign-in against the gateway.
///
/// OAuth 2.0 authorization code with PKCE, the shape RFC 8252 recommends for a
/// native app.
///
/// The page opens in the user's **default browser**, as a tab, rather than in an
/// `ASWebAuthenticationSession`. That class is the usual answer and it does
/// intercept the redirect for you, but it presents its own Safari-backed window
/// on top of the app: a second browser, with its own chrome, that is not the one
/// the user is already looking at. Handing the URL to `NSWorkspace` puts it in
/// the window they already have open, with the session and the password manager
/// they already use. The cost is that the redirect comes back through the
/// registered `mcp-gateway-agent://` scheme instead, so `resume(with:)` below is
/// wired to the app's URL handler.
///
/// What comes back is an ordinary `mcpgw_` API key, which is what the tunnel has
/// always authenticated with. The flow replaces the *human* step of creating one
/// in the dashboard and pasting it into a config file.
@MainActor
final class AgentAuth: NSObject {
    enum Failure: LocalizedError {
        case badGatewayURL
        case cancelled
        case timedOut
        case mismatchedState
        case noCode
        case server(String)

        var errorDescription: String? {
            switch self {
            case .badGatewayURL:
                "That does not look like a gateway address."
            case .cancelled:
                "Sign-in was cancelled."
            case .timedOut:
                "Sign-in was not completed in time. Try again."
            case .mismatchedState:
                "The sign-in response did not match this request. Try again."
            case .noCode:
                "The gateway did not return an authorization code."
            case let .server(message):
                message
            }
        }
    }

    static let callbackScheme = "mcp-gateway-agent"
    static let redirectURI = "mcp-gateway-agent://auth/callback"

    /// Parked while the browser has the user. Resumed by `resume(with:)` when
    /// the redirect comes back through the app's URL handler, or failed by the
    /// deadline below.
    private var pending: CheckedContinuation<URL, Error>?

    /// The gateway forgets a pending authorization after five minutes, so there
    /// is nothing left to come back to past that. Without a deadline an
    /// abandoned sign-in would leave the button spinning until the app quit.
    private static let deadline: Duration = .seconds(300)

    struct Result: Sendable {
        var account: SignedInAccount
        var apiKey: String
    }

    func signIn(gateway: String, agentId: String, allowInsecureTLS: Bool) async throws -> Result {
        guard let apiBase = GatewayURL.apiBase(from: gateway) else { throw Failure.badGatewayURL }

        let verifier = Self.randomURLSafeString(bytes: 32)
        let challenge = Self.challenge(for: verifier)
        let state = Self.randomURLSafeString(bytes: 16)

        var components = URLComponents(
            url: apiBase.appendingPathComponent("agent/authorize"),
            resolvingAgainstBaseURL: false
        )
        components?.queryItems = [
            URLQueryItem(name: "agent_id", value: agentId),
            URLQueryItem(name: "redirect_uri", value: Self.redirectURI),
            URLQueryItem(name: "state", value: state),
            URLQueryItem(name: "code_challenge", value: challenge),
            URLQueryItem(name: "code_challenge_method", value: "S256"),
        ]
        guard let authorizeURL = components?.url else { throw Failure.badGatewayURL }

        let callback = try await present(authorizeURL)

        let items = URLComponents(url: callback, resolvingAgainstBaseURL: false)?.queryItems ?? []
        guard items.first(where: { $0.name == "state" })?.value == state else {
            throw Failure.mismatchedState
        }
        guard let code = items.first(where: { $0.name == "code" })?.value, !code.isEmpty else {
            throw Failure.noCode
        }

        let token = try await exchange(
            code: code,
            verifier: verifier,
            apiBase: apiBase,
            allowInsecureTLS: allowInsecureTLS
        )

        let account = SignedInAccount(
            username: token.username,
            userId: token.userId,
            isOwner: token.isOwner,
            agentId: token.agentId,
            gatewayUrl: GatewayURL.websocket(from: gateway)
        )
        return Result(account: account, apiKey: token.apiKey)
    }

    // ── Browser ─────────────────────────────────────────────────────────

    private func present(_ url: URL) async throws -> URL {
        // A tab in whatever browser the user actually uses, carrying whatever
        // dashboard session and password manager they already have there.
        guard NSWorkspace.shared.open(url) else { throw Failure.cancelled }

        // Nothing tells us the user gave up: they can close the tab, or never
        // finish. The deadline is what stops `signingIn` spinning forever.
        let timeout = Task { [weak self] in
            try? await Task.sleep(for: Self.deadline)
            guard !Task.isCancelled else { return }
            await self?.fail(.timedOut)
        }
        defer { timeout.cancel() }

        return try await withCheckedThrowingContinuation { continuation in
            // A second attempt while one is outstanding supersedes it, rather
            // than leaking a continuation that nothing will ever resume.
            pending?.resume(throwing: Failure.cancelled)
            pending = continuation
        }
    }

    /// The redirect, handed over by the app's `mcp-gateway-agent://` handler.
    ///
    /// Ignores anything that arrives with no sign-in outstanding, so a stale
    /// link opened by hand cannot resume a flow that already finished.
    func resume(with url: URL) {
        guard let continuation = pending else { return }
        pending = nil
        continuation.resume(returning: url)
    }

    private func fail(_ failure: Failure) {
        guard let continuation = pending else { return }
        pending = nil
        continuation.resume(throwing: failure)
    }

    // ── Token exchange ──────────────────────────────────────────────────

    private struct TokenResponse: Decodable {
        let apiKey: String
        let agentId: String
        let username: String
        let userId: String
        let isOwner: Bool
    }

    private struct ServerError: Decodable {
        let error: String
    }

    private func exchange(
        code: String,
        verifier: String,
        apiBase: URL,
        allowInsecureTLS: Bool
    ) async throws -> TokenResponse {
        var request = URLRequest(url: apiBase.appendingPathComponent("agent/token"))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONEncoder().encode([
            "code": code,
            "code_verifier": verifier,
        ])

        let session = URLSession.make(allowInsecureTLS: allowInsecureTLS)
        let (data, response) = try await session.data(for: request)

        guard let http = response as? HTTPURLResponse else {
            throw Failure.server("The gateway sent an unexpected response.")
        }
        guard (200..<300).contains(http.statusCode) else {
            let detail = (try? JSONDecoder().decode(ServerError.self, from: data))?.error
            throw Failure.server(detail ?? "The gateway returned HTTP \(http.statusCode).")
        }

        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        do {
            return try decoder.decode(TokenResponse.self, from: data)
        } catch {
            throw Failure.server("The gateway's response could not be read.")
        }
    }

    // ── PKCE ────────────────────────────────────────────────────────────

    /// base64url, no padding — the alphabet RFC 7636 requires.
    static func base64URL(_ data: Data) -> String {
        data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    static func randomURLSafeString(bytes count: Int) -> String {
        var bytes = [UInt8](repeating: 0, count: count)
        // SecRandomCopyBytes is the CSPRNG; `Int.random` is not one.
        _ = SecRandomCopyBytes(kSecRandomDefault, count, &bytes)
        return base64URL(Data(bytes))
    }

    static func challenge(for verifier: String) -> String {
        base64URL(Data(SHA256.hash(data: Data(verifier.utf8))))
    }
}

// ── TLS ─────────────────────────────────────────────────────────────────

extension URLSession {
    /// A session that optionally accepts a self-signed gateway certificate,
    /// matching the tunnel's "Skip TLS verification" setting.
    static func make(allowInsecureTLS: Bool) -> URLSession {
        guard allowInsecureTLS else { return URLSession(configuration: .ephemeral) }
        return URLSession(
            configuration: .ephemeral,
            delegate: InsecureTLSDelegate.shared,
            delegateQueue: nil
        )
    }
}

private final class InsecureTLSDelegate: NSObject, URLSessionDelegate, @unchecked Sendable {
    static let shared = InsecureTLSDelegate()

    func urlSession(
        _ session: URLSession,
        didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
            let trust = challenge.protectionSpace.serverTrust
        else {
            completionHandler(.performDefaultHandling, nil)
            return
        }
        completionHandler(.useCredential, URLCredential(trust: trust))
    }
}
