import AppKit
import Foundation
import UserNotifications

/// macOS notifications for what goes wrong while nobody is looking at the window.
///
/// This app spends most of its life as a menu-bar icon. A backend that crashes
/// at three in the afternoon, or a gateway that stops answering, used to show
/// only as a colour on a page nobody had open — and the first sign was a tool
/// call failing in some other app, with nothing there to say why.
///
/// The Rust core decides *what* is worth saying and *when* (`core::alerts`: the
/// grace period on a dropped connection, the throttle on a backend failing in a
/// loop, the batching of failed calls). This type only turns those decisions
/// into notifications, honours the switches in Settings → Notifications, and
/// routes a click to the page that explains it.
@MainActor
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    /// One for the process, because the delegate has to be in place before
    /// launch finishes: a click on a notification that launches the app is
    /// delivered then, before any window — or the model a window creates —
    /// exists.
    static let shared = Notifier()

    /// What the user can switch on and off, one row each in Settings.
    enum Category: String, CaseIterable, Identifiable {
        case backends, connection, toolErrors, updates

        var id: String { rawValue }

        /// The `UserDefaults` key the switch is stored under, shared with the
        /// `@AppStorage` in the Settings pane.
        var defaultsKey: String { "notifications.\(rawValue)" }

        var title: String {
            switch self {
            case .backends: "Local MCP servers stop working"
            case .connection: "The gateway connection is lost"
            case .toolErrors: "Tool calls fail"
            case .updates: "An update is available"
            }
        }

        var detail: String {
            switch self {
            case .backends:
                "A server on this Mac fails to start or exits on its own, and when it comes back."
            case .connection:
                "The gateway has been unreachable for 30 seconds, and when it reconnects. "
                    + "Shorter drops — a redeploy, waking from sleep — say nothing."
            case .toolErrors:
                "Calls the gateway routes here that end in an error, grouped so a burst is "
                    + "one notification a minute."
            case .updates:
                "A new version of this app is ready to install, or an install fails."
            }
        }

        /// Tool errors are off by default: an assistant working through a task
        /// can produce a run of them that are nobody's problem.
        var enabledByDefault: Bool { self != .toolErrors }
    }

    /// Set by the model: what "Install" on an update notification does.
    var onInstallUpdate: (() -> Void)?

    private static let categoryUpdate = "update"
    private static let actionInstall = "install"
    private static let lastAnnouncedVersionKey = "notifications.lastAnnouncedVersion"

    /// `UNUserNotificationCenter` needs a bundle. `swift run` outside the .app
    /// has none, and the framework traps rather than failing politely.
    private let isAvailable = Bundle.main.bundleURL.pathExtension == "app"
        && Bundle.main.bundleIdentifier != nil

    private var started = false

    private override init() {
        super.init()
        UserDefaults.standard.register(
            defaults: Dictionary(
                uniqueKeysWithValues: Category.allCases.map { ($0.defaultsKey, $0.enabledByDefault) }
            )
        )
    }

    // ── Lifecycle ───────────────────────────────────────────────────────

    /// Become the delegate, so a click is routed here and a notification shows
    /// even while the app is frontmost.
    func start() {
        guard isAvailable, !started else { return }
        started = true
        let center = UNUserNotificationCenter.current()
        center.delegate = self
        center.setNotificationCategories([
            UNNotificationCategory(
                identifier: Self.categoryUpdate,
                actions: [
                    UNNotificationAction(identifier: Self.actionInstall, title: "Install", options: [.foreground])
                ],
                intentIdentifiers: []
            )
        ])
    }

    /// Ask once, and only with a reason to. Called after sign-in, when there is
    /// finally something to report on; asking on a first launch that has not
    /// connected to anything is asking in a vacuum.
    func requestAuthorizationIfNeeded() async {
        guard isAvailable, Category.allCases.contains(where: isEnabled) else { return }
        guard await authorizationStatus() == .notDetermined else { return }
        _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound])
    }

    func authorizationStatus() async -> UNAuthorizationStatus {
        guard isAvailable else { return .denied }
        return await UNUserNotificationCenter.current().notificationSettings().authorizationStatus
    }

    var canNotify: Bool { isAvailable }

    func isEnabled(_ category: Category) -> Bool {
        UserDefaults.standard.bool(forKey: category.defaultsKey)
    }

    // ── What the core reported ──────────────────────────────────────────

    func handle(_ alerts: [Alert], gatewayUrl: String) {
        for alert in alerts {
            switch alert.kind {
            case .backendFailed:
                post(
                    .backends,
                    id: "backend.\(alert.subject)",
                    title: "\(alert.subject) failed to start",
                    subtitle: "Retrying automatically",
                    body: alert.detail ?? "The MCP server did not start.",
                    page: .backends
                )
            case .backendCrashed:
                post(
                    .backends,
                    id: "backend.\(alert.subject)",
                    title: "\(alert.subject) stopped unexpectedly",
                    subtitle: alert.count > 1 ? "Restarted \(alert.count) times so far" : "Restarting",
                    body: alert.detail ?? "The MCP server exited on its own.",
                    page: .backends
                )
            case .backendRecovered:
                // Same identifier: it replaces the failure in Notification
                // Center rather than piling up beside it.
                post(
                    .backends,
                    id: "backend.\(alert.subject)",
                    title: "\(alert.subject) is running again",
                    body: "Its tools are back on the gateway.",
                    page: .backends,
                    sound: false
                )
            case .connectionLost:
                post(
                    .connection,
                    id: "connection",
                    title: "Disconnected from the gateway",
                    subtitle: "This Mac's tools are unavailable until it reconnects",
                    body: alert.detail.map { ConnectionStatus.readable($0, gatewayUrl: gatewayUrl) }
                        ?? "Retrying automatically.",
                    page: .overview
                )
            case .connectionRestored:
                post(
                    .connection,
                    id: "connection",
                    title: "Reconnected to the gateway",
                    body: "This Mac's tools are available again.",
                    page: .overview,
                    sound: false
                )
            case .callsFailed:
                let tool = alert.subject.range(of: "__").map { String(alert.subject[$0.upperBound...]) }
                    ?? alert.subject
                post(
                    .toolErrors,
                    id: "calls",
                    title: alert.count == 1 ? "A tool call failed" : "\(alert.count) tool calls failed",
                    body: [tool, alert.detail].compactMap { $0 }.joined(separator: ": "),
                    page: .overview
                )
            }
        }
    }

    // ── Updates ─────────────────────────────────────────────────────────

    func updateAvailable(_ version: String) {
        // Once per version. The check runs every six hours, and so does the
        // news otherwise.
        let defaults = UserDefaults.standard
        guard defaults.string(forKey: Self.lastAnnouncedVersionKey) != version else { return }
        guard post(
            .updates,
            id: "update",
            title: "Update available",
            body: "MCP Gateway Agent \(version) is ready to install.",
            page: nil,
            category: Self.categoryUpdate
        ) else { return }
        defaults.set(version, forKey: Self.lastAnnouncedVersionKey)
    }

    func updateFailed(_ message: String) {
        post(.updates, id: "update", title: "The update did not install", body: message, page: nil)
    }

    /// Settings → Notifications → Send a test notification.
    func sendTest() async {
        await requestAuthorizationIfNeeded()
        post(
            nil,
            id: "test",
            title: "Notifications are on",
            body: "This is how MCP Gateway Agent will tell you something needs attention.",
            page: nil
        )
    }

    // ── Posting ─────────────────────────────────────────────────────────

    /// Post, if the category is switched on. Returns whether it was attempted.
    @discardableResult
    private func post(
        _ category: Category?,
        id: String,
        title: String,
        subtitle: String? = nil,
        body: String,
        page: Page?,
        sound: Bool = true,
        category notificationCategory: String? = nil
    ) -> Bool {
        guard isAvailable else { return false }
        if let category, !isEnabled(category) { return false }

        let content = UNMutableNotificationContent()
        content.title = title
        if let subtitle { content.subtitle = subtitle }
        content.body = body
        if sound { content.sound = .default }
        // One stack per kind in Notification Center.
        content.threadIdentifier = category?.rawValue ?? "general"
        if let page { content.userInfo = ["page": page.rawValue] }
        if let notificationCategory { content.categoryIdentifier = notificationCategory }

        let request = UNNotificationRequest(identifier: id, content: content, trigger: nil)
        Task {
            // The first thing worth saying is a fine moment to ask; if the
            // answer is no, `add` quietly does nothing.
            await requestAuthorizationIfNeeded()
            try? await UNUserNotificationCenter.current().add(request)
        }
        return true
    }

    // ── UNUserNotificationCenterDelegate ────────────────────────────────

    /// Show it even when the app is frontmost: the window may be on another
    /// page, or behind something else.
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        [.banner, .list, .sound]
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse
    ) async {
        let page = (response.notification.request.content.userInfo["page"] as? String)
            .flatMap(Page.init(rawValue:))
        let action = response.actionIdentifier
        await MainActor.run {
            if action == Self.actionInstall {
                self.onInstallUpdate?()
            } else if action == UNNotificationDefaultActionIdentifier {
                AppDelegate.shared?.showMainWindow(page: page)
            }
        }
    }
}
