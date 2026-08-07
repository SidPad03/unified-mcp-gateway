import Charts
import SwiftUI

/// What you want to know in the first second: is it connected, how much is
/// flowing, is anything broken.
///
/// Everything here is local state the core already holds, so it paints instantly
/// and never waits on the network.
struct OverviewView: View {
    @Environment(AgentModel.self) private var model
    @Binding var page: Page

    @State private var buckets: [Bucket] = []

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
                HStack(alignment: .top, spacing: Metrics.gutter) {
                    sparkline
                    troubleOrTools
                }
            }
            .padding(Metrics.pagePadding)
            .padding(.top, 22)  // clears the floating traffic lights
        }
        .scrollBounceBehavior(.basedOnSize)
        .onAppear(perform: rebuildBuckets)
        .onChange(of: model.callRevision) { _, _ in rebuildBuckets() }
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

    // ── Sparkline ───────────────────────────────────────────────────────

    /// Local calls, bucketed by hour. The core's ring buffer only goes back as
    /// far as the app has been running, so this is honestly labelled rather
    /// than pretending to be a 24-hour history.
    private var sparkline: some View {
        Card(fillsHeight: true) {
            VStack(alignment: .leading, spacing: 12) {
                SectionHeader("Calls per hour")
                if buckets.allSatisfy({ $0.count == 0 }) {
                    EmptyState(
                        icon: "chart.bar",
                        title: "No tool calls yet",
                        message: "Calls routed through this Mac show up here."
                    )
                } else {
                    Chart(buckets) { bucket in
                        BarMark(
                            x: .value("Hour", bucket.hour, unit: .hour),
                            y: .value("Calls", bucket.count)
                        )
                        .foregroundStyle(Palette.beam)
                        .cornerRadius(1.5)
                    }
                    .chartYAxis {
                        AxisMarks(position: .leading) {
                            AxisGridLine().foregroundStyle(Palette.lineSoft)
                            AxisValueLabel().font(.system(size: Typo.micro))
                        }
                    }
                    .chartXAxis {
                        AxisMarks(values: .stride(by: .hour, count: 6)) {
                            AxisValueLabel().font(.system(size: Typo.micro))
                        }
                    }
                    .frame(height: 150)
                }
            }
        }
    }

    struct Bucket: Identifiable {
        let hour: Date
        let count: Int
        var id: Date { hour }
    }

    /// Rebuilt when the calls change, not on every render — bucketing a thousand
    /// records ten times a second to redraw twenty-four bars is work nobody
    /// sees.
    private func rebuildBuckets() {
        let calendar = Calendar.current
        guard let start = calendar.date(byAdding: .hour, value: -23, to: Date()) else {
            buckets = []
            return
        }
        let startOfHour = calendar.dateInterval(of: .hour, for: start)?.start ?? start

        var counts: [Date: Int] = [:]
        for call in model.toolCalls where call.startedAt >= startOfHour {
            let hour = calendar.dateInterval(of: .hour, for: call.startedAt)?.start ?? call.startedAt
            counts[hour, default: 0] += 1
        }
        buckets = (0..<24).compactMap { offset in
            guard let hour = calendar.date(byAdding: .hour, value: offset, to: startOfHour) else {
                return nil
            }
            return Bucket(hour: hour, count: counts[hour] ?? 0)
        }
    }

    // ── Right column ────────────────────────────────────────────────────

    /// Broken backends if there are any, the inventory if there are not. A panel
    /// that always says "everything fine" trains people to stop reading it.
    @ViewBuilder
    private var troubleOrTools: some View {
        let trouble = model.backends.filter { $0.status == .failed || $0.status == .crashed }
        let shown = trouble.isEmpty ? model.backends : trouble

        Card(padding: 0, fillsHeight: true) {
            VStack(alignment: .leading, spacing: 0) {
                SectionHeader(trouble.isEmpty ? "Backends" : "Needs attention") {
                    Button("Manage") { page = .backends }
                        .buttonStyle(.plain)
                        .font(.system(size: Typo.caption, weight: .medium))
                        .foregroundStyle(Palette.beam)
                }
                .padding(.horizontal, Metrics.cardPadding)
                .padding(.top, Metrics.cardPadding)
                .padding(.bottom, 10)

                if model.backends.isEmpty {
                    EmptyState(
                        icon: "server.rack",
                        title: "No local MCP servers yet",
                        message: "Add one to put this Mac's tools behind the gateway.",
                        action: ("Add backend", { page = .backends })
                    )
                } else {
                    // The gate rail: a column of green with a red notch in it is
                    // readable before any of the words are.
                    VStack(spacing: 0) {
                        ForEach(Array(shown.enumerated()), id: \.element.id) { index, backend in
                            RailRow(tone: backend.status.tone, isLast: index == shown.count - 1) {
                                VStack(alignment: .leading, spacing: 2) {
                                    Mono(backend.name, size: Typo.small, weight: .medium)
                                    Text(backend.error ?? "\(backend.toolCount) tools")
                                        .font(.system(size: Typo.micro))
                                        .foregroundStyle(
                                            backend.error == nil ? Palette.text4 : Palette.deny
                                        )
                                        .lineLimit(1)
                                }
                            } trailing: {
                                Text(backend.status.label)
                                    .font(.system(size: Typo.micro, weight: .medium))
                                    .foregroundStyle(backend.status.tone.color)
                            }
                        }
                    }
                }
            }
        }
        .frame(width: 320)
    }
}

// The update notice used to be a card here. It now lives in the window's
// top-right chrome (`UpdateChip` in RootView), so it is reachable from every
// page rather than only this one, and it stops competing with the hero for the
// one focal slot this view is allowed.
