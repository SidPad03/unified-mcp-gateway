import Foundation
import Security

/// The gateway API key, in the login keychain.
///
/// This is defect #9 from the design doc: the key used to sit in plaintext in
/// `config.toml`, readable by anything running as the user and, more to the
/// point, by anything that got hold of a backup or a synced home directory.
///
/// One wrinkle worth knowing about, and it is a consequence of shipping without
/// an Apple Developer Program membership (decision D1): keychain access is
/// granted to a *signature*, and an ad-hoc signature is different in every
/// build. After an update macOS may ask again whether the app can read its own
/// item — "MCP Gateway Agent wants to use your confidential information".
/// Clicking **Always Allow** is the right answer, and it stops asking. Signing
/// with a Developer ID certificate makes the prompt go away entirely.
enum Keychain {
    static let service = "com.mcpgateway.agent"
    static let account = "gateway-api-key"

    enum Failure: LocalizedError {
        case status(OSStatus)

        var errorDescription: String? {
            switch self {
            case let .status(code):
                let detail = SecCopyErrorMessageString(code, nil) as String?
                return "Keychain error \(code)" + (detail.map { ": \($0)" } ?? "")
            }
        }
    }

    static func read() -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]

        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        guard status == errSecSuccess, let data = item as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    static func write(_ value: String) throws {
        let data = Data(value.utf8)
        let identity: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]

        // Update in place if it is already there; SecItemAdd on an existing item
        // fails with errSecDuplicateItem rather than replacing it.
        let update: [String: Any] = [kSecValueData as String: data]
        let updateStatus = SecItemUpdate(identity as CFDictionary, update as CFDictionary)
        if updateStatus == errSecSuccess { return }
        guard updateStatus == errSecItemNotFound else { throw Failure.status(updateStatus) }

        var insert = identity
        insert[kSecValueData as String] = data
        // The agent reconnects on its own after a reboot, before anyone logs in
        // to unlock anything else, so the item has to be readable then.
        insert[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        insert[kSecAttrLabel as String] = "MCP Gateway Agent"
        insert[kSecAttrDescription as String] = "Gateway API key"

        let addStatus = SecItemAdd(insert as CFDictionary, nil)
        guard addStatus == errSecSuccess else { throw Failure.status(addStatus) }
    }

    static func delete() {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(query as CFDictionary)
    }

    /// `mcpg…••••` — enough to tell two keys apart, not enough to use one.
    static func mask(_ key: String) -> String {
        guard !key.isEmpty else { return "" }
        return key.prefix(4) + String(repeating: "•", count: 16)
    }
}
