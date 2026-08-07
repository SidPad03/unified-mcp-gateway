import SwiftUI

/// The MCP Gateway "Aperture" mark.
///
/// Three traces converge on a single point and pass through a diamond aperture,
/// leaving as one beam: many MCP servers, one guarded endpoint.
///
/// This is a direct transcription of `brand/mcp-gateway-mark.svg` — the same
/// five paths the dashboard's `BrandMark.tsx` renders and the app icon and tray
/// icon are cut from. **The product has one logo.** If this drifts from the SVG,
/// the app and the dashboard stop looking like the same product, so change all
/// three together.
struct BrandMarkShape: Shape {
    func path(in rect: CGRect) -> Path {
        // Drawn on the 24 grid and mapped into `rect`, centred, so the mark
        // keeps its proportions in a non-square frame.
        let scale = min(rect.width, rect.height) / 24
        let originX = rect.midX - 12 * scale
        let originY = rect.midY - 12 * scale
        func point(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
            CGPoint(x: originX + x * scale, y: originY + y * scale)
        }

        var path = Path()

        // The many: three traces converging on the aperture's left vertex.
        // They meet *at* the vertex rather than near it — when convergence and
        // threshold were two separate shapes, the two right-facing wedges fused
        // into a "»" at every size.
        path.move(to: point(2, 6.5))
        path.addLine(to: point(5, 6.5))
        path.addLine(to: point(9, 12))

        path.move(to: point(2, 12))
        path.addLine(to: point(9, 12))

        path.move(to: point(2, 17.5))
        path.addLine(to: point(5, 17.5))
        path.addLine(to: point(9, 12))

        // The threshold. Eight of the 24 units across — anything smaller closes
        // up with antialiasing by 16 px, and 16 is the floor.
        path.move(to: point(9, 12))
        path.addLine(to: point(13, 8))
        path.addLine(to: point(17, 12))
        path.addLine(to: point(13, 16))
        path.closeSubpath()

        // The one that leaves.
        path.move(to: point(17, 12))
        path.addLine(to: point(22, 12))

        return path
    }
}

/// The mark, stroked at the right weight for its size.
///
/// Stroke scales with the glyph — 2.2 units on the 24 grid — so it reads the
/// same at 16 pt in a sidebar and at 72 pt on the welcome screen.
struct BrandMark: View {
    var size: CGFloat
    /// Stroke weight on the 24 grid. 2.2 is the standard cut; go heavier below
    /// about 20 pt, where the standard one goes weak.
    var weight: CGFloat = 2.2

    var body: some View {
        BrandMarkShape()
            .stroke(
                style: StrokeStyle(
                    lineWidth: size / 24 * weight,
                    lineCap: .round,
                    lineJoin: .round
                )
            )
            .frame(width: size, height: size)
            .accessibilityHidden(true)
    }
}

/// The mark on its tinted tile — the lockup used for app identity.
struct BrandTile: View {
    var size: CGFloat
    var cornerRadius: CGFloat = Radius.card

    var body: some View {
        BrandMark(size: size * 0.62, weight: size > 44 ? 2.2 : 2.5)
            .foregroundStyle(Palette.beam)
            .frame(width: size, height: size)
            .background(Palette.beam.opacity(0.10), in: .rect(cornerRadius: cornerRadius))
            .overlay(
                RoundedRectangle(cornerRadius: cornerRadius)
                    .stroke(Palette.beam.opacity(0.24), lineWidth: 1)
            )
    }
}

/// Mark + wordmark. The only place the two appear together is app identity.
struct BrandLockup: View {
    var size: CGFloat = 22
    var subtitle: String?

    var body: some View {
        HStack(spacing: 9) {
            BrandMark(size: size, weight: size < 20 ? 2.5 : 2.2)
                .foregroundStyle(Palette.beam)
            VStack(alignment: .leading, spacing: 2) {
                Text("MCP Gateway")
                    .font(.system(size: Typo.body, weight: .semibold))
                    .tracking(-0.2)
                    .foregroundStyle(Palette.text)
                if let subtitle {
                    Text(subtitle)
                        .font(Typo.mono(Typo.micro))
                        .foregroundStyle(Palette.text4)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }
            Spacer(minLength: 0)
        }
    }
}
