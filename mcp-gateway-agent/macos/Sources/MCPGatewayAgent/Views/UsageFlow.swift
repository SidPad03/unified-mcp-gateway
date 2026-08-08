import SwiftUI

// The Usage page's flow board: application → this Mac → backend → tool, as four
// ordered columns with the traffic drawn between them.
//
// Everything here is deliberately free of `AgentModel`, `UsageGraph` and the
// network: the board takes a `FlowInput` of plain values and returns a
// `FlowModel` of plain values, so both the name-splitting and the layout can be
// rendered and checked on their own. `UsageView` is the only thing that knows
// where the numbers came from.

// ── Names ───────────────────────────────────────────────────────────────

/// The gateway namespaces every tool it serves, and this Mac's tools are
/// namespaced *twice*.
///
/// A local server called `obsidian` publishing `obsidian_patch_note` reaches the
/// gateway as `sids-macbook-pro__obsidian__obsidian_patch_note`: the agent
/// prefixes the backend, and the gateway prefixes the agent. Splitting on the
/// first `__` therefore yields the *agent's* name, not the backend's, which is
/// why the Backends column used to be an exact copy of the This Mac column —
/// one node, this machine's own hostname, carrying all 58 tools — while every
/// tool row still showed the redundant `obsidian__` prefix in the space where
/// the part that identifies it should have been.
///
/// The reliable fix is not a cleverer delimiter rule but the fact that the app
/// already *knows* its own servers: `model.backends` is the list, so the
/// namespace is matched against it rather than guessed at. The delimiter split
/// stays as the fallback for a tool whose backend has since been removed, and
/// the longest known name wins so that `obsidian` cannot claim a tool belonging
/// to `obsidian-dev`.
enum ToolName {
    static func split(
        _ name: String,
        agent: String,
        known: [String] = []
    ) -> (backend: String, tool: String) {
        var rest = Substring(name)
        if !agent.isEmpty, rest.hasPrefix(agent + "__") {
            rest = rest.dropFirst(agent.count + 2)
        }

        let match = known
            .filter { !$0.isEmpty && rest.hasPrefix($0 + "__") }
            .max { $0.count < $1.count }
        if let match {
            return (match, String(rest.dropFirst(match.count + 2)))
        }

        if let separator = rest.range(of: "__") {
            return (String(rest[..<separator.lowerBound]), String(rest[separator.upperBound...]))
        }
        // A tool with no namespace left to strip. Rare enough that it means
        // something has changed upstream, and it still deserves a row.
        return (FlowModel.ungrouped, String(rest))
    }

    /// `obsidian` + `obsidian_patch_note` → `patch_note`.
    ///
    /// Most MCP servers repeat their own name in every tool they publish, and in
    /// a board where the backend is the column immediately to the left that
    /// prefix is the one part of the string carrying no information — while
    /// being the part that survives truncation. Every Obsidian tool read
    /// `obsidian_…` with the verb cut off the end.
    ///
    /// Only ever trimmed on a separator boundary, and never down to nothing, so
    /// a server called `notes` publishing a tool called `notes` keeps its name.
    /// The full name is still on the row's tooltip.
    static func shorten(_ tool: String, backend: String) -> String {
        guard !backend.isEmpty else { return tool }
        for separator in ["_", "-", "."] {
            let prefix = backend + separator
            if tool.hasPrefix(prefix), tool.count > prefix.count {
                return String(tool.dropFirst(prefix.count))
            }
        }
        return tool
    }
}

// ── Input ───────────────────────────────────────────────────────────────

/// The board's world, as plain values.
struct FlowInput: Equatable {
    struct App: Equatable {
        var name: String
        var callCount: Int
        var isLive: Bool
    }

    struct Tool: Equatable {
        var name: String
        var risk: String?
        var callCount: Int
    }

    /// A server this Mac is actually running. Carries the authoritative name for
    /// splitting tool namespaces, and its health, so an idle-but-broken backend
    /// is visible on this page rather than only on Backends.
    struct Backend: Equatable {
        var name: String
        var tone: Tone = .neutral
        var status: String?
    }

    /// One application's calls to one tool. Summed per backend, this is what
    /// draws the Applications → Backends lines; without it those two columns
    /// have nothing truthful to join them.
    struct AppTool: Equatable {
        var app: String
        var tool: String
        var callCount: Int
    }

    var agent: String
    var apps: [App] = []
    var tools: [Tool] = []
    var known: [Backend] = []
    var appTools: [AppTool] = []
}

// ── Model ───────────────────────────────────────────────────────────────

struct FlowNode: Identifiable, Equatable {
    var id: String
    var title: String
    var detail: String
    var tone: Tone = .neutral
    /// Identifiers are mono; an application's display name is not one.
    var mono: Bool = true
    var calls: Int = 0
    /// The unabbreviated name, for the tooltip, when the row shows a short one.
    var full: String?
}

struct FlowColumn: Identifiable, Equatable {
    var id: String
    var title: String
    var nodes: [FlowNode] = []
    /// Rows that did not fit. Drawn as a control you can click, never dropped
    /// silently — a truncated list that does not say so reads as the whole list.
    var hidden: Int = 0
    /// Everything this column could show, so the control knows whether there is
    /// anything to collapse back to.
    var total: Int = 0
}

struct FlowLink: Identifiable, Equatable {
    var from: String
    var to: String
    var weight: Int
    var id: String { from + "\u{2192}" + to }
}

/// Four columns and the traffic between them.
struct FlowModel: Equatable {
    static let ungrouped = "other"
    /// How many tools the column shows before it offers to show the rest.
    static let defaultToolLimit = 8

    var columns: [FlowColumn] = []
    var links: [FlowLink] = []
    /// The page's headline figure, and deliberately the same arithmetic a reader
    /// can do on the Applications column.
    ///
    /// The two totals in this payload do not agree, and cannot: application
    /// counts come from every audit event that named an application, while tool
    /// counts come from the registry joined to those events, so a call to a tool
    /// that has since been unregistered is in one and not the other. Whichever
    /// number the headline shows, it should be the one that adds up on screen —
    /// so it follows the applications while there are any, and falls back to the
    /// tools when the Applications column is empty and the tool counts are the
    /// only figures visible.
    var totalCalls = 0
    var activeTools = 0
    var toolCount = 0
    var appCount = 0
    var backendCount = 0

    /// Nothing but this Mac itself.
    ///
    /// Not `columns.allSatisfy(\.nodes.isEmpty)`: the This Mac column always
    /// holds exactly one node, so that read false on a gateway with no
    /// applications, no backends and no tools — and the page drew a board
    /// containing a single unconnected node instead of saying there was nothing
    /// to show.
    var isEmpty: Bool { appCount == 0 && backendCount == 0 && toolCount == 0 }

    /// Group, rank, cap, and wire up.
    ///
    /// `toolLimit` bounds the tallest column, and that is a layout decision as
    /// much as an editorial one: the columns are drawn side by side, so a
    /// fourteen-row tool list beside a two-row application list is nine hundred
    /// points of card that is empty for three quarters of its width, and it
    /// pushed everything below it off the bottom of the window.
    static func build(_ input: FlowInput, toolLimit: Int = FlowModel.defaultToolLimit) -> FlowModel {
        let knownNames = input.known.map(\.name)

        // 1. Untangle every tool name into (backend, bare tool).
        struct Resolved {
            var backend: String
            var bare: String
            var short: String
            var full: String
            var risk: String?
            var calls: Int
        }
        let resolved: [Resolved] = input.tools.map { tool in
            let parts = ToolName.split(tool.name, agent: input.agent, known: knownNames)
            let bare = parts.tool.isEmpty ? tool.name : parts.tool
            return Resolved(
                backend: parts.backend,
                bare: bare,
                short: ToolName.shorten(bare, backend: parts.backend),
                full: tool.name,
                risk: tool.risk,
                calls: tool.callCount
            )
        }

        let totalCalls = resolved.reduce(0) { $0 + $1.calls }

        // 2. Backends: everything this Mac runs, plus anything the gateway
        // served that no longer maps to one. A server with no calls in range
        // still belongs here — "my new backend is getting nothing" is one of the
        // questions this page exists to answer.
        var callsByBackend: [String: Int] = [:]
        var toolsByBackend: [String: Int] = [:]
        for tool in resolved {
            callsByBackend[tool.backend, default: 0] += tool.calls
            toolsByBackend[tool.backend, default: 0] += 1
        }
        var backendNames = knownNames
        for name in toolsByBackend.keys where !backendNames.contains(name) {
            backendNames.append(name)
        }
        let health = Dictionary(input.known.map { ($0.name, $0) }, uniquingKeysWith: { first, _ in first })

        let backendNodes: [FlowNode] = backendNames
            .sorted {
                let left = callsByBackend[$0] ?? 0
                let right = callsByBackend[$1] ?? 0
                return left == right ? $0 < $1 : left > right
            }
            .map { name in
                let tools = toolsByBackend[name] ?? 0
                let known = health[name]
                let count = "\(tools) \(tools == 1 ? "tool" : "tools")"
                // The dot is never the only signal: an unhealthy backend says so
                // in words. The word goes *first*, because this line is the one
                // that runs out of column — put the count first and it is
                // "44 tools · crash…" that gets truncated away.
                let detail =
                    (known?.tone ?? .neutral) == .neutral || known?.status == nil
                    ? count
                    : "\(known?.status ?? "") · \(count)"
                return FlowNode(
                    id: "backend:" + name,
                    title: name,
                    detail: detail,
                    tone: known?.tone ?? .neutral,
                    calls: callsByBackend[name] ?? 0
                )
            }

        // 3. Tools, ranked by traffic. Ties break on the name so the order does
        // not shuffle between refreshes while you are reading it.
        let ranked = resolved.sorted {
            $0.calls == $1.calls ? $0.bare < $1.bare : $0.calls > $1.calls
        }
        let shown = Array(ranked.prefix(toolLimit))
        let toolNodes: [FlowNode] = shown.map { tool in
            FlowNode(
                id: "tool:" + tool.backend + "/" + tool.bare,
                title: tool.short,
                detail: detail(for: tool.calls, risk: tool.risk),
                tone: riskTone(tool.risk),
                calls: tool.calls,
                full: tool.full
            )
        }

        // 4. Applications.
        let appNodes: [FlowNode] = input.apps
            .sorted { $0.callCount == $1.callCount ? $0.name < $1.name : $0.callCount > $1.callCount }
            .map { app in
                FlowNode(
                    id: "app:" + app.name,
                    title: app.name,
                    detail: app.isLive
                        ? "\(Format.count(app.callCount)) calls · live"
                        : "\(Format.count(app.callCount)) calls",
                    tone: app.isLive ? .ok : .neutral,
                    mono: false,
                    calls: app.callCount
                )
            }

        let appTotal = appNodes.reduce(0) { $0 + $1.calls }
        let headline = appNodes.isEmpty ? totalCalls : appTotal

        // 5. One link per hop, not one per tool. Every tool used to contribute
        // its own line into its backend, so a single backend with fifty-eight
        // tools was fifty-eight identical curves stroked over each other at 40%
        // — which composites to a solid bar, not a hairline.
        //
        var links: [FlowLink] = zip(shown, toolNodes).map { tool, node in
            FlowLink(from: "backend:" + tool.backend, to: node.id, weight: tool.calls)
        }

        // Applications → Backends, summed out of the per-tool pairs.
        //
        // These are real edges, not a fan. The gateway's `app_to_backend` cannot
        // provide them — it groups by `backend_name`, which for an agent is the
        // machine, so every application points at the same single node. The tool
        // name is what carries the sub-backend, and splitting it needs the list
        // of servers this Mac is actually running, which only this app has. So
        // the pairs come down per-tool and are folded up here.
        //
        // Empty against a gateway that predates the field, in which case the two
        // columns are simply not joined — which is the honest state, not a
        // degraded one.
        var appBackend: [String: Int] = [:]
        for pair in input.appTools {
            let parts = ToolName.split(pair.tool, agent: input.agent, known: knownNames)
            appBackend["app:" + pair.app + "\u{1}" + parts.backend, default: 0] += pair.callCount
        }
        links += appBackend.compactMap { key, weight in
            let halves = key.split(separator: "\u{1}", maxSplits: 1)
            guard halves.count == 2 else { return nil }
            return FlowLink(from: String(halves[0]), to: "backend:" + halves[1], weight: weight)
        }

        return FlowModel(
            columns: [
                FlowColumn(
                    id: "apps", title: "Applications", nodes: appNodes, total: appNodes.count),
                FlowColumn(
                    id: "backends", title: "Backends", nodes: backendNodes,
                    total: backendNodes.count),
                FlowColumn(
                    id: "tools",
                    title: "Tools",
                    nodes: toolNodes,
                    hidden: max(0, ranked.count - shown.count),
                    total: ranked.count
                ),
            ],
            links: links,
            totalCalls: headline,
            activeTools: resolved.count { $0.calls > 0 },
            toolCount: resolved.count,
            appCount: appNodes.count,
            backendCount: backendNodes.count
        )
    }

    private static func detail(for calls: Int, risk: String?) -> String {
        let count = "\(Format.count(calls)) \(calls == 1 ? "call" : "calls")"
        // Only the two levels that warrant action take a colour, so only those
        // two need the word that goes with it.
        switch risk {
        case "destructive", "admin": return count + " · " + (risk ?? "")
        default: return count
        }
    }

    private static func riskTone(_ risk: String?) -> Tone {
        switch risk {
        case "destructive": .deny
        case "admin": .warn
        default: .neutral
        }
    }

    // ── Tracing ─────────────────────────────────────────────────────────

    /// Everything reachable from `id`, following links upstream and downstream
    /// separately.
    ///
    /// Walking undirected would light the whole board from any node: step up
    /// from a tool to the agent and every other backend is one hop back down.
    /// Kept directional, hovering a tool answers the question you actually
    /// asked — which server it belongs to, and which applications feed it.
    func trace(from id: String) -> Set<String> {
        var lit: Set<String> = [id]
        var forward: [String: [String]] = [:]
        var backward: [String: [String]] = [:]
        for link in links {
            forward[link.from, default: []].append(link.to)
            backward[link.to, default: []].append(link.from)
        }

        for edges in [forward, backward] {
            var queue = [id]
            while let node = queue.popLast() {
                for next in edges[node] ?? [] where !lit.contains(next) {
                    lit.insert(next)
                    queue.append(next)
                }
            }
        }
        return lit
    }
}

// ── The board ───────────────────────────────────────────────────────────

/// One node's frame, so the links can be drawn between columns.
private struct NodeAnchors: PreferenceKey {
    static let defaultValue: [String: Anchor<CGRect>] = [:]

    static func reduce(
        value: inout [String: Anchor<CGRect>],
        nextValue: () -> [String: Anchor<CGRect>]
    ) {
        value.merge(nextValue()) { _, new in new }
    }
}

/// Four ordered columns with the traffic curved between them.
///
/// The dashboard draws this with a force-directed graph library. Here it is a
/// strictly layered board, which is a better fit for a window than a physics
/// simulation: the layout is stable between refreshes, so the node you were
/// reading does not move, and it fits the pane rather than needing a canvas to
/// pan around.
///
/// **The columns are fractions of whatever width they are given, never a fixed
/// size.** Four 168pt columns and three 46pt gutters is 810pt of content
/// in a detail pane that is 668pt at the window's minimum size: the board was
/// clipped at the right edge, and — because the page's header shares the
/// scroll view's width with it — the range picker was pushed out of the window
/// with it. Nothing about that was visible from the code; it needed the numbers
/// added up.
struct UsageFlowBoard: View {
    let model: FlowModel
    @Binding var expanded: Bool
    /// A caption for the last column's label row. The tracing interaction is
    /// invisible until you happen to hover something, and an affordance nobody
    /// finds is one that does not exist.
    var note: String?

    @State private var focus: String?
    @State private var available: CGFloat = 0

    private static let gutter: CGFloat = 16
    /// Applications · Backends · Tools. The Tools column holds the longest
    /// strings in the product by a wide margin; equal thirds spend the same
    /// width on it as on the one holding application names.
    ///
    /// `nonisolated` because `Layout.width(_:)` reads it, and `Layout` is a
    /// plain value type that the layout pass touches off the main actor. The
    /// array is a `Sendable` constant, so there is nothing to race on.
    nonisolated private static let weights: [CGFloat] = [1, 1.1, 1.4]
    /// One weight unit, at most. Past this the columns stop growing and the
    /// space goes into the gutters instead: a node is a label, and on a wide
    /// display a four-hundred-point plate around the word `patch_note` is not a
    /// bigger label, it is a bigger hole. Longer gutters spend the same space on
    /// the thing this card is actually about — the curves between the columns.
    private static let maxUnit: CGFloat = 180
    private static let maxGutter: CGFloat = 72

    var body: some View {
        // Traced once per render and handed down. As a computed property it was
        // called from every node's body *and* from the link canvas, and each
        // call rebuilt both adjacency maps from scratch — with the Tools column
        // expanded that is sixty walks of a seventy-edge graph to draw one
        // frame.
        let lit = focus.map { model.trace(from: $0) }

        let layout = self.layout

        // `maxWidth`, never `width`, and that is what stops the board taking the
        // window with it.
        //
        // A hard `width` here is a *minimum* as well as a maximum: the row could
        // not be laid out any narrower than the four numbers added up, so the
        // card could not, so the page could not, so the window could not. And
        // those numbers came from `available`, which is measured from the board
        // itself — so once the window had been wide, the board demanded to stay
        // wide, and the measurement it needed in order to shrink could never
        // happen. That is the "make it bigger, then smaller again, and it
        // breaks" loop exactly: drag out, drag back, and the content stayed at
        // its old width while the window shrank around it, pushing the sidebar
        // off the left edge and the range picker off the right.
        //
        // As a maximum the weights still do their job whenever there is room,
        // and when there is not the columns simply give way.
        return HStack(alignment: .top, spacing: layout.gutter) {
            ForEach(Array(model.columns.enumerated()), id: \.element.id) { index, column in
                columnView(column, isLast: index == model.columns.count - 1, lit: lit)
                    .frame(maxWidth: layout.width(index))
            }
        }
        .frame(maxWidth: .infinity, alignment: layout.isTight ? .leading : .center)
        // Measured in the background rather than with a `GeometryReader` around
        // the board: a reader takes the whole height it is offered and reports
        // it as its own, which inside a scroll view means "infinite", and the
        // card would grow without bound. In the background it reads the size the
        // layout already settled on and changes nothing.
        .background {
            GeometryReader { proxy in
                Color.clear
                    .onAppear { available = proxy.size.width }
                    .onChange(of: proxy.size.width) { _, new in available = new }
            }
        }
        .backgroundPreferenceValue(NodeAnchors.self) { anchors in
            GeometryReader { proxy in
                Canvas { context, _ in
                    draw(&context, proxy: proxy, anchors: anchors, lit: lit)
                }
            }
            .accessibilityHidden(true)
        }
        .animation(.easeOut(duration: 0.16), value: focus)
    }

    /// How wide each column is, and how much air is between them.
    ///
    /// Widths are `nil` for the first pass, before the measurement lands — the
    /// columns take their ideal width for one frame and settle on the next.
    private struct Layout {
        var unit: CGFloat?
        var gutter: CGFloat
        /// Every point of the width is spoken for, so the board is pinned left
        /// rather than floating in slack it does not have.
        var isTight: Bool

        func width(_ index: Int) -> CGFloat? {
            guard let unit, index < UsageFlowBoard.weights.count else { return nil }
            return unit * UsageFlowBoard.weights[index]
        }
    }

    private var layout: Layout {
        let count = model.columns.count
        let gaps = CGFloat(max(0, count - 1))
        guard available > 0, count > 0 else {
            return Layout(unit: nil, gutter: Self.gutter, isTight: true)
        }

        let weightTotal = Self.weights.prefix(count).reduce(0, +)
        let forColumns = max(0, available - Self.gutter * gaps)
        let unit = forColumns / weightTotal
        guard unit > Self.maxUnit, gaps > 0 else {
            return Layout(unit: unit, gutter: Self.gutter, isTight: true)
        }

        // Wider than the columns need: the surplus buys gutter, up to a point,
        // and whatever is still left over is centred rather than trailing off
        // the right-hand side.
        let spare = available - Self.maxUnit * weightTotal - Self.gutter * gaps
        let gutter = min(Self.maxGutter, Self.gutter + spare / gaps)
        return Layout(unit: Self.maxUnit, gutter: gutter, isTight: gutter < Self.maxGutter)
    }

    // ── Columns ─────────────────────────────────────────────────────────

    /// The label sits at the top of every column; the nodes are centred in the
    /// space below it.
    ///
    /// Top-aligned, a two-row Applications column beside an eight-row Tools
    /// column reads as a list that ran out rather than as a fan, and the links
    /// all rake downwards from one corner. Centred, the hops are symmetrical
    /// and the board reads as one flow left to right.
    private func columnView(_ column: FlowColumn, isLast: Bool, lit: Set<String>?)
        -> some View
    {
        VStack(alignment: .leading, spacing: 0) {
            // The four column labels *are* the card's heading — "APPLICATION →
            // AGENT → BACKEND → TOOL" above them said the same four words a
            // second time, in the same voice, one line higher.
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(column.title.uppercased())
                    .font(.system(size: Typo.micro, weight: .semibold))
                    .tracking(1.4)
                    .foregroundStyle(Palette.text3)
                    .accessibilityAddTraits(.isHeader)
                if isLast, let note {
                    Spacer(minLength: 4)
                    Text(note)
                        .font(.system(size: Typo.micro))
                        .foregroundStyle(Palette.text4)
                        .lineLimit(1)
                }
            }
            .padding(.bottom, 10)

            // Centred while the board is a fan, top-aligned once it is a list.
            // Expanded to every tool the Tools column is a couple of thousand
            // points tall, and centring against it puts the applications and
            // the agent — the start of the flow — a full screen below the fold.
            if !expanded { Spacer(minLength: 0) }
            VStack(alignment: .leading, spacing: 6) {
                if column.nodes.isEmpty {
                    // A labelled column with nothing under it reads as a column
                    // that failed to load rather than one with nothing to show.
                    Text("none in this range")
                        .font(.system(size: Typo.micro))
                        .foregroundStyle(Palette.text4)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 7)
                } else {
                    ForEach(column.nodes) { node in
                        nodeView(node, lit: lit)
                    }
                }
                overflowRow(column)
            }
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    private func nodeView(_ node: FlowNode, lit: Set<String>?) -> some View {
        let dimmed = lit.map { !$0.contains(node.id) } ?? false
        return HStack(spacing: 7) {
            StatusDot(tone: node.tone)
            VStack(alignment: .leading, spacing: 1) {
                Text(node.title)
                    .font(
                        node.mono
                            ? Typo.mono(Typo.caption, weight: .medium)
                            : .system(size: Typo.caption, weight: .medium)
                    )
                    .foregroundStyle(Palette.text)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Text(node.detail)
                    .font(.system(size: Typo.micro))
                    .monospacedDigit()
                    .foregroundStyle(Palette.text3)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 7)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            focus == node.id ? Palette.high : Palette.raised,
            in: .rect(cornerRadius: Radius.row)
        )
        .overlay(
            RoundedRectangle(cornerRadius: Radius.row)
                .stroke(focus == node.id ? Palette.lineStrong : Palette.line, lineWidth: 1)
        )
        .opacity(dimmed ? 0.32 : 1)
        .anchorPreference(key: NodeAnchors.self, value: .bounds) { [node.id: $0] }
        .onHover { hovering in
            if hovering {
                focus = node.id
            } else if focus == node.id {
                focus = nil
            }
        }
        .help(node.full ?? node.title)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(node.full ?? node.title), \(node.detail)")
    }

    /// The way back out of a truncated column, and the only way in.
    @ViewBuilder
    private func overflowRow(_ column: FlowColumn) -> some View {
        let label: String? =
            column.hidden > 0
            ? "+\(column.hidden) more"
            : (expanded && column.total > FlowModel.defaultToolLimit ? "Show fewer" : nil)

        if let label {
            Button {
                expanded.toggle()
            } label: {
                Text(label)
                    .font(.system(size: Typo.micro, weight: .medium))
                    .monospacedDigit()
                    .foregroundStyle(Palette.text3)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 6)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(.rect)
            }
            .buttonStyle(.plain)
            .help(
                column.hidden > 0
                    ? "Show every tool this Mac serves"
                    : "Show only the busiest tools")
        }
    }

    // ── Links ───────────────────────────────────────────────────────────

    private func draw(
        _ context: inout GraphicsContext,
        proxy: GeometryProxy,
        anchors: [String: Anchor<CGRect>],
        lit: Set<String>?
    ) {
        let heaviest = model.links.map(\.weight).max() ?? 0
        for link in model.links {
            guard let from = anchors[link.from], let to = anchors[link.to] else { continue }
            let start = proxy[from]
            let end = proxy[to]

            var path = Path()
            let a = CGPoint(x: start.maxX, y: start.midY)
            let b = CGPoint(x: end.minX, y: end.midY)
            let bend = (b.x - a.x) * 0.5
            path.move(to: a)
            path.addCurve(
                to: b,
                control1: CGPoint(x: a.x + bend, y: a.y),
                control2: CGPoint(x: b.x - bend, y: b.y)
            )

            // Weight is carried by the line, which is the whole reason to draw
            // links rather than list them: the trunk carrying nine tenths of the
            // traffic should look like the trunk.
            let share = heaviest > 0 ? Double(link.weight) / Double(heaviest) : 0
            let live = link.weight > 0
            let onPath = lit.map { $0.contains(link.from) && $0.contains(link.to) } ?? true
            let opacity: Double = onPath ? (live ? 0.22 + 0.36 * share.squareRoot() : 0.10) : 0.05

            context.stroke(
                path,
                with: .color(Palette.beam.opacity(opacity)),
                lineWidth: live ? 1 + 2 * share.squareRoot() : 1
            )
        }
    }
}
