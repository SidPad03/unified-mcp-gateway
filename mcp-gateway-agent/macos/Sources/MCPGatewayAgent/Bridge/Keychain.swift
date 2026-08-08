import Foundation
import Security

/// The gateway API key, in the login keychain.
///
/// This is defect #9 from the design doc: the key used to sit in plaintext in
/// `config.toml`, readable by anything running as the user and, more to the
/// point, by anything that got hold of a backup or a synced home directory. It
/// is still encrypted at rest here, and it still never appears in a snapshot, a
/// log line or an error message.
///
/// **The item is created with an open ACL — any application on this Mac may read
/// it — and that is a deliberate trade, not an oversight.**
///
/// The keychain identifies an application by its code signature. Without an
/// Apple Developer Program membership (decision D1) this app is signed ad-hoc or
/// self-signed, so it has no stable Team ID; macOS then falls back to the
/// binary's own hash, which changes with **every build**. The result was a
/// password prompt — "MCP Gateway Agent wants to use your confidential
/// information" — on every single update, and clicking Deny is indistinguishable
/// from being signed out, because the app simply came up at the sign-in screen
/// with no explanation. A credential prompt that fires constantly does not
/// protect anything; it teaches people to type their login password at any
/// dialog that asks, which is worse than the exposure it was guarding against.
///
/// What the open ACL actually costs: another process running **as this user**
/// can read the key without a prompt. That process could already read
/// `~/.mcp-gateway-agent/config.toml`, which holds every backend's environment
/// — API keys included — in plaintext, and could equally drive this app's own
/// tunnel. So the ACL was the only lock on a door in a wall that has none.
///
/// The way to get the protection back is a Developer ID certificate: a stable
/// Team ID makes the partition list stable, the prompt fires once ever, and this
/// can revert to the default (creating-application-only) access. `build.sh`
/// already honours `APPLE_SIGNING_IDENTITY`.
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
        let identity: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]

        // The delete below is unconditional, so hold on to whatever is stored
        // first: if the add after it fails, putting the old key back is the
        // difference between a failed rewrite and a signed-out user.
        let previous = read()

        // Deleted and re-added rather than updated in place, because the ACL is
        // set at creation and `SecItemUpdate` will not replace one. An item
        // written by an older build carries that build's signature in its access
        // list and would go on prompting for ever.
        SecItemDelete(identity as CFDictionary)

        func item(holding value: String) -> [String: Any] {
            var insert = identity
            insert[kSecValueData as String] = Data(value.utf8)
            // The agent reconnects on its own after a reboot, before anyone
            // logs in to unlock anything else, so the item has to be readable
            // then. `ThisDeviceOnly`, because the key authenticates *this*
            // machine's agent — it has no business travelling in a backup.
            insert[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            insert[kSecAttrLabel as String] = "MCP Gateway Agent"
            insert[kSecAttrDescription as String] = "Gateway API key"
            if let access = openAccess() {
                insert[kSecAttrAccess as String] = access
            }
            return insert
        }

        let addStatus = SecItemAdd(item(holding: value) as CFDictionary, nil)
        guard addStatus == errSecSuccess else {
            if let previous {
                SecItemAdd(item(holding: previous) as CFDictionary, nil)
            }
            throw Failure.status(addStatus)
        }
    }

    /// An access object that trusts every application, so reading the key never
    /// raises a password prompt. See the note on the type for why.
    ///
    /// `SecACLSetContents` with a nil application list is what "allow all
    /// applications to access this item" means — the same setting as the
    /// checkbox in Keychain Access. Returns nil if any step fails, in which case
    /// the item is created with the default creating-application-only access and
    /// the old prompt-per-build behaviour comes back; that is a worse experience
    /// but never a broken one.
    ///
    /// The four `SecKeychain` calls below are deprecated and warn at build time.
    /// That is expected: ACLs are a file-keychain concept, the replacement
    /// (`kSecUseDataProtectionKeychain`) needs a `keychain-access-groups`
    /// entitlement, and claiming that entitlement without a provisioning profile
    /// gets the process killed on launch. There is no non-deprecated way to do
    /// this without a Developer ID.
    private static func openAccess() -> SecAccess? {
        var access: SecAccess?
        guard SecAccessCreate("MCP Gateway Agent" as CFString, nil, &access) == errSecSuccess,
            let access,
            let acls = SecAccessCopyMatchingACLList(access, kSecACLAuthorizationDecrypt)
                as? [SecACL]
        else { return nil }

        for acl in acls {
            var applications: CFArray?
            var description: CFString?
            var prompt = SecKeychainPromptSelector()
            guard SecACLCopyContents(acl, &applications, &description, &prompt) == errSecSuccess
            else { continue }
            // nil applications == every application.
            SecACLSetContents(acl, nil, (description ?? "" as CFString), prompt)
        }
        return access
    }

    /// Rewrite the stored key so it picks up the access rules above.
    ///
    /// Existing installs hold an item created by an earlier build, with that
    /// build's signature baked into its access list — so the fix has to be
    /// applied to the item that is already there, once, rather than only to
    /// items written from now on. Called after a successful read; a failure is
    /// not worth surfacing, because nothing is lost but the convenience.
    static func relaxAccessIfNeeded(_ key: String) {
        guard !key.isEmpty, !UserDefaults.standard.bool(forKey: relaxedKey) else { return }
        // The flag records success, not the attempt. Recording the attempt
        // meant one failed rewrite deleted the key *and* marked the job done —
        // the user came back to the sign-in screen with nothing to say why.
        guard (try? write(key)) != nil else { return }
        UserDefaults.standard.set(true, forKey: relaxedKey)
    }

    private static let relaxedKey = "com.mcpgateway.agent.keychainAccessRelaxed"

    static func delete() {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(query as CFDictionary)
    }
}
