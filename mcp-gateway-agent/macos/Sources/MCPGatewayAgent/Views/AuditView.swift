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
    /// When the summary band was last fetched — see the note in `refresh`.
    @State private var statsFetched: Date?
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

    /// The page is handed exactly the space the window has, and is never allowed
    /// to ask for more.
    ///
    /// This is the fix for the page that broke the window, and it is deliberately
    /// a structural guarantee rather than another attempt to find the one child
    /// that was demanding too much. This page is the only one with a summary
    /// band *and* a filter bar *and* a ledger stacked in a single non-scrolling
    /// column, so it is the only one where the children's minimum heights add up
    /// to more than a small window has. SwiftUI's answer to "your minimum is
    /// bigger than the offer" is to lay out at the minimum and overflow — and
    /// because `.windowResizability(.contentSize)` derives the window's minimum
    /// from that same number, the overflow was drawn *outside* the window: the
    /// sidebar's brand lockup, the page title and the Refresh button were all
    /// above the top edge, and dragging the window bigger or smaller never
    /// changed it, because the demand was a constant.
    ///
    /// `GeometryReader` accepts any proposal and reports it, so the page's own
    /// minimum becomes zero; the explicit `frame(width:height:)` then forces the
    /// column into that space. The children can all absorb it — the ledger is a
    /// `ScrollView` and everything above it is a fixed-height band.
    var body: some View {
        GeometryReader { proxy in
            VStack(alignment: .leading, spacing: Metrics.gutter) {
                header
                if let failure, !events.isEmpty {
                    InlineBanner(text: failure) { self.failure = nil }
                }
                content
            }
            .padding(Metrics.pagePadding)
            .padding(.top, 16)  // clears the traffic lights and sidebar toggle when the sidebar is hidden
            .frame(width: proxy.size.width, height: proxy.size.height, alignment: .topLeading)
        }
        .task(id: model.backendId) { await poll() }
        .onChange(of: query) { _, _ in refilter() }
        .onChange(of: statusFilter) { _, _ in refilter() }
        .sheet(item: $selected) { AuditDetail(event: $0) }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            PageTitle(
                title: "Audit",
                subtitle: model.account?.scopeDescription ?? "Routed calls"
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

    /// Four figures in one band, all describing **the same events the table
    /// below describes**.
    ///
    /// The headline used to be `events_24h` while the error rate, the latency
    /// and the denial count were all-time — figures in one row across two
    /// different windows. On a machine that had been quiet for a day it read
    /// "Calls · 24h: 0" directly above a ledger listing hundreds of events, and
    /// on a busy one it read 15 above a ledger that said 147. Either way the
    /// band contradicted the thing it was summarising.
    ///
    /// `fixedSize` vertically is what keeps it a band rather than a panel: the
    /// `Card` inside would otherwise answer "as tall as you can be" literally,
    /// because this page fills the window rather than scrolling.
    @ViewBuilder
    private var summary: some View {
        if let stats {
            Card {
                // 24, and no trailing spacer — the same row as Overview's,
                // Backends' and Usage's. Each `Stat` already asks for
                // `maxWidth: .infinity`, so four of them divide the card into
                // even columns; a `Spacer` on the end takes that width back and
                // bunches them against the left, which is what made this one
                // band look unlike every other page.
                HStack(alignment: .top, spacing: 24) {
                    Stat(value: Format.count(stats.totalEvents), label: "Calls")
                    Stat(
                        value: Format.percent(stats.errorRate),
                        label: "Error rate",
                        tint: stats.errorRate > 0.05 ? Palette.deny : Palette.text
                    )
                    // `avg_duration_ms` is `AVG(duration_ms)`, and the gateway
                    // sends 0 when there is nothing to average. "0 ms" is a
                    // measurement; this is the absence of one.
                    Stat(
                        value: stats.totalEvents == 0
                            ? "—" : Format.duration(stats.avgDurationMs),
                        label: "Latency"
                    )
                    Stat(
                        value: Format.count(stats.deniedCount),
                        label: "Denied",
                        tint: stats.deniedCount > 0 ? Palette.warn : Palette.text
                    )
                    // A fifth slot held a 24-hour volume strip. It has been
                    // removed: it answered one question — "was there a spike" —
                    // that the ledger underneath answers directly, it was the
                    // only thing in the band on a different time window from the
                    // rest, and it was the one element that could not survive a
                    // narrow window without truncating its own caption.
                }
            }
            .fixedSize(horizontal: false, vertical: true)
        }
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

    /// The ledger.
    ///
    /// `ScrollView` + `LazyVStack`, not `List`, and that is the whole reason
    /// this page used to break the window.
    ///
    /// A macOS `List` reports a **minimum** height of several hundred points —
    /// it is an `NSTableView` underneath and it will not be squeezed. This page
    /// is the one page whose ledger is a sibling of a summary band and a filter
    /// bar rather than the only thing under the title, so those minimums add up:
    /// the page asked for about 950 points no matter what it was offered. That
    /// number then became the window's minimum content height, by way of
    /// `.windowResizability(.contentSize)` — and because the window had already
    /// been opened at the size it was last left at, the content simply
    /// overflowed it, top and bottom, taking the sidebar and the page header off
    /// the screen with it. Resizing did nothing, because the demand was a
    /// constant.
    ///
    /// A `ScrollView` has a minimum height of zero: it scrolls instead of
    /// insisting. Nothing is lost by the swap — the rows already draw their own
    /// separators and their own rail, and selection is a sheet rather than a
    /// list selection, so `List` was providing nothing this page used.
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
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ScrollView {
                        // Lazy, so two thousand events cost a viewport of rows.
                        // One subview per element — no `if` inside the row — or
                        // the stack has to keep the off-screen ones alive.
                        LazyVStack(spacing: 0) {
                            ForEach(filtered) { event in
                                AuditRow(event: event, compact: compact)
                                    .padding(.horizontal, 12)
                                    .padding(.vertical, 3)
                                    .contentShape(.rect)
                                    .onTapGesture { selected = event }
                            }
                        }
                    }
                    .scrollBounceBehavior(.basedOnSize)
                }
                olderFooter
            }
        }
        // The one flexible child of the page: it takes whatever the header, the
        // summary and the filters leave, and it can be given nothing at all
        // without the page overflowing.
        .frame(maxHeight: .infinity)
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
            } else if page.events.count == 1, page.events[0].eventId == events.first?.eventId {
                // The inclusive `since` bound hands the newest row back on
                // every poll, so "one event, and it is already the top of the
                // ledger" is the quiet-gateway case — not worth the id-set and
                // re-sort that `merge` does.
            } else if !page.events.isEmpty {
                merge(page.events)
            }
            total = page.total
            // Last, and after the rows are already on screen: the summary is
            // context. A stats call that fails should not take the ledger with
            // it, which is what happened when the two were fetched together and
            // the whole `do` block unwound.
            //
            // Once a minute, not once a poll. The events request is
            // incremental; this one is a COUNT and an AVG over the whole audit
            // table, and issuing it six times a minute to redraw four numbers
            // that barely move was the most expensive query the app sent.
            if reset || statsFetched.map({ Date.now.timeIntervalSince($0) > 60 }) ?? true {
                stats = try await api.auditStats()
                statsFetched = .now
            }
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
