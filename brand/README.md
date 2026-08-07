# Brand assets

The **Aperture** mark: three traces converge on a single point and pass through a
diamond aperture, leaving as one beam. Many MCP servers, one guarded endpoint.

Read it left to right and it is the product — many in, one out, through a
threshold.

It is drawn on lucide's 24 grid with round caps and joins so it sits correctly
beside the lucide icons used in the dashboard and the SF Symbols used in the
agent. Stroke is **2.2**, not lucide's 2: this is a logo rather than an icon in a
row, and it needs the extra presence.

Two things are load-bearing, and both were arrived at by rasterising the
alternatives and looking at them rather than by reasoning about them:

- **The traces meet at the aperture's left vertex.** Convergence and threshold
  are one event, not two shapes near each other. When they were separate — a
  converging wedge, then a chevron gate — the two right-facing wedges fused into
  a "»" at *every* size, including 512 px. It read as fast-forward.
- **The aperture spans 8 of the 24 units.** Anything smaller closes up with
  antialiasing by 16 px, and 16 px is the floor.

| File | Use |
|------|-----|
| `mcp-gateway-mark.svg` | The glyph on its own. Uses `currentColor` — set the colour on the parent. |
| `mcp-gateway-wordmark.svg` | Mark + "MCP Gateway" lockup for READMEs and page headers. |
| `agent-app-icon.svg` | macOS app icon artwork for the agent, 1024 on Apple's grid. |
| `agent-tray-Template.svg` | macOS menu-bar icon. Template image — black + alpha only. |

`../mcp-gateway.svg` is a colour-baked copy of the mark for the project README,
because a README `<img>` cannot inherit `currentColor`.

## Colour

The accent is **phosphor** — the green of a live trace on a monitored line. In
this product "the system is working" and "the brand" are the same statement, so
the accent and the healthy state are one colour. That makes green the resting
state of the whole interface, which is what gives amber and red their force.

| Token | Dark surfaces | Light surfaces | Use |
|-------|---------------|----------------|-----|
| Beam (accent · healthy) | `#3FD69B` | `#0B8F63` | The mark; liveness, links, focus, active nav |
| Warn (degraded) | `#E5A244` | `#A86A0B` | Reconnecting, slow, admin-risk |
| Deny (failed) | `#F25F6B` | `#C8323F` | Crashed, denied, destructive-risk |

The mark is **never** a large filled shape and the accent is **never** a button
fill. Primary actions use a near-white fill on dark and near-black on light, so
the accent stays scarce enough to mean something. Roughly 90 % of any screen is
cold steel neutral; colour is the other 10 %, and it always carries information.

`#3FD69B` on white is 1.9:1 — unreadable. Anything that renders the accent on a
light surface must use the light column above.

The agent and the gateway share one mark. The agent's app icon carries the dark
steel plate so the two are distinguishable in the Dock; everywhere else the mark
is the flat phosphor stroke.

## Type

The rule that matters, and it is shared by both apps: **every identifier is set
in mono.** Tool names, backend names, agent IDs, timestamps, durations, ports,
hashes. Most of the nouns in this product are things you could type, and the mono
is what tells you which ones at a glance. Prose, labels and headings are the sans.

What differs is only the face, because the two runtimes want different things:

| | Sans | Mono |
|---|---|---|
| Dashboard | IBM Plex Sans | IBM Plex Mono |
| macOS agent | SF Pro (system) | SF Mono (system) |

Plex is an engineering typeface and its mono shares the sans's skeleton, so on
the web the two sit together without a seam. The Mac app uses the system faces
instead: a native app in a bundled UI face reads as a port, and the rule the two
share is "identifiers are mono", not "the same font file".

The dashboard's fonts are self-hosted (`@fontsource-variable/ibm-plex-sans`,
`@fontsource/ibm-plex-mono`), latin subset only, bundled into the image. **Never
add a font CDN link** — the gateway is meant to run air-gapped.

## The gate rail

A gateway is a channel things pass through, so the interface has one: a **3 px
rail down the left edge of a row, carrying that row's verdict** — green for
allowed and healthy, amber for stopped or degraded, red for failed, and a dim
hairline for idle.

It is the product's signature and it appears at every scale: the audit timeline,
the tool inventory, the backend list, the policy chain, the security-posture
checklist, and (2 px, in the accent) the active item in the dashboard sidebar.
The point is that a page of it is readable *peripherally* — a column of green
with red notches tells you the state of the gateway before you have read a word.

## Generating the macOS icon set

`macos/build.sh` does this on every build, but by hand:

```bash
# Any SVG rasteriser works; qlmanage ships with macOS.
qlmanage -t -s 1024 -o . agent-app-icon.svg
mv agent-app-icon.svg.png agent-app-icon-1024.png
```

For the menu bar, export `agent-tray-Template.svg` at 22×22 and 44×44 as
`agent-tray-Template.png` and `agent-tray-Template@2x.png`. **Keep the
`Template` suffix** — AppKit keys off the filename to decide whether to invert
the icon for a light menu bar and highlight it when the menu is open. A coloured
tray icon looks wrong on a light menu bar and does not highlight.

## Rules

- Minimum size 16 px.
- Clear space: at least the mark's own height on every side.
- Never re-colour the individual strokes; the mark is one colour.
- Never add a stroke to the wordmark text or set it in a different family.
- The mark exists in three places — this directory, `BrandMark.swift`, and
  `BrandMark.tsx`. They are the same five paths. **Change all three together**,
  or the app and the dashboard stop looking like the same product.
