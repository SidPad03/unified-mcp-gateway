import AppKit
import SwiftUI

/// The app owns the tunnel. There is no daemon, no IPC socket, and no version
/// skew between a background service and the thing you are looking at — the
/// window is a view onto a Rust core running inside this same process.
///
/// Closing the window does not quit: the Mac's MCP backends have to stay
/// connected. The app drops to the menu bar and the Dock icon goes away, which
/// is how Tailscale, Docker Desktop and Ollama all behave and what people expect
/// of something that keeps a connection up.
@main
struct MCPGatewayAgentApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var model = AgentModel()

    var body: some Scene {
        Window("MCP Gateway Agent", id: "main") {
            RootView()
                .environment(model)
                .task { await model.launch() }
                .onAppear { AppDelegate.shared?.model = model }
        }
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentSize)
        .defaultSize(width: 1_060, height: 720)
        .commands {
            CommandGroup(replacing: .newItem) {}
            CommandGroup(after: .appInfo) {
                Button("Check for Updates…") {
                    Task { await model.updater.check() }
                }
            }
            CommandMenu("Gateway") {
                Button("Reconnect") { Task { await model.reconnect() } }
                    .keyboardShortcut("r", modifiers: .command)
                Button("Re-register Tools") { Task { await model.reregister() } }
                    .keyboardShortcut("r", modifiers: [.command, .shift])
                Divider()
                Button("Open Dashboard") { model.openDashboard() }
                    .disabled(model.apiBaseURL == nil)
            }
        }

        MenuBarExtra {
            MenuBarView()
                .environment(model)
        } label: {
            TrayIcon()
        }
        .menuBarExtraStyle(.window)

        // Gives the app menu its standard "Settings…" item and ⌘, for free.
        Settings {
            PreferencesView()
                .environment(model)
        }
    }
}

/// The menu-bar glyph.
///
/// A macOS template image: pure black plus alpha, with `Template` in the
/// filename. AppKit keys off that to invert the icon on a light menu bar and to
/// highlight it while the menu is open — a coloured icon does neither and looks
/// broken half the time.
private struct TrayIcon: View {
    var body: some View {
        if let image = NSImage(named: "agent-tray-Template") {
            Image(nsImage: image)
        } else {
            // Only reachable in a development build run outside the .app
            // bundle. Still the same mark — the app never shows two logos.
            BrandMark(size: 16, weight: 2.2)
        }
    }
}

// ── Application delegate ────────────────────────────────────────────────

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    static private(set) var shared: AppDelegate?
    /// Set once the main window exists, so Quit can stop the backends.
    var model: AgentModel?

    private let confirmedQuitKey = "com.mcpgateway.agent.confirmedQuit"

    /// Set by the updater immediately before it quits us so the staged swap
    /// script can run. The quit warning does not apply when the app is about to
    /// reopen itself.
    var isRelaunchingForUpdate = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        Self.shared = self

        // Started at login: come up in the menu bar with no window and no focus
        // steal. Someone who set "start at login" wants their backends
        // connected after a reboot, not an app window in front of whatever they
        // were doing.
        if Self.launchedAsLoginItem {
            NSApp.setActivationPolicy(.accessory)
            for window in NSApp.windows where window.canBecomeMain {
                window.close()
            }
        }

        for name in [NSWindow.willCloseNotification, NSWindow.didBecomeKeyNotification] {
            NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { _ in
                // On willClose the window is still in `NSApp.windows`, so let
                // the run loop turn over before counting.
                Task { @MainActor in AppDelegate.shared?.syncActivationPolicy() }
            }
        }
    }

    /// Menu-bar apps do not quit when their last window closes.
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    /// Reopening from the Dock or Spotlight brings the window back.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        if !hasVisibleWindows {
            NSApp.setActivationPolicy(.regular)
            NSApp.activate(ignoringOtherApps: true)
        }
        return true
    }

    /// Quitting stops every local MCP backend on this machine, which is not
    /// obvious from a ⌘Q. Say so once; after that, trust the user.
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard !isRelaunchingForUpdate else {
            model?.shutdown()
            return .terminateNow
        }

        guard !UserDefaults.standard.bool(forKey: confirmedQuitKey) else {
            model?.shutdown()
            return .terminateNow
        }

        let alert = NSAlert()
        alert.messageText = "Quit MCP Gateway Agent?"
        alert.informativeText =
            "Quitting stops this Mac's MCP servers and disconnects them from the gateway. "
            + "Tools from this machine stop working until you open the app again.\n\n"
            + "To keep it running, close the window instead. The app stays in the menu bar."
        alert.alertStyle = .warning
        alert.addButton(withTitle: "Quit")
        alert.addButton(withTitle: "Cancel")
        alert.showsSuppressionButton = true
        alert.suppressionButton?.title = "Don't ask again"

        guard alert.runModal() == .alertFirstButtonReturn else { return .terminateCancel }
        if alert.suppressionButton?.state == .on {
            UserDefaults.standard.set(true, forKey: confirmedQuitKey)
        }
        model?.shutdown()
        return .terminateNow
    }

    /// Whether launchd started us because the user is a login item, rather than
    /// because someone opened the app.
    ///
    /// The Apple Event that opened the application carries the answer; there is
    /// no cleaner API for it, and `SMAppService` cannot pass an argument.
    private static var launchedAsLoginItem: Bool {
        guard let event = NSAppleEventManager.shared().currentAppleEvent,
            event.eventID == kAEOpenApplication
        else { return false }
        return event.paramDescriptor(forKeyword: keyAEPropData)?.enumCodeValue
            == keyAELaunchedAsLogInItem
    }

    /// Dock icon while a window is open; menu-bar-only when there is not one.
    private func syncActivationPolicy() {
        let hasWindow = NSApp.windows.contains { window in
            window.isVisible && window.canBecomeMain && !(window is NSPanel)
        }
        NSApp.setActivationPolicy(hasWindow ? .regular : .accessory)
    }
}
