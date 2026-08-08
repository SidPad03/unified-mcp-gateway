import Combine
import SwiftUI

/// There is no Activity page.
///
/// It was a live list of the calls this app had routed since launch, and it sat
/// two rows away from Audit, which is the same list kept by the gateway and not
/// thrown away at every restart. Two pages of tool calls, one of which was
/// almost always empty, is one page too many — the live view is now a card on
/// Overview, where "what is happening right now" belongs.
enum Page: String, CaseIterable, Identifiable, Hashable {
    case overview, backends, logs, audit, usage

    var id: String { rawValue }

    var title: String {
        switch self {
        case .overview: "Overview"
        case .backends: "Backends"
        case .logs: "Logs"
        case .audit: "Audit"
        case .usage: "Usage"
        }
    }

    var icon: String {
        switch self {
        case .overview: "gauge.with.dots.needle.33percent"
        case .backends: "server.rack"
        case .logs: "text.alignleft"
        case .audit: "checklist"
        case .usage: "arrow.triangle.branch"
        }
    }
}

struct RootView: View {
    @Environment(AgentModel.self) private var model

    /// The page the window opens on, remembered between launches.
    ///
    /// Closing this window does not quit the app — it drops to the menu bar and
    /// keeps the tunnel up — so the window is opened and closed many times a
    /// day. Coming back to Overview every time makes it feel like it forgot you
    /// were there; the same reason Docker Desktop and Tailscale reopen where you
    /// left off.
    @AppStorage("selectedPage") private var selected = Page.overview.rawValue

    /// Full screen changes two things about the window's top edge, and the app
    /// has to know which one it is in.
    ///
    /// In a normal window `.hiddenTitleBar` means the traffic lights float over
    /// the content, so the sidebar reserves room for them. Full screen has no
    /// traffic lights at all — that reservation becomes fifty points of nothing
    /// — but it does put an auto-hiding title bar across the top, and anything
    /// drawn up there is unreachable whenever that bar is showing.
    @State private var isFullScreen = false

    /// Whether the navigation column is hidden, remembered between launches.
    /// The View menu writes the same key, which is how ⌃⌘S stays in step with
    /// the button in the window.
    @AppStorage("sidebarHidden") private var sidebarHidden = false

    /// Clears the traffic lights in a window, and nothing in full screen.
    ///
    /// 36, not 52. The lights finish 33 points down, so 52 was nineteen points
    /// of air on top of the clearance.
    private var trafficLightClearance: CGFloat { isFullScreen ? 14 : 36 }

    /// The detail pane always has something to draw, and the pages that
    /// navigate — Overview's "Manage" and "Add backend" — get a plain `Page` to
    /// write back to.
    private var current: Binding<Page> {
        Binding(
            get: { Page(rawValue: selected) ?? .overview },
            set: { selected = $0.rawValue }
        )
    }

    var body: some View {
        Group {
            if let failure = model.launchFailure {
                LaunchFailureView(message: failure)
            } else if !model.isSignedIn {
                WelcomeView()
            } else {
                main
            }
        }
        .frame(minWidth: 900, minHeight: 560)
        .background(Palette.canvas)
    }

    /// A hand-laid split, not a `NavigationSplitView`.
    ///
    /// The split view was tried, twice. It insists on installing a window
    /// toolbar, and that toolbar reserves a strip across the top of **both**
    /// columns whether or not anything is in it — which is what pushed the
    /// sidebar's content a hundred points down the window while the material
    /// behind it stopped at the strip's edge. `.toolbar(removing:)` and clearing
    /// `window.toolbar` clawed the detail column back but never the sidebar's,
    /// because the sidebar `List` keeps its own safe-area copy of the strip.
    /// And collapsing meant flipping `columnVisibility`, which the framework
    /// animates on its own clock with its own curve, fighting every animation
    /// placed on it.
    ///
    /// What the split view was providing — the translucent column — is one
    /// `NSVisualEffectView` with the `.sidebar` material, which is exactly what
    /// AppKit uses. Owning the column means the material runs to the window's
    /// very top, the collapse is one width animation this code controls, and
    /// the toggle is one button that rides the sidebar's edge instead of
    /// teleporting between two corners.
    private var main: some View {
        // A `ZStack`, so the collapsed-state toggle is laid out in the same
        // full-window coordinate space as the columns. As an `.overlay` applied
        // after `.ignoresSafeArea` it was placed *inside* the safe area — the
        // hidden title bar still reports one — which pushed it fifty points
        // down the window, straight into every page's title, where a dark
        // glyph on a dark canvas simply vanished.
        ZStack(alignment: .topLeading) {
            HStack(spacing: 0) {
                // The inner frame fixes the column's layout width; the outer
                // frame is what animates. `alignment: .trailing` pins the
                // content to the closing edge, so the column slides out to the
                // left rather than squashing in place, and `.clipped()` is the
                // curtain it slides behind.
                Sidebar(page: current, topInset: trafficLightClearance) { sidebarHidden.toggle() }
                    .frame(width: Metrics.sidebarWidth)
                    .frame(width: sidebarHidden ? 0 : Metrics.sidebarWidth, alignment: .trailing)
                    .clipped()
                detail
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                    .background(Palette.canvas)
            }

            // The way back, once the sidebar — and the toggle in its header —
            // has left the window. Drawn as a bordered chip rather than a bare
            // glyph: this is the only control in a corner of empty canvas, and
            // it has to read as one. Level with the traffic lights, which are
            // the only other thing on that line; full screen has no lights, so
            // it takes the corner itself.
            if sidebarHidden {
                SidebarButton(hidden: true, prominent: true) { sidebarHidden.toggle() }
                    .padding(.top, isFullScreen ? 12 : 8)
                    .padding(.leading, isFullScreen ? 14 : 78)
                    .transition(.opacity)
            }
        }
        // `.hiddenTitleBar` still reports the title-bar strip as a top safe
        // area once `fullSizeContentView` is set, and full screen adds its
        // auto-hiding bar's. Taking the top edge back is what lets the sidebar
        // run to the very top of the window.
        .ignoresSafeArea(edges: .top)
        // On the container, keyed to the flag, so the button, the menu command
        // and an external `defaults write` all animate identically —
        // `withAnimation` at the call sites would cover only the ones that
        // remember to.
        .animation(.snappy(duration: 0.26), value: sidebarHidden)
        .background(WindowConfigurator())
        .onReceive(
            NotificationCenter.default.publisher(for: NSWindow.didEnterFullScreenNotification)
        ) { _ in isFullScreen = true }
        .onReceive(
            NotificationCenter.default.publisher(for: NSWindow.didExitFullScreenNotification)
        ) { _ in isFullScreen = false }
    }

    @ViewBuilder
    private var detail: some View {
        switch current.wrappedValue {
        case .overview: OverviewView(page: current)
        case .backends: BackendsView()
        case .logs: LogsView()
        case .audit: AuditView()
        case .usage: UsageView()
        }
    }
}

// ── Sidebar ─────────────────────────────────────────────────────────────

/// The navigation column: identity at the top, the pages, and a footer holding
/// the two window-level controls — update and Settings.
///
/// Those two used to float in the window's top-right corner, which put them on
/// the same line as whatever a page put in *its* corner (Audit's Refresh,
/// Usage's range picker) and made the update chip read as page furniture. A
/// sidebar footer is where a menu-bar app keeps its app-level controls — it is
/// where the dashboard keeps them too — and it holds them at every window size
/// without crowding anything.
private struct Sidebar: View {
    @Environment(AgentModel.self) private var model
    @Binding var page: Page
    /// Room for the floating traffic lights — none needed in full screen.
    var topInset: CGFloat
    var onToggle: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // One row, so the lockup and the collapse control sit on the same
            // line by construction rather than by two paddings agreeing.
            HStack(spacing: 8) {
                BrandLockup(size: 20)
                Spacer(minLength: 8)
                SidebarButton(hidden: false, action: onToggle)
            }
            .padding(.leading, 16)
            .padding(.trailing, 10)
            .padding(.top, topInset)
            .padding(.bottom, 16)

            FieldLabel("Agent")
                .padding(.horizontal, 19)
                .padding(.bottom, 6)

            VStack(spacing: 1) {
                ForEach(Page.allCases) { item in
                    SidebarRow(item: item, selected: page == item, badge: badge(for: item)) {
                        page = item
                    }
                }
            }
            .padding(.horizontal, 10)

            Spacer(minLength: 12)

            footer
        }
        // The material is the sidebar. `.behindWindow` samples the desktop, the
        // same way every native sidebar does, and it runs edge to edge because
        // the container in `RootView.main` has already taken the top safe area
        // back — no `.ignoresSafeArea` here, deliberately: safe-area expansion
        // escapes the `.clipped()` that hides this column, and the hairline
        // below was drawing itself down the window edge after the sidebar had
        // supposedly left.
        .background(SidebarMaterial())
        .overlay(alignment: .trailing) {
            Rectangle().fill(Palette.line).frame(width: 1)
        }
    }

    private var footer: some View {
        VStack(spacing: 0) {
            Rectangle().fill(Palette.lineSoft).frame(height: 1)
            VStack(spacing: 1) {
                UpdateRow()
                SettingsLink {
                    SidebarActionLabel(icon: "gearshape", title: "Settings")
                }
                .buttonStyle(.plain)
                .help("Settings")
            }
            .padding(10)
        }
    }

    private func badge(for page: Page) -> Int {
        switch page {
        case .backends:
            // Only worth a badge when something is wrong.
            model.backends.filter { $0.status == .failed || $0.status == .crashed }.count
        default:
            0
        }
    }
}

/// One page in the sidebar.
///
/// Hand-drawn rather than a `List` row for two reasons that turned out to be
/// the same reason. A `.sidebar`-styled `List` breaks its own selection the
/// moment a row carries a `.badge()` — the backing table reports no selected
/// row at all, so nothing highlights and clicking does nothing — and it was the
/// `List`'s safe-area handling that pinned the whole column a toolbar-strip
/// down the window. Owning the row costs a hover flag and a fill; it buys back
/// selection, the badge, and the top of the window.
///
/// The active marker is the gate rail — the same 3-point beam that marks every
/// row of data in the product — so navigation and data speak one language.
private struct SidebarRow: View {
    let item: Page
    let selected: Bool
    let badge: Int
    let select: () -> Void

    @State private var hovering = false

    var body: some View {
        Button(action: select) {
            HStack(spacing: 9) {
                Image(systemName: item.icon)
                    .font(.system(size: Typo.body, weight: .medium))
                    .foregroundStyle(selected ? Palette.beam : Palette.text3)
                    .frame(width: 19)
                Text(item.title)
                    .font(.system(size: Typo.body, weight: selected ? .medium : .regular))
                    .foregroundStyle(selected ? Palette.text : Palette.text2)
                Spacer(minLength: 4)
                if badge > 0 {
                    Text("\(badge)")
                        .font(.system(size: Typo.micro, weight: .semibold))
                        .monospacedDigit()
                        .foregroundStyle(Palette.deny)
                        .accessibilityLabel("\(badge) not running")
                }
            }
            .padding(.horizontal, 9)
            .frame(height: 30)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(.rect(cornerRadius: Radius.row))
        }
        .buttonStyle(.plain)
        .background(
            RoundedRectangle(cornerRadius: Radius.row)
                .fill(Color.primary.opacity(selected ? 0.09 : hovering ? 0.05 : 0))
        )
        .overlay(alignment: .leading) {
            if selected {
                Capsule().fill(Palette.beam).frame(width: Metrics.rail, height: 14)
            }
        }
        .onHover { hovering = $0 }
        .animation(.easeOut(duration: 0.12), value: hovering)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// A footer control drawn in the same voice as the rows above it, minus the
/// selection state — Settings is a door, not a place the sidebar can be.
private struct SidebarActionLabel: View {
    let icon: String
    let title: String
    var tint: Color = Palette.text2
    var iconTint: Color = Palette.text3

    @State private var hovering = false

    var body: some View {
        HStack(spacing: 9) {
            Image(systemName: icon)
                .font(.system(size: Typo.body, weight: .medium))
                .foregroundStyle(iconTint)
                .frame(width: 19)
            Text(title)
                .font(.system(size: Typo.body))
                .foregroundStyle(tint)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 9)
        .frame(height: 30)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: Radius.row)
                .fill(Color.primary.opacity(hovering ? 0.05 : 0))
        )
        .contentShape(.rect(cornerRadius: Radius.row))
        .onHover { hovering = $0 }
        .animation(.easeOut(duration: 0.12), value: hovering)
    }
}

/// Show or hide the navigation column.
///
/// The same `sidebar.left` glyph AppKit puts in a split view's toolbar, so it
/// reads as the control people already know. In the sidebar's header it stays
/// quiet — a glyph that fills on hover, like every other control in the column.
/// `prominent` is the collapsed state: alone on the window it *is* window
/// chrome, and it gets the same glass the other chrome-level buttons wear —
/// a panel-coloured fill was three percent of lightness away from the canvas,
/// which is to say invisible, which is to say the sidebar had no way back.
private struct SidebarButton: View {
    let hidden: Bool
    var prominent = false
    let action: () -> Void

    @State private var hovering = false

    var body: some View {
        if prominent {
            Button(action: action) {
                Image(systemName: "sidebar.left")
                    .font(.system(size: Typo.body, weight: .medium))
                    .frame(width: 22, height: 20)
                    .contentShape(.rect)
            }
            .buttonStyle(.glass)
            .help("Show the sidebar")
            .accessibilityLabel("Show the sidebar")
        } else {
            Button(action: action) {
                Image(systemName: "sidebar.left")
                    .font(.system(size: Typo.body, weight: .medium))
                    .foregroundStyle(hovering ? Palette.text : Palette.text3)
                    .frame(width: 26, height: 24)
                    .background(
                        RoundedRectangle(cornerRadius: Radius.control)
                            .fill(Color.primary.opacity(hovering ? 0.07 : 0))
                    )
                    .contentShape(.rect)
            }
            .buttonStyle(.plain)
            .onHover { hovering = $0 }
            .animation(.easeOut(duration: 0.12), value: hovering)
            .help("Hide the sidebar")
            .accessibilityLabel("Hide the sidebar")
        }
    }
}

/// The `.sidebar` material — the exact surface AppKit gives a real source list,
/// desktop tint and window-active dimming included.
private struct SidebarMaterial: NSViewRepresentable {
    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = .sidebar
        view.blendingMode = .behindWindow
        view.state = .followsWindowActiveState
        return view
    }

    func updateNSView(_ view: NSVisualEffectView, context: Context) {}
}

// ── Window chrome ───────────────────────────────────────────────────────

/// Reaches the `NSWindow` for the things SwiftUI has no modifier for.
///
/// The grey bar across the top of a full screen is the window's own background,
/// showing through the strip AppKit reserves for the title bar. `.hiddenTitleBar`
/// hides the *bar*; it does not stop the strip existing, and it does not paint
/// it. Three settings between them close it: `fullSizeContentView` lets the
/// content occupy the strip, `titlebarAppearsTransparent` stops the title bar
/// drawing its own material over it, and a background colour matching the canvas
/// means that even where neither applies there is nothing to see — the strip is
/// the same colour as the app behind it.
///
/// Re-applied on every layout pass rather than once: the window does not exist
/// when the view is first made, and going in and out of full screen re-creates
/// enough of the chrome to be worth being blunt about.
private struct WindowConfigurator: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        DispatchQueue.main.async { configure(view.window) }
        return view
    }

    func updateNSView(_ view: NSView, context: Context) {
        DispatchQueue.main.async { configure(view.window) }
    }

    /// One colour object for the window's lifetime. `updateNSView` runs on
    /// every observed change — ten a second — and assigning a *new* dynamic
    /// `NSColor` each time dirtied the window just to repaint it the same
    /// colour. A cached instance makes every re-apply below a compare-and-skip.
    @MainActor private static let canvas = NSColor(
        name: NSColor.Name("mcpgw.canvas")
    ) { appearance in
        NSColor(hex: appearance.isDark ? 0x0608_0B : 0xF1F4_F9)
    }

    private func configure(_ window: NSWindow?) {
        guard let window else { return }
        if !window.styleMask.contains(.fullSizeContentView) {
            window.styleMask.insert(.fullSizeContentView)
        }
        if !window.titlebarAppearsTransparent {
            window.titlebarAppearsTransparent = true
        }
        // The `Window` scene installs an `NSToolbar` even with nothing in it,
        // and this OS draws an empty toolbar as a floating glass pill beside
        // the traffic lights — a control-shaped object that controls nothing.
        // Clearing the object is the only thing that removes it; the title bar,
        // and with it the traffic lights, is separate and stays.
        if window.toolbar != nil {
            window.toolbar = nil
        }
        if window.backgroundColor !== Self.canvas {
            window.backgroundColor = Self.canvas
        }
    }
}

/// The update affordance, at the top of the sidebar's footer.
///
/// Drawn only when there is something to act on. A control that is always
/// present but usually inert teaches people to stop looking at it, which is the
/// opposite of what an update notice is for.
///
/// A background check that fails resolves to `.idle`, so `.failed` here only
/// ever follows something the user asked for — an explicit check, or an install
/// this build has no signing key for. Either way it needs somewhere to land
/// rather than the notice silently vanishing.
private struct UpdateRow: View {
    @Environment(AgentModel.self) private var model

    var body: some View {
        switch model.updater.state {
        case let .available(release):
            Button {
                Task { await model.updater.requestUpdate(release) }
            } label: {
                SidebarActionLabel(
                    icon: "arrow.down.circle.fill",
                    title: "Update available",
                    tint: Palette.beam,
                    iconTint: Palette.beam
                )
            }
            .buttonStyle(.plain)
            .help("Version \(release.version) is available. Click to download it.")

        case let .downloading(progress):
            progressRow(label: "Downloading…") {
                ProgressView(value: progress).progressViewStyle(.circular).controlSize(.mini)
            }
            .help("Downloading and verifying the update…")

        case let .readyToInstall(release):
            // The download already happened — "Not now" on the restart ask
            // lands here, and the row waits with the bundle staged.
            Button {
                Task { await model.updater.requestUpdate(release) }
            } label: {
                SidebarActionLabel(
                    icon: "arrow.down.circle.fill",
                    title: "Update now",
                    tint: Palette.beam,
                    iconTint: Palette.beam
                )
            }
            .buttonStyle(.plain)
            .help("Version \(release.version) is downloaded. Installing quits and reopens the app.")

        case .readyToRelaunch:
            progressRow(label: "Relaunching…") {
                ProgressView().progressViewStyle(.circular).controlSize(.mini)
            }

        case let .failed(message):
            SidebarActionLabel(
                icon: "exclamationmark.triangle",
                title: "Update failed",
                tint: Palette.warn,
                iconTint: Palette.warn
            )
            .help(message)

        case .idle, .checking, .upToDate:
            EmptyView()
        }
    }

    private func progressRow(label: String, @ViewBuilder spinner: () -> some View) -> some View {
        HStack(spacing: 9) {
            spinner().frame(width: 19)
            Text(label)
                .font(.system(size: Typo.body))
                .foregroundStyle(Palette.text3)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 9)
        .frame(height: 30)
    }
}

// ── Launch failure ──────────────────────────────────────────────────────

private struct LaunchFailureView: View {
    let message: String

    var body: some View {
        VStack(spacing: 14) {
            Image(systemName: "exclamationmark.octagon")
                .font(.system(size: Typo.display, weight: .light))
                .foregroundStyle(Palette.deny)
            Text("The agent could not start")
                .font(.system(size: Typo.medium, weight: .semibold))
            Text(message)
                .font(.system(size: Typo.small))
                .foregroundStyle(Palette.text3)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 380)
            Button("Quit") { NSApplication.shared.terminate(nil) }
                .buttonStyle(.glass)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
