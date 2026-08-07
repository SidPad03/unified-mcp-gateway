import Foundation
import ServiceManagement

/// Start at login.
///
/// `SMAppService.mainApp` is the modern replacement for hand-writing a
/// LaunchAgent plist into `~/Library/LaunchAgents`: macOS registers the bundle
/// itself, the item shows up in System Settings → General → Login Items under
/// the app's own name, and the user can turn it off there without the app being
/// confused about it.
///
/// There is no `KeepAlive` equivalent and that is fine. Restart-on-crash fights
/// a deliberate Quit, and "start at login" already covers the case people
/// actually care about — the Mac rebooted.
enum LoginItem {
    static var isEnabled: Bool {
        SMAppService.mainApp.status == .enabled
    }

    /// True when the user has switched the app off in System Settings. The UI
    /// says so rather than silently showing a toggle that will not stick.
    static var isBlockedByUser: Bool {
        SMAppService.mainApp.status == .requiresApproval
    }

    static func setEnabled(_ enabled: Bool) throws {
        if enabled {
            try SMAppService.mainApp.register()
        } else {
            try SMAppService.mainApp.unregister()
        }
    }

    static func openSystemSettings() {
        SMAppService.openSystemSettingsLoginItems()
    }
}
