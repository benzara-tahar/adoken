# Pins

Status: v0.1 (spec accepted 2026-10-01). Owner: ux-designer. Behavior, acceptance and budgets: [`docs/specs/0014-pins.md`](../specs/0014-pins.md). Tokens: [tokens.md](tokens.md). States: [principles.md](principles.md) §3. Layout context: [principles.md](principles.md) §2. Hover intent: [`docs/specs/0010-zeron-reuse-batch-2.md`](../specs/0010-zeron-reuse-batch-2.md) Z-7.

Pins are the three to ten things I bounce between all day, always on screen, each with one glanceable status and one keystroke away. The design goal is a list you read in a quarter second: **shape says what it is, tint and a trailing glyph say how it's doing, a dot says it changed.** Pins carry no numbers or jump keys (spec 0025 FR-2). Nothing animates unless it explains where something went or that something happened without me.

This doc owns row anatomy, glyphs, status tints, rail icons, peek motion and copy. Where it says "spec", it means 0014. Sketch glyphs: ⎇ pull request, ☑ work item, ▶ pipeline definition, ⚲ pin, ● change dot, ◌ running; the real Lucide glyphs are in §3.2 and §3.3.

## 1. Concept and placement

The **Pinned** group lives in the sidebar after the sections (Today … Search) and before the flex spacer that pushes the bottom block (sidebar toggle, Settings, org) down. It exists in all three sidebar presentations:

| View | What shows | Section |
|---|---|---|
| Full (sidebar 232) | Header "Pinned 5", one row per pin | §2, §3 |
| Folded (either width) | Header only, as a summary line; rail: one pin icon with a badge | §2.2, §4.3 |
| Compact (rail 56) | One icon per pin with tint and change dot; hover or focus floats the **peek** | §4, §5 |

```
┌──────────────────────────────────────────────────────────────────────────┐
│ ● ● ●   ‹ ›   Contoso / Payments ▾          ⌘P Search…            ◔  ⚙ │ titlebar 38
├────────────┬─────────────────────────────────────────────────────────────┤
│ ☼ Today    │                                                             │
│ ✉ Inbox 12 │                                                             │
│ ⎇ PRs    3 │                                                             │ sections
│ ☑ Work it… │                                                             │ (fixed)
│ ▶ Pipelin… │                                                             │
│ ⌕ Search   │                                                             │
│╶──────────╴│  content                                                    │
│ Pinned ⌄ 5 │                                                             │ header 28
│ ⎇ Retry i ⎇│                                                             │
│ ⎇ Bump az ✓│                                                             │ pin rows,
│ ☑ Fix SSO A│                                                             │ scroll inside
│ ▶ paymen… ✕│                                                             │
│ ▶ nightl… ◌│                                                             │
│            │                                                             │ flex spacer
│ ◧ Collapse │                                                             │
│ ⚙ Settings │                                                             │ bottom block
│ (DK) Conto │                                                             │ (fixed)
├────────────┴─────────────────────────────────────────────────────────────┤
│ ● Synced 2m ago · Contoso                                                │ statusbar 24
└──────────────────────────────────────────────────────────────────────────┘
```

### 1.1 Metrics (from `crates/app/src/shell/sidebar.rs`)

| Metric | Value | Token / constant |
|---|---|---|
| Sidebar width / rail width | 232 / 56 | `size.sidebar` / `size.rail` |
| Sidebar padding | 8 all sides | `space.8` |
| Gap between rows | 2 | `space.2` |
| Row height (sections, header, pins, hint) | 28 | `size.control.md` |
| Row width (expanded) | 216 (232 − 2 × 8) | — |
| Row left padding = icon inset | 12, same in both widths so icons never move while the width animates | `ICON_INSET` = (`size.rail` − 2 × `space.8` − `size.icon.md`) / 2 |
| Row right padding | 8 | `space.8` |
| Icon ↔ label gap | 8 (sidebar convention; not the generic `space.6`) | `space.8` |
| Row radius | 6 | `radius.sm` |
| Rail badge | 14 high pill, "9+" above 9 | `BADGE_HEIGHT`, `RAIL_BADGE_MAX` |

**Height budget.** The header is a normal 28 px row and adds no margins, so the spec's room count holds: at the 600 px minimum window the sidebar has 522 px inside its padding, sections + header + bottom block + gaps take 310, leaving 212 = **7 pin rows**; at 900 px, **17**. Beyond that the pin rows scroll inside the group; the header, the sections and the bottom block never move.

**Layout rule.** The group is a column of `[header][scroll area]`. The scroll area takes its content height up to the space left above the bottom block (flex shrink, min height 0); the existing flex spacer takes whatever remains. When the rows overflow, a 1 px `border.subtle` hairline appears under the header while the list is scrolled away from the top (static, no fade).

### 1.2 Separation from the sections

The header carries a 1 px `border.subtle` hairline along its top edge, inset from `ICON_INSET` to the row's right padding, painted inside the header's 28 px (no extra height). It reads as "the navigation ends here, your things start here" in both widths (the rail's `╶──╴` in the sketches).

## 2. Group header

### 2.1 Anatomy

```
 ← 216 ─────────────────────────────────────────────── →
 ┌────────────────────────────────────────────────────┐
 │12│Pinned│4│⌄│                                5│8│   unfolded
 │12│Pinned│4│›│          5 · 2 changed · 1 failing│8│   folded
 └────────────────────────────────────────────────────┘
```

| Part | Spec |
|---|---|
| Label | "Pinned", `type.label` (11/14, 560), `text.secondary`, sentence case. Starts at `ICON_INSET` (aligned with the glyph column's left edge), not after an icon: the header is a group label, and the saved 24 px is what makes the folded summary fit. |
| Fold chevron | `chevron-down` (unfolded) / `chevron-right` (folded), `size.icon.xs` 12, `text.tertiary`, `space.4` after the label. Always visible (principles §1.2: no hover-only affordance). |
| Count (unfolded) | Pin count, right-aligned, `type.label` + `type.numeric` (tabular), `text.tertiary`. Tertiary, not the `text.secondary` of section counts: section counts are actionable (unread), this one is inventory. |
| Summary (folded) | Replaces the count, right-aligned, same style; format §2.2. |
| Hit target | The whole 216 × 28 row toggles the fold. Rest: no background; hover `bg.hover`; pressed `bg.active`; keyboard cursor: focus ring (§3.6). |
| Tooltip (500 ms) | "Fold" / "Unfold". |
| Empty group | Label only: no chevron, no count, not interactive, not in the keyboard walk (nothing to fold). |

The fold state persists as `[sidebar] pinned_collapsed` (spec FR-4).

### 2.2 Folded summary line

Format, left to right, separator ` · ` (U+00B7 with a space each side), figures tabular:

| Part | Shown when | Text |
|---|---|---|
| Total | always | `{n}` |
| Changed | ≥ 1 pin has a lit change dot | `{c} changed` |
| Failing | ≥ 1 pin's headline is **CI failing** (PR) or **Failed** (definition) | `{f} failing` |
| Quiet | changed = 0 and failing = 0 | `all quiet` |

Examples: "5 · 2 changed · 1 failing", "5 · 2 changed", "5 · 1 failing", "5 · all quiet".

- "Failing" counts only red build results. Merge conflicts, changes requested, partially succeeded and Not available aren't failing: they have their own glyphs one unfold away. This matches the spec §3.1 fixture (a conflicted PR and a failed definition read "1 failing").
- The total, the separators and "all quiet" are `text.tertiary`; "2 changed" and "1 failing" are `text.secondary`. No colored text: `status.danger.fg` measures 4.42:1 on `bg.hover` over `bg.sidebar` in Dojo Day, under AA for 11 px text (§11.3). The words carry the meaning.
- **Overflow.** The full form needs ≈ 115 px; the header leaves 134 at 232 wide and `font_scale` 1.0. When the text doesn't fit (sidebar at its 200 px minimum, `font_scale` ≥ 1.2, long counts), it switches to the **compact form** `5 · ●2 · ✕1`: the change dot (6 px, §3.4) and `circle-x` at `size.icon.xs` in `pipeline.failed`, each followed by its number. "all quiet" never compacts. The tooltip and the accessible label always carry the full sentence.

## 3. Pin row (expanded sidebar and peek)

### 3.1 Anatomy

```
 ← 216 ─────────────────────────────────────────────────────── →
 ┌──────────────────────────────────────────────────────────────┐
 │12│ ⎇● │8│ Retry idempotency keys on paym… │8│ ⎇ │8│        28 │  PR, conflict, changed
 │12│ ☑  │8│ Fix SSO redirect         │8│ Active │8│           │  work item
 │12│ ▶  │8│ nightly-e2e           │8│ ◌ 12m │8│              │  definition, running
 │12│ ⎇  │8│ Retry idempotency k… │8│ ⎇ │4│ ⊘ │2│            │  keyboard cursor / hover
 └──────────────────────────────────────────────────────────────┘
    16 kind glyph     title (flex, truncates)      status   unpin 24
```

| Slot | Spec |
|---|---|
| Kind glyph | `size.icon.md` 16 at `ICON_INSET`, shape by kind (§3.2), tinted by the headline status (§3.3). Carries the change dot on its top-right corner (§3.4). |
| Title | Local label if set (FR-14), else the ADO title. `type.body` 13/20, `text.secondary` at rest (as section labels), `text.primary` + `type.body_strong` when changed, `text.primary` when on screen, hovered or under the keyboard cursor. One line, trailing ellipsis. No id in the row: ids live in the tooltip, the accessible label and the `⌘J` rows. |
| Status | Trailing, right-aligned, `space.8` after the title. A 14 px glyph (`size.icon.sm`) for PRs and definitions, the state word for work items, spinner + elapsed time for running definitions (§3.3). Collapses to zero width when the headline has no trailing mark (Active, Draft, Completed, Abandoned PRs). |
| Unpin button | Appears on hover **and** under the keyboard cursor (§3.7). |

**Text room.** 216 − 12 − 16 − 8 − 8 − 14 − 8 = **150 px** for the title with a status glyph (the spec's "about 150 px"); 172 with no trailing mark; about 100 next to a 64 px work-item state; 128 while the unpin button shows. Titles re-truncate in place; the row never changes height.

**Title truncation.** Ellipsis at the end (tokens §4 rule). Labels are at most 60 characters (FR-14) and truncate the same way. A label gets no marker in the row; the tooltip shows the real id and title (§3.8).

### 3.2 Kind glyphs

All Lucide, all already in `adoken_ui::IconName` (tokens §2.8 shapes):

| Kind | Glyph | Notes |
|---|---|---|
| Pull request | `git-pull-request` | Active and every in-flight headline. |
| Draft pull request | `git-pull-request-draft` | While `isDraft`, whatever the headline (a draft with failing CI keeps the draft shape, tinted `pipeline.failed`). |
| Completed pull request | `git-merge` | |
| Abandoned pull request | `git-pull-request-closed` | |
| Work item | by type: Bug `bug`, Task `square-check`, User Story / PBI / Requirement `book-open`, Feature `trophy`, Epic `crown`, Issue / Impediment `circle-alert`, any other type `square` | Shape by type; tint by state category (§3.3), not by the `wi.*` type color: in pins the color is status. |
| Pipeline definition | `workflow` | The Pipelines section glyph. |

### 3.3 Headline status (FR-9)

One signal per pin. The kind glyph and the trailing glyph take the same token. The word is what the tooltip, the accessible label and the `⌘J` subtitle say. Color is never alone: every tinted state also has a shape (kind glyph shape or trailing glyph) and a word.

**Pull request** (FR-9 order, first match wins):

| Headline | Kind glyph | Tint | Trailing glyph | Word |
|---|---|---|---|---|
| Completed | `git-merge` | `pr.completed` | — | Completed |
| Abandoned | `git-pull-request-closed` | `pr.abandoned` | — | Abandoned |
| Merge conflict | PR / draft | `status.danger.fg` | `git-merge` (as Today's conflict blocker) | Merge conflict |
| CI failing | PR / draft | `pipeline.failed` | `circle-x` | CI failing |
| Changes requested | PR / draft | `vote.rejected` if any Reject, else `vote.waiting` | `octagon-x` / `clock` | Changes requested |
| CI running | PR / draft | `pipeline.running` | `loader-circle`, spinning (§9) | CI running |
| Approved | PR / draft | `vote.approved` | `badge-check` | Approved |
| Draft | `git-pull-request-draft` | `pr.draft` | — | Draft |
| Active | `git-pull-request` | `pr.active` | — | Active |

A conflicted PR's trailing `git-merge` and a completed PR's kind glyph share a shape but never a slot: completed has the merge glyph in front and nothing behind.

**Work item:** the state name, tinted by category. The kind glyph takes the tint; the word is `type.label`, `text.secondary`, max 64 px, then ellipsis. The word stays neutral because `accent.fg` measures 4.28:1 on `bg.selected_muted` in Dojo Day, under AA at 11 px (§11.3).

| State category | Tint (kind glyph) | Word example |
|---|---|---|
| In progress | `accent.fg` (as the Work items view's accent chip) | Active, Doing, Committed |
| Resolved | `status.success.fg` | Resolved |
| Proposed | `text.secondary` | New, To Do |
| Completed / Removed | `text.tertiary` (steps back before auto-unpin) | Closed, Done, Removed |

**Pipeline definition:** its latest run.

| Headline | Tint | Trailing | Word |
|---|---|---|---|
| Running | `pipeline.running` | `loader-circle` spinning, `space.4`, elapsed in `type.label` + `type.numeric`, `text.tertiary` | Running 12m |
| Failed | `pipeline.failed` | `circle-x` | Failed |
| Partially succeeded | `pipeline.partial` | `circle-alert` | Partially succeeded |
| Canceled | `pipeline.canceled` | `circle-slash` | Canceled |
| Succeeded | `pipeline.succeeded` | `circle-check` | Succeeded |
| No runs | `text.tertiary` | `circle-dashed` | No runs |

Elapsed format: `<1m`, `12m`, `1h 5m` (≤ 7 characters). It updates on minute boundaries from a minute timer, not the frame clock. Spoken: "running for 12 minutes".

### 3.4 Change dot (FR-10)

The inbox's unread mark, reused: a 6 px circle (`space.6`), `radius.full`, `accent.solid`, absolutely placed at the kind glyph's top-right corner (`top` −`space.2`, `right` −`space.2`), so it takes no row width and sits in the same spot in the row, the rail and the `⌘J` rows. A lit dot also sets the title to `type.body_strong` + `text.primary` (a second, non-color channel, like unread mail).

- Lights: opacity 0 → 1 over `motion.base`, `ease.out` (something happened that I didn't do; principles §1.1). Reduce motion: snaps.
- Clears when the pin opens: instant.
- Contrast: `accent.solid` on `bg.sidebar`, `bg.selected` and `bg.overlay` measures ≥ 3.83:1 in every built-in (§11.3).

### 3.5 Special rows

| Row | Anatomy |
|---|---|
| **Fetching** (pinned from a URL or id, item not in the store yet) | Kind glyph by kind in `text.tertiary` (work items `square` until the type is known). The id in `type.code_inline`, `text.secondary` ("!4821"). Then a skeleton bar: `bg.raised`, 8 px high, `radius.xs`, 60 % of the remaining title width, `skeleton-shimmer` on the shared clock (reduce motion: flat). No status, no dot. Replaced in place (`motion.instant`) when the fetch lands. |
| **Not available** (404 / 403, FR-13) | Kind glyph and last title in `text.tertiary`. Trailing word "Not available", `type.label`, `text.secondary`. No status glyph, no dot. Unpin as on every row (hover, cursor, `⌫`, `⌘K`); the tooltip names the reason (§6.4). |
| **Stale** (older than 3 × the tier interval, 0003 rule) | Status "dims": the kind glyph and trailing glyph (or spinner) are drawn in `text.tertiary`, shapes kept; the work-item word is unchanged. The title keeps its color. Recolor rather than `opacity.dimmed`: a 50 % `status.danger.fg` glyph on Dojo Day falls under the 3:1 state-icon minimum, and `opacity.dimmed` must never push content under contrast (tokens §5.4). The tooltip adds "Updated 18m ago". |
| **Running** | Trailing `loader-circle` spins with `glyph-spin` (§9) on the shared ~30 fps clock and only while visible. Reduce motion: a still `loader-circle`, the elapsed time keeps updating. |
| **Empty hint** | §6.1. |

### 3.6 Interaction states

| State | Background | Title | Other |
|---|---|---|---|
| Rest | none | `text.secondary` | |
| Changed | none | `text.primary`, `type.body_strong` | dot |
| Hover | `bg.hover` (in over `motion.fast`; 0 ms when moving to a sibling row) | `text.primary` | unpin button; tooltip after 500 ms |
| Pressed | `bg.active` | `text.primary` | |
| On screen (the item in the content pane) | `bg.selected_muted` | `text.primary` | Muted, not `bg.selected`: the section row above already shows the strong selection, and one strong selection per sidebar keeps "where am I" unambiguous. |
| Keyboard cursor (sidebar focused) | `bg.hover` + focus ring: 2 px `border.focus`, 1 px gap, radius `radius.sm` + 2 | `text.primary` | unpin button; tooltip after 500 ms |
| Drag origin | unchanged, content at `opacity.dimmed` | | §8 |
| Renaming | §3.9 | | |

Hover on the on-screen row paints `bg.selected_muted` then `bg.hover`, nothing else (tokens §1 alpha rule). Selection and focus moves are `motion.instant`.

### 3.7 Unpin button

- Ghost icon button, 24 × 24 (`size.control.sm`, meets `size.hit_min`), `radius.sm`, `pin-off` at `size.icon.sm` in `text.tertiary`; hovered: `text.primary` on `bg.hover`; pressed `bg.active`.
- The button is a right-anchored absolute overlay, inset `space.2`. Hover/cursor reserves its 24 px width plus `space.4` in the content gutter; compared with the normal `space.8` right padding, the title gives up 22 px. Its layout stays present and opacity changes instantly: switching `display: none` through GPUI group-hover can paint children that were never prepainted.
- Shows on pointer hover and under the keyboard cursor (principles §1.2). Not a Tab stop: the keyboard path is `⌫` (and `⌘K › Unpin`); the button teaches it.
- Tooltip: "Unpin  ⌫". Accessible label: "Unpin pull request 4821".
- Click: unpins, Undo toast (§6.3), the row leaves with `row-remove`.

### 3.8 Tooltip

Rows in the expanded sidebar and the peek get the same tooltip as rail icons (FR-5), after the standard 500 ms. Format, parts joined with ` · `:

```
{ref} {title} · {status} [· Changed since you opened it] [· Updated 18m ago]
```

- `{ref}`: `!4821` / `#48213`; definitions have no id, the definition name is the title.
- With a label: `{label} ({ref} {title})`, e.g. "Payments CI (payments-ci) · Failed" (FR-14: the tooltip always shows the real id and title).
- Examples: "!4821 Retry idempotency keys · Merge conflict · Changed since you opened it"; "nightly-e2e · Running 12m"; "#48213 Fix SSO redirect · Active".
- Not available / fetching: §6.4.

### 3.9 Rename (`⌘K › Rename pin…`)

Inline, in place. The title becomes a text field (`bg.input`, `radius.sm`, 2 px `border.focus` ring, full row height), prefilled with the current label or title, all selected. The kind glyph stays. Placeholder when emptied: the ADO title in `text.tertiary` (empty resets to it). From 50 characters on, a `type.label` `text.tertiary` counter "52/60" replaces the status. `↩` or a click elsewhere saves; `Esc` cancels. In the rail, rename opens the peek and edits the row there; the peek ignores pointer leave while a row is being edited.

### 3.10 Row actions menu

`⌘K` on a pin, or right-click (the pointer path), opens the palette action menu (command-palette.md §4.4) anchored to the row: Open `↩` · Open in browser `⌘↩` · Rename pin… · Move up `⌥⌘↑` · Move down `⌥⌘↓` · Unpin `⌫`.

## 4. Rail (sidebar collapsed, 56)

### 4.1 Pin icon

```
  rail 56 ─────────────── row 40 (56 − 2 × 8) ──
  ┌────────────────────────────────────────┐
  │            ┌────────────┐●             │  y 4–10   change dot (6), glyph corner
  │    12      │    16      │              │
  │            │   glyph    │              │  y 11–25
  │            └────────────┘              │
  └────────────────────────────────────────┘ 28
  x:  0        12          28 30          40
```

| Part | Spec |
|---|---|
| Glyph | Kind glyph (§3.2) at `ICON_INSET`, `size.icon.md`, tinted by the headline status (§3.3). Stale: `text.tertiary`. Fetching: `text.tertiary`. Not available: `text.tertiary`. |
| Change dot | Same as §3.4: 6 px, `accent.solid`, glyph top-right corner (−2, −2). |
| Row states | As §3.6: hover `bg.hover`, on screen `bg.selected_muted`, keyboard cursor focus ring. |

**No collision with the count badge.** Section rail badges straddle the icon's **top-right** corner (sidebar.rs `rail_icon`: pill from icon-relative y −5). A pin icon never carries a count badge, and has no number (spec 0025 FR-2); the folded group icon (§4.3) carries a badge and never a dot.

In the rail, the tint is a glance aid; the word lives one step away, in the peek (hover or focus) and the tooltip, and always in the accessible label. The peek pairs every tint with a trailing glyph and the word.

### 4.2 Rail header

The header row keeps its 28 px slot. Glyph: `pin`, `size.icon.md`, `text.tertiary`, at `ICON_INSET`, with the §1.2 hairline above. Click or `↩` folds / unfolds. Tooltip: "Pinned · 5" (unfolded), "Pinned · 5 · 2 changed · 1 failing" (folded). Empty group: no header in the rail (spec: "Rail: nothing").

### 4.3 Folded group in the rail

One icon: the header's `pin` glyph with the existing rail badge (accent pill, `rail_badge`: count, "9+" above 9) counting pins with a lit change dot; no badge at 0 (AC-12: "one icon with badge 2"). Click unfolds. Hover or focus still opens the peek with the full list, so a folded rail still gives one-glance status.

### 4.4 Rail tooltips and the peek

Hovering the rail's pins opens the peek at 150 ms (§5.3), before the 500 ms tooltip. So the rail icon's tooltip (FR-5, AC-11) anchors **to the right of the peek**, level with the matching peek row, never over the peek. The peek row under the same y shows `bg.hover` as long as the pointer is on its rail icon (linked highlight).

## 5. Peek

### 5.1 Geometry

```
 rail        peek (size.sidebar wide, over the content)
 │ ⌕  │
 │╶──╴│ ┌───────────────────────────────┐   ← top = header top − space.4
 │ ⚲  │ │ Pinned                      5 │   header, aligned with the rail's ⚲
 │ ⎇●¹│ │ ⎇● Retry idempotency keys…  ⎇ │   every row at the same y
 │ ⎇ ²│ │ ⎇  Bump azure-identity to…  ✓ │   as its rail icon
 │ ☑ ³│ │ ☑  Fix SSO redirect    Active │
 │ ▶●⁴│ │ ▶● payments-ci              ✕ │
 │ ▶ ⁵│ │ ▶  nightly-e2e         ◌ 12m  │
 │    │ └───────────────────────────────┘
 │    │   content keeps its width and layout
```

| Property | Value |
|---|---|
| Anchor | Left edge at the rail's right edge + `space.4`. Top at the rail header's top − `space.4`, so the peek header and every peek row sit at exactly the y of the rail header and rail icons: moving the pointer straight right lands on the same pin. |
| Size | Width `size.sidebar` (232); inner padding `space.4` vertical, `space.8` horizontal, so rows are the 216 × 28 rows of §3 with `space.2` gaps. Height = the rail group's visible height + padding; the peek scrolls with the rail group (one shared offset). |
| Content | Header ("Pinned" + count, no chevron: fold belongs to the sidebar header) and every pin row with the full §3 anatomy, unfolded even when the group is folded. |
| Surface | `bg.overlay` (× `overlay_opacity` and frost blur with a custom background, tokens §10.2), 1 px `border.default`, `radius.md`, `elevation.2` (the menu level). |
| Layer | `z.dropdown` (20): above sticky headers, under toasts, palette and dialogs. |
| Scrim | None. The peek is non-modal: the content stays visible and live. A click outside closes the peek and is delivered normally. |
| Layout | The content pane's width and layout never change (AC-11 layout assertion). |

### 5.2 Motion

| Trigger | Open | Close |
|---|---|---|
| Pointer (hover intent) | `peek-in`: opacity 0 → 1, translate X −4 → 0, `motion.quick` 140, `ease.out` | `peek-out`: opacity 1 → 0, `motion.quick_exit` 90, `ease.in` |
| Keyboard (focus enters the rail pins) | instant (`motion.instant`; keyboard beats smoothness, principles §1.1) | instant on `Esc` or focus leaving |
| Opening a pin (click, `↩`) | — | instant, so the view that opens is never behind a fading peek |
| `⌘B`, palette or dialog opens, window loses focus | — | instant |
| Reduce motion | appears at once; no animation frames run (AC-11) | disappears at once |

The first peek frame paints within one frame of the intent firing (spec §7). Input during `peek-in` jumps it to its end state (tokens §7.3).

### 5.3 Hover intent and corridor (0010 Z-7)

Uses `HoverIntent` from the Z-7 port (`crates/ui/src/popover/hover_intent.rs`; not yet in the tree as of 2026-10-01, so pins depend on that port).

- **Trigger zone:** the rail header plus all rail pin icons, one rectangle, 40 px wide.
- **Open:** the pointer dwells in the trigger zone for `motion.hover_intent` (150 ms, new token §13). Passing through on the way to Settings or the org doesn't open it. A click, key press or drag start in the zone cancels a pending open.
- **Stay open:** while the pointer is in the trigger zone, in the peek, or inside Z-7's safe corridor (the triangle from the last pointer position in the trigger zone to the peek's near corners), with Z-7's 300 ms grace after leaving all three.
- **Close:** pointer leave after grace, `Esc`, opening a pin, or the triggers in §5.2.

### 5.4 Keyboard and focus

- Focus stays in the sidebar list. With the rail focused, `↑`/`↓` reaching the header or a pin opens the peek (instant). The rail icon under the cursor shows the focus ring, and its peek row mirrors it with `bg.hover` plus the unpin button, so the full status is readable without a pointer.
- `↩` opens the pin (peek closes); `⌫` unpins; `⌥⌘↑`/`⌥⌘↓` reorder; `⌘K` opens the row menu anchored to the peek row; `Esc` closes the peek and keeps the cursor on the rail icon. Moving the cursor back into the sections closes the peek.
- Screen readers: the peek is a visual mirror and isn't exposed; the rail items carry the full FR-17 labels (§11.1). No focus trap: the peek never takes focus.

## 6. Hints, toggles, toasts, copy

### 6.1 Empty hint row

The only row of an empty group (expanded sidebar only; the rail shows nothing):

```
│12│ ⚲ │8│ Pin what you use daily        [⌘D] │8│
```

`pin` glyph `size.icon.md` `text.tertiary`; text `type.small` `text.tertiary`; the `⌘D` keycap (principles §4) right-aligned, showing the active binding. The spec copy "Pin what you use daily · ⌘D" renders as the sentence plus the keycap (the keycap replaces "· ⌘D"). Static: no hover, not in the keyboard walk. Spoken: "No pins. Pin what you use daily, Command D."

### 6.2 Pin toggle in detail headers

In the PR review header, the work item detail header and the pipeline detail header: an icon button placed immediately left of each header's existing "Open in browser" button. In the work item header that button is now the compact "Open `o`" ([work-item-detail.md](work-item-detail.md) §3.1); the pin stays immediately left of it.

| State | Look | Tooltip |
|---|---|---|
| Not pinned | Ghost, 24 × 24 (`size.control.sm`), `pin` at `size.icon.sm`, `text.secondary`; hover `bg.hover` | "Pin  ⌘D"; pipeline definitions: "Pin · failures notify you (P2)  ⌘D" |
| Pinned | `pin` in `accent.fg` on `accent.wash`, `radius.sm`; pressed state exposed | "Unpin  ⌘D" |
| Unavailable (scope missing, FR-1) | `pin` in `text.disabled`; stays focusable so the reason is reachable | "Can't pin pipelines: your token doesn't include Build (Read)" / "Can't pin work items: your token doesn't include Work Items (Read)" |

The swap is `motion.instant`. At the 20-pin cap the button stays enabled; pressing it shows the limit toast (§6.3) and nothing changes (FR-3). Offline changes nothing: pins are local.

### 6.3 Toasts

Standard toast (principles §4): bottom-right, 360 wide, `elevation.2`, `radius.md`, leading 16 px glyph, polite live region. A repeat of an identical message restarts the visible toast instead of stacking a second one.

| Event | Glyph | Message | Action | Duration |
|---|---|---|---|---|
| Unpin by hand (button, `⌫`, `⌘D`, `⌘K`) | `pin-off`, `text.tertiary` | "Unpinned !4821" / "Unpinned #48213" / "Unpinned payments-ci" (the label if set, else the definition name) | [Undo `⌘Z`] | 5 s (spec §3.3; overrides the 8 s action-toast default), pauses on hover/focus |
| 21st pin | `circle-alert`, `status.warning.fg` | "20 pins is the limit · unpin one first" | — | 5 s |
| `⌘D` on something unpinnable | `info`, `text.tertiary` | "Only pull requests, work items and pipelines can be pinned" | — | 5 s |
| `⌘D` on a kind whose scope is missing | `circle-alert`, `status.warning.fg` | "Can't pin pipelines: your token doesn't include Build (Read)" (work items: "Work Items (Read)") | — | 5 s |

- Undo (button or `⌘Z` outside text fields) restores the row at its old position with its label (`row-insert`) and closes the toast; no second toast. Several unpins stack up to three toasts; `⌘Z` undoes the most recent.
- Pinning shows no toast: the row appearing is the confirmation. Auto-unpin is silent (FR-12).
- In `⌘K` menus and `>` command results, an unavailable Pin shows in `text.disabled` with the same reason as its subtitle (command-palette.md §4.3 "Unavailable command").

### 6.4 Tooltip copy for special rows

| Row | Tooltip |
|---|---|
| Not available, 404 or omitted by the batch | "!4821 Retry idempotency keys · Not available: deleted or moved" |
| Not available, 403 | "!4821 Retry idempotency keys · Not available: your token can't read it" |
| Fetching | "!4821 · Fetching…" |
| Fetching while offline | "!4821 · Not fetched yet · offline" |
| Stale | "…· Updated 18m ago" appended (§3.8) |

## 7. `⌘J`: the palette in `pin:` scope

`⌘J` opens the palette (command-palette.md; the FR-19 amendment lands there) with the `pin:` chip and the pins in pin order.

| Part | Spec |
|---|---|
| Chip | Label "Pins", the standard scope chip. Keyword `pin:`; no sigil. |
| Rows | A fixed pin row (not customizable in v0.1). Leading: the kind glyph in its headline tint with the change dot (§3.2–3.4). Title: label or title (`type.body_strong`, `text.match` on matches). Subtitle (`type.small`, `text.secondary`): `{ref} · {status word}`, plus ` · {ADO title}` when a label is set, plus ` · Updated 18m ago` when stale. No trailing keycap: pins have no jump keys and palette rows no number keys (spec 0025 FR-1, FR-2); `↩` opens the selected row. |
| Empty query in `⌘P` | A **Pinned** group before Recent, same rows, max 9 (FR-19). |
| Nothing pinned | Empty state in the list: `pin` 20 px `text.tertiary`, headline "No pins yet", body "Press ⌘D on a pull request, work item or pipeline to pin it." No action. |
| Filtered to nothing | "No pins match "ssx"", the standard no-match state without the server-search row (pins are local). |
| Footer, left | `↩ Open` · `⌘↩ Browser` · `⌘D Unpin` · `⌘K Actions` (keycaps, `type.small`, `text.tertiary`). |
| Footer, right (before `?`) | "Opened from pins 41× this week", `type.small`, `text.tertiary`, figures tabular. Hidden at 0. Tooltip: "Counted on this Mac, never sent". |

## 8. Reorder

### 8.1 Drag (expanded sidebar and peek)

```
│ ⎇  Retry idempotency keys…  ⎇ │   origin row, content at opacity.dimmed
│ ⎇  Bump azure-identity to…  ✓ │
│━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━│   drop indicator, 2 px, in the 2 px gap
│ ☑  Fix SSO redirect    Active │
   ┌───────────────────────────────┐
   │ ⎇  Retry idempotency keys…  ⎇ │   lifted copy follows the pointer (y only)
   └───────────────────────────────┘
```

- **Start:** pointer down on a row and move ≥ 4 px (`space.4`). Below that, it's a click.
- **Lifted copy:** the row rendered on `bg.overlay`, `radius.sm`, `elevation.2`, `z.dropdown`, x locked to the column, y follows the pointer 1:1 (no easing, no lag).
- **Origin:** keeps its slot; content at `opacity.dimmed`.
- **Drop indicator:** a 2 px `accent.solid` bar, `radius.full`, from `ICON_INSET` to the row's right padding, centered in the `space.2` gap at the insertion point. The **rows never reflow during the drag**; only the indicator moves (tokens §7.3: no layout animation under the user's hands).
- **Edges:** within 24 px (`size.hit_min`) of the scroll area's top or bottom, the group scrolls one row per `motion.base`.
- **Drop:** the order changes in one frame (`motion.instant`); the rail follows. **Cancel:** `Esc`, or dropping more than 24 px outside the group horizontally: the lifted copy disappears, nothing moves.
- The rail's icons aren't drag sources; dragging happens in the peek, which has the same visuals.

### 8.2 Keyboard (`⌥⌘↑` / `⌥⌘↓`)

The focused pin swaps with its neighbor instantly, the cursor moves with it, nothing animates. Announcement (polite): "Moved to position 1 of 5". At the top or bottom the key does nothing.

## 9. Motion and sound

| Moment | Motion |
|---|---|
| Pin added while the group is visible (`⌘D`, header toggle, Undo) | `row-insert` (height 0 → 28, opacity; `motion.base`, `ease.out`). If the new row is below the scroll area, the group scrolls to it (instant). |
| Unpin by pointer, auto-unpin while visible | `row-remove` (90 + 130, `ease.in`) |
| Any change while the sidebar has keyboard focus (`⌫`, `⌥⌘↑/↓`, Undo) | instant (tokens §7.3: never animate a list under the keyboard cursor) |
| Change dot lights / clears | opacity over `motion.base`, `ease.out` / instant |
| Status or tint changes | instant |
| Fold / unfold by pointer | height over `motion.base` (`ease.out`) / `motion.base_exit` (`ease.in`) |
| Fold / unfold by keyboard | instant |
| Peek | §5.2 |
| Running glyph | `glyph-spin`: one turn per 1000 ms, `ease.linear`, shared ~30 fps clock, only while mounted and visible (new catalog entry §13) |
| Fetching row | `skeleton-shimmer` |
| Drag | 1:1 with the pointer, drop instant |
| `⌘B` width animation | existing `pane-resize` over `motion.base`; icons don't move (`ICON_INSET`) |

**Reduce motion:** everything above snaps; the running glyph is a still `loader-circle` next to its elapsed time; the skeleton is a flat `bg.raised` bar; the peek appears and disappears at once.

**Sound** (sound.md; sounds are off by default and each event below is off by default within them): pin and Undo → `ui.toggle_on`; unpin → `ui.toggle_off`; auto-unpin, reorder, peek and change dots are silent. A failed run of a pinned definition plays whatever sound.md maps to its P2 notification class; pins add no sound of their own. Mute and quiet hours apply as everywhere.

**Ken:** pins add no arcade moments. They're a navigation surface; Quiet mode has nothing to hide here.

## 10. States

| State | Expanded sidebar | Rail | Peek / `⌘J` |
|---|---|---|---|
| Empty | Header "Pinned" (no chevron, no count) + hint row (§6.1) | Nothing, not even the header | No peek; `⌘J` empty state (§7) |
| Loading / first sync | Pins paint from the store with the rest of the sidebar, no extra wait, no group skeleton. Unfetched items: fetching row (§3.5) | Glyph in `text.tertiary`, no dot | Same rows |
| Error: Not available (404 / 403) | Not available row (§3.5); never removed automatically | Glyph `text.tertiary` | Subtitle "!4821 · Not available" |
| Error: other refresh failures | Nothing per row; the statusbar sync states (principles §3.3) | same | same |
| Offline / stale | Rows keep their last status; past the stale threshold the status glyphs recolor to `text.tertiary` and tooltips add "Updated 18m ago". Opening works offline. The statusbar "Offline" pill owns the offline message; no per-row badge. | Glyphs recolor the same way | Subtitle adds "Updated 18m ago" |
| Throttled by ADO | Nothing pin-specific (0002 statusbar states) | same | same |
| Cap reached (20) | Unchanged; the limit toast on the 21st (§6.3) | same | same |
| Kind unavailable (missing scope) | Existing pins of that kind render as stored; new ones are refused with the reason (§6.2, §6.3) | same | same |
| Auto-unpinned | Row leaves with `row-remove` if visible; no toast | Icon leaves | Gone from the list |

## 11. Accessibility

### 11.1 Roles and labels

| Element | Role | Label (example) | State / actions |
|---|---|---|---|
| Group | `Role::List` | "Pinned, 5 pins" | |
| Header | `Role::Button` | "Pinned, 5 pins" · folded: "Pinned, folded, 5 pins, 2 changed, 1 failing" | `aria_expanded`; `↩` / `Space` toggle, `←` fold, `→` unfold |
| Pin row / rail icon | `Role::ListItem` | FR-17: "Pinned pull request 4821, Retry idempotency keys, merge conflict, changed since you opened it" | `aria_selected` when on screen; default action open; `on_a11y_action` for unpin, move up/down, rename |
| Label template | | `Pinned {kind} {id}, {title}[, titled {ADO title}], {status}[, running for {n} minutes][, updated {n} minutes ago][, changed since you opened it]` | Kinds: "pull request", "draft pull request", "work item" (then its type: "…48213, Fix SSO redirect, user story, active"), "pipeline" (no id). |
| Fetching row | `Role::ListItem` | "Pinned pull request 4821, loading" | busy |
| Not available row | `Role::ListItem` | "Pinned pull request 4821, Retry idempotency keys, not available, deleted or moved" | |
| Unpin button | `Role::Button` | "Unpin pull request 4821" | |
| Empty hint | `Role::StaticText` | "No pins. Pin what you use daily, Command D." | not focusable |
| Folded rail icon | `Role::Button` | "Pinned, folded, 5 pins, 2 changed, 1 failing" | `aria_expanded` false; `↩` unfolds |
| Peek | not exposed | the rail items carry everything | |
| Header pin toggle | `Role::Button` | "Pin pull request 4821" / "Unpin pull request 4821" | pressed state; unavailable: disabled + reason as description |
| Toasts, reorder, fold | polite live region | "Unpinned !4821, Undo, Command Z" · "Moved to position 1 of 5" | |

### 11.2 Focus order

Tab reaches the sidebar as one list stop (principles §6: arrows inside lists, not Tab per row). Inside: `↑`/`↓` walk the sections, then the Pinned header, then pins 1…n (the hint row is skipped); `↩` opens, `⌫` unpins, `⌥⌘↑`/`⌥⌘↓` reorder, `⌘K` opens the row menu. In the rail, the same walk opens the peek (§5.4). After a pin opens, focus goes to the opened view. `Esc` from a rename or the peek returns to the row.

### 11.3 Contrast

Rule: **text in pin rows and the header uses `text.*` tokens only; hue lives in glyphs.** Status glyphs need 3:1 (state icons), text needs 4.5:1, and the tinted `*.fg` tokens don't reach 4.5 on sidebar washes in every theme. Measured (WCAG 2.x, alpha flattened over `bg.sidebar`):

| Check | Dojo Night | Dojo Day | Arcade | Sakura |
|---|---|---|---|---|
| Change dot `accent.solid` on `bg.sidebar` / `bg.selected` / `bg.overlay` | 6.18 / 5.04 / 5.31 | 4.43 / 3.83 / 5.08 | 5.66 / 4.65 / 5.09 | 5.29 / 4.61 / 6.13 |
| Lowest of `status.success.fg`, `pr.completed`, `accent.solid` glyph on `bg.selected_muted` | 5.19 | 3.93 | 4.51 | 4.13 |
| `status.danger.fg` / `status.warning.fg` on `bg.hover` (why text stays neutral) | 5.42 / 8.83 | **4.42 / 4.29** | 6.19 / 11.70 | 4.67 / 4.76 |
| `accent.fg` on `bg.selected_muted` (why the work-item word stays neutral) | 6.77 | **4.28** | 6.47 | 5.47 |
| `text.tertiary` on `bg.selected_muted` | 5.33 | 4.65 | 8.30 | 5.15 |

- Every tinted glyph clears 3:1 on every row background in the four built-ins. User and community themes: the validator enforces 3:1 for domain icons on their bases (tokens §9.3), not on washes [INFERENCE: margins like the ones above make washes safe in practice; the per-theme gallery snapshots are the check].
- **Custom backgrounds:** the sidebar paints `bg.sidebar` at `surface_opacity`, and the legibility pass (tokens §10.4) guarantees `text.*` ≥ 4.5 and `border.focus` ≥ 3 there, which covers every word in the group. Glyph tints aren't in that pass [INFERENCE: they track `text.secondary` closely enough to follow it]. The peek is an overlay: `bg.overlay` at `overlay_opacity` (≥ 0.9) with frost blur (tokens §10.2).
- Stale and closed states recolor to `text.tertiary` (AA on every wash) instead of dropping opacity (§3.5).

### 11.4 Other

| Concern | Behavior |
|---|---|
| Color independence | Expanded rows and the peek pair every tint with a shape and a word (§3.3). Rail: tint + shape; the word is in the tooltip, the peek (opens on focus too) and the spoken label. The change dot is a shape and also bolds the title. |
| Reduce motion | §9. AC-11: no peek animation frames. |
| Mute | §9; nothing pin-specific. |
| Text scale | `font_scale` up to 1.3: titles truncate first; the folded summary switches to its compact form (§2.2); rows keep 28 px. |
| Targets | Rows 216 × 28 / 40 × 28; unpin button and header toggle 24 × 24 (`size.hit_min`). |

## 12. Gallery stories (FR-18)

In `crates/gallery` (story ids follow the existing kebab-case, e.g. `shell-search`), each with one snapshot per built-in theme (Dojo Night, Dojo Day, Arcade, Sakura; AGENTS.md). Spinners and shimmer render at phase 0 so snapshots are stable.

| Story id | Shows | Fixture |
|---|---|---|
| `pins-expanded` | Expanded sidebar; one row on screen (`bg.selected_muted`), one under the keyboard cursor with its unpin button | Demo seed: PR with merge conflict + change dot, approved PR, active work item, failing definition + dot |
| `pins-folded` | Folded header "5 · 2 changed · 1 failing" beside the folded rail icon with badge 2 (AC-12), plus a "4 · all quiet" header | §3.1 five-pin fixture; the demo seed with dots cleared |
| `pins-rail` | Collapsed sidebar: glyphs, tints, dots, one on-screen pin | Demo seed |
| `pins-peek` | Rail + open peek at its end state, row 1 linked-highlighted with its tooltip right of the peek | Demo seed |
| `pins-empty-hint` | Expanded header + hint row; the rail with no group | No pins |
| `pins-twenty-scrolling` | 20 pins at the 600 px window height, scrolled to the middle; includes a running definition (still frame), a fetching row, a draft, a labeled pin | 20-pin fixture covering every FR-9 headline |
| `pins-not-available` | A 404 PR and a deleted work item, one under the keyboard cursor with unpin visible | Mock 404 / omitted ids |
| `pins-stale` | Rows past the stale threshold with recolored glyphs, tooltip "Updated 18m ago" open | Injected clock |

## 13. New tokens and icons

Colors, spacing, radius, elevation and z-layers: **none** new. Shared sizes and the two pin-specific sizes are listed below and in tokens §5.2. Motion values are constants shared by every theme (tokens §1), so each value below is the same for all built-in families.

| New | Kind | Value | Used by |
|---|---|---|---|
| `size.pin_state_word_max` | size token (§5.2) | 64 px | Work-item state word cap (§3.3) |
| `size.pin_skeleton_bar` | size token (§5.2) | 8 px | Fetching title skeleton (§3.5) |
| `size.badge` / `size.toast` / `size.action_menu_width` | shared size tokens (§5.2) | 14 / 360 / 280 px | Existing badge, toast and action-menu dimensions, now shared constants |
| `motion.hover_intent` | duration token (§7.1) | 150 ms: dwell before a hover-opened surface opens | Peek (§5.3); reusable for Z-7 submenus |
| `peek-in` | motion catalog (§7.3) | opacity 0 → 1, translate X −4 → 0; `motion.quick` (140), `ease.out` | §5.2 |
| `peek-out` | motion catalog (§7.3) | opacity 1 → 0; `motion.quick_exit` (90), `ease.in` | §5.2 |
| `glyph-spin` | motion catalog (§7.3) | rotation 0 → 360°, 1000 ms loop, `ease.linear`, shared ~30 fps clock, static under reduce motion; `Icon::transform` on `loader-circle` (the running glyph tokens §2.8 already says is "static under reduce-motion") | §3.3, §3.5 |
| `pin` | icon (Lucide, ISC) → `IconName::Pin` | — | Header (rail), folded rail icon, hint row, header toggle, `⌘J` empty state |
| `pin-off` | icon (Lucide, ISC) → `IconName::PinOff` | — | Unpin button, Undo toast |

Existing icons used, all already in `IconName`: `git-pull-request`, `git-pull-request-draft`, `git-merge`, `git-pull-request-closed`, `circle-x`, `octagon-x`, `clock`, `loader-circle`, `badge-check`, `bug`, `square-check`, `book-open`, `trophy`, `crown`, `circle-alert`, `square`, `workflow`, `circle-check`, `circle-slash`, `circle-dashed`, `chevron-down`, `chevron-right`, `info`.
