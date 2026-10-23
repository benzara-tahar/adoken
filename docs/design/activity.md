# Activity: sync activity and network log

Status: v0.1, **Accepted** (2026-10-01, owner: all recommendations; spec **Accepted** the same day). Owner: ux-designer. Behavior, acceptance and budgets are in [`docs/specs/0016-statusbar-activity.md`](../specs/0016-statusbar-activity.md), whose two separable FR groups are **Sync activity** and **Network log**. The rest of this doc says "spec" for 0016. Tokens: [tokens.md](tokens.md). States: [principles.md](principles.md) §3. Statusbar anatomy: [principles.md](principles.md) §2. Sync states and lanes: [`docs/specs/0002-sync.md`](../specs/0002-sync.md) §3, FR-4/FR-4a, FR-7, FR-10. Format modeled on [pins.md](pins.md).

The user wants two things from the bottom of the window. First, "what is sync doing, and what did it just bring in?" Second, "what is adoken saying to Azure DevOps right now, and did it work?" Both answers sit one click or one key away, in one **Activity drawer** with two tabs, **Sync** and **Network**. The statusbar shows the headline: the sync item on the left (now a button), and a network indicator on the right. The design goal is **calm at rest, legible under load**. Nothing animates per request, success is quiet, failures carry a shape, a word and a count, and the log never moves under the cursor.

This doc owns the statusbar layout, the drawer, both tabs, glyphs, pills, motion, copy and accessibility. The spec owns what is recorded, how long it is kept, tone rules, and every number marked "spec". Each tab ships or gets cut on its own: if one tab is cut, the drawer has no tab strip, and the cut tab's statusbar item stays as it is today (§3.2).

Sketch glyphs: ⇅ network (`arrow-up-down`), ◌ spinning (`loader-circle`), ✓ `circle-check`, ✕ `circle-x`, △ `circle-alert`, ⏱ `timer`, ‖ `pause`, ⊘ `circle-slash`, ◇ `circle-dot-dashed`, ⚿ `lock`, ⌁ `wifi-off`. The real Lucide names are in §11.

## 1. Concept and placement

```
┌──────────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●   ‹ ›   Contoso / Payments ▾              ⌘P Search…                ◔  ⚙ │ titlebar 38
├────────────┬─────────────────────────────────────────────────────────────────────┤
│ ☼ Today    │                                                                     │
│ ✉ Inbox 12 │  content (list + detail) — shrinks while the drawer is open         │
│ ⎇ PRs    3 │                                                                     │
│ …          │                                                                     │
│            ├═════════════════════════════════════════════════════════════════════┤ resize handle
│ Pinned   5 │ Sync   Network ✕2                                      ⤢   ✕       │ drawer header 32
│ …          │ ─────────────────────────────────────────────────────────────────── │
│            │  tab content (§4 Sync, §5 Network)                                  │ drawer (content
│ ⚙ Settings │                                                                     │ column only)
│ (DK) Conto │                                                                     │
├────────────┴─────────────────────────────────────────────────────────────────────┤
│ ● Synced 12s ago · Contoso                         ⇅ 200 ✕2   ×3 Combo  ✶5      │ statusbar 24
└──────────────────────────────────────────────────────────────────────────────────┘
```

| Surface | What it answers | Opens from |
|---|---|---|
| Statusbar sync item (§2.2) | Is my data fresh? | always visible |
| Statusbar network indicator (§2.3) | Is anything in flight? Did something fail? | always visible (hidden in demo and when turned off, spec) |
| Drawer › Sync tab (§4) | What does each kind sync, when, and what changed in recent passes? | click the sync item · `⌘⇧A` · ⌘P "Activity: Show sync activity" |
| Drawer › Network tab (§5) | Every request: when, what for, status, latency, size, cost, and totals per endpoint | click the network indicator · `⌘⇧A` · ⌘P "Activity: Show network log" |

**Why a docked drawer.** A popover (0002 §3's original "click opens a popover") closes on the first click outside it, which defeats "watch what's being synced". A dense eight-column log also needs the width of the content column. The drawer is a pane, not an overlay. It sits in the **content column only**: the sidebar or rail keeps its full height, so the pins height budget ([pins.md](pins.md) §1.1, 7 pin rows at 600 px) holds (spec FR-2). Opening an item from the drawer updates the content above it, and the drawer stays open.

## 2. Statusbar

### 2.1 Layout

```
 ← 12 →                                                                            ← 12 →
 │ [sync item ────────────] · org  [notice…]  ┄┄ flex ┄┄  [network] 8 [Ken chip] 8 [meter] 8 [demo pill] │
   button, §2.2               text   truncates            §2.3       existing     existing
```

Left to right:
1. **Sync item** (button, §2.2): state glyph + label.
2. **Org**: "· Contoso", `type.small`, `text.secondary`. Plain text, not interactive, the same as today.
3. **Notice** (existing one-shot message, e.g. the code-font fallback): `status.warning.fg`. It truncates first.
4. **Center:** quiet-hours / snooze text, as principles §2 describes. It isn't in `render_statusbar` today. When it lands, it sits in the flex region and follows the §2.5 priority.
5. **Flex spacer.**
6. **Network indicator** (button, §2.3): the leftmost item of the right group. Its width changes only on real state changes, and those changes take room from the spacer, never from the items to its right.
7. **Ken chip** (existing, only when the Ken layer is on).
8. **Resource meter** (spec 0023, only when `[activity] resource_meter` is on; off by default): §2.7. After the Ken chip so that the network indicator's and the Ken chip's width changes never move it (amended 2026-10-01, owner notes; accepted 2026-10-01).
9. **Demo pill** (existing, demo mode only).

Gaps: `space.6` inside the left group (as today), `space.8` between right-group items. Bar: `size.statusbar` 24, `bg.sidebar`, top hairline `border.subtle`, `type.small`, `text.secondary` (all as today).

**Offline.** Principles §3.5 asks for an "Offline" pill on the right. The network indicator's slot *becomes* that pill while offline (§2.3), so the window has one offline signal on the right and the sync label on the left keeps its existing "Offline — last synced 3 min ago" text.

### 2.2 Sync item (button)

| Part | Spec |
|---|---|
| Glyph | Existing statusbar glyphs and tones (`crates/app/src/sync.rs` `indicator`): ● `status.success.fg` (ok), ◔ `status.info.fg` (busy), ▲ `status.warning.fg` / `status.danger.fg`, ○ `text.tertiary` (quiet). **New:** Paused uses Lucide `pause` at `size.icon.xs` in `text.secondary` (the Ken chip already uses that icon in the statusbar). |
| Label | Copy in §2.6; `type.small`, `text.secondary`. |
| Hit target | Glyph + label, full bar height 24 (`size.hit_min`), padding `space.6` each side. The org text is outside the button. |
| Rest | No background: it reads exactly as today, so no new chrome when idle. |
| Hover | `bg.hover` fill, inset `space.2` top and bottom (20 px tall), `radius.xs`; in over `motion.fast`. |
| Pressed | `bg.active`. |
| Drawer open on Sync | `bg.selected_muted` fill (same inset) while the Sync tab shows: it says "this is what you're looking at". |
| Focus | Focus ring (principles §4) when reached by keyboard (§6.3). |
| Tooltip (500 ms) | "{full state sentence} · Sync activity ⌘⇧A · Sync now ⌘R", e.g. "Synced at 10:41:12 · Sync activity ⌘⇧A · Sync now ⌘R". Paused adds "· ⌘R still syncs once". |
| Click | Opens the drawer on the Sync tab. If the drawer is already open on Sync, it closes (toggle). If it's open on Network, it switches to Sync. |

### 2.3 Network indicator (button)

```
 idle      │ ⇅ 200 │                 glyph text.tertiary, last status text.tertiary
 busy      │ ◌ 3 │                   loader-circle status.info.fg (spinning), in-flight count text.secondary
 problems  │ ⇅ 200 ┃✕ 2┃ │           + danger chip: circle-x status.danger.fg, count text.primary on status.danger.wash
 warning   │ ⇅ 429 ┃△ 1┃ │           + warning chip: circle-alert status.warning.fg on status.warning.wash
 offline   │ ┃⌁ Offline┃ │           the offline pill (principles §3.5): wifi-off + "Offline", text.secondary on bg.raised
 no data   │ ⇅ │                     glyph only (no request since launch, no token, token rejected before any response)
```

| Part | Spec |
|---|---|
| Glyph | `arrow-up-down` (new icon) at `size.icon.xs` 12 in `text.tertiary`. While busy, `loader-circle` in `status.info.fg` with `glyph-spin` on the shared ~30 fps clock, only while visible. Under reduce motion it is a still `loader-circle`, and the different shape alone says "busy". |
| Value | `type.label` + `type.numeric` (tabular). Idle: the status code of the last completed request ("200", "304"), or "Canceled" / "Failed" when there was no code, in `text.tertiary` whatever the class. Color lives in the chip, never in this number. Busy: the in-flight count in `text.secondary`, "99+" above 99. The slot has a min width equal to three tabular digits, so "200" → "3" → "200" never moves the glyph. |
| Problems chip | Only when the spec's unseen-failure count (last 5 min, not yet seen) is > 0. Pill: height 16, `radius.full`, horizontal padding `space.4`, `space.4` gap after the value. Danger tone: `circle-x` 12 in `status.danger.fg` + count in `text.primary` on `status.danger.wash`. Warning tone (no danger failures, only warning ones): `circle-alert` in `status.warning.fg` on `status.warning.wash`. Danger wins when both exist; the tooltip lists both. Text stays `text.primary`, because tinted 11 px text falls under 4.5:1 on light-theme washes (§9.3). |
| Offline | The whole indicator turns into the offline pill: `wifi-off` 12 + "Offline" (`type.label`), `text.secondary` on `bg.raised`, `radius.full`, height 16, padding `space.6`. It stays a button (it still opens the Network tab). |
| Hit target / states | Same as the sync item: 24 tall, padding `space.6`, hover `bg.hover` inset `space.2`, pressed `bg.active`, `bg.selected_muted` while the Network tab shows. |
| Hidden | In demo mode, and when the setting "Show network activity in the statusbar" is off (Settings › Sync, spec). The drawer stays reachable through `⌘⇧A` and ⌘P. |
| Click | Opens the drawer on the Network tab (toggle rule as §2.2). Opening the Network tab marks failures seen (spec), so the chip clears **after** the tab has painted, never on the click frame. |

Tooltip (500 ms), one line per fact:

| State | Tooltip |
|---|---|
| Idle | "Last request 10:41:12 · GET Work items · 200 · 84 ms" / "No requests yet" · then "Network log ⌘⇧A" |
| Busy | "3 requests in flight · oldest 1.2 s" · "Network log ⌘⇧A" |
| Problems | "2 failed in the last 5 min · latest 10:40:58 POST Work items batch · 503" (+ "· 1 warning" when both) · "Network log ⌘⇧A" |
| Offline | "Offline — nothing is sent until you reconnect · Network log ⌘⇧A" |

### 2.4 Micro-states without flicker (coalescing)

A sync pass fires bursts of 5–15 requests, some only tens of ms long. The indicator samples, it never mirrors:

| Rule | Value | Why |
|---|---|---|
| Repaint cap | At most one indicator repaint per 250 ms (≤ 4 Hz, spec), and only when the sampled state differs from the painted one. No timer runs while nothing changes. | Frame budget. A burst of 15 responses is 1–2 repaints, not 15. |
| Busy appears | Only after ≥ 1 request has been in flight for 150 ms (principles §3.2) | Sub-150 ms requests never show a spinner. |
| Busy holds | ≥ 300 ms once shown (principles §3.2), and **bridges** idle gaps < 500 ms | A pass reads as one busy stretch, not a strobe. |
| Count while busy | Sampled on the 250 ms tick; never animates | Numbers change in place. |
| Last status | Updated only when busy ends (or on the idle tick) | "200" doesn't tick through every code during a burst. |
| Problems chip | Appears on the first tick after a failure. Its count changes in place and it clears when seen or aged out (spec) | The only state that persists, because it's the one worth noticing. |
| Offline | Swaps in the next frame (FR-5: "Offline at once"); on reconnect it swaps back to idle with `motion.base` opacity (principles §3.5 "pill fades out") | |
| Reduce motion | Spinner still; everything else is already instant | |

Model and view stay separate. The recorder publishes start/finish events to an indicator model on the UI thread. The model holds `in_flight`, `busy_since`, `busy_shown_at`, `last_completed`, and the unseen danger/warning counts, and calls `cx.notify()` only from the 250 ms sampler. [INFERENCE: implementation shape; the spec owns the event contract.]

### 2.5 Width priority (900 px minimum window)

At 900 px the bar has 876 px inside its padding. Worst realistic case at `font_scale` 1.0, measured at ~6.3 px per `type.small` character: "▲ Throttled by Azure DevOps — resuming in 40s" ≈ 285, "· contoso-engineering-platform" ≈ 190, network with chip ≈ 85, Ken chip ≈ 95 → 655 with gaps, so everything fits. Demo can't co-occur with sync states ("● Demo data" ≈ 80 + demo pill ≈ 257). At `font_scale` 1.3, or with a long notice, items give way in this order:

1. **Notice** truncates (min width 0, trailing ellipsis; full text in its tooltip).
2. **Resource meter** drops its CPU part: "260 MB" (spec 0023 FR-8; amended 2026-10-01, owner notes; accepted 2026-10-01).
3. **Org** truncates to 64 px, then hides. It's also in the titlebar's project switcher.
4. **Resource meter** hides (its palette commands stay; amended 2026-10-01, accepted 2026-10-01).
5. **Quiet-hours text** collapses to its `moon` glyph (tooltip keeps the sentence).
6. **Demo pill** goes to its short form "Demo mode" (tooltip: "Demo mode · fake data, stays on this Mac").
7. **Network value** hides: glyph + problems chip only.
8. **Sync label** goes to its short form (§2.6 "Short" column). The glyph and the short label never hide.

The Ken chip already has its own compact form (`moon` while calm, combo + streak otherwise) and never hides. Every collapsed item keeps its full sentence as tooltip and accessible label.

### 2.6 Statusbar copy

Sync item (existing copy from `crates/app/src/sync.rs` `label` and `render_statusbar`, plus **new** rows):

| State | Glyph | Label | Short (§2.5) | Spoken (label) |
|---|---|---|---|---|
| Opening local data | ◔ info | Opening local data… | Opening… | "Opening local data" |
| Local data failed | ▲ danger | Couldn't open local data | Local data error | same |
| Demo | ● info | Demo data | Demo data | "Demo data, not synced" |
| Connecting | ◔ info | Connecting… | Connecting… | same |
| First sync | ◔ info | First sync… | First sync… | same |
| Syncing | ◔ info | Syncing… | Syncing… | "Syncing" |
| Synced | ● success | Synced 12s ago | 12s ago | "Synced 12 seconds ago" |
| Not synced yet | ○ tertiary | Not synced yet | Not synced | same |
| **Paused, timed (new)** | `pause` secondary | Sync paused · until 15:00 | Paused | "Sync paused until 3 PM. Command R still syncs once" |
| **Paused, until resumed (new)** | `pause` secondary | Sync paused | Paused | "Sync paused. Command R still syncs once" |
| Offline | ▲ warning | Offline — last synced 3 min ago | Offline · 3 min ago | same |
| Throttled | ▲ warning | Throttled by Azure DevOps — resuming in 40s | Throttled · 40s | same, "40 seconds" |
| Token rejected | ▲ danger | Token rejected | Token rejected | "Token rejected. Azure DevOps refused the token" |
| No token | ▲ danger | No token in Keychain | No token | same |
| No projects | ○ tertiary | No projects | No projects | same |

Paused while a ⌘R pass runs shows "Syncing…" (the pass is real), then returns to "Sync paused · until 15:00". Principles §3.3's "▲ Sync delayed — retrying in 30s" isn't produced by the code today; per-kind failures show in the drawer (§4.2), and the statusbar keeps the copy above.

Network indicator: values and tooltips in §2.3. Spoken labels in §9.1.

### 2.7 Resource meter (spec 0023)

Amended 2026-10-01 (owner notes; accepted 2026-10-01 with spec 0023). Off by default; Settings › General "Show memory and CPU in the statusbar", or `⌘P` › "Statusbar: Show memory and CPU".

```
 normal     │ 260 MB · 1% CPU │      type.small + type.numeric, text.tertiary; no glyph
 no CPU yet │ 260 MB │               first 2 s after turning on (no CPU interval yet)
 collapsed  │ 260 MB │               §2.5 step 2
 large      │ 1.2 GB · 3% CPU │
```

| Part | Spec |
|---|---|
| Text | "{memory} · {cpu}% CPU". Memory: "260 MB" (nearest 10 MB below 995 MB), "1.2 GB" from 995 MB; CPU whole percent of one core. Rounding and hysteresis: spec FR-6. `type.small` with `type.numeric` (tabular), `text.tertiary` always: no tone, no color, no glyph at any value (spec FR-13). |
| Width | Min width = the tabular width of "990 MB · 99% CPU" so values inside that range never move a neighbor; it grows only for "1.2 GB" / three-digit CPU. |
| Hit target / states | Button, as the sync item (§2.2): 24 tall, padding `space.6`, hover `bg.hover` inset `space.2`, pressed `bg.active`, focus ring on keyboard focus. Last Tab stop of the statusbar. |
| Tooltip (500 ms hover, or on focus) | Four lines: "adoken is using 260 MB of memory, as Activity Monitor counts it." · "Resident 325 MB · peak 373 MB since launch" · "CPU 1% of one core over the last 10 s · 2m 14s since launch" · "adoken keeps caches so views open instantly. Memory rises as you open things and settles; a high number alone isn't a leak." Peak is omitted when the fallback source is in use (spec FR-4). Tooltip numbers are whole MB, not rounded to 10. |
| Click / `↩` / `Space` / right-click | Menu (`menu-in`, `size.action_menu_width` 280, anchored above the meter, right-aligned): "Copy resource usage" (`copy` icon) · "Hide from statusbar" (`eye-off` icon). `Esc` closes it and returns focus to the meter. |
| Copied line | "adoken 0.1.0 · memory 260 MB (resident 325 MB, peak 373 MB) · CPU 1% over 10 s, 2m 14s total · up 3h 12m". Toast (existing copy toast): "Copied resource usage". |
| Spoken label | "adoken memory 260 megabytes, CPU 1 percent" (collapsed states keep it). Not a live region: value changes are never announced. |
| Motion | None: the text changes in place (`motion.instant`), at most once per 5 s (spec FR-6). |
| Gallery | `statusbar-resource-meter`, `statusbar-resource-meter-narrow`, `statusbar-resource-meter-gigabytes`, `statusbar-resource-meter-menu` (spec AC-11). |

## 3. Activity drawer

### 3.1 Geometry

| Property | Value |
|---|---|
| Placement | Between the content and the statusbar, inside the **content column** (right of the sidebar or rail). The content above shrinks; nothing overlays. |
| Height | Default 40 % of the window, min 160, max 70 % (spec), with a content floor of 120 px (§3.3). Persisted `[activity] panel_height` (spec). At the 600 px minimum window: body 538, drawer 240 by default, content 298. |
| Surface | `bg.surface` (a pane: × `surface_opacity` over custom backgrounds, tokens §10.2), `elevation.0`, top edge 1 px `border.default`. Sticky table headers `bg.raised` (z `z.sticky`). |
| Inner padding | `space.12` horizontal (table and list rows run edge to edge inside it), no vertical padding: header, toolbars and rows stack flush. |
| Header | 32 px (`size.row.compact`): tab strip left, window buttons right (§3.2). Bottom hairline `border.subtle`. |

### 3.2 Header and tabs

```
 │12│ Sync │16│ Network ┃✕2┃ │                                  flex │ ⤢ │4│ ✕ │12│
```

| Part | Spec |
|---|---|
| Tab | Text `type.body_strong` 13; inactive `text.secondary`, active `text.primary` with a 2 px `accent.solid` underline flush with the header's bottom edge, the tab's text width + `space.4` each side. Hover `text.primary`. Tabs are 32 tall, padding `space.8`, gap `space.8`. |
| Tab badge | Network only: the same problems chip as the statusbar (§2.3), shown while the Network tab is not active. Sync shows a still `loader-circle` 12 in `status.info.fg` after its label while a pass runs, and a `circle-x` 12 in `status.danger.fg` when a kind is failing. Shapes, not dots, so they are never color-only. |
| One tab shipped | No tab strip: the header shows the tab's title ("Sync activity" / "Network log") in `type.body_strong`, `text.primary`. |
| Maximize / restore | Ghost icon button 24 × 24, `maximize-2` / `minimize-2` (new icons) at `size.icon.sm`, `text.secondary`. Maximize = max height (70 %); restore = the persisted height. Tooltip "Maximize ⌘⇧↩" / "Restore ⌘⇧↩" (§6.2). |
| Close | Ghost icon button 24 × 24, `x` (new icon), tooltip "Close  ⎋". |

### 3.3 Resize

- **Handle:** the drawer's top edge, a 1 px `border.default` line with an 8 px hit band centered on it (pointer `row-resize`). Hover or drag: the line becomes 2 px `border.strong` over the full content width.
- **Drag** is 1:1 and instant (tokens `pane-resize`: drag = instant). It clamps at min 160 and max 70 % (spec), and also never leaves the content less than 120 px (`size.row2.comfortable` 68 + list header): at a 600 px window, max = min(70 %, 538 − 120) = 418. Double-click the handle toggles maximize / restore.
- **Keyboard:** the handle is a focusable `Role::Splitter` (§9.1): `↑` / `↓` move it 24 px, `⇧↑` / `⇧↓` 96 px, `Home` / `End` max / min. Each step is instant.
- **Window resize:** the drawer keeps its fraction of the window; it clamps, it never animates.

### 3.4 Open and close motion

| Trigger | Open | Close |
|---|---|---|
| Pointer (statusbar click, close button) | Height 0 → h over `motion.base` 200, `ease.out`; the content's height follows in the same frames (`pane-resize`). Drawer content renders at full height from frame 1, clipped, so nothing reflows inside it. | Height → 0 over `motion.base_exit` 130, `ease.in` |
| Keyboard (`⌘⇧A`, palette, `⎋`) | Instant (principles §1.1: keyboard beats smoothness) | Instant |
| Tab switch (click, `⌃Tab`, statusbar item while open) | Instant | — |
| Reduce motion | Instant | Instant |

Input during the animation jumps it to its end state (tokens §7.3). Focus moves into the drawer on the first frame (§6.3), not when the animation ends.

## 4. Sync tab

### 4.1 Layout

```
 ┌ toolbar 32 ─────────────────────────────────────────────────────────────────────────────┐
 │ ● Synced 12s ago · next in 40s                    [⟳ Sync now ⌘R] [‖ Pause ▾] [Settings…] │
 ├ kinds (300) ─────────────────────┬ recent passes (flex) ─────────────────────────────────┤
 │ ✓ My pull requests        1 min  │ ◌ now     All active PRs, Work items   Syncing… 6 req │
 │   Synced 12s ago · next in 48s   │ ▾ 10:41  My pull requests  3 PRs updated   9 req 1.1 s│
 │ ◌ All active pull requests 2 min │     Pull requests 3                                   │
 │   Syncing · 3 of 12 projects     │     ⎇ !4821 Retry idempotency keys      New comment   │
 │ ✓ Work items              2 min  │     ⎇ !4817 Bump azure-identity          Vote changed  │
 │   Synced 1 min ago · next in 1m  │     ⎇ !4809 Drop legacy retry            Pushed        │
 │ ✓ Pipeline runs            30 s  │ ▸ 10:40  Pipeline runs  1 run finished      2 req 0.3 s│
 │ ✓ Search index and projects 30 m │ ▸ 10:38–10:40  No changes · 4 passes                  │
 │ ⚲ Pinned items                   │ ▸ 10:37  You: Sync #20101  1 work item changed        │
 └──────────────────────────────────┴────────────────────────────────────────────────────────┘
```

- **Two columns** when the drawer is ≥ 760 px wide: kinds 300 px (`border.subtle` divider), passes flex. **One column** below 760: kinds, then a "Recent passes" group label, then the passes, all in one scroll. Each column scrolls on its own in two-column mode.
- The **toolbar** (32 px, bottom hairline `border.subtle`) repeats the statusbar sentence at `type.body` and adds the next due pass: "Synced 12s ago · next in 40s". The actions (Sync now, Pause / Resume, Settings…) sit at the right end of it, at the top, so they're seen first and the Tab order reaches them first.

### 4.2 Kind rows

Two-line rows, `size.row2.compact` 48, padding `space.12`, `radius.sm` selection inset 4 px (principles §4). Order fixed, as Settings › Sync:

| Row | Name (line 1) | Cadence (line 1, trailing) |
|---|---|---|
| My pull requests | "My pull requests" | "1 min" |
| All active pull requests | "All active pull requests" | "2 min" |
| Work items | "Work items" | "2 min" |
| Pipeline runs | "Pipeline runs" | "30 s" |
| Search index and projects | "Search index and projects" | "30 min" |
| Pinned items (derived, spec) | "Pinned items" | none (refreshed inside other passes) |

Anatomy: state glyph `size.icon.sm` 14 at `space.12`, `space.8` gap; line 1 = name in `type.body_strong` 13, `text.primary`, trailing cadence in `type.small` + `type.numeric`, `text.tertiary` ("Manual only" / "Paused" replace the cadence when they apply); line 2 = state sentence in `type.small`, `text.secondary`, one line, trailing ellipsis, full text in the tooltip.

| State (spec) | Glyph | Line 2 copy |
|---|---|---|
| Synced | `circle-check` `status.success.fg` | "Synced 12s ago · next in 48s" |
| Syncing, progress known | `loader-circle` `status.info.fg`, spinning | "Syncing · 3 of 12 projects" (unit by kind: projects, PRs, work items, runs) |
| Syncing, progress unknown | same | "Syncing…" |
| Not synced yet | `circle-dot-dashed` `text.tertiary` | "Not synced yet · first pass in 10s" / "Not synced yet" |
| Manual only | `circle-dashed` `text.tertiary` | "Manual only · synced 14 min ago · ⌘R syncs it" |
| Paused | `pause` `text.secondary` | "Paused until 15:00 · synced 4 min ago" / "Paused · synced 4 min ago" |
| Background slower | `circle-check` `status.success.fg` | "Synced 2 min ago · next in 8 min (in the background)" |
| Waiting: rate limit | `timer` `status.warning.fg` | "Slowed to protect your rate limit · next in 8 min" |
| Throttled | `clock` `status.warning.fg` | "Waiting for Azure DevOps · resuming in 40s" (spec 0016 AC-A9 and 0004 FR-15 wording; the resource is in the toolbar banner, §4.5) |
| Failed | `circle-x` `status.danger.fg` | "Couldn't sync: Azure DevOps returned 503 · retrying in 30s" / "Couldn't sync: no response · retrying in 2 min" |
| Offline | `wifi-off` `text.tertiary` | "Offline · synced 3 min ago" |
| Needs scope | `lock` `text.tertiary` | "Your token doesn't include Build (Read)" (name per scope, as pins.md §6.2) |
| Pinned items | `pin` `text.tertiary` | "Refreshed with All active pull requests · 1 min ago" / "Nothing pinned" |

Relative times update on the existing 5 s statusbar tick (`STATUS_TICK`) and only while the drawer is visible. "next in" counts in 10 s steps under a minute ("next in 40s"), whole minutes above ("next in 8 min"). It never ticks every second.

Interaction: `↑`/`↓` (`j`/`k`) move between rows; `↩` opens Settings › Sync at that kind's cadence row; `⌘K` opens the row menu (§6.2). Row tooltip: the full line 2 + "Last pass 10:41:12 · 9 requests · 0.004 TSTU".

### 4.3 Recent passes

Newest first; the last 200 passes or 2 h, memory only (spec). One row per pass, 28 px (`size.control.md`), padding `space.12`:

```
│▸│ 10:41:12 │ My pull requests, Pipeline runs │ 3 PRs updated · 1 run finished │ 9 req · 1.1 s · 0.004 │
  12  time 64     lanes (flex, max 40 %)           summary (flex)                      meta (trailing)
```

| Part | Spec |
|---|---|
| Disclosure | `chevron-right` / `chevron-down` 12, `text.tertiary`. Only when the pass has changes. |
| Time | `HH:MM:SS`, `type.small` + `type.numeric`, `text.tertiary`. A coalesced run shows "10:38–10:40". |
| Lanes | Kind names joined ", " in `type.small`, `text.secondary`. User-started passes start with the `user` glyph 12 in `text.secondary` and read "You: Sync now", "You: Sync #20101" (per-item sync, spec 0015), or "You: Open !4821" (user-tier fetches). In demo, "Demo feed". |
| Summary | `type.small`, `text.primary` when there are changes ("3 PRs updated · 2 work items changed · 1 run finished"), `text.tertiary` "No changes" otherwise. Plural rules: "1 PR updated" / "3 PRs updated", "1 work item changed", "1 run finished" / "2 runs started", "214 search entries updated". |
| Meta | "9 req · 1.1 s · 0.004" (requests · wall time · TSTU), `type.label` + `type.numeric`, `text.tertiary`, right-aligned. Hidden below 760 px drawer width (it stays in the tooltip). |
| Outcome | Shown in the leading slot instead of the chevron when the pass didn't succeed: `circle-x` `status.danger.fg` + summary "Failed: 503 on Work items · retried in 30s"; `clock` `status.warning.fg` + "Waiting for Azure DevOps · paused after 4 requests"; `wifi-off` `text.tertiary` + "Stopped: offline". |
| Per-item sync ("You: Sync #20101", spec 0015) | The same row, with copy shared with the work item header ([work-item-detail.md](work-item-detail.md)). Running: spinner + "Syncing…". Done: summary "Synced just now" (it ages like any time, "Synced 2 min ago"), or the change summary when the item changed ("1 work item changed"). Re-pressed within 10 s: no new row; the row's summary briefly reads "Just synced" (0015 FR-30). Failed: `circle-x` + "Couldn't sync: {cause}" (the full cause here; the header shows only "Couldn't sync · Retry"). Throttled: `clock` `status.warning.fg` + "Waiting for Azure DevOps…" (0004 FR-15 wording, same as the header). |
| In progress | Pinned to the top while running: `loader-circle` spinning `status.info.fg`, time "now", summary "Syncing… 6 requests so far" (the count updates on the 250 ms sampler, §2.4). When it ends, it becomes a normal row in place (`motion.instant`). |

**Coalescing (display only).** Consecutive no-change, successful passes with the same lane set fold into one row: "10:38–10:40 · All active pull requests · No changes · 4 passes". A pass with changes or a failure always gets its own row. The newest pass with changes is expanded by default the first time the tab opens; after that, expansion state is the user's.

**Insertion.** New passes enter at the top. If the passes column is scrolled to the top and has no keyboard cursor below row 1, the row appears with `row-insert` (`motion.base`, pointer-only context). Otherwise it is inserted above the viewport with no visible movement (the scroll offset compensates) and a "↑ 2 new passes" pill appears at the top center (style §5.6). No layout moves under the cursor (tokens §7.3).

### 4.4 Changed items (expanded pass)

Grouped by kind in fixed order: Pull requests, Work items, Pipeline runs, Search index. Group label in `type.label`, `text.secondary`, with a count ("Pull requests 3"), 24 px, indent `space.24`.

Item row, 28 px, indent `space.24`:

```
│ ⎇ │8│ !4821 │8│ Retry idempotency keys on payment… │8│ New comment │
```

| Part | Spec |
|---|---|
| Glyph | Kind/type glyph per tokens §2.8 (`git-pull-request`, work item type glyphs, `circle-check`/`circle-x`… for runs), in its domain token. |
| Ref | `!4821` / `#20101` / run number "#20261001.3", `type.code_inline`, `text.secondary`. |
| Title | `type.body`, `text.primary`, flex, trailing ellipsis. |
| Change word | `type.label`, `text.secondary`, trailing. PRs: "New", "Updated", "New comment", "Vote changed", "Pushed", "Completed", "Abandoned", "Mentioned you". Work items: "New", "State: Active", "Assigned to you", "New comment", "Updated". Runs: "Started", "Succeeded", "Failed", "Partially succeeded", "Canceled". Several changes: the first + "+2" ("New comment +2"); the tooltip lists all. |
| Search index | No item rows (counts only): "214 entries updated", `text.secondary`. |
| Cap | 20 rows per group, then "Show all 54" (link button, `text.link`); it expands in place. |
| Actions | `↩` / click: open the canonical view (principles §1.5); the drawer stays open, focus stays in the drawer. `⌘↩` open in browser; `⌘D` pin (global, spec 0014); `⌘K` row menu (§6.2). |
| Gone item | An item no longer in the store: glyph and title in `text.tertiary`, change word "No longer synced"; `↩` does nothing; tooltip "Not in adoken anymore". |

### 4.5 Toolbar actions and banners

| Control | Look | Behavior copy |
|---|---|---|
| Sync now | Secondary button `size.control.sm`, `refresh-cw` 14 + "Sync now" + keycap `⌘R` (same as Settings › Sync). While a ⌘R pass runs: inline spinner replaces the icon, width kept (principles §3.2). | Offline: disabled, tooltip "Offline — reconnect to sync". Within 10 s of the last ⌘R: stays enabled; pressing shows the existing "just synced" hint (0002 §3), toolbar sentence "Synced just now". |
| Pause | Secondary button `pause` 14 + "Pause" + `chevron-down` 12 → menu: "Pause for 1 hour", "Pause until resumed". When paused: becomes "Resume" with `play` 14 (one click, no menu). | Toolbar sentence while paused: "Sync paused until 15:00 · ⌘R still syncs once". |
| Settings… | Ghost button "Sync settings…" | Opens Settings › Sync (`⌘,` lands there when the Sync tab is active, spec). |
| Throttled banner | Full-width row under the toolbar, 28 px, `status.warning.wash`, `circle-alert` `status.warning.fg`, `type.small` `text.primary`: "Azure DevOps is throttling requests · sync resumes in 40s · resource: {X-RateLimit-Resource}". Resource text in `type.code_inline`. | |
| Auth | See §7 (empty state replaces the kinds). | |

## 5. Network tab

### 5.1 Toolbar and filters

```
│ [Requests | Totals] │ [⌕ Filter purpose or path   /] [✕ Failed only 2] [All origins ▾] │ flex │ 1,284 requests │ [⧉] [⤓ Export…] │
```

| Control | Spec |
|---|---|
| View switch | Segmented control `size.control.sm`: "Requests" · "Totals". `←` / `→` switch while the table has focus (the list-tab convention of the PR, work item and pipeline lists). |
| Filter field | `bg.input`, `size.control.sm`, 200 px (flex down to 140), `search` 14 leading, placeholder "Filter purpose or path" (`text.tertiary`), keycap `/` trailing while empty. Matches purpose, path template, method and status code as plain substrings ("503", "post", "threads"), case-insensitive, applied on each keystroke. 10,000 rows filter in < 16 ms (spec budget). |
| Failed only | Toggle chip `size.control.sm`, `radius.full`: `circle-x` 12 + "Failed only" + unseen count. Off: `text.secondary`, 1 px `border.default`. On: `text.primary` on `bg.selected_muted`, pressed state exposed. "Failed" = danger + warning tones (spec). |
| Origin | Menu button: "All origins" · "Sync" · "You" · "Avatars"; label shows the choice ("Origin: You"). |
| Endpoint chip | Appears after "Show requests" on a Totals row (§5.5): "Endpoint: GET …/pullRequests/{n}/threads" with an `x` to clear; `⌫` in the empty filter field removes it (palette convention). |
| Count | "1,284 requests" / "12 of 1,284 requests" when filtered, `type.small` + `type.numeric`, `text.tertiary`. |
| Copy | Ghost icon button `copy`: copies the selected row (§5.7). Tooltip "Copy request ⌘C". |
| Export | Ghost button `download` (new icon) + "Export…" (§5.7). |
| Clear filters | `⌘⌫` (list convention) clears text, chip and origin. |

### 5.2 Requests table

Virtualized (`uniform_list`), newest at the **bottom**, following the tail (§5.6). Rows `size.row.dense` 24 (new token), header 24 sticky on `bg.raised` with `type.label` `text.secondary` column titles. Column gap `space.12`. Row text `type.small` + `type.numeric`; separators: none (density comes from row height; selection and hover washes carry the eye). Zebra off.

| Column | Width | Content | Shown when drawer is |
|---|---|---|---|
| Time | 88 | "10:41:12.304", `text.tertiary` | always |
| Method | 48 | "GET" / "POST" / "PATCH", `type.code_inline`, `text.secondary` | always |
| Status | 72 | Pill (§5.3) | always |
| Origin | 120 | `refresh-cw` 12 + "My PRs" / `user` 12 + "You: Vote" / `image` 12 + "Avatars", `text.secondary` | ≥ 1100 px; narrower: the glyph alone leads the Purpose cell |
| Purpose | flex, min 140 | "PR threads · !4821", "Work items batch", "Avatar"; `text.primary`; the subject (`!4821`, `#20101`) in `type.code_inline` `text.secondary` (in-app only, spec) | always |
| Path | flex, min 160 | Templated path (placeholders `{org}`, `{project}`, `{repo}`, `{id}`, `{sha}`, `{n}`, `{name}` per spec 0016 FR-19; `{repo}` and `{name}` arrive with the spec's route table), `type.code_inline`, `text.tertiary`, **leading** ellipsis ("…/pullRequests/{n}/threads"): the tail names the resource. Host in the tooltip and the detail. | ≥ 600 px (always at the 900 px window: content ≥ 668) |
| Latency | 64, right | "84 ms", "1.2 s", "12 s"; `text.secondary`. Queued ≥ 50 ms adds a leading `clock` 12 `text.tertiary` ("waited for a slot"). In flight: elapsed in `text.tertiary`, updated on the 250 ms sampler. | always |
| Size | 64, right | "12.3 KB", "0 B", "—" (no response), `text.tertiary` | ≥ 900 px |
| TSTU | 56, right | "0.004", "—" (header absent), `text.tertiary` | ≥ 760 px |

Column drop order as the drawer narrows: Origin → glyph (< 1100), Size (< 900), TSTU (< 760), Path (< 600). Hidden values remain in the row detail and the tooltip. Width math at the 900 px window with the sidebar (content 668 − 24 padding = 644): Time + Method + Status + Latency = 272, five gaps = 60, so Purpose and Path share 312 (140 / 172).

Row states: hover `bg.hover`; selected (table focused) `bg.selected`, unfocused `bg.selected_muted`; keyboard cursor = selection (principles §4). Purpose and Path of a row whose detail is open turn `text.primary`. Row tooltip (500 ms): full purpose, host + full path, status sentence.

### 5.3 Status pills

Success is quiet; anything else gets a wash, a glyph and a word or code. Color is never alone: every pill has text, and every non-success pill has a distinct shape. Pill: height 16, `radius.xs`, padding `space.4`, glyph `size.icon.xs` 12 + `space.2` + text `type.label` + `type.numeric`. **Text is `text.primary` on every wash** (§9.3); hue lives in the glyph and the wash.

| Class (spec tone) | Wash | Glyph | Text |
|---|---|---|---|
| 2xx, 304 (quiet) | none | none | Code, `text.secondary` ("200") |
| Other 3xx (quiet) | none | none | Code, `text.secondary` |
| Canceled (quiet) | none | `circle-slash` `text.tertiary` | "Canceled", `text.tertiary` |
| In flight | none | `loader-circle` `status.info.fg`, spinning while visible (one shared clock for all rows) | "…" `text.tertiary` |
| 429, or `Retry-After` / `X-RateLimit-Delay` > 0 on any code (warning) | `status.warning.wash` | `timer` `status.warning.fg` | Code ("429", or "200" for a delayed success) |
| Other 4xx (warning) | `status.warning.wash` | `circle-alert` `status.warning.fg` | Code ("404", "409") |
| 401, 403 (danger) | `status.danger.wash` | `lock` `status.danger.fg` | Code |
| 5xx (danger) | `status.danger.wash` | `circle-x` `status.danger.fg` | Code ("503") |
| No response (danger) | `status.danger.wash` | `circle-x` `status.danger.fg`; `wifi-off` when the failure was offline | "Failed" / "Offline" |

### 5.4 Row detail

`↩` or click toggles an inline detail block under the row (one open at a time; opening another closes the first). It's a user-caused layout change under the user's own cursor, so it's instant and never animated. Block: `bg.raised`, inset `space.12` left, padding `space.12`, `radius.sm`, two key/value columns (keys `type.label` `text.tertiary` width 96; values `type.code_inline` `text.primary`):

```
 Started     10:41:12.304 · Oct 1, 2026          Origin    Sync · Work items (warm lane)
 Host        dev.azure.com                       Attempt   2 of 3
 Path        /{org}/{project}/_apis/wit/workitemsbatch
 Status      503 Service Unavailable             Timing    waited 120 ms for a slot · 1.2 s request
 Size        —                                   Cost      — (no X-RateLimit-Cost)
 Error       Azure DevOps returned 503. Sync retries in 30s.
 Rate        Limit 200 · Remaining 182 · Reset 10:45:00 · Delay — · Resource —
 Retry-After 30 s
 Activity id 6f1c0e2a-…-9d41                     [⧉ Copy details ⌘C]  [Show totals for this endpoint]
```

- Rate headers render only when present; absent ones show "—". Rows with no rate headers and no error collapse to Started, Host, Path, Status, Timing, Size, Cost.
- Error text: the cause, in the error copy template (principles §3.4): "Azure DevOps returned 503. Sync retries in 30s." / "No response: the connection timed out after 30 s." / "Azure DevOps refused the token (401)." / "Your token doesn't allow this (403): Work Items (Read & write) is missing." The ADO free-text message appears in-app only; Copy/Export carry the code only (spec).
- Activity id: shown only when recorded (spec: `x-vss-activityid` is allow-listed and unverified), with its own copy button `copy` 12, label "Copy activity id".
- "Show totals for this endpoint" switches to Totals with that row selected.
- `Esc` closes the detail first; a second `Esc` closes the drawer.

### 5.5 Totals

The same toolbar with "Totals" active. A summary strip (28 px, `type.small`, `text.secondary`) sits above the table:

```
 Last hour  412 requests · 1.84 TSTU · 3 failed   │   Last 5 min 0.09 TSTU of 200 (0.05%)   │   Sync 1.62 · You 0.18 · Avatars 0.04
```

The window segmented control ("Last hour" · "Since launch", spec) leads the strip. Table rows 24 px, the same table chrome as §5.2:

| Column | Width | Content |
|---|---|---|
| Method | 48 | `type.code_inline` |
| Endpoint | flex, min 200 | Path template, one line, leading ellipsis, `type.code_inline` `text.primary`. The purpose label is in the tooltip and is what the Requests filter matches. |
| Requests | 64, right | count |
| Failed | 64, right | count; > 0 → `circle-x` 12 `status.danger.fg` before the number (text `text.primary`), else "0" `text.tertiary` |
| p50 / p95 | 64 / 64, right | "84 ms" / "410 ms" |
| Bytes | 72, right | "1.2 MB" (dropped < 900 px) |
| TSTU | 64, right | "0.412" — the default sort, descending |
| Mix | 56 | Stacked 6 px bar, `radius.full`: 2xx/304 `status.success.fg`, 4xx/429 `status.warning.fg`, 5xx/401/403/no response `status.danger.fg`, proportional. Decorative: the Failed column and the tooltip ("398 OK · 2 throttled · 1 failed") carry the meaning. Dropped < 760 px. |

Sort: click a header, or `⌘K › Sort by…` (Requests, Failed, p95, Bytes, TSTU, Endpoint). The sorted column shows `chevron-down` / `chevron-up` 12 next to its title. `↩` on a row = "Show requests" (Requests view with the Endpoint chip).

### 5.6 Live tail and stability

| Situation | Behavior |
|---|---|
| Following (scrolled to the bottom, cursor on the last row or none) | New rows append below; the viewport stays pinned to the bottom; the cursor, if on the last row, stays on its **entry** (it does not jump to the new last row). No row animation, ever: it's a log. |
| Not following (scrolled up, or cursor on an older row) | The viewport and cursor stay exactly where they are. A pill floats bottom-center, 8 px above the table bottom: `bg.overlay`, `elevation.2`, `radius.full`, `size.control.sm`, `arrow-down` 12 + "24 new requests" (`type.small`, `text.primary`). Click or `End`: jump to the bottom, resume following, pill gone (`motion.instant`). The pill's count updates on the 250 ms sampler. |
| Ring eviction (10,000 kept, spec) | Rows leave from the top. When scrolled up, the offset compensates so visible rows don't move. If the cursor's entry is evicted, the cursor moves to the oldest remaining row and the polite announcement says "Older requests were dropped". |
| Repaint | Table invalidation is coalesced to ≤ 4 Hz (as §2.4). In-flight rows update their status and latency cells in place. |
| Filters while live | New rows that don't match never appear and don't count in the pill. |

### 5.7 Copy and export

- **Copy row** (`⌘C`, the toolbar button, `⌘K › Copy`): the selected rows as JSON Lines, one object per row with the spec's export fields (spec FR-25, Q-9), never ids, names or ADO messages. Toast (existing, `copy` glyph): "Copied request" / "Copied 3 requests". With detail open: "Copy details" copies the detail block as `key: value` lines (error code only, activity id included when present): toast "Copied request details". Sound `ui.copy` (off by default, sound.md).
- **Export…** (button, ⌘P "Network log: Export…"): native save panel straight away, default name `adoken-network-2026-10-01-1041.jsonl`, JSON Lines only (spec FR-25, Q-9: one format for Export, `⌘C` and `ADOKEN_SYNC_LOG`). Exports the **current filter** (the button's tooltip gives the count: "Export 12 requests…"); with no filter, everything kept. Fields per spec. Success toast "Exported 1,284 requests" [Show in Finder]. Failure: inline toast "Couldn't save the export: {OS reason}. The log is unchanged." [Try again]. Nothing is uploaded; the save panel is the only destination.

### 5.8 Privacy footer

A 24 px footer under the table on both views, always visible (spec): `lock` 12 `text.tertiary` + "Stays on this Mac. No query strings, bodies, headers or token." (`type.small`, `text.tertiary`). Right side: "Kept since 09:12 · cleared when you quit" (`type.small`, `text.tertiary`, tabular), "1,284 of 10,000 kept" when the ring is full.

## 6. Keyboard

### 6.1 Global

Checked against [command-palette.md](command-palette.md) §7.2 and every `KeyBinding::new` / `*_KEYS` in `crates/app/src/**` on 2026-10-01: `secondary-shift-a` is bound only in the onboarding context (`Onboarding && !TextInput`, `ClearRows`), which never coexists with the Workspace. `ctrl-tab` and `ctrl-shift-tab` are unbound. `⌘⇧R` is claimed by spec 0015 (per-item sync, work item contexts) and is **not** used here.

| Key | Action | Context | Action id (proposed) |
|---|---|---|---|
| `⌘⇧A` | Toggle the Activity drawer (reopens the last tab) | Workspace | `activity::Toggle` |
| `⌘R` | Sync now (existing; also the Sync tab button) | global | `sync::SyncNow` |

Palette (`>`): "Activity: Show sync activity", "Activity: Show network log", "Activity: Hide", "Network log: Export…", "Network log: Clear filters", plus the existing "Sync: Pause for 1 hour", "Sync: Pause until resumed", "Sync: Resume", "Sync: Settings…". "Show sync activity" and "Show network log" display `⌘⇧A` as their keycap, because that's the key that opens the drawer.

### 6.2 Inside the drawer (`ActivityPanel && !TextInput` context)

| Key | Action |
|---|---|
| `⌃Tab` / `⌃⇧Tab` | Next / previous tab (Sync ⇄ Network) |
| `←` / `→` | Network: Requests ⇄ Totals (list-tab convention) |
| `↑` `↓` / `j` `k` | Move the cursor in the focused list or table |
| `Home` / `End` | First / last row; `End` resumes following (§5.6) |
| `PageUp` / `PageDown` | Page |
| `↩` | Sync: open the item / expand or collapse a pass / open Settings at a kind. Network: toggle row detail. Totals: show that endpoint's requests. |
| `→` / `←` on a pass row | Expand / collapse (Sync tab only, where `←`/`→` have no view switch) |
| `⌘↩` | Open the selected item in the browser (Sync tab item rows) |
| `/` | Focus the Network filter field |
| `⌘⌫` | Clear Network filters |
| `⌘C` | Copy the selected request (or its details when open) |
| `⌘K` | Row menu: Sync items: Open · Open in browser · Pin/Unpin · Copy link. Requests: Copy · Copy details · Show totals for this endpoint · Filter to this endpoint. Totals: Show requests · Sort by… · Copy. Kinds: Sync settings for this kind… |
| `⌘⇧↩` | Maximize / restore the drawer (same meaning as the global full-width toggle: "make this pane big"; the drawer context shadows it only while focus is inside) |
| `Tab` / `⇧Tab` | Cycle toolbar controls → kinds list → passes list (Sync) or toolbar → table (Network), then the resize handle |
| `Esc` | Unwind: close menu → close row detail → leave the filter field → close the drawer (focus returns to the invoker) |

Single letters (`j`, `k`, `/`) apply only in the `ActivityPanel && !TextInput` context, so they type normally in the filter field and never collide with spec 0015's work-item letters, which live in `WorkItemList` / `WorkItemDetail`.

### 6.3 Focus

- **Opening** by any path moves focus into the drawer: the Sync tab focuses the kinds list (cursor on the first row that isn't "Synced", else row 1); the Network tab focuses the table (cursor on the newest row, following). Menus and the palette opened from the drawer return focus to it when they close.
- **Closing** (`Esc`, close button, `⌘⇧A`, statusbar click) returns focus to the element focused before the drawer opened. If that element is gone, focus goes to the content's list.
- **Clicking content above** moves focus there; the drawer stays open. `⌘⇧A` always toggles, so from the content it **closes** an open drawer. `F6` moves focus between content and drawer without closing (it extends principles §4's `F6` toast cycle: content → drawer → toasts).
- **Statusbar items** aren't Tab stops (the bar would add two stops to every Tab walk). Keyboard users reach both through `⌘⇧A`, `⌃Tab` and the palette. Screen-reader users get both items as labelled buttons in the a11y tree (§9.1), operable with their AT's default action.

## 7. States

Every state for both surfaces. The drawer reads only memory and the store, so it has **no loading state**: the first frame shows whatever is known.

| State | Statusbar sync item | Network indicator | Sync tab | Network tab |
|---|---|---|---|---|
| Normal idle | "● Synced 12s ago" | "⇅ 200" | Kinds Synced; passes listed | Rows, following |
| Syncing | "◔ Syncing…" | "◌ 3" (after 150 ms) | Running kinds spin; in-progress pass on top | In-flight rows with "…" pills |
| First sync | "◔ First sync…" | "◌ 4" | Kinds "Not synced yet" → "Syncing · 3 of 12 projects"; passes: the in-progress pass only | Rows streaming in |
| Empty history (just launched, nothing run yet) | "○ Not synced yet" / "Connecting…" | "⇅" glyph only | Kinds with "Not synced yet · first pass in 10s"; passes column: `EmptyState` (§7.1) | `EmptyState` "No requests yet" |
| Failures (some) | unchanged unless sync itself fails | "⇅ 200 ✕2" | Failed kind row (§4.2) + failed pass row | Danger/warning pills; "Failed only 2" chip |
| Throttled | "▲ Throttled by Azure DevOps — resuming in 40s" | "⇅ 429 △1" | Throttled kinds + banner with resource (§4.5) | `timer` 429 pills; detail shows Retry-After |
| Rate pressure (slowed, not throttled) | unchanged | unchanged | Affected kinds "Slowed to protect your rate limit · next in 8 min" | Delayed rows get the `timer` warning pill |
| Paused | "‖ Sync paused · until 15:00" | idle | Kinds "Paused…"; toolbar "Resume"; banner none (toolbar sentence says it) | ⌘R and write traffic still listed |
| Background slower / paused | unchanged label | unchanged | Kinds "…(in the background)" / "Paused while adoken is in the background" | — |
| Offline | "▲ Offline — last synced 3 min ago" | Offline pill "⌁ Offline" | Kinds "Offline · synced 3 min ago"; Sync now disabled | Kept rows; offline failures show "Offline" pills |
| No token | "▲ No token in Keychain" | "⇅" glyph only | `EmptyState` (§7.1) replaces kinds and passes | `EmptyState` "No requests: no token" |
| Token rejected (401) | "▲ Token rejected" (or "▲ Token expired" when `validTo` is past); no banner, modal or toast (spec 0001 FR-15; amended 2026-10-01, owner notes; accepted 2026-10-01) | "⇅ 401 ✕1" | `EmptyState` token rejected (§7.1) | The 401 rows (`lock` pills) |
| Missing scope (one kind) | unchanged | unchanged | That kind "Your token doesn't include Build (Read)", passive: no button, no `⌘K` action (spec 0001 FR-9; amended 2026-10-01, owner notes; accepted 2026-10-01) | 403 rows if any were sent |
| No projects | "○ No projects" | glyph only | `EmptyState` no projects | `EmptyState` "No requests yet" |
| Demo | "● Demo data" | hidden (spec) | Note row (§7.1) + "Demo feed" passes | `EmptyState` demo (§7.1) |
| Filtered to nothing | — | — | — | `EmptyState` filtered (§7.1) |
| Network indicator turned off | unchanged | hidden | unchanged | unchanged (reachable via `⌘⇧A`) |
| Drawer too short for content (160 px) | — | — | One column; kinds scroll | Toolbar + header + 3 rows + footer; table scrolls |

### 7.1 Empty-state and note copy

All use `EmptyState` (principles §3.1), centered in the column or table, max width 360. Actions show their keycap.

| Where | Icon | Headline | Body | Action |
|---|---|---|---|---|
| Sync › passes, none yet | `refresh-cw` | No passes yet | The first sync pass shows up here as it runs. Demo: "Demo feed activity shows up here when the feed adds some (it checks every 30 s)." (a tick that adds nothing records no pass, spec FR-11) | [Sync now ⌘R] (none in demo) |
| Sync, no token | `key-round` | No token in Keychain | adoken can't sync without one. Nothing was sent to Azure DevOps. | [Add token] (onboarding PAT sheet) |
| Sync, token rejected | `key-round` | Azure DevOps rejected adoken's token | Sync stopped. Your local data is kept. | [Replace token…] (opens Settings › Account and starts the replacement; spec 0001 FR-15, FR-16; amended 2026-10-01, owner notes; accepted 2026-10-01) |
| Sync, token expired | `key-round` | adoken's token expired on {date} | Sync stopped. Your local data is kept. | [Replace token…] |
| Sync, no projects | `folder-kanban` | No projects selected | Pick the projects adoken should sync. | [Choose projects…] (Settings › Projects) |
| Sync, demo note (row above the kinds, `info` 14 `text.tertiary`, `type.small` `text.secondary`) | — | — | "Demo mode: nothing syncs. A local feed adds fake activity every 30 s." Kinds rows read "Demo data isn't synced". | — |
| Network, none yet | `arrow-up-down` | No requests yet | Requests to Azure DevOps appear here as adoken makes them. | — |
| Network, no token | `key-round` | No requests | adoken has no token, so it sends nothing. | [Add token] |
| Network, demo | `arrow-up-down` | Demo mode sends no requests | Everything you see is fake data on this Mac. | — |
| Network, filtered to nothing | `search` | No requests match "threads 503" | (Failed only on, no text:) "No failed requests since 09:12." | [Clear filters ⌘⌫] |
| Totals, none | `arrow-up-down` | No requests in the last hour | Switch to Since launch to see older totals. | — |

## 8. Motion and sound

| Moment | Motion |
|---|---|
| Drawer open / close | §3.4 (`pane-resize` timing, pointer only; keyboard and reduce motion instant) |
| Resize drag | 1:1, instant |
| Tab switch, Requests ⇄ Totals | Instant |
| Network indicator busy glyph, in-flight pills, running kinds, in-progress pass | `glyph-spin` (1000 ms loop, `ease.linear`, shared ~30 fps clock, only while visible); still under reduce motion |
| Network indicator state changes | Instant swaps on the ≤ 4 Hz sampler (§2.4); offline pill out: opacity over `motion.base` |
| New log rows, evictions | None (§5.6) |
| New pass row | `row-insert` only when at the top with no cursor below row 1; otherwise none (§4.3) |
| Pass expand / collapse | Pointer: height over `motion.base` `ease.out` / `motion.base_exit` `ease.in`; keyboard: instant |
| Row detail | Instant |
| "N new" pills | Appear and disappear instantly; the count changes in place |
| Problems chip | Appears instantly; clears instantly after the Network tab has painted |

**Reduce motion:** every row above snaps; spinners are still `loader-circle` glyphs next to their words ("Syncing…", "…"). Nothing moves.

**Sound:** no new events, and the drawer is silent. Sync now from the drawer plays the existing `ui.sync_done` when that pass finishes (off by default). Copy plays `ui.copy` (off by default). Request failures play nothing: a failed write already plays `ui.error` from the surface that made it, and background failures are not the user's action. Mute and quiet hours apply as everywhere ([sound.md](sound.md)).

**Ken:** no arcade moments. This is an instrument panel, so Quiet mode has nothing to hide here, and a combo for "200 OK" would be noise.

## 9. Accessibility

### 9.1 Roles and labels

| Element | Role | Label (example) | Notes |
|---|---|---|---|
| Sync item | `Role::Button` inside a `Role::Status` wrapper | "Synced 12 seconds ago. Show sync activity, Command Shift A" | The wrapper is the existing polite live region (principles §6): label changes announce politely, at most once per 5 s tick. `aria_expanded` = drawer open on Sync. |
| Network indicator | `Role::Button` (not a live region) | Idle: "Network: last request returned 200. Show network log, Command Shift A" · busy: "Network: 3 requests in flight" · problems: "Network: 2 failed requests in the last 5 minutes, latest 503" · offline: "Offline. Nothing is sent until you reconnect" | Requests are never announced one by one. **One** polite announcement when the unseen danger count goes from 0 to ≥ 1: "Azure DevOps request failed: 503, work items batch", then at most once per 30 s ("3 more requests failed"). `aria_expanded` = drawer open on Network. |
| Drawer | `Role::Group` labelled "Activity" | | |
| Tab strip / tabs / panels | `Role::TabList` / `Role::Tab` (`aria_selected`) / `Role::TabPanel` | "Sync, syncing" · "Network, 2 failed" | Tab badges are part of the tab label. |
| Resize handle | `Role::Splitter` | "Resize activity drawer" | Value = height in px; keys in §3.3. |
| Maximize / close | `Role::Button` | "Maximize activity drawer" / "Restore…" · "Close activity drawer" | |
| Kinds list / row | `Role::List` / `Role::ListItem` | "Work items, synced 1 minute ago, next in 1 minute, every 2 minutes" · "All active pull requests, syncing, 3 of 12 projects" | |
| Passes list / pass row | `Role::List` / `Role::ListItem` (`aria_expanded`) | "10:41:12, my pull requests and pipeline runs, 3 PRs updated and 1 run finished, 9 requests, 1.1 seconds" | Coalesced: "10:38 to 10:40, all active pull requests, no changes, 4 passes". |
| Changed item | `Role::ListItem` | "Pull request 4821, Retry idempotency keys, new comment" | Default action opens. |
| Requests table | `Role::Table` with `Role::ColumnHeader`, `Role::Row`, `Role::Cell` | Row label is one sentence: "10:41:12, POST, work items batch, failed with 503 Service Unavailable, 1.2 seconds, sync" | Not `Role::Log`: that role is an implicit live region and would read every request. Virtualized rows expose index and the total ("row 1,201 of 1,284"). [Roles from accesskit 0.24.1 (Cargo.lock); none is used in the tree yet.] |
| Status pill | part of the row label | "failed with 503 Service Unavailable" / "throttled, 429" / "canceled" / "in flight" | |
| Row detail | `Role::Group` labelled "Request details" | Each line as `StaticText` "Status, 503 Service Unavailable" | |
| Filter field / chips / menus | `Role::SearchInput` "Filter requests" · `Role::Switch` "Failed only, 2" · `Role::Button` "Origin: all" (menu) | | |
| "N new" pills | `Role::Button` | "24 new requests. Jump to latest, End" | Not live (it would chatter); the count is in the label. |
| Toasts | existing polite live region | "Copied request" · "Exported 1,284 requests" | |

### 9.2 Keyboard and focus

Full operation without a pointer (§6). No traps: `Esc` always unwinds toward closing, `F6` cycles regions, focus returns to the invoker on close. Tab order = visual order (toolbar → lists → resize handle).

### 9.3 Contrast

Rule (as pins.md §11.3): **text uses `text.*` only; hue lives in glyphs and washes.** Measured with WCAG 2.x, alpha flattened, built-in values from tokens §2:

| Check | Dojo Night | Dojo Day | Arcade | Sakura |
|---|---|---|---|---|
| Tinted `status.*.fg` 11 px text on its wash over `bg.sidebar` (why statusbar chip text is `text.primary`) — lowest of success/warning/danger/info | 5.41 | **4.21** | 6.07 | **4.16** |
| Same, on its wash over `bg.selected` over `bg.surface` (why table pill text is `text.primary`) — lowest | **4.07** | **4.15** | 4.52 | **4.16** |
| `text.primary` on any status wash over `bg.sidebar` / `bg.surface` / `bg.selected` — lowest | 9.58 | 13.80 | 11.12 | 12.73 |
| Status glyphs (`*.fg`) on their wash over `bg.sidebar`, `bg.surface`, `bg.selected`, `bg.hover` — lowest (3:1 needed) | 4.07 | 3.91 | 4.52 | 3.88 |
| `text.tertiary` (idle value, time column) on `bg.sidebar` / `bg.hover` over sidebar / `bg.selected` | 6.36 / 5.53 / 4.95 | 5.24 / 4.85 / 5.15 | 10.42 / 8.87 / 8.32 | 5.74 / 5.34 / 5.75 |

- Bold values fail 4.5:1 for 11–12 px text, which is why no status text is tinted.
- User and community themes: the validator enforces `status.*.fg` ≥ 4.5 on surfaces and 3:1 for icons (tokens §9.3), not on washes over selection. [INFERENCE: the margins above make glyphs safe in practice; the per-theme gallery snapshots are the check.]
- Custom backgrounds: the drawer and statusbar are panes at `surface_opacity`, covered by the legibility pass (tokens §10.4) for every `text.*` token. The "N new" pill is an overlay (`bg.overlay` ≥ 0.9 + frost).

### 9.4 Other

| Concern | Behavior |
|---|---|
| Color independence | Every tone has a glyph shape and a word or code (§5.3); the Mix bar is decorative and has text equivalents. |
| Reduce motion | §8. |
| Mute | Nothing new (§8). |
| Text scale | `font_scale` 1.3: statusbar priority (§2.5); table columns drop earlier (thresholds scale with the font); rows grow with `size.row.dense` × scale. |
| Targets | Statusbar items 24 tall; icon buttons 24 × 24; resize hit band 8 px tall across the full width plus the keyboard path (WCAG 2.5.7 dragging alternative). |

## 10. Gallery stories

In `crates/gallery`, one snapshot per built-in theme (Dojo Night, Dojo Day, Arcade, Sakura; AGENTS.md). Spinners render at phase 0; clocks are injected; fixtures are synthetic (no real org data). Story names use words, not digits ("narrow", "ten-thousand"), because the gallery naming test only accepts lowercase words joined by hyphens. Each story is one frame, so variants are separate stories.

| Story id | Shows | Fixture |
|---|---|---|
| `statusbar-activity` | A grid of statusbars (as `sync-status`): idle, in flight, danger chip, warning chip, paused (both), throttled, offline pill, no token, token rejected, demo (indicator hidden), indicator off | State fixtures |
| `statusbar-activity-narrow` | 900 px statusbar at `font_scale` 1.3 with a long org, a notice, throttled and the Ken chip: shows the §2.5 collapse order | Width fixture |
| `activity-sync` | Drawer on Sync, idle: six kinds, passes with one expanded (3 PRs, 2 work items, 1 run), one coalesced no-change row, one "You: Sync #20101" row | Demo seed + pass fixture |
| `activity-sync-syncing` | In-progress pass on top, "Syncing · 3 of 12 projects", tab spinner | Mid-pass fixture |
| `activity-sync-first` | First sync: kinds "Not synced yet" and syncing, passes empty except the running pass | Fresh store |
| `activity-sync-throttled` | Throttled banner with resource, throttled kinds, statusbar throttled | 429 fixture |
| `activity-sync-failed` | Work items failing with 503 + retry countdown, failed pass row, tab `circle-x` | 503 fixture |
| `activity-sync-paused` | Paused until 15:00: Resume button, kind rows paused, a ⌘R pass in the list | Paused prefs |
| `activity-sync-scope` | Pipeline runs "Needs a scope: vso.build", no button | Missing-scope fixture |
| `activity-sync-demo` | Demo rows and a "Demo feed" pass | Demo seed |
| `activity-sync-empty` | No passes yet empty state | Empty pass list |
| `activity-sync-no-token` | No-token variant | No token |
| `activity-sync-narrow` | Drawer < 760 px: one column | 900 px window, sidebar expanded |
| `activity-drawer-rail` | Drawer beside the collapsed rail | 900 px window, rail |
| `activity-network` | Requests following the tail: mixed 200s, one 304, one in flight, one 429, one 503, one offline failure, one canceled; cursor on a row | Ledger fixture |
| `activity-network-failed` | "Failed only" on, "N new" pill visible (scrolled up) | Ledger fixture |
| `activity-network-detail` | A 503 row's detail with rate headers, Retry-After and activity id | Ledger fixture |
| `activity-network-totals` | Totals, last hour, sorted by TSTU, summary strip, Mix bars | Ledger fixture |
| `activity-network-empty` | No requests yet | Empty ledger |
| `activity-network-filtered` | Filtered to nothing | Ledger fixture |
| `activity-network-demo` | "Demo mode sends no requests" | Demo |
| `activity-network-ten-thousand` | 10,000 rows, scrolled to the middle, at 900 px (column drops) | Generated ledger |
| `activity-network-ten-thousand-wide` | Same at 1440 px (all columns) | Generated ledger |
| `activity-drawer-min` | Drawer at 160 px on a 600 px window, both tabs | Size fixture |

## 11. New tokens and icons

Colors, radius, elevation, z-layers, motion: **none** new. All values come from tokens §2–§7. Each value below is a constant shared by every theme (tokens §1).

| New | Kind | Value | Used by |
|---|---|---|---|
| `size.row.dense` | size token (tokens §5.2) | 24 px (× `font_scale`); independent of density | Requests and Totals rows, table header, privacy footer (§5.2, §5.5, §5.8) |
| `size.drawer_min` | size token | 160 px | Drawer min height (§3.1; the value is the spec's) |
| `size.drawer_header` | size token | 32 px (= `size.row.compact` at default density, but fixed across densities) | Drawer header and toolbars (§3.1, §4.1, §5.1) |
| `arrow-up-down` | icon (Lucide, ISC) → `IconName::ArrowUpDown` | — | Network indicator, Network empty states |
| `arrow-down` | icon → `IconName::ArrowDown` | — | "N new requests" pill |
| `download` | icon → `IconName::Download` | — | Export… |
| `maximize-2` / `minimize-2` | icons → `IconName::Maximize2` / `Minimize2` | — | Drawer maximize / restore |
| `x` | icon → `IconName::X` | — | Drawer close, Endpoint chip clear |
| `circle-slash` (already in `IconName`) | — | — | Canceled pill |

Existing icons used, all already in `IconName` (`crates/ui/src/icon.rs`): `loader-circle`, `circle-check`, `circle-x`, `circle-alert`, `circle-dashed`, `circle-dot-dashed`, `circle-slash`, `timer`, `clock`, `pause`, `play`, `wifi-off`, `lock`, `key-round`, `refresh-cw`, `user`, `image`, `pin`, `copy`, `search`, `info`, `folder-kanban`, `chevron-down`, `chevron-right`, `chevron-up`, `git-pull-request` and the work item and pipeline glyphs from tokens §2.8. Record the new Lucide files in `THIRD_PARTY_NOTICES.md` / `assets/manifest.toml` when bundled.

## 12. Amendments to other docs (applied 2026-10-01)

| Doc | Amendment |
|---|---|
| [principles.md](principles.md) §2 | Statusbar: "left → right: sync state (button: opens the Activity drawer's Sync tab), org name; center: quiet-hours / snooze state; right: network indicator (opens the Network tab; becomes the Offline pill while offline), Ken combo counter (only if enabled), demo pill." Window anatomy sketch adds "⇅ 200". Add: "The Activity drawer (`⌘⇧A`, [activity.md](activity.md)) docks above the statusbar in the content column." |
| [principles.md](principles.md) §3.3 | Add the Paused statusbar state: "‖ Sync paused · until 15:00" (`pause`, `text.secondary`). Note that "Sync delayed — retrying in 30s" lives per kind in the Sync tab. |
| [principles.md](principles.md) §3.5 | "Statusbar right: the network indicator becomes the pill 'Offline'…" (same visuals, defined in activity.md §2.3). |
| [principles.md](principles.md) §4 | `F6` cycles content → Activity drawer (when open) → toasts. |
| [principles.md](principles.md) index | Add "[activity.md](activity.md) — statusbar sync item and network indicator, Activity drawer (sync activity, network log)." |
| [tokens.md](tokens.md) §4 rule | Exception: path templates in the network log truncate with a **leading** ellipsis (the tail names the resource); full text in tooltip and detail. |
| [tokens.md](tokens.md) §5.2 | Add `size.row.dense`, `size.drawer_min`, `size.drawer_header` (§11). |
| [command-palette.md](command-palette.md) §7.2 | Add `⌘⇧A` "Activity drawer (sync activity, network log)" and `F6` "Cycle focus: content → Activity drawer → toasts". Add a §7 note: "`ActivityPanel && !TextInput` context keys, including `⌘⇧↩` maximize / restore: [activity.md](activity.md) §6.2". |
| [sound.md](sound.md) §2.1 | `ui.sync_done` trigger also covers "Sync now" from the Sync tab (it's the same ⌘R action). |
| `docs/specs/0002-sync.md` §3 item 1 | "Click opens a popover with per-tier last-success times…" → "Click opens the Activity drawer's Sync tab (spec 0016), with per-kind state and the `X-RateLimit-Resource` string when throttled." (Owned by the spec's amendment list.) |
| `docs/specs/0002-sync.md` FR-10 | The developer ledger "hidden behind a setting" becomes the Network tab's Totals view (spec 0016). (Owned by the spec's amendment list.) |

## 13. Open questions — resolved 2026-10-01 (spec wins on behavior)

All recommendations accepted by the owner.

1. **Idle value: status code or nothing?** **Resolved:** the last code ("⇅ 200") in quiet `text.tertiary`, and the in-flight count only after 150 ms (spec FR-21). Copy for no-code outcomes stays "Failed" / "Canceled" (sentence case and ADO's US spelling, as `pipeline.canceled`).
2. **Should "expected" refusals light the chip?** **Resolved:** no. Background avatar 401/404 (and 401s on optional transports, 0012 FR-9) are logged and listed under "Failed only" but don't change the indicator's tone or count unless the user caused the request (spec FR-21, Q-10, AC-B7).
3. **Default for "Show network activity in the statusbar".** **Resolved:** on (spec FR-28, Q-3): quiet at rest, loud only on failures.
4. **Sync tab actions at the top (toolbar) vs a footer.** **Resolved** as a design call: top toolbar (§4.1). The spec only requires that the drawer has the actions. ("You: Sync #20101" rows are a pass trigger in spec 0016 FR-11.)
5. **Coalescing no-change passes into one row.** **Resolved:** yes, display only (spec FR-13). Each pass still counts toward the 200-pass history.

## 14. Unverified or dependent

- `x-vss-activityid` appearing on responses: no Microsoft Learn page found by the spec author; the spec requires a real-org probe. The detail row shows it only when recorded.
- Work items, pipelines and PR "change words" (§4.4) depend on what the spec says each pass can attribute (sync events vs row diffs). Unsupported words are dropped rather than guessed.
- accesskit `Table`/`Row`/`Cell`/`ColumnHeader`/`TabList`/`Tab`/`TabPanel`/`Splitter` exist in accesskit 0.24.1 (the version in `Cargo.lock`). How zui maps them to VoiceOver hasn't been exercised: no view uses them yet.
- Quiet-hours text in the statusbar (principles §2 center) isn't rendered by `render_statusbar` today; §2.5 covers it for when it lands.
- `F6` region cycling (principles §4) has no binding in `crates/**` today; spec 0016 FR-3 / AC-S4 now requires it with the drawer.
