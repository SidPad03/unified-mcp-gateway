import SwiftUI

/// Tool calls as they happen, newest first.
///
/// Rows are matched to completions by `request_id`. The old TUI matched on tool
/// name and took the first record without a duration, so two concurrent calls to
/// the same tool wrote into each other's row — defect #6, and the reason the
/// duration column used to be quietly wrong under load.
struct ActivityView: View {
    @Environment(AgentModel.self) private var model

    @State private var query = ""
    @State private var showErrorsOnly = false
    @State private var expanded: String?

    /// Recomputed when the calls or the filters change, not on every render.
    ///
    /// A completion re-stamps an existing record rather than appending one, so
    /// unlike the log this cannot be maintained incrementally — but it is a
    /// thousand records at most, and it now runs on change rather than ten
    /// times a second.
    @State private var calls: [ToolCall] = []

    private func rebuild() {
        var result: [ToolCall] = []
        result.reserveCapacity(model.toolCalls.count)
        for call in model.toolCalls.reversed() {
            if showErrorsOnly && call.status != .error { continue }
            if !query.isEmpty {
                let matches =
                    call.tool.localizedCaseInsensitiveContains(query)
                    || (call.backend?.localizedCaseInsensitiveContains(query) ?? false)
                if !matches { continue }
            }
            result.append(call)
        }
        calls = result
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Metrics.gutter) {
            header
            if calls.isEmpty {
                Card {
                    EmptyState(
                        icon: "bolt.horizontal",
                        title: model.toolCalls.isEmpty ? "No tool calls yet" : "Nothing matches those filters",
                        message: model.toolCalls.isEmpty
                            ? "Calls the gateway routes to this Mac appear here as they run."
                            : nil
                    )
                }
            } else {
                table
            }
        }
        .padding(24)
        .padding(.top, 22)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .onAppear(perform: rebuild)
        .onChange(of: model.callRevision) { _, _ in rebuild() }
        .onChange(of: showErrorsOnly) { _, _ in rebuild() }
        .onChange(of: query) { _, _ in rebuild() }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            PageTitle(
                title: "Activity",
                subtitle: "\(model.toolCalls.count) calls held · newest first"
            )
            Spacer()
            Toggle("Errors only", isOn: $showErrorsOnly)
                .toggleStyle(.checkbox)
                .font(.system(size: Typo.caption))
            SearchField(text: $query, prompt: "Filter by tool or backend")
                .frame(width: 220)
        }
    }

    private var table: some View {
        Card(padding: 0) {
            VStack(spacing: 0) {
                columnHeader
                Divider()
                // `List` is lazy, so a thousand held calls cost a viewport of
                // rows. No glass on the rows themselves — see `Card`.
                List {
                    ForEach(calls) { call in
                        CallRow(call: call, expanded: expanded == call.requestId)
                            .listRowInsets(EdgeInsets(top: 3, leading: 12, bottom: 3, trailing: 12))
                            .listRowSeparator(.hidden)
                            .contentShape(.rect)
                            .onTapGesture {
                                guard call.error != nil else { return }
                                withAnimation(.snappy(duration: 0.18)) {
                                    expanded = expanded == call.requestId ? nil : call.requestId
                                }
                            }
                    }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
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
        .padding(.horizontal, 12)
        .padding(.vertical, 9)
    }
}

private struct CallRow: View {
    let call: ToolCall
    let expanded: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 10) {
                Text(Format.time(call.startedAt))
                    .font(.system(size: Typo.caption, design: .monospaced))
                    .foregroundStyle(Palette.text3)
                    .frame(width: 74, alignment: .leading)

                HStack(spacing: 5) {
                    Text(call.bareTool)
                        .font(.system(size: Typo.small, design: .monospaced))
                        .lineLimit(1)
                        .truncationMode(.middle)
                    if call.error != nil {
                        Image(systemName: expanded ? "chevron.down" : "chevron.right")
                            .font(.system(size: 8, weight: .bold))
                            .foregroundStyle(Palette.text3)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                Text(call.backend ?? "—")
                    .font(.system(size: Typo.caption))
                    .foregroundStyle(Palette.text3)
                    .frame(width: 110, alignment: .leading)
                    .lineLimit(1)

                Text(Format.duration(call.durationMs))
                    .font(.system(size: Typo.caption, design: .monospaced))
                    .foregroundStyle(durationTint)
                    .frame(width: 74, alignment: .trailing)

                HStack(spacing: 5) {
                    Spacer(minLength: 0)
                    if call.status == .running {
                        ProgressView().controlSize(.mini).scaleEffect(0.6)
                    }
                    Text(label)
                        .font(.system(size: Typo.micro, weight: .medium))
                        .foregroundStyle(call.status.tint)
                }
                .frame(width: 68, alignment: .trailing)
            }

            if expanded, let error = call.error {
                Text(error)
                    .font(.system(size: Typo.caption, design: .monospaced))
                    .foregroundStyle(Palette.deny)
                    .textSelection(.enabled)
                    .padding(9)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Palette.deny.opacity(0.1), in: .rect(cornerRadius: Radius.control))
            }
        }
        .padding(.vertical, 3)
        // The rail spans the expansion too, so an errored call reads as one
        // block rather than a red line above an unrelated detail panel.
        .railed(call.status.tone)
    }

    private var label: String {
        switch call.status {
        case .running: "running"
        case .ok: "ok"
        case .error: "error"
        }
    }

    /// A call that took more than five seconds is worth a second look.
    private var durationTint: Color {
        guard let ms = call.durationMs else { return Palette.text4 }
        return ms > 5_000 ? Palette.warn : Palette.text2
    }
}
