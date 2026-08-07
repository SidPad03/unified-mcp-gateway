# MCP Gateway — interface design system

Direction: **Signal Desk**. A cold instrument you read, with one thing alive on
it. This is an ops console someone opens between two terminal windows when they
need to know something — not an app they live in. It answers a question fast and
gets out of the way.

The human: someone who self-hosts. They wired up eight or ten MCP servers, and
they open this when something broke, when they're adding a server, or when they
want to know what the AI actually did with the destructive tools.

Applies to both surfaces:

- `mcp-gateway-dashboard` — React 19 + Tailwind v4 + Vite
- `mcp-gateway-agent/macos` — SwiftUI on macOS 26 (Liquid Glass)

They are one product. Same tokens, same names, same vocabulary; only the
painting differs (flat fills on the web, OS-composited materials on the Mac).

---

## Direction

| | |
|---|---|
| Feel | Cold, quiet, precise. A rack panel in an unlit room, not a SaaS brochure. |
| Domain | The gate · traces & routes · the manifest · the ledger · clearance levels · the perimeter · telemetry · heartbeat |
| Colour world | Unlit-room black that is *cold*, not violet. Phosphor green on a scope. An amber annunciator lamp. A signal-red interlock. Anodized aluminium — a blue-grey, never a warm grey. |
| Density | Compact. 13px base, 16px card padding, 36px rows, 28px page padding. |
| Depth | **Tonal elevation + hairline borders.** Shadows only on genuinely floating layers (dropdown, modal). Never mixed. |

**The accent is the healthy state.** In a gateway, "traffic is flowing and
verified" and "the brand" are the same statement. That makes green the resting
state of the whole interface, which is exactly what gives amber and red their
force. The accent is **never a button fill** — a primary action is a near-white
fill on dark, near-black on light. That keeps green scarce enough to mean
"alive" rather than "clickable".

Roughly 90% of any screen is cold steel neutral. Colour is the other 10% and it
always carries information. There is no decorative colour, and no info-blue.

---

## The signature: the gate rail

A gateway is a channel things pass through, so the interface has one.

**A 3px rail down the left edge of a row, carrying that row's verdict.** Green
allowed/healthy · amber stopped/degraded · red failed · dim hairline idle.

It appears at every scale, and this is what makes the product recognisable:

1. the audit timeline (the ledger's spine)
2. tool inventory rows — carrying risk severity
3. backend list rows — carrying health
4. the policy chain — carrying each rule's decision, dark when disabled
5. the security-posture checklist
6. log lines and activity rows in the Mac app
7. 2px, in the accent, as the active item in the dashboard sidebar

The point is *peripheral* reading: a column of green with red notches tells you
the state of the gateway before you have read a word.

**The Mac's sidebar is the deliberate exception.** A macOS sidebar's selection is
an OS-drawn glass capsule; replacing it with a web-style rail would fight the
platform for nothing. Navigation stays native there; the rail lives on the data.

Implementations: `railStyle(tone)` / `<RailRow>` / `<RailList>` in
`components/ui.tsx`; `RailRow` and `.railed(tone)` in `Design/Components.swift`.

---

## Tokens

Source of truth: `mcp-gateway-dashboard/src/index.css`, mirrored in
`mcp-gateway-agent/macos/Sources/MCPGatewayAgent/Design/Theme.swift`. They are
duplicated rather than shared because the dashboard's Docker build context is
`./mcp-gateway-dashboard` only. **Change both together.**

Every surface is the same cold hue (~217°); only lightness moves, in steps of
2–3%. You should feel the stack rather than see it.

| Token | Dark | Light |
|---|---|---|
| `--void` (canvas, sidebar) | `#06080B` | `#F1F4F9` |
| `--panel` (cards) | `#0B0E14` | `#FFFFFF` |
| `--raised` (hover, nested) | `#10141C` | `#F6F8FC` |
| `--high` (popover, modal) | `#161B25` | `#FFFFFF` |
| `--inset` (fields, wells) | `#04060A` | `#EDF1F7` |
| `--line` / `-soft` / `-strong` | `rgba(148,173,214, .10/.055/.18)` | `rgba(15,35,70, .11/.07/.20)` |
| `--text` → `--text-4` | `#E4E9F2` `#9FADC4` `#6B7A93` `#47536A` | `#0D131C` `#46536B` `#6B7A93` `#98A4B8` |
| `--beam` (accent = healthy) | `#3FD69B` | `#0B8F63` |
| `--warn` | `#E5A244` | `#A2670C` |
| `--deny` | `#F25F6B` | `#C8323F` |
| `--solid` / `--on-solid` | `#E4E9F2` / `#06080B` | `#10151E` / `#F7F9FC` |

Rules that are easy to get wrong:

- **Inputs are `--inset` — darker than the surface around them, in both
  themes.** A field receives content, so it reads as inset, not raised.
- **The sidebar is `--void`, the same as the canvas.** Giving it its own fill
  splits the app into two worlds; a hairline is enough.
- `#3FD69B` is 1.9:1 on white. Anything rendering the accent on a light surface
  must use the light column.
- Light carries elevation with a three-layer shadow; dark collapses to a 1px
  ring. Depth shadows do not read on near-black.
- Theme is an explicit `data-theme` on `<html>`, set by an inline script in
  `index.html` *before first paint*. The stylesheet therefore never needs a
  duplicate `prefers-color-scheme` block, and "system" keeps tracking the OS live.

### Type

**Every identifier is set in mono** — tool names, backend names, agent IDs,
timestamps, durations, ports, hashes. Most of the nouns in this product are
things you could type, and the mono is what tells you which ones at a glance.
This is a rule, not a flourish.

| | Sans | Mono |
|---|---|---|
| Dashboard | IBM Plex Sans (variable, latin, self-hosted) | IBM Plex Mono 400/500/600 |
| macOS agent | SF Pro | SF Mono |

Never add a font CDN link — the gateway is meant to run air-gapped.

Scale — a minor third off a 13px base, whole pixels only. Weight and colour do
more of the hierarchy work than size:

`micro 10 · 2xs 11 · xs 12 · sm 13 (base) · md 15 · lg 18 · xl 22 · 2xl 26 · 3xl 34`

Label voice, used everywhere: `10px / 600 / .16em / uppercase / --text-3`.

### Radius (concentric — inner = outer − padding)

`control 6 · row 8 · card 12 · panel 16`

### Motion

Under 300ms, ease-out only, `transform`/`opacity` only, never `transition: all`.
`--ease-out-quint: cubic-bezier(.23,1,.32,1)`. Press feedback `scale(0.97)`.
Overlays start at `scale(.96)`, never 0. `prefers-reduced-motion` respected
globally. The **only** animation carrying meaning is the pulsing status dot —
it's how "connected" differs from a screenshot of connected.

---

## Component patterns

Web: `src/components/ui.tsx` · Mac: `Design/Components.swift`. Same names.

| Pattern | Spec |
|---|---|
| `Card` | `--panel` · 1px `--line` · radius 12 · `p-4` · shadow only on light |
| `PageHeader` | 22px/600 title · 12px `--text-3` description (≤68ch) · actions right |
| `SectionHeader` / `Label` | 10px/600/.16em uppercase `--text-3` |
| `Stat` (the focal figure) | 26px/600/tabular, `-0.02em` · 10px label above · **one per view** |
| `MiniStat` (supporting tier) | 15px/600/tabular · 10px label · deliberately ~half the Stat |
| `Button` | h 28/32/36 · radius 6 · 12px/500 · `active:scale-[.97]` · primary = `--solid`, never the accent |
| `IconButton` | 32px visible, hit area pushed to 40 with `after:-inset-1` |
| Fields | h32 (h36 in forms) · `--inset` fill · focus = `--beam-edge` border + 3px `--beam-wash` ring, and the global focus outline is suppressed on `.field` so there aren't three treatments |
| `Badge` / `RiskBadge` | h20 · radius 6 · 10px/500 |
| `RailRow` | grid `[3px 1fr auto]` · 36px min · `--line-soft` divider |
| `Table` | `Th` 36px, 10px uppercase · `Td` 12px · rail via `railStyle(tone)` on the first cell |
| `Modal` | native `<dialog>` + `showModal()` |
| `UsageFlowBoard` (Mac) | four columns, widths `1 : 1.1 : 1 : 1.4` of the space offered · 16pt gutter · nodes centred vertically (top-aligned once expanded) · links drawn behind, one per hop |
| `AuditHeaderRow` / `AuditRow` (Mac) | 32pt header · time 86 · tool flexible · application 120 · risk 84 · duration 70 · status 74 · rail inset 11, matched by the header's leading padding |

### The names in this product are namespaced twice

A local server called `obsidian` publishing `obsidian_patch_note` reaches the
gateway as `sids-macbook-pro__obsidian__obsidian_patch_note`: the agent prefixes
the backend, the gateway prefixes the agent, and the server repeats itself in the
tool. Splitting on the first `__` therefore yields the *agent's* name — which is
how the Mac's Usage page came to draw a Backends column that was an exact copy of
the This Mac column beside it.

`ToolName.split(_:agent:known:)` strips the agent, then matches the remainder
against the machine's *actual* backends (longest name wins) rather than guessing
at the delimiter, and `ToolName.shorten(_:backend:)` drops a backend name the
tool repeats. Anything that draws a tool name in a context that already says
which machine and which server it came from should use them: the prefix is the
part carrying no information and the part that survives truncation.

### Two decisions worth keeping

**Risk is a ramp, not six hues.** Risk is *ordinal*, so emphasis climbs with
severity and only the two levels that warrant action take a colour:

`read` `--text-4` → `write` `--text-3` → `execute` `--text` → `admin` amber →
`destructive` red → `unclassified` dashed outline (it's a *gap*, not a level).

The same ramp drives chart fills (`RISK_FILL` in `components/chart.tsx`). The old
build gave each category its own hue — emerald, blue, orange, red, purple, grey —
which made a page of tools look like a paint chart and said nothing about which
ones to worry about.

**Overlays are native `<dialog>`.** `showModal()` gives the focus trap, Escape,
the top layer, background `inert` and a real `::backdrop`. The previous build had
ten hand-rolled `fixed inset-0` modals with none of it. The backdrop is styled in
`index.css` with a literal colour, not a `backdrop:` utility — `::backdrop` only
inherits custom properties in recent browsers, and a modal with no scrim is a
broken modal.

Similarly: `<select>` stays native (styled closed state, OS-drawn popup) so it
keeps its keyboard handling, ARIA and type-ahead; checkboxes are native with
`accent-color`.

---

## Responsive

The dashboard is used at a desk, but it has to survive a phone — checking why a
backend went red is exactly the thing you do away from one.

**The shell.** `h-dvh`, never `h-screen`: on mobile browsers `100vh` includes the
collapsing URL bar, so a vh-sized shell puts its own bottom edge off-screen. The
shell is a flex column with `min-h-0` on the scroller, so the top bar takes its
height and the content gets the rest — no viewport arithmetic. Sidebar becomes an
off-canvas drawer below `lg`.

**Page padding is the shell's job.** `px-4 py-5 · sm:px-5 sm:py-6 · lg:px-7 lg:py-7`.
A page that wants the full area declares it in `FULL_BLEED` in `Layout.tsx`; it
does **not** claw the padding back with a negative margin. The usage graph used
to carry `-m-8` against the old `p-8`, and it silently broke the moment the
shell's padding changed — 4px of bleed on desktop, 12px on mobile, plus 48px of
phantom scroll from an `h-screen` nested in a `calc(100vh-3rem)` parent.

**Tables rank their columns; they do not reflow.** Eight columns at 375px is a
925px scroller, and a nested horizontal scrollbar is not a mobile layout. `Th`
and `Td` take `hide="sm|md|lg|xl"` — the breakpoint at which that column
*appears*. Everything hidden stays reachable by expanding the row, so the
expansion has to actually contain it (the audit expansion was missing its own
timestamp).

Two things that are easy to get wrong here:

- **The railed column is never the hidden one.** `railStyle` is an inline
  `box-shadow`, and no responsive class can turn an inline style off — hiding it
  just moves the rail to a second cell and draws two.
- **`max-width` on a `<td>` does nothing.** An auto-layout table sizes columns to
  min-content, and `truncate` sets `white-space: nowrap`, which makes min-content
  the *whole* string. Give the cell's inner element a definite width —
  `w-[min(150px,34vw)] sm:w-[190px] …` — so it can actually shrink.

**The Mac app has the same problem in miniature, and the same answer.** The
window's floor is 900×560, so the detail pane is 668 points wide — and four
168-point columns with three 46-point gutters is 810, which is how the Usage
page came to be clipped at the right edge *and* have its range picker pushed out
of the window, since the page header shares the scroll view's width with
whatever is widest below it. Nothing in a fixed-size layout says what it adds up
to; the numbers have to be added up.

- Columns that hold data take **fractions of the width they are offered**, never
  a fixed size. Measure with a `GeometryReader` inside `.background`, never one
  wrapped around the content — a reader takes the whole height it is offered and
  reports it as its own, so inside a scroll view the card grows without bound.
- A table **ranks its columns and drops one** rather than squeezing all of them:
  `AuditColumn.wideEnoughForApplication`. Anything dropped stays in the row's
  detail sheet and in its accessibility label.
- A column that can run out of room puts the **word that carries the state
  first** — `crashed · 44 tools`, not `44 tools · crashed`.

**Watch the `lg` boundary.** The sidebar appears at `lg`, which shrinks the
content box by 232px at the exact moment `lg:` columns would reveal. Heavy
columns therefore wait for `xl`. Budget at 1024 with the sidebar is ~736px;
at 1280 it is ~992px.

Verify with `responsive.mjs`: it sweeps 320→1600 and asserts both that the
document never scrolls horizontally *and* that each table's natural width fits
its container — the second is the one that catches this, since a table inside
`overflow-x-auto` hides its own overflow from the first.

## Rules to hold

- Colour means something. Tone is `ok | warn | deny | neutral` and nothing else.
- **Red means broken; amber means stopped.** A policy denial is the gateway
  working, not failing — it must not read like an upstream 502.
- One focal element per view. If everything is the same size, nothing leads.
- No icon tile on a list row unless it carries information. They cost ~28px of
  every row and usually say nothing.
- Bind to tokens, never to a hex or a Tailwind ramp. `grep -E
  '(bg|text|border)-(gray|slate|zinc|emerald|blue|violet|…)-[0-9]'` over `src/`
  should return nothing.
- A number that can change gets tabular figures.
- States are not optional: default, hover, active, focus, disabled; loading,
  empty, error.

## Rules the two server-backed pages earned

Audit and Usage are the only pages that talk to the gateway, and every one of
these is something that read as a broken layout before it read as a broken
request.

- **A first load that fails is the page, not a banner.** A spinner with the
  reason exiled to a strip above it never resolves and never explains itself.
  Once there is something on screen, the same failure is a banner: the numbers
  are stale, not gone.
- **"Not configured" is a different state from "loading".** Every path that can
  return without data — no credentials, not registered — says which.
- **A count is only honest about what it searched.** "12 of 4,213" while the
  filter has only seen the hundred rows in memory is a lie in both directions;
  it now reads "12 of 200 loaded · 4.2K total", and there is a way to load more.
- **Page backwards by timestamp, never by offset.** Rows arrive at the top the
  whole time the page is open, and each one shifts an offset-based window by a
  row.
- **Discard a response whose request is stale.** Three range buttons mean three
  requests in flight, and they do not return in order.
- **Derive once, not per frame.** `body` reruns on every core tick — ten times a
  second — so a filter over two thousand events or a grouping over a hundred
  tools is memoised in `@State` and rebuilt on the things that actually change
  it, not recomputed in a `var`.

## Verifying

`scratchpad/shoot.mjs`-style sweep: every page × light/dark at 1440, plus a
narrow pass at 820. It asserts **no horizontal document overflow** and collects
console errors — the two cheapest signals that a layout is broken. Charts should
be checked by reading tick labels out of the DOM, not by eye: `domain={[0,
'auto']}` silently let recharts draw a `-45` tick on a count series.

**The Mac app renders offscreen with `ImageRenderer`.** `screencapture` needs a
screen-recording grant a build script does not have; `ImageRenderer` needs
nothing, and a throwaway SwiftPM target that symlinks `Theme.swift`,
`Components.swift` and the view under test will render it to a PNG at any width
you like. Three things do not survive the trip and are worth knowing before you
conclude a view is broken: `glassEffect` renders as nothing *and takes the card's
contents with it* (stand a flat panel in for `Card`), `List` and `ScrollView`
render empty (draw the rows in a clipped `VStack` instead), and AppKit-backed
controls — `Picker`, `TextField` — come out as yellow blocks.

This is why the views worth checking take plain values: `UsageFlowBoard` knows
nothing about `AgentModel`, `UsageGraph` or the network, and `AuditRow` needs
only an `AuditEvent`, so both can be rendered and looked at without a gateway.
Keep it that way — a view that can only be seen by signing in is a view nobody
checks.
