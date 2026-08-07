import SwiftUI

// ── Rows ────────────────────────────────────────────────────────────────

/// The columns, in one place, because a header that disagrees with its rows by
/// four points is worse than no header at all.
enum AuditColumn {
    /// `11:22:25 AM` is eleven mono characters. At 74 it fitted on some rows and
    /// wrapped to two lines on others, so the rows were different heights down
    /// the column depending on the hour.
    static let time: CGFloat = 86
    static let application: CGFloat = 120
    static let risk: CGFloat = 84
    static let duration: CGFloat = 70
    static let status: CGFloat = 74
    static let spacing: CGFloat = 10
    /// `railed` insets its content by this much to make room for the rail, and
    /// the header has to start in the same place.
    static let railInset: CGFloat = 11

    /// Below this table width, a column is dropped rather than every column
    /// squeezed.
    ///
    /// The fixed columns come to 434 points before the tool name gets any, so in
    /// the detail pane at the window's minimum width the one column that
    /// identifies the row had 130 and truncated to `obsidi…ch_note`. Application
    /// is the one to lose: it is the same value on most rows, and it is still on
    /// the row's detail sheet. Ranking the columns is the rule the dashboard's
    /// tables already follow; they do not reflow either.
    static let wideEnoughForApplication: CGFloat = 780
}

struct AuditHeaderRow: View {
    var compact = false

    var body: some View {
        HStack(spacing: AuditColumn.spacing) {
            cell("Time", width: AuditColumn.time)
            cell("Tool", width: nil)
            if !compact {
                cell("Application", width: AuditColumn.application)
            }
            cell("Risk", width: AuditColumn.risk)
            cell("Duration", width: AuditColumn.duration, alignment: .trailing)
            cell("Status", width: AuditColumn.status, alignment: .trailing)
        }
        .padding(.leading, 12 + AuditColumn.railInset)
        .padding(.trailing, 12)
        .frame(height: 32)
        .overlay(alignment: .bottom) {
            Rectangle().fill(Palette.lineSoft).frame(height: 1)
        }
        .accessibilityHidden(true)
    }

    private func cell(
        _ title: String, width: CGFloat?, alignment: Alignment = .leading
    ) -> some View {
        Text(title.uppercased())
            .font(.system(size: Typo.micro, weight: .semibold))
            .tracking(1.2)
            .foregroundStyle(Palette.text3)
            .lineLimit(1)
            .frame(maxWidth: width == nil ? .infinity : nil, alignment: alignment)
            .frame(width: width, alignment: alignment)
    }
}

struct AuditRow: View {
    let event: AuditEvent
    var compact = false

    /// `sids-macbook-pro__obsidian__obsidian_patch_note` → `obsidian` +
    /// `patch_note`.
    ///
    /// Every row on this page belongs to this machine — that is what the
    /// `backend=` filter means — so the machine's own name at the head of every
    /// tool string is the same forty characters repeated down the widest column,
    /// pushing the part that differs into the ellipsis. The event carries the
    /// name to strip: `backendName` *is* the agent. The whole string stays on
    /// the tooltip and in the detail sheet.
    private var name: (backend: String, tool: String) {
        let parts = ToolName.split(event.toolName, agent: event.backendName)
        return (parts.backend, ToolName.shorten(parts.tool, backend: parts.backend))
    }

    var body: some View {
        HStack(spacing: AuditColumn.spacing) {
            Text(Format.time(event.timestamp))
                .font(Typo.mono(Typo.caption))
                .foregroundStyle(Palette.text3)
                .lineLimit(1)
                .frame(width: AuditColumn.time, alignment: .leading)

            // Two texts rather than one concatenation, so the part that
            // identifies the row can outrank the part that qualifies it: the
            // tool name is given its width first and the backend gives way.
            HStack(spacing: 5) {
                Text(name.backend)
                    .foregroundStyle(Palette.text3)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .layoutPriority(0)
                Text("/")
                    .foregroundStyle(Palette.text4)
                    .layoutPriority(2)
                Text(name.tool)
                    .foregroundStyle(Palette.text)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .layoutPriority(1)
                Spacer(minLength: 0)
            }
            .font(Typo.mono(Typo.small))
            .frame(maxWidth: .infinity, alignment: .leading)

            if !compact {
                Text(event.application ?? "—")
                    .font(.system(size: Typo.caption))
                    .foregroundStyle(Palette.text3)
                    .frame(width: AuditColumn.application, alignment: .leading)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }

            RiskBadge(risk: event.riskCategory)
                .frame(width: AuditColumn.risk, alignment: .leading)

            Text(Format.duration(event.durationMs))
                .font(Typo.mono(Typo.caption))
                .foregroundStyle(Palette.text3)
                .frame(width: AuditColumn.duration, alignment: .trailing)

            Text(label)
                .font(.system(size: Typo.micro, weight: .medium))
                .foregroundStyle(tone.color)
                .lineLimit(1)
                .frame(width: AuditColumn.status, alignment: .trailing)
        }
        .padding(.vertical, 3)
        // The ledger's spine. Red means broken, amber means the policy stopped
        // it — a denial is the gateway working, not failing, so it must not
        // read the same as an upstream error.
        .railed(tone, inset: AuditColumn.railInset)
        .help(event.toolName)
        // Spelled out rather than combined from the cells, so the column the
        // narrow layout drops is still read out.
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(
            [
                Format.time(event.timestamp),
                event.toolName,
                event.application.map { "from \($0)" } ?? "no application",
                event.riskCategory ?? "unclassified",
                Format.duration(event.durationMs),
                label,
            ].joined(separator: ", ")
        )
    }

    /// `tool_error` is a tool that returned a failure, which is not the same
    /// event as the gateway failing to reach it, but it is the same news.
    private var label: String {
        event.status == "tool_error" ? "tool error" : event.status
    }

    private var tone: Tone {
        if event.isError { return .deny }
        if event.isDenied { return .warn }
        return .ok
    }
}
