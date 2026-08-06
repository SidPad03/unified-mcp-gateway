# Brand assets

The **Converging Gate** mark: three traces converge on a single point and pass
through a diamond threshold, leaving as one beam — many MCP servers, one guarded
endpoint.

It is drawn on lucide's 24 grid at stroke 2 with round caps and joins, so it sits
correctly beside the lucide icons already used in the dashboard and the agent
app, and it stays legible down to 16 px.

| File | Use |
|------|-----|
| `mcp-gateway-mark.svg` | The glyph on its own. Uses `currentColor` — set the colour on the parent. |
| `mcp-gateway-wordmark.svg` | Mark + "MCP Gateway" lockup for READMEs and page headers. |
| `agent-app-icon.svg` | macOS app icon artwork for the agent, 1024 on Apple's grid. |
| `agent-tray-Template.svg` | macOS menu-bar icon. Template image — black + alpha only. |

## Colour

| Token | Value | Use |
|-------|-------|-----|
| Accent | `#7C5CFC` | The mark on dark surfaces; primary UI accent |
| Accent (on light) | `#5B3FD4` | The mark on white or light surfaces |
| Agent tile | `#9B7BFF` → `#7C5CFC` → `#3AA9D9` | The agent app icon only |

The agent and the gateway share one mark. The agent's app icon carries the
violet-to-cyan tile so the two are distinguishable in the Dock; everywhere else
the mark is flat accent violet.

## Generating the macOS icon set

Tauri builds the whole `.icns` + PNG set from a single 1024 PNG:

```bash
# Any SVG rasteriser works; qlmanage ships with macOS.
qlmanage -t -s 1024 -o . agent-app-icon.svg
mv agent-app-icon.svg.png agent-app-icon-1024.png

npm run tauri icon brand/agent-app-icon-1024.png
```

For the menu bar, export `agent-tray-Template.svg` at 22×22 and 44×44 as
`agent-tray-Template.png` and `agent-tray-Template@2x.png`. **Keep the
`Template` suffix** — AppKit keys off the filename to decide whether to invert
the icon for a light menu bar and highlight it when the menu is open. A coloured
tray icon looks wrong on a light menu bar and does not highlight.

## Rules

- Minimum size 16 px. Below that, the diamond threshold closes up.
- Clear space: at least the mark's own height on every side.
- Never re-colour the individual strokes; the mark is one colour.
- Never add a stroke to the wordmark text or set it in a different family.
