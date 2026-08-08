import SwiftUI

/// One tool call, as it happens.
///
/// Lifted out of the Activity page when that page was folded into Overview's
/// "Recent activity" card. Rows are matched to completions by `request_id`, not
/// by tool name: the old TUI took the first record without a duration, so two
/// concurrent calls to the same tool wrote into each other's row — defect #6,
/// and the reason the duration column used to be quietly wrong under load.
struct CallRow: View {
    let call: ToolCall
    let expanded: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 10) {
                Text(Format.time(call.startedAt))
                    .font(Typo.mono(Typo.caption))
                    .foregroundStyle(Palette.text3)
                    .frame(width: 74, alignment: .leading)

                HStack(spacing: 5) {
                    Text(call.bareTool)
                        .font(Typo.mono(Typo.small))
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
                    .font(Typo.mono(Typo.caption))
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
                    .font(Typo.mono(Typo.caption))
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
        .accessibilityElement(children: .combine)
        .accessibilityLabel(
            [
                Format.time(call.startedAt),
                call.tool,
                call.backend.map { "on \($0)" } ?? "unrouted",
                Format.duration(call.durationMs),
                label,
            ].joined(separator: ", ")
        )
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
