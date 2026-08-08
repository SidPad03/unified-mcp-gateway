import AppKit
import SwiftUI

/// The design tokens, in step with `mcp-gateway-dashboard/src/index.css`.
///
/// Identity is shared with the dashboard — same cold steel structure, same
/// phosphor accent, same three lamps, same type scale, same radii. What differs
/// is only *how a surface is painted*: on the web those are flat fills, here
/// they are Liquid Glass materials the OS composites. Side by side the two read
/// as one product, one native to the Mac and one to the browser.
///
/// **Keep this file in step with the dashboard's token block.** The values are
/// duplicated rather than shared because the dashboard's Docker build context is
/// `./mcp-gateway-dashboard` only, and a shared directory would break it.
enum Palette {
    // Every surface is the same cold hue (~217°); only lightness moves, in
    // steps of two or three percent. You should feel the stack rather than see
    // it. Dark is the primary design target, but a Mac app that ignores the
    // system appearance looks broken, so every token has a light value too.

    /// The room. Window background, behind the glass chrome.
    static let canvas = dynamic("mcpgw.canvas", dark: 0x0608_0B, light: 0xF1F4_F9)
    /// A flat panel, where glass would be wrong (rows in a long list).
    static let panel = dynamic("mcpgw.panel", dark: 0x0B0E_14, light: 0xFFFF_FF)
    /// Hover, and nested surfaces.
    static let raised = dynamic("mcpgw.raised", dark: 0x1014_1C, light: 0xF6F8_FC)
    /// Popovers and menus — one step above their parent.
    static let high = dynamic("mcpgw.high", dark: 0x161B_25, light: 0xFFFF_FF)
    /// *Darker* than its surroundings in both appearances: a field receives
    /// content, so it reads as inset rather than raised.
    static let inset = dynamic("mcpgw.inset", dark: 0x0406_0A, light: 0xEDF1_F7)

    // Borders should disappear when you are not looking for them and be
    // findable when you need the structure. Low-opacity, never a solid hex.
    static let line = dynamicAlpha("mcpgw.line", dark: (0x94AD_D6, 0.10), light: (0x0F23_46, 0.11))
    static let lineSoft = dynamicAlpha("mcpgw.lineSoft", dark: (0x94AD_D6, 0.055), light: (0x0F23_46, 0.07))
    static let lineStrong = dynamicAlpha("mcpgw.lineStrong", dark: (0x94AD_D6, 0.18), light: (0x0F23_46, 0.20))

    // Four levels of text. Two is not a hierarchy.
    static let text = dynamic("mcpgw.text", dark: 0xE4E9_F2, light: 0x0D13_1C)
    static let text2 = dynamic("mcpgw.text2", dark: 0x9FAD_C4, light: 0x4653_6B)
    static let text3 = dynamic("mcpgw.text3", dark: 0x6B7A_93, light: 0x6B7A_93)
    static let text4 = dynamic("mcpgw.text4", dark: 0x4753_6A, light: 0x98A4_B8)

    /// The beam: the accent, and also the healthy state. In a gateway "traffic
    /// is flowing and verified" and "the brand" are the same statement, which
    /// makes green the resting state of the whole interface — and that is what
    /// gives amber and red their force.
    ///
    /// `#3FD69B` is only 1.9:1 on white, so light mode gets its own value.
    static let beam = dynamic("mcpgw.beam", dark: 0x3FD6_9B, light: 0x0B8F_63)
    static let warn = dynamic("mcpgw.warn", dark: 0xE5A2_44, light: 0xA267_0C)
    static let deny = dynamic("mcpgw.deny", dark: 0xF25F_6B, light: 0xC832_3F)

    // ── Helpers ─────────────────────────────────────────────────────────

    private static func dynamic(_ name: String, dark: UInt32, light: UInt32) -> Color {
        Color(
            nsColor: NSColor(name: NSColor.Name(name)) { appearance in
                NSColor(hex: appearance.isDark ? dark : light)
            })
    }

    private static func dynamicAlpha(
        _ name: String,
        dark: (UInt32, CGFloat),
        light: (UInt32, CGFloat)
    ) -> Color {
        Color(
            nsColor: NSColor(name: NSColor.Name(name)) { appearance in
                let (hex, alpha) = appearance.isDark ? dark : light
                return NSColor(hex: hex, alpha: alpha)
            })
    }
}

extension NSAppearance {
    var isDark: Bool {
        bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
    }
}

extension NSColor {
    convenience init(hex: UInt32, alpha: CGFloat = 1) {
        self.init(
            srgbRed: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: alpha
        )
    }
}

// ── Tone ────────────────────────────────────────────────────────────────

/// The only colour vocabulary in the app. Everything that carries a state maps
/// to one of these four, and nothing else is allowed to be coloured.
enum Tone {
    case ok, warn, deny, neutral

    var color: Color {
        switch self {
        case .ok: Palette.beam
        case .warn: Palette.warn
        case .deny: Palette.deny
        case .neutral: Palette.text3
        }
    }

    /// The rail's colour. Neutral goes to a line rather than to text, because a
    /// rail is structure — it should be findable, not readable.
    var rail: Color {
        self == .neutral ? Palette.lineStrong : color
    }

    var wash: Color {
        self == .neutral ? Palette.lineSoft : color.opacity(0.12)
    }
}

// ── Status colours ──────────────────────────────────────────────────────

extension BackendStatus {
    var tone: Tone {
        switch self {
        case .ready: .ok
        case .starting: .warn
        case .failed, .crashed: .deny
        case .disabled, .stopped: .neutral
        }
    }
}

extension ConnState {
    var tone: Tone {
        switch self {
        case .connected: .ok
        case .connecting, .reconnecting: .warn
        case .error: .deny
        case .idle: .neutral
        }
    }

    var tint: Color { tone.color }
}

extension LogLevel {
    var tone: Tone {
        switch self {
        case .error: .deny
        case .warn: .warn
        // There is no info-blue in this system. An informational line is just a
        // line; only the exceptions earn a colour.
        case .info, .debug, .trace: .neutral
        }
    }

    var tint: Color { tone.color }
}

extension CallStatus {
    var tone: Tone {
        switch self {
        case .ok: .ok
        case .error: .deny
        case .running: .warn
        }
    }

    var tint: Color { tone.color }
}

// ── Metrics ─────────────────────────────────────────────────────────────

/// Corner radii are concentric: an inner radius is the outer radius minus the
/// padding between them. Nesting a 16 inside a 16 makes the inner shape look
/// wrong, and it is the single most visible tell of a UI that has not been
/// looked at. Same four steps as the dashboard.
enum Radius {
    static let control: CGFloat = 6
    static let row: CGFloat = 8
    static let card: CGFloat = 12
}

/// The type scale — a minor third off a 13pt base, whole points only. The same
/// nine steps the dashboard uses.
///
/// Weight and colour do more of the hierarchy work than size does: a 13pt value
/// at `.semibold` in primary text separates from a 13pt label at `.medium` in
/// tertiary text more cleanly than two regular weights two points apart.
enum Typo {
    static let micro: CGFloat = 10
    static let caption: CGFloat = 11
    static let small: CGFloat = 12
    static let body: CGFloat = 13
    static let medium: CGFloat = 15
    static let large: CGFloat = 18
    static let title: CGFloat = 22
    static let display: CGFloat = 34

    /// Identifiers — tool names, agent ids, hosts, timestamps, durations — are
    /// always monospaced. Most of the nouns in this product are things you
    /// could type, and the mono is what tells you which ones at a glance.
    ///
    /// The Mac uses the system monospace rather than the dashboard's IBM Plex
    /// Mono: the *rule* is shared, not the font file, and a Mac app in a
    /// bundled UI face reads as a port rather than a native app.
    static func mono(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
        .system(size: size, weight: weight, design: .monospaced)
    }
}

enum Metrics {
    /// The same 232 as the dashboard's sidebar. Next to an unbounded content
    /// column it says navigation serves content; a 320 would say they are peers.
    static let sidebarWidth: CGFloat = 232
    static let cardPadding: CGFloat = 16
    static let gutter: CGFloat = 14
    static let pagePadding: CGFloat = 20
    /// The width of the gate rail. Same 3 points as the web.
    static let rail: CGFloat = 3
}
