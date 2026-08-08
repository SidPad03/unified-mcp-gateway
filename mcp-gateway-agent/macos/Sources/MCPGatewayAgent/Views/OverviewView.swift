import SwiftUI

/// What you want to know in the first second: is it connected, how much is
/// flowing, is anything broken.
///
/// Everything here is local state the core already holds, so it paints instantly
/// and never waits on the network.
struct OverviewView: View {
    @Environment(AgentModel.self) private var model
    @Environment(\.controlActiveState) private var activeState
    @Binding var page: Page

    /// The most recent calls, newest first. Memoised rather than derived in
    /// `body`: the snapshot ticks ten times a second and `body` reads it, so
    /// reversing and slicing a thousand records in there is work nobody sees.
    @State private var recent: [ToolCall] = []
    /// Which errored call is showing its message.
    @State private var expanded: String?

    /// Enough to answer "is anything happening", not so many that the card
    /// becomes a second Audit page. The full history is one click away.
    private static let recentLimit = 8

    /// Uptime is a clock, and this page had nothing driving it.
    ///
    /// Every other figure here is a counter that only moves when a call
    /// arrives, so refreshing on events was enough for them. The snapshot was
    /// refreshed at launch, at sign-in and after an action, and never in
    /// between, which left the uptime frozen at whatever it read when the page
    /// was built: it appeared to advance only when you navigated away and came
    /// back, which reads as broken rather than as slow.
    ///
    /// A second is affordable because none of this is a network call. The core
    /// already holds every number on this page, so a poll is one local IPC
    /// round trip, and it stops entirely when the window loses focus.
    private static let refreshInterval = Duration.seconds(1)

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Metrics.gutter) {
                PageTitle(
                    title: "Overview",
                    subtitle: "This Mac's local MCP servers, and the gateway they sit behind."
                )
                .padding(.bottom, 2)

                if let error = model.lastError {
                    InlineBanner(text: error) { model.lastError = nil }
                }

                hero
                activity
            }
            .padding(Metrics.pagePadding)
            .padding(.top, 16)  // clears the traffic lights and sidebar toggle when the sidebar is hidden
        }
        .scrollBounceBehavior(.basedOnSize)
        .onAppear(perform: rebuildRecent)
        .onChange(of: model.callRevision) { _, _ in rebuildRecent() }
        .task { await keepCurrent() }
    }

    private func rebuildRecent() {
        recent = Array(model.toolCalls.reversed().prefix(Self.recentLimit))
    }

    // ── Hero ────────────────────────────────────────────────────────────
    //
    // One figure leads and the rest drop to a supporting tier. Four equal stat
    // cards in four colours — which is what this was — made you read all four
    // before learning anything, and none of them answered "is it working".

    private var hero: some View {
        Card {
            VStack(alignment: .leading, spacing: 15) {
                if let connection = model.connection {
                    HStack(alignment: .top, spacing: 12) {
                        VStack(alignment: .leading, spacing: 5) {
                            HStack(spacing: 8) {
                                StatusDot(
                                    tone: connection.state.tone,
                                    pulsing: connection.state == .connecting
                                        || connection.state == .reconnecting
                                )
                                Text(connection.state.label)
                                    .font(.system(size: Typo.large, weight: .semibold))
                                    .tracking(-0.3)
                                    .foregroundStyle(connection.state.tone.color)
                            }
                            Mono(subtitle(for: connection), size: Typo.caption, color: Palette.text3)
                                .lineLimit(1)
                                .truncationMode(.middle)
                                .textSelection(.enabled)
                        }
                        Spacer(minLength: 12)
                        actions
                    }

                    if let error = connection.readableError, connection.state != .connected {
                        Text(error)
                            .font(.system(size: Typo.caption))
                            .foregroundStyle(Palette.deny)
                            .lineLimit(3)
                            .fixedSize(horizontal: false, vertical: true)
                            .textSelection(.enabled)
                    }

                    Divider().overlay(Palette.lineSoft)

                    HStack(alignment: .top, spacing: 24) {
                        Stat(
                            value: Format.count(model.stats?.callsTotal ?? 0),
                            label: "Calls routed",
                            detail: "since launch"
                        )

                        Stat(value: "\(model.stats?.toolsRegistered ?? 0)", label: "Tools")
                        Stat(
                            value:
                                "\(model.stats?.backendsReady ?? 0)/\(model.stats?.backendsTotal ?? 0)",
                            label: "Backends up",
                            tint: backendTone.color
                        )
                        Stat(
                            value: Format.count(model.stats?.callsErrors ?? 0),
                            label: "Errors",
                            tint: (model.stats?.callsErrors ?? 0) > 0 ? Palette.deny : Palette.text
                        )
                        Stat(value: Format.uptime(model.snapshot?.uptimeSecs), label: "Uptime")
                    }
                } else {
                    ProgressView().controlSize(.small)
                }
            }
        }
    }

    private func subtitle(for connection: ConnectionStatus) -> String {
        switch connection.state {
        case .reconnecting:
            let retry = connection.retryInMs.map { " · retrying in \($0 / 1000)s" } ?? ""
            return "Attempt \(connection.attempt)\(retry)"
        case .idle:
            return "Sign in to connect this Mac"
        default:
            return connection.gatewayUrl
        }
    }

    private var actions: some View {
        HStack(spacing: 8) {
            Button("Reconnect") { Task { await model.reconnect() } }
                .buttonStyle(.glass)
            Button("Re-register") { Task { await model.reregister() } }
                .buttonStyle(.glass)
                .disabled(model.connection?.state != .connected)
            Button {
                model.openDashboard()
            } label: {
                Label("Dashboard", systemImage: "arrow.up.right.square")
            }
            .buttonStyle(.glass)
            .disabled(model.apiBaseURL == nil)
        }
        .controlSize(.small)
    }

    private var backendTone: Tone {
        guard let stats = model.stats, stats.backendsTotal > 0 else { return .neutral }
        if stats.backendsReady == stats.backendsTotal { return .ok }
        return stats.backendsReady == 0 ? .deny : .warn
    }

    /// Cancelled with the view, and idle while the window is in the background:
    /// a Mac that is not being looked at has no reason to be counting.
    private func keepCurrent() async {
        while !Task.isCancelled {
            try? await Task.sleep(for: Self.refreshInterval)
            if Task.isCancelled { return }
            guard activeState != .inactive else { continue }
            await model.refreshSnapshot()
        }
    }

    // ── Activity ────────────────────────────────────────────────────────

    /// What the gateway is routing here, live.
    ///
    /// This was its own page in the sidebar, listing every call since launch.
    /// It sat two rows above Audit — the same list, kept by the gateway and not
    /// discarded at every restart — so the app had two pages of tool calls, the
    /// shorter of which was empty most of the time. "Is anything happening right
    /// now" is an Overview question; "what happened" is an Audit one.
    private var activity: some View {
        Card(padding: 0) {
            VStack(alignment: .leading, spacing: 0) {
                SectionHeader("Recent activity") {
                    Button("Audit trail") { page = .audit }
                        .buttonStyle(.plain)
                        .font(.system(size: Typo.caption, weight: .medium))
                        .foregroundStyle(Palette.beam)
                }
                .padding(.horizontal, Metrics.cardPadding)
                .padding(.top, Metrics.cardPadding)
                .padding(.bottom, 10)

                if recent.isEmpty {
                    // Not "no tool calls yet" — that read as "this machine has
                    // never been used" on a machine with a full audit trail.
                    // This list is an in-memory buffer that starts empty at
                    // every launch, and the page has to say so.
                    EmptyState(
                        icon: "bolt.horizontal",
                        title: "No calls since the app started",
                        message: "Calls the gateway routes here appear as they run. "
                            + "The gateway keeps the full history.",
                        action: ("Open the audit trail", { page = .audit })
                    )
                } else {
                    columnHeader
                    VStack(spacing: 0) {
                        ForEach(recent) { call in
                            CallRow(call: call, expanded: expanded == call.requestId)
                                .padding(.horizontal, 12)
                                .padding(.vertical, 3)
                                .contentShape(.rect)
                                .onTapGesture {
                                    guard call.error != nil else { return }
                                    withAnimation(.snappy(duration: 0.18)) {
                                        expanded = expanded == call.requestId ? nil : call.requestId
                                    }
                                }
                        }
                    }
                    .padding(.bottom, 8)
                }
            }
        }
    }

    private var columnHeader: some View {
        HStack(spacing: 10) {
            Text("Time").frame(width: 74, alignment: .leading)
            Text("Tool").frame(maxWidth: .infinity, alignment: .leading)
            Text("Backend").frame(width: 110, alignment: .leading)
            Text("Duration").frame(width: 74, alignment: .trailing)
            Text("Status").frame(width: 68, alignment: .trailing)
        }
        .font(.system(size: Typo.micro, weight: .medium))
        .tracking(0.8)
        .foregroundStyle(Palette.text3)
        .padding(.horizontal, 12 + Metrics.rail + 8)
        .padding(.bottom, 6)
        .accessibilityHidden(true)
    }
}

// The update notice used to be a card here. It now lives in the sidebar's
// footer (`UpdateRow` in RootView), so it is reachable from every page rather
// than only this one, and it stops competing with the hero for the one focal
// slot this view is allowed.
