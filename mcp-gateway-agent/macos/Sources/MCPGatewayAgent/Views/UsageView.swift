import SwiftUI

/// Who calls what, through this Mac.
///
/// Four figures and the flow board they summarise. There used to be two more
/// cards under the board — a "Busiest tools" bar chart and an "Applications"
/// list — and both drew, from the same payload, exactly what the Tools and
/// Applications columns of the board already showed, one screen further down.
/// A second view of the same numbers is not a second piece of information; the
/// space went to the figures the page genuinely lacked.
struct UsageView: View {
    @Environment(AgentModel.self) private var model
    @Environment(\.controlActiveState) private var activeState

    @State private var graph: UsageGraph?
    /// Memoised, for the same reason the Audit page memoises its filter: `body`
    /// reruns on every core tick, ten times a second, because it reads the
    /// snapshot — and building this sorts a hundred tools into groups.
    @State private var flow: FlowModel?
    @State private var range = "7d"
    @State private var failure: String?
    @State private var loading = false
    /// Whether the Tools column is showing everything or just the busiest.
    @State private var expanded = false

    private static let pollInterval = Duration.seconds(30)

    private static let ranges = [("24h", "24 hours"), ("7d", "7 days"), ("30d", "30 days")]

    private var rangeLabel: String {
        Self.ranges.first { $0.0 == range }?.1 ?? range
    }

    /// The page is pinned to the width it is given, for the same reason the
    /// Audit page is pinned to its height.
    ///
    /// The flow board sized its columns from a measurement of itself, and asked
    /// for those widths as a hard minimum. That made the board's width a demand
    /// rather than a response: the card could not be narrower, so the page could
    /// not, so the window could not — and the measurement that would have let it
    /// shrink could never happen, because the thing being measured was the thing
    /// refusing to shrink. Dragging the window out and back in left the content
    /// at its old width with the window smaller around it, which pushed the
    /// sidebar off the left edge and the range picker off the right.
    ///
    /// The board itself now asks for its widths as maximums (see
    /// `UsageFlowBoard`); this is the belt to that pair of braces.
    var body: some View {
        GeometryReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: Metrics.gutter) {
                    header
                    if let failure, flow != nil {
                        // A refresh that failed while there is still a graph on
                        // screen is a banner, not a page: the numbers are stale,
                        // not gone. With nothing drawn yet it becomes the page
                        // itself, below.
                        InlineBanner(text: failure) { self.failure = nil }
                    }
                    content
                }
                .padding(Metrics.pagePadding)
                .padding(.top, 16)  // clears the traffic lights and sidebar toggle when the sidebar is hidden
                .frame(width: proxy.size.width, alignment: .topLeading)
            }
            .scrollBounceBehavior(.basedOnSize)
        }
        .task(id: model.backendId) { await poll() }
        .onChange(of: expanded) { _, _ in rebuild() }
        // The local servers are half the input — they are what turns a namespace
        // into a backend — so the board has to follow them, but comparing the
        // list itself would compare every tool on every one of them ten times a
        // second. The names and their health are the only parts that change what
        // gets drawn.
        .onChange(of: backendFingerprint) { _, _ in rebuild() }
    }

    private var backendFingerprint: String {
        model.backends.map { $0.name + ":" + $0.status.rawValue }.joined(separator: ",")
    }

    // ── Sections ────────────────────────────────────────────────────────

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            PageTitle(
                title: "Usage",
                subtitle: model.account?.scopeDescription ?? "Routed calls"
            )
            Spacer(minLength: 12)
            // A fixed slot, so the picker does not step sideways every time a
            // poll starts and finishes.
            ProgressView()
                .controlSize(.small)
                .opacity(loading ? 1 : 0)
                .frame(width: 16)
            Picker("Range", selection: $range) {
                ForEach(Self.ranges, id: \.0) { value, label in
                    Text(label).tag(value)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            .onChange(of: range) { _, _ in Task { await refresh() } }
        }
    }

    @ViewBuilder
    private var content: some View {
        if model.backendId == nil {
            Card {
                EmptyState(
                    icon: "arrow.triangle.branch",
                    title: "Not registered with the gateway yet",
                    message: "Usage is filtered by this machine's gateway id, which it receives "
                        + "when the tunnel connects."
                )
            }
        } else if let flow {
            summary(flow)
            board(flow)
        } else if let failure {
            // The first load failed. Without this the page sat on a spinner for
            // ever with the reason banished to a banner above it.
            Card {
                EmptyState(
                    icon: "exclamationmark.triangle",
                    title: "Could not load usage",
                    message: failure,
                    action: ("Try again", { Task { await refresh() } })
                )
            }
        } else {
            Card {
                ProgressView()
                    .controlSize(.small)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 40)
            }
        }
    }

    /// The supporting tier: what the board shows, counted. The figures the page
    /// had none of — it opened straight into the graph, and the only numbers on
    /// it were the per-node ones.
    private func summary(_ flow: FlowModel) -> some View {
        Card {
            HStack(alignment: .top, spacing: 24) {
                Stat(value: Format.count(flow.totalCalls), label: "Calls · \(rangeLabel)")
                Stat(value: "\(flow.appCount)", label: "Applications")
                Stat(value: "\(flow.backendCount)", label: "Backends")
                // A pair, like Overview's "Backends up" — a used count on its
                // own means nothing without the manifest it came out of, and a
                // detail line under one of four stats leaves the row ragged.
                Stat(value: "\(flow.activeTools)/\(flow.toolCount)", label: "Tools used")
            }
        }
        .fixedSize(horizontal: false, vertical: true)
    }

    private func board(_ flow: FlowModel) -> some View {
        Card {
            if flow.isEmpty {
                EmptyState(
                    icon: "arrow.triangle.branch",
                    title: "Nothing has called this machine in this range",
                    message: "Once an app makes a tool call through the gateway it shows here."
                )
            } else {
                UsageFlowBoard(model: flow, expanded: $expanded, note: "Hover to trace")
            }
        }
    }

    // ── Data ────────────────────────────────────────────────────────────

    /// The gateway's payload plus what only this app knows: the local servers
    /// behind this machine's single gateway backend. They are what makes the
    /// Backends column a column of real servers rather than a second copy of
    /// This Mac — see `ToolName.split`.
    private func rebuild() {
        guard let graph else {
            flow = nil
            return
        }
        flow = FlowModel.build(
            FlowInput(
                agent: model.connection?.agentId ?? "",
                apps: graph.applications.map {
                    .init(name: $0.application, callCount: $0.callCount, isLive: $0.isConnected)
                },
                tools: graph.tools.map {
                    .init(name: $0.toolName, risk: $0.riskCategory, callCount: $0.callCount)
                },
                known: model.backends.map {
                    .init(name: $0.name, tone: $0.status.tone, status: $0.status.label.lowercased())
                },
                appTools: graph.appToTool.map {
                    .init(app: $0.source, tool: $0.target, callCount: $0.callCount)
                }
            ),
            toolLimit: expanded ? Int.max : FlowModel.defaultToolLimit
        )
    }

    // ── Polling ─────────────────────────────────────────────────────────

    private func poll() async {
        await refresh()
        while !Task.isCancelled {
            try? await Task.sleep(for: Self.pollInterval)
            if Task.isCancelled { return }
            guard activeState != .inactive else { continue }
            await refresh()
        }
    }

    private func refresh() async {
        guard let api = model.gatewayAPI() else {
            // Registered, but with no way to ask. Saying so beats a spinner that
            // never resolves.
            if model.backendId != nil {
                failure = "This Mac has no gateway credentials. Sign in again in Settings."
            }
            return
        }
        loading = true
        defer { loading = false }
        do {
            // Flicking across the three ranges leaves several requests in
            // flight, and they do not come back in the order they were sent: a
            // slow 30-day query landing after a quick 24-hour one would leave
            // the board showing a month while the picker said a day.
            let requested = range
            let result = try await api.usageGraph(range: requested)
            guard requested == range else { return }
            graph = result
            rebuild()
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
