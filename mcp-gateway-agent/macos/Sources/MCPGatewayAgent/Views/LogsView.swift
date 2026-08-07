import AppKit
import SwiftUI

/// The agent's own log plus every backend's stderr, merged.
///
/// The backend half of this did not exist before: stderr was `Stdio::inherit()`,
/// which in an app with no terminal means `/dev/null`. A Python MCP server
/// printing a traceback on startup produced a backend that simply did not work,
/// with nowhere to find out why.
///
/// Lines are redacted by the core on the way *in*, not on the way out, because
/// Copy and Export are exactly where a credential would leave the machine.
struct LogsView: View {
    @Environment(AgentModel.self) private var model

    @State private var query = ""
    @State private var minimumLevel = LogLevel.info
    @State private var source: String = allSources
    @State private var followTail = true

    /// The filtered view of the buffer, maintained incrementally.
    ///
    /// Rebuilt in full only when a filter changes. When new lines arrive — up to
    /// ten times a second — only the new ones are tested, so a tick costs
    /// O(lines added) rather than O(buffer). Filtering inside `body` instead
    /// would re-test all five thousand on every observed change, and there is
    /// one of those every 100 ms.
    @State private var lines: [LogLine] = []
    @State private var lastSeq = 0
    @State private var sources: [String] = [allSources]

    private static let allSources = "All sources"

    private func matches(_ line: LogLine) -> Bool {
        guard line.level.severity >= minimumLevel.severity else { return false }
        if source != Self.allSources && line.source != source { return false }
        guard !query.isEmpty else { return true }
        return line.message.localizedCaseInsensitiveContains(query)
            || line.source.localizedCaseInsensitiveContains(query)
    }

    private func rebuild() {
        lines = model.logLines.filter(matches)
        lastSeq = model.logLines.last?.seq ?? 0
        refreshSources()
    }

    private func appendNew() {
        guard let newest = model.logLines.last?.seq else {
            // The buffer was cleared.
            lines = []
            lastSeq = 0
            return
        }
        guard newest != lastSeq else { return }
        if newest < lastSeq {
            rebuild()  // cleared and refilled
            return
        }
        // Walk back from the newest until we reach what we already have. Sorting
        // the whole result afterwards would cost more than the filtering this is
        // avoiding, so collect in reverse and flip the short tail instead.
        var fresh: [LogLine] = []
        for line in model.logLines.reversed() {
            guard line.seq > lastSeq else { break }
            if matches(line) { fresh.append(line) }
        }
        lines.append(contentsOf: fresh.reversed())
        // The core's ring buffer evicts from the front; this one has to as well,
        // or a long-running session grows an array the source no longer holds.
        if lines.count > Self.maxVisible {
            lines.removeFirst(lines.count - Self.maxVisible)
        }
        lastSeq = newest
        refreshSources()
    }

    /// Matches `MAX_LOG_LINES` in the core.
    private static let maxVisible = 5_000

    private func refreshSources() {
        var names = Set(model.logLines.map(\.source))
        names.formUnion(model.backends.map(\.name))
        names.insert("agent")
        let updated = [Self.allSources] + names.sorted()
        if updated != sources { sources = updated }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Metrics.gutter) {
            header
            filters
            if model.logLinesDropped > 0 {
                Text(
                    "\(Format.count(model.logLinesDropped)) older lines were dropped. The buffer "
                        + "holds the most recent 5,000."
                )
                .font(.system(size: Typo.caption))
                .foregroundStyle(Palette.text3)
            }
            body(for: lines)
        }
        .padding(24)
        .padding(.top, 22)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .onAppear(perform: rebuild)
        .onChange(of: model.logRevision) { _, _ in appendNew() }
        .onChange(of: minimumLevel) { _, _ in rebuild() }
        .onChange(of: source) { _, _ in rebuild() }
        .onChange(of: query) { _, _ in rebuild() }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline) {
            PageTitle(title: "Logs", subtitle: "\(lines.count) of \(model.logLines.count) lines")
            Spacer()
            HStack(spacing: 7) {
                Button {
                    copy()
                } label: {
                    Label("Copy", systemImage: "doc.on.doc")
                }
                Button {
                    export()
                } label: {
                    Label("Export…", systemImage: "square.and.arrow.down")
                }
                Button {
                    revealInFinder()
                } label: {
                    Label("Reveal", systemImage: "folder")
                }
                Button(role: .destructive) {
                    Task { await model.clearLogs() }
                } label: {
                    Label("Clear", systemImage: "trash")
                }
            }
            .buttonStyle(.glass)
            .controlSize(.small)
            .labelStyle(.titleAndIcon)
        }
    }

    private var filters: some View {
        HStack(spacing: 10) {
            Picker("Level", selection: $minimumLevel) {
                ForEach(LogLevel.allCases) { level in
                    Text(level.label).tag(level)
                }
            }
            .pickerStyle(.menu)
            .labelsHidden()
            .frame(width: 100)

            Picker("Source", selection: $source) {
                ForEach(sources, id: \.self) { Text($0).tag($0) }
            }
            .pickerStyle(.menu)
            .labelsHidden()
            .frame(width: 160)

            SearchField(text: $query, prompt: "Search")
                .frame(maxWidth: 300)

            Toggle("Follow", isOn: $followTail)
                .toggleStyle(.checkbox)
                .font(.system(size: Typo.caption))

            Spacer()
        }
    }

    private func body(for lines: [LogLine]) -> some View {
        Card(padding: 0) {
            if lines.isEmpty {
                EmptyState(
                    icon: "text.alignleft",
                    title: model.logLines.isEmpty ? "No log lines yet" : "Nothing matches those filters",
                    message: model.logLines.isEmpty
                        ? "The agent's own messages and every backend's stderr land here."
                        : nil
                )
            } else {
                ScrollViewReader { proxy in
                    // Lazy: five thousand lines cost a viewport of rows.
                    List(lines) { line in
                        LogRow(line: line)
                            .id(line.seq)
                            .listRowInsets(
                                EdgeInsets(top: 1, leading: 12, bottom: 1, trailing: 12)
                            )
                            .listRowSeparator(.hidden)
                    }
                    .listStyle(.plain)
                    .scrollContentBackground(.hidden)
                    .onChange(of: lines.last?.seq) { _, seq in
                        guard followTail, let seq else { return }
                        withAnimation(.linear(duration: 0.12)) {
                            proxy.scrollTo(seq, anchor: .bottom)
                        }
                    }
                    .onAppear {
                        if let seq = lines.last?.seq { proxy.scrollTo(seq, anchor: .bottom) }
                    }
                }
            }
        }
    }

    // ── Actions ─────────────────────────────────────────────────────────

    private var plainText: String {
        lines
            .map { "\(Format.dateTime($0.ts))  \($0.level.rawValue.uppercased())  [\($0.source)]  \($0.message)" }
            .joined(separator: "\n")
    }

    private func copy() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(plainText, forType: .string)
    }

    private func export() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "mcp-gateway-agent.log"
        panel.canCreateDirectories = true
        guard panel.runModal() == .OK, let url = panel.url else { return }
        try? plainText.write(to: url, atomically: true, encoding: .utf8)
    }

    private func revealInFinder() {
        guard let path = model.config?.configPath else { return }
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
    }
}

private struct LogRow: View {
    let line: LogLine

    var body: some View {
        HStack(alignment: .top, spacing: 9) {
            Text(Format.time(line.ts))
                .font(Typo.mono(Typo.micro))
                .foregroundStyle(Palette.text4)
                .frame(width: 66, alignment: .leading)

            Text(line.level.rawValue.uppercased())
                .font(.system(size: Typo.micro, weight: .semibold))
                .foregroundStyle(line.level.tint)
                .frame(width: 38, alignment: .leading)

            Text(line.source)
                .font(.system(size: Typo.micro))
                .foregroundStyle(Palette.text4)
                .frame(width: 92, alignment: .leading)
                .lineLimit(1)
                .truncationMode(.middle)

            Text(line.message)
                .font(Typo.mono(Typo.caption))
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.vertical, 2)
        // A grey column with red and amber notches in it: the shape of the log
        // is readable before any of the lines are.
        .railed(line.level.tone)
    }
}
