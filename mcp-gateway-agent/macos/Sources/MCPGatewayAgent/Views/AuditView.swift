import Charts
import SwiftUI

/// The gateway's audit trail for this machine.
///
/// The ledger is the page — the summary above it is context, and it is sized
/// like context. It used to be a hundred-point row of four stat cards *and* a
/// hundred-and-seventy-five point card holding a full-axis bar chart, which
/// together took more of an 800pt window than the events did.
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
    /// Memoised. `body` reruns on every core tick — ten times a second — and
    /// this page holds up to two thousand events; filtering them in a computed
    /// property is twenty thousand predicate evaluations a second to redraw
    /// rows that did not change.
    @State private var filtered: [AuditEvent] = []
    @State private var stats: AuditStats?
    @State private var total = 0
    @State private var failure: String?
    @State private var loading = false
    @State private var loadingOlder = false
    /// The server has nothing older left to give.
    @State private var exhausted = false
    @State private var selected: AuditEvent?
    @State private var query = ""
    @State private var statusFilter = "all"
    @State private var tableWidth: CGFloat = 0

    private static let pollInterval = Duration.seconds(10)
    private static let pageSize = 200
    /// Ten pages. Past this, "load older" is the wrong tool and the dashboard's
    /// export is the right one.
    private static let maxEvents = 2_000

    var body: some View {
        VStack(alignment: .leading, spacing: Metrics.gutter) {
            header
            if let failure, !events.isEmpty {
                InlineBanner(text: failure) { self.failure = nil }
            }
            content
        }
        .padding(Metrics.pagePadding)
        .padding(.top, 22)  // clears the floating traffic lights
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .task(id: model.backendId) { await poll() }
        .onChange(of: query) { _, _ in refilter() }
        .onChange(of: statusFilter) { _, _ in refilter() }
        .sheet(item: $selected) { AuditDetail(event: $0) }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            PageTitle(
                title: "Audit",
                subtitle: model.account?.scopeDescription ?? "Calls to this machine"
            )
            Spacer(minLength: 12)
            ProgressView()
                .controlSize(.small)
                .opacity(loading ? 1 : 0)
                .frame(width: 16)
            Button {
                Task { await refresh(reset: true) }
            } label: {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .buttonStyle(.glass)
            .controlSize(.small)
            .disabled(loading)
        }
    }

    @ViewBuilder
    private var content: some View {
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
        } else if events.isEmpty, stats == nil, let failure {
            // Nothing arrived and nothing is on screen: the reason is the page.
            Card {
                EmptyState(
                    icon: "exclamationmark.triangle",
                    title: "Could not load the audit trail",
                    message: failure,
                    action: ("Try again", { Task { await refresh(reset: true) } })
                )
            }
        } else {
            summary
            filters
            table
        }
    }

    // ── Summary ─────────────────────────────────────────────────────────

    /// Four figures and the shape of the last day, in one band.
    ///
    /// `fixedSize` vertically is what keeps it a band. The cards inside a row
    /// ask for `maxHeight: .infinity` so they match each other, and this page —
    /// unlike every other one here — is a plain `VStack` filling the window
    /// rather than a `ScrollView`, so "as tall as you can be" is answered
    /// literally and a hundred-point row grows to six hundred.
    @ViewBuilder
    private var summary: some View {
        if let stats {
            Card {
                HStack(alignment: .top, spacing: 20) {
                    Stat(value: Format.count(stats.events24h), label: "Calls · 24h")
                    Stat(
                        value: Format.percent(stats.errorRate),
                        label: "Error rate",
                        tint: stats.errorRate > 0.05 ? Palette.deny : Palette.text
                    )
                    Stat(value: Format.duration(stats.avgDurationMs), label: "Latency")
                    Stat(
                        value: Format.count(stats.deniedCount),
                        label: "Denied",
                        tint: stats.deniedCount > 0 ? Palette.warn : Palette.text
                    )
                    volume(stats)
                }
            }
            .fixedSize(horizontal: false, vertical: true)
        }
    }

    /// A strip, not a chart, and the fifth thing in the row rather than a band
    /// of its own.
    ///
    /// A y-axis, gridlines and six-hourly tick labels cost a hundred and
    /// seventy-five points of window to answer one question — "was there a
    /// spike" — that thirty-six points of bar answers just as well. The scale it
    /// loses with the axis comes back as the peak, in words, on the label. Its
    /// label sits where the other four have theirs, so it reads as one more
    /// figure and not as a chart that wandered in.
    @ViewBuilder
    private func volume(_ stats: AuditStats) -> some View {
        if !stats.hourlyVolume.isEmpty {
            let peak = stats.hourlyVolume.map(\.count).max() ?? 0
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    FieldLabel("Volume · 24h")
                    Spacer(minLength: 4)
                    Text("peak \(peak)/h")
                        .font(.system(size: Typo.micro))
                        .monospacedDigit()
                        .foregroundStyle(Palette.text4)
                }
                Chart(stats.hourlyVolume) { point in
                    BarMark(
                        x: .value("Hour", point.hour, unit: .hour),
                        y: .value("Calls", point.count)
                    )
                    .foregroundStyle(Palette.beam)
                    .cornerRadius(1.5)
                }
                // The bar covering the newest hour needs the scale to run past
                // it, or it is drawn half outside the plot and clipped.
                .chartXScale(domain: volumeStart...volumeEnd)
                .chartXAxis(.hidden)
                .chartYAxis(.hidden)
                .chartPlotStyle { $0.padding(.zero) }
                .frame(height: 36)
            }
            .frame(width: 200)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Calls per hour over the last day, peaking at \(peak) an hour")
        }
    }

    private var volumeStart: Date { stats?.hourlyVolume.first?.hour ?? Date() }

    /// One hour past the last point, so the newest bar is drawn in full.
    private var volumeEnd: Date {
        (stats?.hourlyVolume.last?.hour ?? Date()).addingTimeInterval(3600)
    }

    // ── Filters ─────────────────────────────────────────────────────────

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
            .fixedSize()

            SearchField(text: $query, prompt: "Filter by tool or application")
                .frame(maxWidth: 280)
            Spacer(minLength: 8)
            Text(countLine)
                .font(.system(size: Typo.caption))
                .monospacedDigit()
                .foregroundStyle(Palette.text3)
                .lineLimit(1)
                .help(
                    events.count < total
                        ? "Filters apply to the events loaded so far. Load older events to search "
                            + "further back."
                        : "Every event the gateway holds for this machine is loaded."
                )
        }
    }

    /// Honest about what the filter searched.
    ///
    /// This read "12 of 4,213" while the filter had only ever seen the hundred
    /// events in memory — so a tool that had not been called recently came back
    /// "nothing matches" against a total that said there were four thousand
    /// events to match against.
    private var countLine: String {
        guard events.count < total else {
            return "\(filtered.count) of \(Format.count(total))"
        }
        return "\(filtered.count) of \(events.count) loaded · \(Format.count(total)) total"
    }

    // ── Table ───────────────────────────────────────────────────────────

    private var table: some View {
        Card(padding: 0) {
            VStack(spacing: 0) {
                AuditHeaderRow(compact: compact)
                if filtered.isEmpty {
                    EmptyState(
                        icon: "checklist",
                        title: events.isEmpty
                            ? "No audit events yet" : "Nothing matches those filters",
                        message: events.isEmpty
                            ? "Calls the gateway routes to this Mac are recorded here."
                            : nil
                    )
                    .frame(maxHeight: .infinity)
                } else {
                    List(filtered) { event in
                        AuditRow(event: event, compact: compact)
                            .listRowInsets(EdgeInsets(top: 3, leading: 12, bottom: 3, trailing: 12))
                            .listRowSeparator(.hidden)
                            .contentShape(.rect)
                            .onTapGesture { selected = event }
                    }
                    .listStyle(.plain)
                    .scrollContentBackground(.hidden)
                }
                olderFooter
            }
        }
        // The one flexible child of the page, so it takes whatever the header,
        // the summary and the filters leave. The floor is well under what is
        // left at the window's minimum height (about 250 points), so it never
        // forces the page to overflow — it only stops the table collapsing to
        // nothing if something above it ever grows.
        .frame(minHeight: 160, maxHeight: .infinity)
        // Measured in the background so the reader cannot claim the height —
        // see the same note on `UsageFlowBoard`.
        .background {
            GeometryReader { proxy in
                Color.clear
                    .onAppear { tableWidth = proxy.size.width }
                    .onChange(of: proxy.size.width) { _, new in tableWidth = new }
            }
        }
    }

    /// Narrow enough that a column has to go.
    private var compact: Bool {
        tableWidth > 0 && tableWidth < AuditColumn.wideEnoughForApplication
    }

    /// Either the way to more, or — when the cap has been reached — a line
    /// saying that is as far as this page goes. A view that quietly stops
    /// growing reads as a view that has shown you everything.
    @ViewBuilder
    private var olderFooter: some View {
        let capped = events.count >= Self.maxEvents
        if !exhausted, events.count < total {
            Divider().overlay(Palette.lineSoft)
            HStack(spacing: 8) {
                if capped {
                    Text(
                        "Showing the \(Format.count(Self.maxEvents)) most recent of "
                            + "\(Format.count(total)). Export the rest from the dashboard."
                    )
                    .font(.system(size: Typo.micro))
                    .foregroundStyle(Palette.text4)
                } else {
                    Button {
                        Task { await loadOlder() }
                    } label: {
                        Text(loadingOlder ? "Loading…" : "Load older")
                    }
                    .buttonStyle(.glass)
                    .controlSize(.small)
                    .disabled(loadingOlder)
                    Text("\(Format.count(total - events.count)) older events")
                        .font(.system(size: Typo.micro))
                        .monospacedDigit()
                        .foregroundStyle(Palette.text4)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 9)
        }
    }

    // ── Filtering ───────────────────────────────────────────────────────

    private func refilter() {
        let needle = query.trimmingCharacters(in: .whitespaces)
        filtered = events.filter { event in
            switch statusFilter {
            case "error" where !event.isError: return false
            case "denied" where !event.isDenied: return false
            case "success" where event.status != "success": return false
            default: break
            }
            guard !needle.isEmpty else { return true }
            return event.toolName.localizedCaseInsensitiveContains(needle)
                || (event.application?.localizedCaseInsensitiveContains(needle) ?? false)
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
        guard let api = model.gatewayAPI() else {
            if model.backendId != nil {
                failure = "This Mac has no gateway credentials. Sign in again in Settings."
            }
            return
        }
        loading = true
        defer { loading = false }

        do {
            // Incremental: ask only for what happened since the newest event
            // already on screen.
            let since = reset ? nil : events.first?.timestamp
            let page = try await api.auditEvents(since: since, limit: Self.pageSize)

            if reset {
                events = page.events
                exhausted = page.events.count < Self.pageSize
                refilter()
            } else if !page.events.isEmpty {
                merge(page.events)
            }
            total = page.total
            // Last, and after the rows are already on screen: the summary is
            // context. A stats call that fails should not take the ledger with
            // it, which is what happened when the two were fetched together and
            // the whole `do` block unwound.
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

    /// Another page, older than everything held.
    ///
    /// Paged by timestamp rather than by `offset`: rows are arriving at the top
    /// the whole time this page is open, and every one of them shifts an
    /// offset-based window by one, which duplicates a row for each new event.
    private func loadOlder() async {
        guard !loadingOlder, let api = model.gatewayAPI(), let oldest = events.last?.timestamp
        else { return }
        loadingOlder = true
        defer { loadingOlder = false }

        do {
            let page = try await api.auditEvents(before: oldest, limit: Self.pageSize)
            let added = merge(page.events)
            total = page.total
            // `before` is inclusive, so the oldest row always comes back. Adding
            // nothing new is the only reliable signal that there is no more.
            if added == 0 { exhausted = true }
            failure = nil
        } catch is CancellationError {
            return
        } catch let error as URLError where error.code == .cancelled {
            return
        } catch {
            failure = error.localizedDescription
        }
    }

    /// Returns how many were genuinely new.
    @discardableResult
    private func merge(_ incoming: [AuditEvent]) -> Int {
        let known = Set(events.map(\.eventId))
        let fresh = incoming.filter { !known.contains($0.eventId) }
        guard !fresh.isEmpty else { return 0 }
        events.append(contentsOf: fresh)
        events.sort { $0.timestamp > $1.timestamp }
        if events.count > Self.maxEvents { events.removeLast(events.count - Self.maxEvents) }
        refilter()
        return fresh.count
    }
}

private struct AuditDetail: View {
    @Environment(\.dismiss) private var dismiss
    let event: AuditEvent

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(event.toolName)
                        .font(Typo.mono(Typo.body, weight: .semibold))
                        .lineLimit(2)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                    Text(Format.dateTime(event.timestamp))
                        .font(.system(size: Typo.caption))
                        .foregroundStyle(Palette.text3)
                }
                Spacer(minLength: 12)
                Button("Done") { dismiss() }
                    .buttonStyle(.glass)
                    .keyboardShortcut(.defaultAction)
            }
            .padding(18)
            Divider()

            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    row("Status", event.status)
                    row("Backend", event.backendName.isEmpty ? "—" : event.backendName)
                    row("Application", event.application ?? "—")
                    row("Duration", Format.duration(event.durationMs))
                    row("Risk", event.riskCategory ?? "unclassified")
                    row("Policy", event.policyDecision ?? "—")
                    row("Trace", event.traceId.isEmpty ? "—" : event.traceId)
                    row("Event", event.eventId)
                    if let message = event.errorMessage {
                        VStack(alignment: .leading, spacing: 5) {
                            SectionHeader("Error")
                            Text(message)
                                .font(Typo.mono(Typo.caption))
                                .foregroundStyle(Palette.deny)
                                .textSelection(.enabled)
                                .fixedSize(horizontal: false, vertical: true)
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
                    .fixedSize(horizontal: false, vertical: true)
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
                .font(Typo.mono(Typo.small))
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }
    }
}
