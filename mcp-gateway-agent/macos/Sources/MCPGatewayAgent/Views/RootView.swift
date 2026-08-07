import SwiftUI

enum Page: String, CaseIterable, Identifiable, Hashable {
    case overview, backends, activity, logs, audit, usage

    var id: String { rawValue }

    var title: String {
        switch self {
        case .overview: "Overview"
        case .backends: "Backends"
        case .activity: "Activity"
        case .logs: "Logs"
        case .audit: "Audit"
        case .usage: "Usage"
        }
    }

    var icon: String {
        switch self {
        case .overview: "gauge.with.dots.needle.33percent"
        case .backends: "server.rack"
        case .activity: "bolt.horizontal"
        case .logs: "text.alignleft"
        case .audit: "checklist"
        case .usage: "arrow.triangle.branch"
        }
    }

    /// Server-backed pages need a registered agent before they have anything to
    /// ask about.
    var needsGateway: Bool {
        self == .audit || self == .usage
    }
}

struct RootView: View {
    @Environment(AgentModel.self) private var model

    /// Optional, because that is the type `List` selection actually has.
    ///
    /// This was a non-optional `Page` projected into a `Binding<Page?>` at the
    /// sidebar. That compiled, shipped, and still selected nothing: the rows
    /// were plain tagged labels, so the list had a selection binding but no
    /// selectable controls to drive it, and the projection quietly papered over
    /// the type mismatch that would have pointed at it. Holding the real type
    /// here lets the sidebar bind straight to it.
    @State private var page: Page? = .overview

    /// The detail pane always has something to draw, and the pages that
    /// navigate — Overview's "Manage" and "Add backend" — get a plain `Page` to
    /// write back to.
    private var current: Binding<Page> {
        Binding(get: { page ?? .overview }, set: { page = $0 })
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

    private var main: some View {
        NavigationSplitView {
            Sidebar(page: $page)
                .navigationSplitViewColumnWidth(Metrics.sidebarWidth)
        } detail: {
            detail
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                .background(Palette.canvas)
        }
        .navigationSplitViewStyle(.balanced)
        // On the window, not on the detail pane.
        //
        // Hung off the detail column these sat inside the page, level with
        // whatever that page put in its own top-right — the Audit page's Refresh
        // button lands exactly there — so the two crowded each other and the
        // corner stopped reading as the window's. Out here it is the window's
        // top-right corner and nothing else can claim it.
        //
        // This is the far side of the title bar from the traffic lights, which
        // is where a Mac keeps window-level state, and that is what these are:
        // the connection describes the window, not the page.
        .overlay(alignment: .topTrailing) {
            HStack(spacing: 10) {
                ConnectionChip()
                UpdateChip()
            }
            .padding(.top, 11)
            .padding(.trailing, 14)
        }
    }

    @ViewBuilder
    private var detail: some View {
        switch page ?? .overview {
        case .overview: OverviewView(page: current)
        case .backends: BackendsView()
        case .activity: ActivityView()
        case .logs: LogsView()
        case .audit: AuditView()
        case .usage: UsageView()
        }
    }
}

// ── Sidebar ─────────────────────────────────────────────────────────────

private struct Sidebar: View {
    @Environment(AgentModel.self) private var model
    @Binding var page: Page?

    /// The failed-backend count is drawn in the row, not attached with
    /// `.badge()`, and that is load-bearing rather than cosmetic.
    ///
    /// `.badge()` on a row in a `.sidebar`-styled `List` destroys the list's
    /// selection: the backing table reports no selected row at all, so nothing
    /// highlights, nothing takes hover, and clicking does nothing. Every form of
    /// it does this — `Int`, a non-zero `Int`, an optional `Text` that is nil
    /// when there is nothing to show — and it happens whether the row is a
    /// `NavigationLink` or a `Label` carrying a `.tag`. That is the whole reason
    /// this sidebar was inert; the earlier suspect, the shape of the selection
    /// binding, was never it, which is why correcting the binding changed
    /// nothing. Drawing the count as ordinary content leaves selection alone.
    ///
    /// `NavigationLink(value:)` is kept because it is what `NavigationSplitView`
    /// documents for its sidebar column and it makes the row a real control,
    /// but a tagged `Label` also works once the badge is gone.
    var body: some View {
        List(selection: $page) {
            Section("Agent") {
                ForEach(Page.allCases) { item in
                    NavigationLink(value: item) {
                        HStack(spacing: 0) {
                            Label(item.title, systemImage: item.icon)
                            let trouble = badge(for: item)
                            if trouble > 0 {
                                Spacer(minLength: 8)
                                Text("\(trouble)")
                                    .font(.system(size: Typo.micro, weight: .semibold))
                                    .monospacedDigit()
                                    .foregroundStyle(Palette.deny)
                                    .accessibilityLabel("\(trouble) not running")
                            }
                        }
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .top, spacing: 0) { header }
        .safeAreaInset(edge: .bottom, spacing: 0) { footer }
    }

    /// The traffic lights float over this, which is why it carries the top
    /// inset. Navigation itself stays a native `List` — a Mac sidebar's
    /// selection is an OS-drawn glass capsule, and replacing it with a web-style
    /// rail would fight the platform for no gain. The gate rail earns its keep
    /// on the *data* here: backends, activity, logs and audit rows all carry it.
    /// No subtitle. It carried this Mac's own hostname, which is the one fact a
    /// person running the app on their own machine already has, printed under
    /// the product name where the eye goes first.
    ///
    /// The top inset clears the traffic lights, which float over this column
    /// because the title bar is hidden. They finish 33pt down, and the lockup
    /// used to start at 38 — five points, with the mark sitting directly to
    /// their right, so the two ran together and the corner read as a jumble.
    /// The lockup is 20pt tall, so this leaves it clearly below them rather
    /// than beside them.
    private var header: some View {
        BrandLockup(size: 20)
            .padding(.horizontal, 12)
            .padding(.top, 52)
            .padding(.bottom, 12)
    }

    /// The account, beside the way to change it. Connection state moved to the
    /// window corner: down here it was the boldest thing in the column and read
    /// as a heading for the Settings row under it.
    private var footer: some View {
        VStack(alignment: .leading, spacing: 8) {
            Divider()
            HStack {
                SettingsLink {
                    Label("Settings", systemImage: "gearshape")
                        .font(.system(size: Typo.caption))
                }
                .buttonStyle(.plain)
                .foregroundStyle(Palette.text3)
                Spacer(minLength: 8)
                if let account = model.account {
                    Text(account.username)
                        .font(.system(size: Typo.micro))
                        .foregroundStyle(Palette.text4)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }
            .padding(.horizontal, 14)
            .padding(.bottom, 10)
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

// ── Window chrome ───────────────────────────────────────────────────────

/// Whether the gateway is reachable, in the corner of the window.
///
/// Deliberately the quietest thing on screen while it says "Connected": that is
/// the state it is in nearly all the time, and a status light you have learned
/// to ignore is worse than none. The dot keeps its tone so the colour is still
/// there to be found, and the word takes the tone only when the news is not
/// good, which is the only time it should pull the eye.
private struct ConnectionChip: View {
    @Environment(AgentModel.self) private var model

    var body: some View {
        if let connection = model.connection {
            let state = connection.state
            let settled = state == .connected
            HStack(spacing: 5) {
                StatusDot(
                    tone: state.tone,
                    pulsing: state == .connecting || state == .reconnecting
                )
                Text(state.label)
                    .font(.system(size: Typo.micro, weight: .medium))
                    .foregroundStyle(settled ? Palette.text4 : state.tone.color)
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel("Gateway \(state.label)")
            .help(connection.readableError ?? "Gateway \(state.label.lowercased()) as \(connection.agentId).")
        }
    }
}

/// The update affordance, and the only thing in the top-right chrome.
///
/// Drawn only when there is something to act on. A control that is always
/// present but usually inert teaches people to stop looking at it, which is the
/// opposite of what an update notice is for.
///
/// A background check that fails resolves to `.idle`, so `.failed` here only
/// ever follows something the user asked for — an explicit check, or an install
/// this build has no signing key for. Either way it needs somewhere to land
/// rather than the icon silently vanishing.
private struct UpdateChip: View {
    @Environment(AgentModel.self) private var model

    var body: some View {
        switch model.updater.state {
        case let .available(release):
            Button {
                Task { await model.updater.install(release) }
            } label: {
                // Labelled, not icon-only. A lone glyph in the corner of a
                // window reads as a status light rather than a control, and
                // this is the one thing up here that does something: it is
                // worth a word.
                Label("Update", systemImage: "arrow.down.circle")
                    .font(.system(size: Typo.caption, weight: .medium))
                    .foregroundStyle(Palette.beam)
            }
            .buttonStyle(.glass)
            .controlSize(.small)
            .help("Version \(release.version) is available. Installing quits and reopens the app.")

        case let .downloading(progress):
            HStack(spacing: 6) {
                ProgressView(value: progress).progressViewStyle(.circular).controlSize(.small)
                Text("Updating…")
                    .font(.system(size: Typo.caption))
                    .foregroundStyle(Palette.text3)
            }
            .help("Downloading and verifying the update…")

        case .readyToRelaunch:
            HStack(spacing: 6) {
                ProgressView().progressViewStyle(.circular).controlSize(.small)
                Text("Relaunching…")
                    .font(.system(size: Typo.caption))
                    .foregroundStyle(Palette.text3)
            }

        case let .failed(message):
            Image(systemName: "exclamationmark.triangle")
                .font(.system(size: Typo.body, weight: .medium))
                .foregroundStyle(Palette.warn)
                .help(message)

        case .idle, .checking, .upToDate:
            EmptyView()
        }
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
