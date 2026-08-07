import SwiftUI

// The shared vocabulary: card, section header, stat, status dot, badge, rail.
// The dashboard has the same set under the same names in `components/ui.tsx`,
// and keeping the two in step is what makes them read as one product.

// ── Glass ───────────────────────────────────────────────────────────────

/// A card: content on its own plane, above the window background.
///
/// Glass is for **chrome**. Cards, the sidebar, toolbars, popovers and sheets
/// get it; the rows inside a scrolling list do not. A material re-composites as
/// it moves, so putting one on every row of a five-thousand-line log turns a
/// free effect into a per-frame cost — the same rule the web version enforces by
/// hand with `backdrop-filter`, except here the OS does the work and the rule is
/// about how many surfaces exist, not how expensive each one is.
struct Card<Content: View>: View {
    var padding: CGFloat = Metrics.cardPadding
    /// Take the full height offered instead of hugging the content.
    ///
    /// Set this on cards that sit beside each other. Without it each one is as
    /// tall as whatever happens to be inside, so a column showing an empty state
    /// and a column showing data come out different heights and the row looks
    /// broken. The height has to be applied here, inside the card and before
    /// `glassEffect`, or the material would still be drawn at the content's size
    /// while the frame around it stretched.
    ///
    /// "The full height offered" is the catch, and it is worth knowing before
    /// reaching for this. Inside a `ScrollView` the offer is the content's own
    /// height and these cards match each other, which is the intent. In a stack
    /// that fills the window there is no such limit, and the row will take the
    /// whole page: four cards of about a hundred points became six hundred on
    /// the Audit page and pushed the table out of the window. Give the *row*
    /// `.fixedSize(horizontal: false, vertical: true)` there — the cards go on
    /// matching each other inside a row that is only as tall as it needs to be.
    var fillsHeight: Bool = false
    @ViewBuilder var content: Content

    var body: some View {
        content
            .padding(padding)
            .frame(
                maxWidth: .infinity,
                maxHeight: fillsHeight ? .infinity : nil,
                alignment: .topLeading
            )
            .glassEffect(.regular, in: .rect(cornerRadius: Radius.card))
    }
}

/// A flat panel, for the places glass would be wrong: long scrolling lists, and
/// anything that needs a hairline rather than a material.
struct Panel<Content: View>: View {
    var padding: CGFloat = 0
    @ViewBuilder var content: Content

    var body: some View {
        content
            .padding(padding)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Palette.panel, in: .rect(cornerRadius: Radius.card))
            .overlay(
                RoundedRectangle(cornerRadius: Radius.card).stroke(Palette.line, lineWidth: 1)
            )
    }
}

// ── Type ────────────────────────────────────────────────────────────────

/// `10pt / semibold / 0.16em / uppercase` — the label voice, used everywhere.
struct SectionHeader: View {
    let title: String
    var trailing: AnyView?

    init(_ title: String) {
        self.title = title
        self.trailing = nil
    }

    init<T: View>(_ title: String, @ViewBuilder trailing: () -> T) {
        self.title = title
        self.trailing = AnyView(trailing())
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(title.uppercased())
                .font(.system(size: Typo.micro, weight: .semibold))
                .tracking(1.6)
                .foregroundStyle(Palette.text3)
            Spacer(minLength: 12)
            trailing
        }
    }
}

struct FieldLabel: View {
    let text: String

    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text.uppercased())
            .font(.system(size: Typo.micro, weight: .semibold))
            .tracking(1.4)
            .foregroundStyle(Palette.text3)
    }
}

/// The one focal element of a page: what you came here to do.
struct PageTitle: View {
    let title: String
    var subtitle: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .font(.system(size: Typo.title, weight: .semibold))
                .tracking(-0.4)
                .foregroundStyle(Palette.text)
            if let subtitle {
                Text(subtitle)
                    .font(.system(size: Typo.small))
                    .foregroundStyle(Palette.text3)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

/// An identifier — a tool name, an agent id, a host, a duration. Always mono.
struct Mono: View {
    let text: String
    var size: CGFloat = Typo.small
    var weight: Font.Weight = .regular
    var color: Color = Palette.text

    init(_ text: String, size: CGFloat = Typo.small, weight: Font.Weight = .regular, color: Color = Palette.text) {
        self.text = text
        self.size = size
        self.weight = weight
        self.color = color
    }

    var body: some View {
        Text(text)
            .font(Typo.mono(size, weight: weight))
            .foregroundStyle(color)
    }
}

// ── Status ──────────────────────────────────────────────────────────────

/// A status dot.
///
/// `pulsing` is the one animation in the product that carries meaning rather
/// than polish — it is how "connected" differs from a screenshot of connected.
/// Never the only signal: every dot is paired with a word, because a colour
/// difference is not information to everyone.
struct StatusDot: View {
    let tint: Color
    var pulsing = false

    @State private var animating = false

    init(tint: Color, pulsing: Bool = false) {
        self.tint = tint
        self.pulsing = pulsing
    }

    init(tone: Tone, pulsing: Bool = false) {
        self.tint = tone.color
        self.pulsing = pulsing
    }

    var body: some View {
        Circle()
            .fill(tint)
            .frame(width: 6, height: 6)
            .overlay {
                if pulsing {
                    Circle()
                        .fill(tint)
                        .scaleEffect(animating ? 2.6 : 1)
                        .opacity(animating ? 0 : 0.6)
                }
            }
            .onAppear {
                guard pulsing else { return }
                withAnimation(.easeOut(duration: 1.8).repeatForever(autoreverses: false)) {
                    animating = true
                }
            }
            .accessibilityHidden(true)
    }
}

/// Status as a word plus a dot — the pair, never one alone.
struct StatusLabel: View {
    let tone: Tone
    let text: String
    var pulsing = false

    var body: some View {
        HStack(spacing: 6) {
            StatusDot(tone: tone, pulsing: pulsing)
            Text(text)
                .font(.system(size: Typo.small, weight: .medium))
                .foregroundStyle(tone.color)
        }
        .accessibilityElement(children: .combine)
    }
}

struct Badge: View {
    let text: String
    var tone: Tone = .neutral

    init(_ text: String, tone: Tone = .neutral) {
        self.text = text
        self.tone = tone
    }

    // Kept so existing call sites that pass a colour still compile. Colours
    // that are not one of the three lamps collapse to neutral, which is the
    // point — there are only ever three coloured states.
    init(text: String, tint: Color = Palette.text3) {
        self.text = text
        self.tone =
            tint == Palette.beam
            ? .ok : tint == Palette.warn ? .warn : tint == Palette.deny ? .deny : .neutral
    }

    var body: some View {
        Text(text)
            .font(.system(size: Typo.micro, weight: .medium))
            .tracking(0.3)
            .padding(.horizontal, 6)
            .padding(.vertical, 2.5)
            .background(tone.wash, in: .rect(cornerRadius: Radius.control - 1))
            .overlay(
                RoundedRectangle(cornerRadius: Radius.control - 1)
                    .stroke(tone == .neutral ? Palette.line : tone.color.opacity(0.24), lineWidth: 1)
            )
            .foregroundStyle(tone == .neutral ? Palette.text2 : tone.color)
    }
}

/// Risk classification as a *ramp*, not six unrelated colours: emphasis climbs
/// with severity, and only the two levels that warrant action take a hue.
/// Identical rule to the dashboard's `RiskBadge`.
struct RiskBadge: View {
    let risk: String?

    private var level: String { (risk?.isEmpty == false ? risk! : "unclassified") }

    private var tone: Tone {
        switch level {
        case "destructive": .deny
        case "admin": .warn
        default: .neutral
        }
    }

    private var ink: Color {
        switch level {
        case "read": Palette.text4
        case "write": Palette.text3
        case "execute": Palette.text
        case "unclassified": Palette.text4
        default: tone.color
        }
    }

    var body: some View {
        Text(level)
            .font(.system(size: Typo.micro, weight: .medium))
            .padding(.horizontal, 6)
            .padding(.vertical, 2.5)
            .background(
                level == "unclassified" ? Color.clear : tone.wash,
                in: .rect(cornerRadius: Radius.control - 1)
            )
            .overlay(
                RoundedRectangle(cornerRadius: Radius.control - 1)
                    .stroke(
                        tone == .neutral ? Palette.line : tone.color.opacity(0.24),
                        style: StrokeStyle(lineWidth: 1, dash: level == "unclassified" ? [2.5, 2.5] : [])
                    )
            )
            .foregroundStyle(ink)
    }
}

// ── The gate rail ───────────────────────────────────────────────────────
//
// The signature. A gateway is a channel things pass through, so the interface
// has one: a 3pt rail down the left of every row, carrying that row's verdict.
// Scroll a thousand log lines and the health of the system is readable
// peripherally, as a column of green with red notches in it. The same rail is
// the active marker in the sidebar and the status edge on a backend row.

/// A list of railed rows, on a flat panel.
struct RailList<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        VStack(spacing: 0) { content }
            .background(Palette.panel, in: .rect(cornerRadius: Radius.card))
            .overlay(
                RoundedRectangle(cornerRadius: Radius.card).stroke(Palette.line, lineWidth: 1)
            )
            .clipShape(.rect(cornerRadius: Radius.card))
    }
}

struct RailRow<Content: View, Trailing: View>: View {
    var tone: Tone = .neutral
    var isLast = false
    @ViewBuilder var content: Content
    @ViewBuilder var trailing: Trailing

    var body: some View {
        HStack(spacing: 0) {
            Rectangle()
                .fill(tone.rail)
                .frame(width: Metrics.rail)
            HStack(spacing: 10) {
                content
                Spacer(minLength: 8)
                trailing
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 9)
        }
        .frame(minHeight: 36)
        .overlay(alignment: .bottom) {
            if !isLast {
                Rectangle().fill(Palette.lineSoft).frame(height: 1)
            }
        }
    }
}

extension RailRow where Trailing == EmptyView {
    init(tone: Tone = .neutral, isLast: Bool = false, @ViewBuilder content: () -> Content) {
        self.tone = tone
        self.isLast = isLast
        self.content = content()
        self.trailing = EmptyView()
    }
}

extension View {
    /// The rail, for rows that already have their own layout — log lines, call
    /// rows, audit rows. Cheaper than restructuring a dense row into a
    /// `RailRow`, and identical to look at.
    ///
    /// Scroll five thousand log lines and the errors are findable without
    /// reading any of them.
    func railed(_ tone: Tone, inset: CGFloat = 11) -> some View {
        self
            .padding(.leading, inset)
            .overlay(alignment: .leading) {
                Capsule()
                    .fill(tone.rail)
                    .frame(width: Metrics.rail)
            }
    }
}

// ── Fields ──────────────────────────────────────────────────────────────

/// The filter box on Activity, Logs and Audit.
///
/// **Inset, not raised.** A field receives content, so it reads as a well cut
/// into the surface rather than a plate sitting on it — the same rule as the
/// dashboard's `.field`. This was a `.quaternary` fill, which is a *lighter*
/// hierarchical fill, so the one control on the page that you type into was the
/// one that looked like it was floating above everything else.
struct SearchField: View {
    @Binding var text: String
    var prompt: String

    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: Typo.micro))
                .foregroundStyle(Palette.text3)
            TextField(prompt, text: $text)
                .textFieldStyle(.plain)
                .font(.system(size: Typo.small))
                .focused($focused)
            if !text.isEmpty {
                Button {
                    text = ""
                } label: {
                    Image(systemName: "xmark.circle.fill").font(.system(size: Typo.micro))
                }
                .buttonStyle(.plain)
                .foregroundStyle(Palette.text3)
                .accessibilityLabel("Clear the filter")
            }
        }
        .padding(.horizontal, 9)
        .padding(.vertical, 6)
        .background(Palette.inset, in: .rect(cornerRadius: Radius.control))
        .overlay(
            RoundedRectangle(cornerRadius: Radius.control)
                .stroke(focused ? Palette.beam.opacity(0.55) : Palette.line, lineWidth: 1)
        )
        .overlay(
            RoundedRectangle(cornerRadius: Radius.control)
                .stroke(Palette.beam.opacity(focused ? 0.14 : 0), lineWidth: 3)
                .padding(-2)
        )
        .animation(.easeOut(duration: 0.12), value: focused)
        .contentShape(.rect)
        .onTapGesture { focused = true }
    }
}

// ── Numbers ─────────────────────────────────────────────────────────────

/// One figure and its label. Every number in the app is one of these, at one
/// size.
///
/// There were two: a 26pt headline `Stat` and a 15pt `MiniStat` meant to read as
/// a supporting tier. Standing side by side in a summary bar they did not read
/// as a hierarchy, they read as two bars that happened to share a row, and since
/// each page nominated its own headline the size jumped to a different column on
/// every tab. Rank is carried by order here, which is free; size is spent on
/// telling a figure from its label.
struct Stat: View {
    let value: String
    let label: String
    var tint: Color = Palette.text
    var detail: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            FieldLabel(label)
            Text(value)
                .font(.system(size: Typo.title, weight: .semibold))
                .monospacedDigit()
                .tracking(-0.3)
                .foregroundStyle(tint)
                .contentTransition(.numericText())
            if let detail {
                Text(detail)
                    .font(.system(size: Typo.caption))
                    .foregroundStyle(Palette.text4)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(label): \(value)")
    }
}

// ── Empty and error states ──────────────────────────────────────────────

struct EmptyState: View {
    let icon: String
    let title: String
    var message: String?
    var action: (label: String, run: () -> Void)?

    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: icon)
                .font(.system(size: 22, weight: .light))
                .foregroundStyle(Palette.text4)
            Text(title)
                .font(.system(size: Typo.body, weight: .medium))
                .foregroundStyle(Palette.text)
            if let message {
                Text(message)
                    .font(.system(size: Typo.small))
                    .foregroundStyle(Palette.text3)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 340)
            }
            if let action {
                Button(action.label, action: action.run)
                    .buttonStyle(.glassProminent)
                    .padding(.top, 4)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 40)
    }
}

/// A one-line problem report that does not steal focus.
struct InlineBanner: View {
    let text: String
    var tone: Tone = .deny
    var onDismiss: (() -> Void)?

    init(text: String, tone: Tone = .deny, onDismiss: (() -> Void)? = nil) {
        self.text = text
        self.tone = tone
        self.onDismiss = onDismiss
    }

    // Kept so existing call sites that pass a colour still compile.
    init(text: String, tint: Color, onDismiss: (() -> Void)? = nil) {
        self.text = text
        self.tone = tint == Palette.warn ? .warn : tint == Palette.beam ? .ok : .deny
        self.onDismiss = onDismiss
    }

    var body: some View {
        HStack(spacing: 9) {
            Image(systemName: tone == .deny ? "exclamationmark.triangle.fill" : "info.circle.fill")
                .font(.system(size: 11))
            Text(text)
                .font(.system(size: Typo.small))
                .lineLimit(2)
            Spacer(minLength: 8)
            if let onDismiss {
                Button {
                    onDismiss()
                } label: {
                    Image(systemName: "xmark").font(.system(size: 9, weight: .bold))
                }
                .buttonStyle(.plain)
            }
        }
        .foregroundStyle(tone.color)
        .padding(.horizontal, 12)
        .padding(.vertical, 9)
        .background(tone.wash, in: .rect(cornerRadius: Radius.row))
        .overlay(
            RoundedRectangle(cornerRadius: Radius.row).stroke(tone.color.opacity(0.25), lineWidth: 1)
        )
    }
}

// ── Formatting ──────────────────────────────────────────────────────────

/// The same rules as the dashboard's `fmt`, so a latency reads identically in
/// both.
enum Format {
    /// Local time, because the app runs on the machine the events happened on.
    ///
    /// The old TUI computed `secs % 86400` against the UNIX epoch and displayed
    /// UTC time-of-day as if it were local. Timestamps cross the FFI boundary as
    /// RFC 3339 UTC and are rendered here, where the timezone is actually known.
    static func time(_ date: Date) -> String {
        date.formatted(.dateTime.hour().minute().second())
    }

    static func dateTime(_ date: Date) -> String {
        date.formatted(.dateTime.month(.abbreviated).day().hour().minute().second())
    }

    static func duration(_ milliseconds: Int?) -> String {
        guard let milliseconds else { return "—" }
        if milliseconds < 1_000 { return "\(milliseconds) ms" }
        return String(format: "%.2f s", Double(milliseconds) / 1_000)
    }

    /// `Int(_:)` on a Double traps on a value outside `Int`'s range and on a
    /// non-number, and this one comes off the wire as `AVG(duration_ms)` from a
    /// database this app does not own. A latency is not worth a crash.
    static func duration(_ milliseconds: Double?) -> String {
        guard let milliseconds, milliseconds.isFinite,
            milliseconds.magnitude < Double(Int.max)
        else { return "—" }
        return duration(Int(milliseconds.rounded()))
    }

    /// `3d 4h`, `4h 12m`, `12m`, `45s` — one unit of precision past the first.
    static func uptime(_ seconds: Int?) -> String {
        guard let seconds, seconds >= 0 else { return "—" }
        let days = seconds / 86_400
        let hours = (seconds % 86_400) / 3_600
        let minutes = (seconds % 3_600) / 60
        if days > 0 { return "\(days)d \(hours)h" }
        if hours > 0 { return "\(hours)h \(minutes)m" }
        if minutes > 0 { return "\(minutes)m" }
        return "\(seconds)s"
    }

    static func count(_ value: Int) -> String {
        value.formatted(.number.notation(.compactName))
    }

    static func percent(_ fraction: Double) -> String {
        fraction.formatted(.percent.precision(.fractionLength(0...1)))
    }
}
