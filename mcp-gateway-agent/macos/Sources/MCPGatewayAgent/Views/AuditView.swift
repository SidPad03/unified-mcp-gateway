import Charts
import SwiftUI

/// The gateway's audit trail for this machine.
///
/// Polling rules, because this is the one page that talks to a server:
/// it refreshes **only while visible and focused** — `.task` is cancelled when
/// the view goes away, and `controlActiveState` stops the loop when the window
/// loses focus — and each poll is incremental, asking only for events newer than
/// the newest one already held.
struct AuditView: View {
    @Environment(AgentModel.self) private var model
    @Environment(\.controlActiveState) private var activeState

    @State private var events: [AuditEvent] = []
    @State private var stats: AuditStats?
    @State private var total = 0
    @State private var failure: String?
    @State private var loading = false
    @State private var selected: AuditEvent?
    @State private var query = ""
    @State private var statusFilter = "all"

    private static let pollInterval = Duration.seconds(10)

    private var filtered: [AuditEvent] {
        events.filter { event in
            switch statusFilter {
            case "error" where !event.isError: return false
            case "denied" where !event.isDenied: return false
            case "success" where event.status != "success": return false
            default: break
            }
            guard !query.isEmpty else { return true }
            return event.toolName.localizedCaseInsensitiveContains(query)
                || (event.application?.localizedCaseInsensitiveContains(query) ?? false)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Metrics.gutter) {
            header
            if let failure {
                InlineBanner(text: failure) { self.failure = nil }
            }
            if model.backendId == nil {
                Card {
                    EmptyState(
                        icon: "checklist",
                        title: "Not registered with the gateway yet",
                        message:
                            "Audit events are filtered by this machine's gateway id, which it "
                            + "receives when the tunnel connects."
                    )
                }
            } else {
                summary
                filters
                table
            }
            Spacer(minLength: 0)
        }
        .padding(24)
        .padding(.top, 22)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .task(id: model.backendId) { await poll() }
        .sheet(item: $selected) { AuditDetail(event: $0) }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline) {
            PageTitle(
                title: "Audit",
                subtitle: model.account?.scopeDescription ?? "Calls to this machine"
            )
            Spacer()
            if loading {
                ProgressView().controlSize(.small)
            }
            Button {
                Task { await refresh(reset: true) }
            } label: {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .buttonStyle(.glass)
            .controlSize(.small)
        }
    }

    @ViewBuilder
    private var summary: some View {
        if let stats {
            HStack(spacing: Metrics.gutter) {
                Card(fillsHeight: true) { Stat(value: Format.count(stats.events24h), label: "Calls · 24h") }
                Card(fillsHeight: true) {
                    Stat(
                        value: Format.percent(stats.errorRate),
                        label: "Error rate",
                        tint: stats.errorRate > 0.05 ? Palette.deny : Palette.text
                    )
                }
                Card(fillsHeight: true) {
                    Stat(value: Format.duration(stats.avgDurationMs), label: "Average latency")
                }
                Card(fillsHeight: true) { Stat(value: Format.count(stats.deniedCount), label: "Denied by policy") }
            }

            if !stats.hourlyVolume.isEmpty {
                Card {
                    VStack(alignment: .leading, spacing: 10) {
                        SectionHeader("Call volume · 24h")
                        Chart(stats.hourlyVolume) { point in
                            BarMark(
                                x: .value("Hour", point.hour, unit: .hour),
                                y: .value("Calls", point.count)
                            )
                            .foregroundStyle(Palette.beam.gradient)
                            .cornerRadius(2)
                        }
                        .chartYAxis { AxisMarks(position: .leading) }
                        .frame(height: 110)
                    }
                }
            }
        }
    }

    private var filters: some View {
        HStack(spacing: 10) {
            Picker("Status", selection: $statusFilter) {
                Text("All").tag("all")
                Text("Success").tag("success")
                Text("Errors").tag("error")
                Text("Denied").tag("denied")
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 300)

            SearchField(text: $query, prompt: "Filter by tool or application")
                .frame(maxWidth: 280)
            Spacer()
            Text("\(filtered.count) of \(total)")
                .font(.system(size: Typo.caption))
                .foregroundStyle(Palette.text3)
        }
    }

    private var table: some View {
        Card(padding: 0) {
            if filtered.isEmpty {
                EmptyState(
                    icon: "checklist",
                    title: events.isEmpty ? "No audit events yet" : "Nothing matches those filters",
                    message: events.isEmpty
                        ? "Calls the gateway routes to this Mac are recorded here."
                        : nil
                )
            } else {
                List(filtered) { event in
                    AuditRow(event: event)
                        .listRowInsets(EdgeInsets(top: 3, leading: 12, bottom: 3, trailing: 12))
                        .listRowSeparator(.hidden)
                        .contentShape(.rect)
                        .onTapGesture { selected = event }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .frame(minHeight: 240)
            }
        }
    }

    // ── Polling ─────────────────────────────────────────────────────────

    private func poll() async {
        await refresh(reset: true)
        while !Task.isCancelled {
            try? await Task.sleep(for: Self.pollInterval)
            if Task.isCancelled { return }
            // A background window should not be asking the gateway anything.
            guard activeState != .inactive else { continue }
            await refresh(reset: false)
        }
    }

    private func refresh(reset: Bool) async {
        guard let api = model.gatewayAPI() else { return }
        loading = true
        defer { loading = false }

        do {
            // Incremental: ask only for what happened since the newest event
            // already on screen.
            let since = reset ? nil : events.first?.timestamp
            let page = try await api.auditEvents(since: since)

            if reset {
                events = page.events
            } else if !page.events.isEmpty {
                let known = Set(events.map(\.eventId))
                events.insert(contentsOf: page.events.filter { !known.contains($0.eventId) }, at: 0)
                events = Array(events.prefix(500))
            }
            total = page.total
            stats = try await api.auditStats()
            failure = nil
        } catch is CancellationError {
            return
        } catch let error as URLError where error.code == .cancelled {
            return
        } catch {
            failure = error.localizedDescription
        }
    }
}

// ── Rows ────────────────────────────────────────────────────────────────

private struct AuditRow: View {
    let event: AuditEvent

    var body: some View {
        HStack(spacing: 10) {
            Text(Format.time(event.timestamp))
                .font(.system(size: Typo.caption, design: .monospaced))
                .foregroundStyle(Palette.text3)
                .frame(width: 74, alignment: .leading)

            Text(event.toolName)
                .font(.system(size: Typo.small, design: .monospaced))
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: .infinity, alignment: .leading)

            Text(event.application ?? "—")
                .font(.system(size: Typo.caption))
                .foregroundStyle(Palette.text3)
                .frame(width: 120, alignment: .leading)
                .lineLimit(1)

            if let risk = event.riskCategory {
                RiskBadge(risk: risk)
                    .frame(width: 84, alignment: .leading)
            } else {
                Spacer().frame(width: 84)
            }

            Text(Format.duration(event.durationMs))
                .font(.system(size: Typo.caption, design: .monospaced))
                .foregroundStyle(Palette.text3)
                .frame(width: 70, alignment: .trailing)

            Text(event.status)
                .font(.system(size: Typo.micro, weight: .medium))
                .foregroundStyle(tone.color)
                .frame(width: 74, alignment: .trailing)
        }
        .padding(.vertical, 3)
        // The ledger's spine. Red means broken, amber means the policy stopped
        // it — a denial is the gateway working, not failing, so it must not
        // read the same as an upstream error.
        .railed(tone)
    }

    private var tone: Tone {
        if event.isError { return .deny }
        if event.isDenied { return .warn }
        return .ok
    }

    private var tint: Color { tone.color }
}

private struct AuditDetail: View {
    @Environment(\.dismiss) private var dismiss
    let event: AuditEvent

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(event.toolName)
                        .font(.system(size: Typo.body, weight: .semibold, design: .monospaced))
                    Text(Format.dateTime(event.timestamp))
                        .font(.system(size: Typo.caption))
                        .foregroundStyle(Palette.text3)
                }
                Spacer()
                Button("Done") { dismiss() }
                    .buttonStyle(.glass)
                    .keyboardShortcut(.defaultAction)
            }
            .padding(18)
            Divider()

            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    row("Status", event.status)
                    row("Backend", event.backendName)
                    row("Application", event.application ?? "—")
                    row("Duration", Format.duration(event.durationMs))
                    row("Risk", event.riskCategory ?? "—")
                    row("Policy", event.policyDecision ?? "—")
                    row("Trace", event.traceId)
                    row("Event", event.eventId)
                    if let message = event.errorMessage {
                        VStack(alignment: .leading, spacing: 5) {
                            SectionHeader("Error")
                            Text(message)
                                .font(.system(size: Typo.caption, design: .monospaced))
                                .foregroundStyle(Palette.deny)
                                .textSelection(.enabled)
                                .padding(10)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .background(
                                    Palette.deny.opacity(0.1), in: .rect(cornerRadius: Radius.row))
                        }
                    }
                    Text(
                        "Arguments and responses are never stored: the gateway keeps hashes so a "
                            + "call can be correlated without retaining its contents."
                    )
                    .font(.system(size: Typo.caption))
                    .foregroundStyle(Palette.text3)
                }
                .padding(18)
            }
        }
        .frame(width: 520, height: 520)
        .background(Palette.canvas)
    }

    private func row(_ label: String, _ value: String) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Text(label.uppercased())
                .font(.system(size: Typo.micro, weight: .medium))
                .tracking(0.9)
                .foregroundStyle(Palette.text3)
                .frame(width: 92, alignment: .leading)
            Text(value)
                .font(.system(size: Typo.small, design: .monospaced))
                .textSelection(.enabled)
            Spacer(minLength: 0)
        }
    }
}
