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
    @State private var page: Page = .overview

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
                // The window chrome sits opposite the traffic lights, which is
                // where a Mac app's toolbar actions live and where people
                // already look for one.
                .overlay(alignment: .topTrailing) {
                    UpdateChip()
                        .padding(.top, 13)
                        .padding(.trailing, 16)
                }
        }
        .navigationSplitViewStyle(.balanced)
    }

    @ViewBuilder
    private var detail: some View {
        switch page {
        case .overview: OverviewView(page: $page)
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
    @Binding var page: Page

    /// `List` drives single selection through a `Binding<SelectionValue?>`.
    /// Handing it the non-optional `Binding<Page>` compiles, and then selects
    /// nothing, ever: every row in this sidebar was inert and the detail pane
    /// only ever showed Overview. Project the page into an optional here, and
    /// drop a nil write, which is what a click on empty sidebar space sends, so
    /// the detail pane can never be left with nothing to render.
    private var selection: Binding<Page?> {
        Binding(get: { page }, set: { if let new = $0 { page = new } })
    }

    var body: some View {
        List(selection: selection) {
            Section {
                ForEach(Page.allCases) { item in
                    Label(item.title, systemImage: item.icon)
                        .tag(item)
                        .badge(badge(for: item))
                }
            } header: {
                Text("Agent")
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
    private var header: some View {
        BrandLockup(size: 20, subtitle: model.connection?.agentId ?? "—")
            .padding(.horizontal, 12)
            .padding(.top, 38)
            .padding(.bottom, 12)
    }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 8) {
            Divider()
            if let connection = model.connection {
                HStack(spacing: 7) {
                    StatusLabel(
                        tone: connection.state.tone,
                        text: connection.state.label,
                        pulsing: connection.state == .connecting || connection.state == .reconnecting
                    )
                    Spacer(minLength: 0)
                    if let account = model.account {
                        Text(account.username)
                            .font(.system(size: Typo.micro))
                            .foregroundStyle(Palette.text4)
                            .lineLimit(1)
                    }
                }
                .padding(.horizontal, 14)
            }
            HStack {
                SettingsLink {
                    Label("Settings", systemImage: "gearshape")
                        .font(.system(size: Typo.caption))
                }
                .buttonStyle(.plain)
                .foregroundStyle(Palette.text3)
                Spacer()
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
                Image(systemName: "arrow.down.circle")
                    .font(.system(size: Typo.body, weight: .medium))
                    .foregroundStyle(Palette.beam)
            }
            .buttonStyle(.glass)
            .controlSize(.small)
            .help("Version \(release.version) is available. Updating relaunches the app.")

        case let .downloading(progress):
            ProgressView(value: progress)
                .progressViewStyle(.circular)
                .controlSize(.small)
                .help("Downloading the update…")

        case .readyToRelaunch:
            ProgressView()
                .progressViewStyle(.circular)
                .controlSize(.small)
                .help("Relaunching…")

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
