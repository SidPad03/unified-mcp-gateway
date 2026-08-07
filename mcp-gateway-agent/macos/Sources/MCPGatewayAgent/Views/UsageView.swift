import Charts
import SwiftUI

/// Who calls what, through this Mac.
///
/// The dashboard draws this with a force-directed graph library. Here it is four
/// ordered columns — application → this agent → local backend → tool — with the
/// edges drawn behind them. For a strictly layered graph that is a better fit
/// than a physics simulation: the layout is stable between refreshes, so the
/// node you were reading does not move, and it scales down to a window rather
/// than needing a canvas to pan around.
struct UsageView: View {
    @Environment(AgentModel.self) private var model
    @Environment(\.controlActiveState) private var activeState

    @State private var graph: UsageGraph?
    @State private var range = "7d"
    @State private var failure: String?
    @State private var loading = false
    @State private var highlighted: String?

    private static let pollInterval = Duration.seconds(30)

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Metrics.gutter) {
                header
                if let failure {
                    InlineBanner(text: failure) { self.failure = nil }
                }

                if model.backendId == nil {
                    Card {
                        EmptyState(
                            icon: "arrow.triangle.branch",
                            title: "Not registered with the gateway yet",
                            message: "Usage is filtered by this machine's gateway id."
                        )
                    }
                } else if let graph {
                    GraphBoard(graph: graph, agentId: agentLabel, highlighted: $highlighted)
                    charts(graph)
                } else {
                    Card { ProgressView().frame(maxWidth: .infinity).padding(30) }
                }
            }
            .padding(24)
            .padding(.top, 22)
        }
        .task(id: model.backendId) { await poll() }
    }

    private var agentLabel: String {
        model.connection?.agentId ?? "this Mac"
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline) {
            PageTitle(
                title: "Usage",
                subtitle: model.account?.scopeDescription ?? "Calls to this machine"
            )
            Spacer()
            if loading { ProgressView().controlSize(.small) }
            Picker("Range", selection: $range) {
                Text("24 hours").tag("24h")
                Text("7 days").tag("7d")
                Text("30 days").tag("30d")
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 260)
            .onChange(of: range) { _, _ in Task { await refresh() } }
        }
    }

    @ViewBuilder
    private func charts(_ graph: UsageGraph) -> some View {
        HStack(alignment: .top, spacing: Metrics.gutter) {
            Card {
                VStack(alignment: .leading, spacing: 10) {
                    SectionHeader("Busiest tools")
                    let top = graph.tools.filter { $0.callCount > 0 }
                        .sorted { $0.callCount > $1.callCount }
                        .prefix(8)
                    if top.isEmpty {
                        EmptyState(icon: "chart.bar", title: "No calls in this range")
                    } else {
                        Chart(Array(top)) { tool in
                            BarMark(
                                x: .value("Calls", tool.callCount),
                                y: .value("Tool", tool.toolName)
                            )
                            .foregroundStyle(Palette.beam.gradient)
                            .cornerRadius(3)
                        }
                        .chartXAxis { AxisMarks(position: .bottom) }
                        .frame(height: CGFloat(top.count) * 26 + 20)
                    }
                }
            }

            Card {
                VStack(alignment: .leading, spacing: 10) {
                    SectionHeader("Applications")
                    if graph.applications.isEmpty {
                        EmptyState(icon: "app.dashed", title: "No applications in this range")
                    } else {
                        ForEach(graph.applications.sorted { $0.callCount > $1.callCount }) { app in
                            HStack(spacing: 9) {
                                StatusDot(tint: app.isConnected ? Palette.beam : Palette.text3)
                                Text(app.application)
                                    .font(.system(size: Typo.small))
                                Spacer(minLength: 8)
                                Text(Format.count(app.callCount))
                                    .font(.system(size: Typo.caption, design: .monospaced))
                                    .foregroundStyle(Palette.text3)
                            }
                            .padding(.vertical, 3)
                        }
                    }
                }
            }
            .frame(width: 320)
        }
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
        guard let api = model.gatewayAPI() else { return }
        loading = true
        defer { loading = false }
        do {
            graph = try await api.usageGraph(range: range)
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

// ── The graph ───────────────────────────────────────────────────────────

/// Anchor of one node, so edges can be drawn between columns.
private struct NodeAnchors: PreferenceKey {
    static let defaultValue: [String: Anchor<CGRect>] = [:]

    static func reduce(
        value: inout [String: Anchor<CGRect>],
        nextValue: () -> [String: Anchor<CGRect>]
    ) {
        value.merge(nextValue()) { _, new in new }
    }
}

private struct GraphBoard: View {
    let graph: UsageGraph
    let agentId: String
    @Binding var highlighted: String?

    /// `app → agent`, `agent → backend`, `backend → tool`.
    ///
    /// The gateway sees this machine as one backend; the sub-backends behind it
    /// are a local concept, so that hop is grouped here from the tool names
    /// rather than asked for.
    private var edges: [(String, String, Int)] {
        var result: [(String, String, Int)] = []
        for edge in graph.appToBackend {
            result.append(("app:" + edge.source, "agent", edge.callCount))
        }
        for tool in graph.tools {
            let backend = Self.namespace(of: tool.toolName)
            result.append(("agent", "backend:" + backend, tool.callCount))
            result.append(("backend:" + backend, "tool:" + tool.toolName, tool.callCount))
        }
        return result
    }

    private var backends: [(name: String, tools: [UsageGraph.ToolNode])] {
        Dictionary(grouping: graph.tools) { Self.namespace(of: $0.toolName) }
            .map { (name: $0.key, tools: $0.value.sorted { $0.callCount > $1.callCount }) }
            .sorted { $0.name < $1.name }
    }

    static func namespace(of tool: String) -> String {
        guard let range = tool.range(of: "__") else { return "direct" }
        return String(tool[tool.startIndex..<range.lowerBound])
    }

    var body: some View {
        Card {
            VStack(alignment: .leading, spacing: 14) {
                SectionHeader("Application → Agent → Backend → Tool")

                if graph.tools.isEmpty && graph.applications.isEmpty {
                    EmptyState(
                        icon: "arrow.triangle.branch",
                        title: "Nothing has called this machine yet",
                        message: "Once an app makes a tool call through the gateway it shows here."
                    )
                } else {
                    HStack(alignment: .top, spacing: 46) {
                        column("Applications") {
                            ForEach(graph.applications) { app in
                                node(
                                    id: "app:" + app.application,
                                    title: app.application,
                                    detail: "\(Format.count(app.callCount)) calls",
                                    tint: app.isConnected ? Palette.beam : Palette.text3
                                )
                            }
                        }
                        column("This Mac") {
                            node(
                                id: "agent",
                                title: agentId,
                                detail: "\(graph.tools.count) tools",
                                tint: Palette.beam
                            )
                        }
                        column("Backends") {
                            ForEach(backends, id: \.name) { backend in
                                node(
                                    id: "backend:" + backend.name,
                                    title: backend.name,
                                    detail: "\(backend.tools.count) tools",
                                    tint: Palette.text2
                                )
                            }
                        }
                        column("Tools") {
                            ForEach(graph.tools.sorted { $0.callCount > $1.callCount }.prefix(14)) {
                                tool in
                                node(
                                    id: "tool:" + tool.toolName,
                                    title: Self.bare(tool.toolName),
                                    detail: "\(Format.count(tool.callCount))",
                                    tint: tool.riskCategory == "destructive"
                                        ? Palette.deny : Palette.text3
                                )
                            }
                        }
                    }
                    .backgroundPreferenceValue(NodeAnchors.self) { anchors in
                        GeometryReader { proxy in
                            Canvas { context, _ in
                                for (from, to, count) in edges {
                                    guard let a = anchors[from], let b = anchors[to] else { continue }
                                    let start = proxy[a]
                                    let end = proxy[b]
                                    var path = Path()
                                    let p1 = CGPoint(x: start.maxX, y: start.midY)
                                    let p2 = CGPoint(x: end.minX, y: end.midY)
                                    let dx = (p2.x - p1.x) * 0.5
                                    path.move(to: p1)
                                    path.addCurve(
                                        to: p2,
                                        control1: CGPoint(x: p1.x + dx, y: p1.y),
                                        control2: CGPoint(x: p2.x - dx, y: p2.y)
                                    )
                                    let live = count > 0
                                    let dim = highlighted != nil
                                        && highlighted != from && highlighted != to
                                    context.stroke(
                                        path,
                                        with: .color(
                                            Palette.beam.opacity(dim ? 0.05 : (live ? 0.4 : 0.14))
                                        ),
                                        lineWidth: live ? 1.4 : 1
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    private func column<Content: View>(
        _ title: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            Text(title.uppercased())
                .font(.system(size: Typo.micro, weight: .semibold))
                .tracking(1.1)
                .foregroundStyle(Palette.text3)
                .padding(.bottom, 2)
            content()
        }
        .frame(width: 168, alignment: .leading)
    }

    private func node(id: String, title: String, detail: String, tint: Color) -> some View {
        HStack(spacing: 7) {
            StatusDot(tint: tint)
            VStack(alignment: .leading, spacing: 1) {
                Text(title)
                    .font(.system(size: Typo.caption, weight: .medium))
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(detail)
                    .font(.system(size: Typo.micro))
                    .foregroundStyle(Palette.text3)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 9)
        .padding(.vertical, 7)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            .quaternary.opacity(highlighted == id ? 0.8 : 0.4),
            in: .rect(cornerRadius: Radius.row)
        )
        .anchorPreference(key: NodeAnchors.self, value: .bounds) { [id: $0] }
        .onHover { hovering in highlighted = hovering ? id : nil }
    }

    static func bare(_ tool: String) -> String {
        guard let range = tool.range(of: "__") else { return tool }
        return String(tool[range.upperBound...])
    }
}
