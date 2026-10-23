# Work item detail

Status: **Accepted** (2026-10-01, owner: all recommendations; spec [`docs/specs/0015-work-item-editing.md`](../specs/0015-work-item-editing.md) Accepted: slices A + B in v0.1, slice C (move to project: §4.6, `⇧P`) in v0.2). Owner: ux-designer. Behavior, acceptance and budgets live in spec 0015; this doc owns layout, visuals, motion, sound and copy. Tokens: [tokens.md](tokens.md). States: [principles.md](principles.md) §3. Layout context: [principles.md](principles.md) §2. Read-only baseline: [`docs/specs/0008-work-items-pipelines.md`](../specs/0008-work-items-pipelines.md). Write patterns reused: [`docs/specs/0004-pr-review.md`](../specs/0004-pr-review.md) FR-14 (links), FR-16 (status menu), FR-17 (work item picker); token hints follow [`docs/specs/0001-onboarding.md`](../specs/0001-onboarding.md) FR-9 (passive, no button). Pin toggle: [pins.md](pins.md) §6.2. Sync activity row: [activity.md](activity.md).

The detail pane is where I look at one work item and change it without opening Boards. The design goal: **read it in one glance, change any field in two keystrokes, and never wonder whether Azure DevOps has it.** The title reads like a document heading; the four fields that move during a sprint sit in a grid you can scan in a quarter second; description and discussion render like the web, not like a log file. Every field edits in place, every edit shows its pending state until Azure DevOps confirms it, and the header always says how fresh this item is, with one button to refresh just this item.

Where this doc says "spec", it means 0015. Where the spec and this doc disagree on behavior, the spec wins.

Sketch glyphs used in the mockups (the real Lucide names are in §15): ✱ work item type glyph, ↻ sync, ◌ spinning, ⚲ pin, ↗ open in Azure DevOps, ⋯ more actions, ⌄ menu chevron, ◷ pending clock, ✎ edit, ⛓ link, ✕ remove, ‹ back, (AL) avatar.

## 1. What changes

| Today (`work-items` snapshot) | Redesign |
|---|---|
| Header: type + id label, title, state chip, priority, text "Pin" and a large "Open in Azure DevOps" button | Toolbar row with freshness + **Sync this item**, pin toggle (pins.md §6.2), compact **Open `o`**, **⋯** action menu; title is an editable heading; the state chip is a menu (§3) |
| Plain key/value list, six rows | **Property grid**: Assigned to, Project, Area, Iteration as 2 × 2 editable cells with avatar/icons and breadcrumb paths; Created / Changed as one quiet meta line (§4) |
| Description as plain paragraphs | Rich text: headings, lists, links, code, quotes, tables; Markdown editor with Write / Preview (§5) |
| Links: parent + PRs, one flat list | Grouped by relation (Parent, Children, Related, Predecessors, Successors, Duplicates, Pull requests, Commits, Other) with Add link and Remove (§6) |
| Discussion: author name + text | Avatars, mentions, relative times, rich text, composer at the bottom (§7) |
| History: flat list | Same model, field-change sentences, last five + Show all (§8) |
| Read-only | Every editable field has pending, failed, conflict, read-only-token, rule-violation and offline states (§9) |

## 2. Layout

The detail pane keeps its place from principles §2: right of the 380 px list at window widths ≥ 1100, full content width (sliding over the list) below 1100, full width with the list hidden under `⌘⇧↩`. Inside the pane the content takes one of two arrangements, chosen by the **pane's** width, not the window's:

| Pane width | Arrangement | Where it happens |
|---|---|---|
| < 1000 (`size.detail_wide_min`) | **Single column**: header, property grid, meta line, then sections | Default side-by-side at any window up to 1611 px; every stacked width |
| ≥ 1000 | **Two columns**: main column (header, sections) + **properties rail** 280 (`size.detail_properties`) on the right | `⌘⇧↩` full width; side-by-side on windows ≥ 1612 px |

Switching arrangement is `motion.instant` (it only happens on a resize or `⌘⇧↩`, where `pane-resize` already animates the pane edge; the content re-lays out at the end of the resize, never mid-animation).

### 2.1 Metrics

| Metric | Value | Token |
|---|---|---|
| Pane padding | 24 left/right, 16 top, 48 bottom (room to scroll the composer above the statusbar) | `space.24`, `space.16`, `space.48` |
| Prose max width (title, description, comments) | 720; wider panes leave the extra on the right | `size.detail_prose_max` (new) |
| Section gap | 24 | `space.24` |
| Section head | 28 high: label `type.heading` 15/20 `text.primary`, count `type.label` + `type.numeric` `text.tertiary`, trailing action button | `size.control.md` |
| Property cell | 52 high (8 + label 14 + 2 + value 20 + 8) | `size.property_cell` (new) |
| Properties rail | 280 wide, 1 px `border.subtle` left hairline, padding 16 | `size.detail_properties` (new), `space.16` |
| Sticky compact header | 40 high, `bg.surface`, 1 px `border.subtle` bottom, `z.sticky` | `size.row.default` |

### 2.2 Default width (side-by-side, window 1440, pane 828)

```
 ← list 380 ──────────────┬─ detail pane 828 ──────────────────────────────────────────────────────── →
                          │                                                                             
 Work items               │  ✱ Bug · #20101                   ↻ Synced 2m ago   ⚲   ↗ Open  o    ⋯    toolbar 28
 Mine Assigned Recent …   │                                                                             
                          │  Timezone bug in daily settlement job                                      title, type.title
 Active 39 ───────────    │  ◉ Doing ⌄    ⌃ Priority 4    infra    settlement                          state · priority · tags
 ▌✱ Timezone bug in da…   │                                                                             
   ✱ Order confirmati…    │  ┌──────────────────────────────────┬──────────────────────────────────┐    property grid
   ✱ Review threat mo…    │  │ Assigned to                      │ Project                          │    2 × 2 cells, 52 each
   ✱ Add integration …    │  │ (AL) Ada Lindqvist               │ ▦ Storefront                     │    
   ✱ Refund amount of…    │  ├──────────────────────────────────┼──────────────────────────────────┤    
                          │  │ Area                             │ Iteration                        │    
                          │  │ ⌗ Storefront › Infra             │ ▤ Sprint 41 · Sep 29 – Oct 10  ● │    "●" = Current tag
                          │  └──────────────────────────────────┴──────────────────────────────────┘    
                          │  Created Apr 22 by Kwame Mensah · Changed 4d ago by Mei Tanaka            meta line
                          │                                                                             
                          │  Description                                                 ✎ Edit  d     section head
                          │  Settlement runs at 00:05 UTC, but merchants in UTC+10 see the            rich text, 720 max
                          │  previous day's totals.                                                     
                          │   • Repro: set merchant tz to Australia/Sydney                              
                          │   • Expected: totals for the local calendar day                             
                          │  ┌───────────────────────────────────────────────────────────┐             
                          │  │ SELECT * FROM settlements WHERE day = :local_day           │             code block, bg.code
                          │  └───────────────────────────────────────────────────────────┘             
                          │                                                                             
                          │  Links  3                                                                   
                          │  Parent                                                                     group label
                          │   ✱ #20092  As support I can see why a payment failed    Active   (KM)      
                          │  Related                                                                    
                          │   ✱ #20146  Settlement totals off for DST change         New      (MT)      
                          │  Pull requests                                                              
                          │   ⎇ !4295   Fix timezone bug in daily settlement job      Completed         
                          │   ⛓ Add link…  l                                                            ghost row, last
                          │                                                                             
                          │  Discussion  2                                                              
                          │  (KM) Kwame Mensah · Apr 22                                                 
                          │       Seen on 3 merchants this week. @Ada Lindqvist can you take it?        
                          │  (AL) Ada Lindqvist · 4d                                                    
                          │       On it. Root cause is the job's hard-coded UTC day boundary.           
                          │  (AL) ┌──────────────────────────────────────────────────────────┐         composer
                          │       │ Add a comment…                                           │         
                          │       └──────────────────────────────────────────────────────────┘         
                          │                                                                             
                          │  History                                                                    
                          │   ↻ Mei Tanaka changed State  New → Doing                       4d          
                          │   ◉ Kwame Mensah created it                                     Apr 22      
                          │   Earlier revisions aren't synced; history shows what sync observed.        
```

### 2.3 Wide (`⌘⇧↩` full width, window 1440, pane 1208)

```
 ← detail pane 1208 ───────────────────────────────────────────────────────────────────────────────────────────── →
  ✱ Bug · #20101                                                         ↻ Synced 2m ago   ⚲   ↗ Open  o    ⋯     toolbar spans both columns
 ┌─ main column (flex, prose ≤ 720) ──────────────────────────────────┬─ properties rail 280 ────────────────────┐
 │ Timezone bug in daily settlement job                               │ State        ◉ Doing ⌄                   │
 │ ⌃ Priority 4    infra    settlement                                │ Assigned to  (AL) Ada Lindqvist          │ rail rows 36:
 │                                                                    │ Project      ▦ Storefront                │ label 96 + value
 │ Description                                           ✎ Edit  d    │ Area         ⌗ Storefront › Infra        │
 │ Settlement runs at 00:05 UTC, but merchants in UTC+10 see the      │ Iteration    ▤ Sprint 41  ●              │
 │ previous day's totals.                                             │              Sep 29 – Oct 10             │ dates wrap under
 │  …                                                                 │ ──────────────────────────────────────── │
 │ Links  3                                                           │ Created      Apr 22 · Kwame Mensah       │
 │  …                                                                 │ Changed      4d ago · Mei Tanaka         │
 │ Discussion  2                                                      │                                          │
 │  …                                                                 │                                          │
 └────────────────────────────────────────────────────────────────────┴──────────────────────────────────────────┘
```

- In the rail the state chip moves from under the title to the rail's first row, so the rail holds every editable property; under the title only priority and tags stay. The chip, its menu and its keys are identical (§3.3).
- The rail is `position: sticky` at the pane top below the toolbar: it stays visible while the main column scrolls through a long discussion (rail content taller than the pane scrolls with the page instead; never a nested scrollbar).
- Rail row: 36 high (`size.control.lg`), label column 96 `type.small` `text.tertiary`, value `type.body`; the whole row is the hit target and takes the same hover / cursor / pending / error treatment as a grid cell (§4.1). Inline errors and the rule-violation notice (§9) render under the row inside the rail.

### 2.4 Stacked (window < 1100: detail slides over the list)

Window 1024 × 700, sidebar 232, pane 792:

```
 ← detail pane 792 (list hidden behind it) ──────────────────────────────────────── →
  ‹ Work items  esc   ✱ Bug · #20101           ↻ Synced 2m ago   ⚲   ↗ Open  o   ⋯     toolbar 28: back first
                                                                                       
  Timezone bug in daily settlement job                                                 
  ◉ Doing ⌄    ⌃ Priority 4    infra    settlement                                     
  ┌────────────────────────────────────┬────────────────────────────────────┐          
  │ Assigned to                        │ Project                            │          
  │ (AL) Ada Lindqvist                 │ ▦ Storefront                       │          
  ├────────────────────────────────────┼────────────────────────────────────┤          
  │ Area                               │ Iteration                          │          
  │ ⌗ Storefront › Infra               │ ▤ Sprint 41 · Sep 29 – Oct 10  ●   │          
  └────────────────────────────────────┴────────────────────────────────────┘          
  Created Apr 22 by Kwame Mensah · Changed 4d ago by Mei Tanaka                        
  …sections as §2.2                                                                    
```

- The back button leads the toolbar exactly like the PR review header (0004 §3.1): ghost, `chevron-left` + "Work items" + `Esc` keycap; click or `Esc` (when no editor or menu has the keyboard, §10.3) slides back to the list with the row still selected.
- At the 900 px minimum window the pane is 668 (sidebar) or 844 (rail): the grid keeps two columns down to a 240 px cell. Below that (`font_scale` ≥ 1.2 at 668), the grid becomes one column of four cells; nothing else changes.
- The freshness label shortens before anything else when the toolbar runs out of room: "Synced 2m ago" → "2m"; "Couldn't sync: …" → "Couldn't sync"; the tooltip and accessible label keep the full text. Then the Open button drops its label to the icon + keycap.

### 2.5 Sticky compact header

When the title row scrolls above the pane top, a 40 px bar pins to the top of the pane: type glyph, `#20101` (`type.code_inline`, `text.secondary`), the title (`type.body_strong`, one line, ellipsis), the state chip-menu, then the freshness button and ⋯. It appears and disappears with `motion.instant` (it tracks scroll under the user's hand; tokens §7.3). Its controls are the same controls: `s` still opens the state menu anchored to whichever chip is visible.

## 3. Header

### 3.1 Toolbar row

```
 ← pane content ──────────────────────────────────────────────────────────────────────────── →
 │✱│6│Bug · #20101│ flex │↻ Synced 2m ago│8│⚲│4│↗ Open  o│4│⋯│
  16   type.label          ghost sm 24      24    ghost sm   24
```

| Part | Spec |
|---|---|
| Type glyph | `size.icon.md` 16, shape and `wi.*` color by type (tokens §2.8; in the detail the color is the type, unlike pins where color is status). |
| Type · id | "Bug · #20101": type in `type.label` `text.tertiary`, `#20101` in `type.code_inline` `text.secondary`. Clicking the id copies it (same as `⇧Y`), toast "Copied #20101" (0004 FR-14 pattern). Tooltip "Copy id  ⇧Y". |
| Freshness + Sync this item | §3.4. Right-aligned group start; its width changes grow leftward so Pin, Open and ⋯ never move. |
| Pin toggle | pins.md §6.2 unchanged: icon-only 24 × 24, immediately left of Open. Replaces today's text "Pin" button. |
| Open | Ghost button 24 high: `external-link` 14 + "Open" + keycap `o`. Tooltip "Open in Azure DevOps  o". Demo org: disabled, tooltip "Demo org: fake data, nothing opens in Azure DevOps" (existing copy). Replaces today's secondary "Open in Azure DevOps" button: the header gets quieter and matches the PR review's "Open" (0004 FR-14). |
| ⋯ More | Ghost icon button 24 × 24, `ellipsis`. Opens the row action menu (command-palette.md §4.4, `⌘K`) anchored under the button: Copy link `y` · Copy id `⇧Y` · Sync this work item `⌘⇧R` · Move to project… `⇧P` (slice C, v0.2: absent in v0.1) · Remove link… · Unassign · Open in Azure DevOps `o` · Pin / Unpin `⌘D`. Tooltip "More actions  ⌘K". |

### 3.2 Title

- `type.title` 20/26, `text.primary`, wraps (no truncation; titles are up to 255 characters), prose max width.
- **Rest:** text only. **Hover / detail cursor on the title:** `bg.hover` behind the text block, `radius.sm`, inset `space.4` around it, and a `pencil` 14 `text.tertiary` after the last word. Tooltip "Edit title  t".
- **Editing** (`t`, `↩` on the cursor, or a click): the text becomes a text field in place, same font and size, so nothing moves: `bg.input`, 1 px `border.default`, `radius.sm`, 2 px `border.focus` focus ring, padding `space.4`. All text is selected. Under it a 16 px hint line (`type.small` `text.tertiary`): "↩ Save · Esc Cancel"; from 230 characters the hint's right end shows "240/255" (`type.numeric`).
- Newlines can't be typed; pasted newlines become spaces. `↩` or a click outside saves; `Esc` restores the old title and discards.
- **Empty:** nothing is sent (spec FR-12); the hint line reads "Title can't be empty" with `circle-alert` 12 in `status.warning.fg`.
- Saving shows the new title immediately with the pending row under it (§9.1).

### 3.3 State chip-menu, priority, tags

```
 ◉ Doing ⌄    ⌃ Priority 4    infra    settlement
 └ chip 24 ┘  └ read-only ┘   └ tag chips ─────┘
```

**State chip:** the existing `Chip` with its category tone (Proposed/Completed/Removed neutral, In progress accent, Resolved success; `state_tone`), plus a category glyph before the word and a `chevron-down` 12 after it. As a control: 24 high (`size.control.sm`, meets `size.hit_min`), hover adds `bg.hover` over the chip wash, pressed `bg.active`, keyboard cursor focus ring (radius `radius.xs` + 2). Click, `s`, `⇧S` or `↩` on the cursor opens the menu.

| State category | Glyph (in chip and menu) |
|---|---|
| Proposed | `circle-dashed` |
| In progress | `circle-dot` |
| Resolved | `circle-check` |
| Completed | `circle-check-big` |
| Removed | `circle-slash` |

The glyph gives the chip a non-color channel (principles §6 color independence); it takes the chip's foreground token.

**State menu** (`menu-in`, anchored under the chip, left-aligned, 280 wide = `size.action_menu_width`, `bg.overlay`, 1 px `border.default`, `radius.md`, `elevation.2`, `z.dropdown`):

```
 ┌──────────────────────────────────────┐
 │ ⌕ res▌                               │  type-to-filter line (appears on the first keystroke)
 │ Change state                         │  type.label text.tertiary (shown while unfiltered)
 │ ◉  Doing                    ✓        │  current: check, not selectable
 │ ◉  Code review                       │
 │ ✓  Resolved                          │  ← selection
 │ ✓  Closed                            │
 │ ⊘  Removed                           │
 └──────────────────────────────────────┘
```

- Rows 32 (`size.row.compact`), padding `space.8`, glyph 14 tinted by category (pins.md §3.3 work-item tints: In progress `accent.fg`, Resolved `status.success.fg`, Proposed `text.secondary`, Completed/Removed `text.tertiary`), word `type.body` `text.primary`, the current state's `check` 14 `text.secondary`.
- Only the states reachable from the current one, in category order (spec FR-11). No digit keys: typing filters (spec §3.2: `s` `r` `e` `s` `↩`). The first printable key replaces the "Change state" label with a filter line (`search` 12 `text.tertiary` + the typed text, `type.body`); matched characters `text.match` 560; `⌫` on an empty filter restores the label.
- `↑ ↓` move (wrap), `↩` applies, `Esc` clears the filter, then closes; a click outside closes. Selection starts on the first state after the current one (the likely next step), or the first match while filtering. Filtered to nothing: one row "No state matches "zz"" `text.tertiary`.
- Type metadata not cached yet (spec FR-33): nothing for 150 ms, then a "Loading…" row (`text.tertiary`), kept ≥ 300 ms once shown.
- Applying closes the menu instantly; the chip shows the new state at once with the pending line (§9.1). If Azure DevOps answers with a rule (a required field for that state), the chip reverts and the rule notice opens under the chip row (§9.5).

**Priority:** read-only, as today: `priority.*` glyph 14 + "Priority 4", `type.label` `text.secondary`. Not a control, not a cursor stop.

**Tags:** read-only neutral chips, as today, wrapping after priority.

### 3.4 Freshness and Sync this item

One ghost button: a 14 px glyph + label, `type.small`, 24 high. The label is the freshness; the button syncs this one item (spec FR-29, FR-30; `⌘⇧R`; also ⋯ › Sync this work item and `⌘P` › "Work item: Sync this work item"). Tooltip: the spec's full freshness, then the action: "Last confirmed 14:02:11 · refreshes every 2 min with Work items · Sync this work item  ⌘⇧R"; for items no lane covers "Last confirmed 14:02:11 · only when you sync it · Sync this work item  ⌘⇧R".

| Variant | Glyph | Label | Colors | Click |
|---|---|---|---|---|
| Fresh (< 1 min) | `refresh-cw` | "Synced just now" | glyph + label `text.tertiary` | syncs |
| Fresh (< 15 min) | `refresh-cw` | "Synced 2m ago" | `text.tertiary` | syncs |
| Stale (≥ 15 min, principles §3.3) | `refresh-cw` | "Updated 18m ago" | `text.secondary` (one step louder, no hue) | syncs |
| Partly synced (description and links never fetched, spec FR-29) | `circle-dashed` | "Partly synced" | `text.secondary` | syncs (FR-31 also fetches it once after the selection rests 400 ms; the label then shows "Syncing…") |
| Syncing | `loader-circle`, `glyph-spin` (pins.md §9) replacing the icon (principles §3.2 inline spinner) | "Syncing…" | glyph `status.info.fg`, label `text.tertiary` | no-op |
| Just synced (pressed again within 10 s, spec FR-30) | `refresh-cw` | "Just synced" for 2 s, then the normal label | `text.tertiary` | nothing sent |
| Failed | `circle-x` | "Couldn't sync · Retry" | glyph `status.danger.fg`, label `text.secondary` | retries; tooltip "Couldn't sync: {cause} · Retry  ⌘⇧R" (e.g. "Couldn't sync: Azure DevOps returned 503") |
| Waiting (throttled, after 3 s in the governor's user queue) | `clock` | "Waiting for Azure DevOps…" (0004 FR-15 wording) | `text.tertiary` | no-op; tooltip "Azure DevOps asked adoken to slow down. This item syncs as soon as it allows." |
| Offline | `wifi-off` | "Offline · synced 2h ago" | `text.tertiary`; button disabled (spec §3.4) | tooltip "Offline — reconnect to sync" |
| Demo org | `refresh-cw` | "Demo data" | `text.tertiary`; button disabled | tooltip "Demo org: nothing syncs from Azure DevOps" |

- Times: "Synced 2m ago" uses principles §5 relative times; the label updates on minute boundaries from a minute timer, not the frame clock. The freshness is the later of the item's last fetch and its covering lane's last good pass (spec FR-29), so an unchanged item of mine reads "Synced 1m ago" without ever being refetched.
- "Syncing…" appears on the press frame (it's the button's own feedback) and stays at least 300 ms once shown (principles §3.2, no flash); then the label becomes "Synced just now" instantly. Reduce motion: still `loader-circle`, same words.
- The statusbar Sync activity list shows the same request as a "You asked" row (activity.md) with the same words: "Syncing…", "Synced just now", and the failure with its cause spelled out ("Couldn't sync: {cause}"); the header keeps the short "Couldn't sync · Retry" because the toolbar has no room for causes.
- After any successful write the answer is the re-read (server wins, spec FR-7) and the label becomes "Synced just now" without a spinner: the write's own pending line (§9.1) was the progress.
- Auto-sync passes (background) don't spin this button; they only refresh the label.

## 4. Fields

### 4.1 Property cell (grid)

```
 ← cell (half the grid) ────────────────────────────── →
 ┌──────────────────────────────────────────────────────┐
 │12│ Assigned to                                    │12│  label: type.label, text.tertiary
 │12│ (AL)│6│ Ada Lindqvist                    │ ⌄ │12│  value row 20: avatar/icon, type.body, chevron
 └──────────────────────────────────────────────────────┘  52 high, radius.sm
```

| Part | Spec |
|---|---|
| Grid | 2 columns, `space.8` column gap, `space.4` row gap; whole grid framed by nothing (no card): cells are quiet until touched. A single 1 px `border.subtle` hairline sits above and below the grid, inset to the prose width, to separate it from the title block and the meta line. |
| Label | `type.label` 11/14 `text.tertiary`, sentence case: "Assigned to", "Project", "Area", "Iteration". |
| Value | `type.body` 13/20 `text.primary`; leading avatar or icon (§4.2), `space.6` gap; one line, ellipsis (full value in tooltip and label). |
| Chevron | `chevron-down` 12 `text.tertiary`, right edge. Visible on hover **and** under the keyboard cursor (principles §1.2), hidden at rest (Q-2). Never shown when the field can't be edited (read-only token, §9.4; Not available, §12). |
| Rest | No background. |
| Hover | `bg.hover` (in over `motion.fast`), chevron, tooltip after 500 ms: "Change assignee  a" / "Move to another project…  ⇧P" / "Change area  ⇧A" / "Change iteration  ⇧I". Until slice C ships (v0.2) the Project cell isn't editable: it renders like a read-only cell (§9.4: no chevron, no hover wash, not a cursor stop). |
| Pressed | `bg.active`. |
| Keyboard cursor | `bg.hover` + focus ring (2 px `border.focus`, 1 px gap, radius `radius.sm` + 2), chevron. |
| Open (picker showing) | `bg.selected_muted`, chevron rotated to `chevron-up` (instant). |
| Pending / failed / conflict / rule | §9; the cell grows to fit the status line (52 → 68), the row's other cell grows with it so the grid stays aligned. |

### 4.2 Values

| Field | Leading | Value text | Empty / special |
|---|---|---|---|
| Assigned to | Avatar `size.avatar.md` 20 (principles §4: image when cached, else initials) | Display name | "Unassigned" in `text.tertiary` with `user` 16 `text.tertiary` |
| Project | `folder-kanban` 16 `text.tertiary` | Project name | — |
| Area | `folder-tree` 16 `text.tertiary` | Breadcrumb path (§4.3) | — |
| Iteration | `calendar-range` 16 `text.tertiary` | Leaf name, then " · Sep 29 – Oct 10" in `text.tertiary` when dates are known (spec), then a **Current** marker for the current sprint: `type.label` `text.secondary` on `bg.active`, `radius.xs`, 16 high | Root iteration (no sprint): the project name + " · No sprint" in `text.tertiary` |

The value is the server's until a write is confirmed; a pending value shows the new value (it's what I asked for) plus the pending line (§9.1).

### 4.3 Breadcrumb paths

ADO paths (`Storefront\Infra\Payments`) render as segments joined by ` › ` (U+203A with a space each side):

- Separators and ancestor segments `text.tertiary`; the leaf `text.primary`. The leaf is what changes most and what you look for.
- Too long for the cell: keep the first segment and the leaf, collapse the middle into "…" (`Storefront › … › Payments`), then truncate the leaf with an ellipsis if still too long. Never truncate from the start: the root says which project, the leaf says which team.
- Tooltip and accessible label: the full path spoken with "in": "Area: Payments, in Storefront, Infra".
- Same rendering in pickers (§4.5), the rail, the Move sheet (§4.6) and history sentences.

### 4.4 Meta line

`type.small` 12/16 `text.tertiary`, under the grid's bottom hairline with `space.8` above: "Created Apr 22 by Kwame Mensah · Changed 4d ago by Mei Tanaka". Times relative per principles §5, full timestamps in the tooltip of each time. Read-only; not a cursor stop. In the wide rail, Created and Changed are two rows below a hairline (§2.3).

### 4.5 Pickers (Assigned to, Area, Iteration)

One popover pattern for the three fields; State uses the menu (§3.3), Project uses the sheet (§4.6).

```
 ┌───────────────────────────────────────────────┐  anchored under the cell, left edges aligned
 │ ⌕  dan                                        │  search field 32, bg.input, autofocus
 │───────────────────────────────────────────────│
 │ (AL) Assign to me                          i  │  rows 32 (size.row.compact)
 │  ⊘  Unassigned                                │
 │ Recent                                        │  group label type.label text.tertiary
 │ (DK) Dana Kim         dana@contoso.com     ✓  │  current: check
 │ People                                        │
 │ (DO) Dan Okafor       dan.o@contoso.com       │
 │───────────────────────────────────────────────│
 │ From your directory                           │  appears by itself (spec FR-13)
 │ (DR) Dana Ruiz        dana.ruiz@contoso.com   │
 └───────────────────────────────────────────────┘
   ↑↓ Move · ↩ Choose · Esc Close                    footer hint 16, type.small text.tertiary
```

| Property | Value |
|---|---|
| Surface | `bg.overlay` (× `overlay_opacity` and frost blur over a custom background, tokens §10.2), 1 px `border.default`, `radius.md`, `elevation.2`, `z.dropdown`, `menu-in` / `menu-out`. |
| Width | max(cell width, 320 = `size.picker_popover`, new). Height: up to 8.5 rows, the half row hints at more (palette convention). |
| Search field | Text field 32 high, `search` 14 `text.tertiary`, placeholder "Search people" / "Search areas" / "Search iterations" (`text.tertiary`). Typing filters with the `Cmd+P` fuzzy matcher; matched characters `text.match` 560. Printable keys always go to the field. |
| Rows | Leading avatar 20 or icon 16; primary text `type.body`; trailing secondary text `type.small` `text.tertiary`; `check` 14 on the current value; keycap where a key exists. Selection = `bg.selected` (the picker has focus), hover `bg.hover` (hover does not move the selection, palette rule). |
| Keys | `↑ ↓` move (wrap), `↩` chooses and closes, `Esc` closes without change (focus returns to the cell), a click outside closes. Area tree: `→` expands, `←` collapses or goes to the parent. |
| Loading | Local lists paint on the first frame. Data not cached yet (classification trees, spec FR-33): nothing for 150 ms, then a "Loading…" row (`text.tertiary`, kept ≥ 300 ms once shown); until the tree arrives the area/iteration pickers list the paths seen in loaded work items (spec FR-14). |
| Directory search (people only, spec FR-13) | No row to press: when 3+ characters are typed, fewer than 5 local people match and typing pauses 300 ms, a **From your directory** group appears at the end with one "Searching your directory…" row (`loader-circle` still for 150 ms, then spinning; reduce motion: still). Results replace that row without animation (keyboard list) and never move the selection. Failure: the group shows "Couldn't search your directory · Retry" (`↩` retries). Offline: no group. |
| Empty | "No people match "zzz"" / "No areas match" / "No iterations match", `type.body` `text.tertiary`, one 32 row (a pending directory search keeps its group under it). |

**Assigned to.** Rows: "Assign to me" (my avatar, keycap `i`), "Unassigned" (`user-x`), **Recent** (people I assigned in adoken, most recent first, max 5), **People** (people already in the store, matched on name and sign-in name; groups excluded, spec FR-13), then **From your directory** (above). Secondary text: the sign-in name, so two "Dan"s differ.

**Area.** A tree of the project's area paths (indent `space.16` per level, `chevron-right` / `chevron-down` 12 disclosure, `folder-tree` 14 on the root). The current path is expanded and selected on open. While a query is typed the tree flattens into matching paths rendered as breadcrumbs (§4.3), matches highlighted in any segment.

**Iteration.** Above the tree, three shortcut groups (spec FR-14): **Current** (dates include today, with the Current marker), **Next** (the next start date), **Recent** (the last two finished). Then the full tree, as the area picker, with the root as the first row (shown as the project name + " · No sprint"). Shortcut rows: leaf name + dates (`type.small` `text.tertiary`), parent path as secondary text only when two leaves share a name. The current value is checked wherever it appears.

### 4.6 Move to project sheet (`⇧P`; slice C, v0.2)

Not built in v0.1 (spec 0015 Q-1). Kept here so v0.2 starts from an agreed design.

Moving changes Project, Area and Iteration at once and resets State, so it's never a one-key write (spec FR-24): a dialog sheet. Clicking the Project cell opens it too.

```
 ┌───────────────────────────────────────────────────────────────┐  480 wide (size.sheet_width, new)
 │ Move #20101 to another project                                │  type.heading
 │ Timezone bug in daily settlement job                          │  type.body text.secondary, 1 line
 │                                                               │
 │ Project     ▦ Payments                                    ⌄   │  rows 36: label 96 + picker field
 │ Area        ⌗ Payments                                    ⌄   │  defaults to the target's root
 │ Iteration   ▤ Payments · No sprint                        ⌄   │
 │                                                               │
 │ ⓘ State resets to New. Child items stay in Storefront.        │  consequences (spec FR-24), type.small text.secondary
 │                                                               │
 │ [ Move  ↩ ]                                    [ Cancel  esc ]│
 └───────────────────────────────────────────────────────────────┘
```

- Surface: `bg.overlay`, 1 px `border.default`, `radius.lg`, `elevation.3`, padding `space.20`, over `bg.scrim`; `dialog-in`; `z.dialog`. Same frame as the review's confirm dialogs (0004).
- Focus starts on Project with its picker open (typing filters the org's projects at once, synced or not). Choosing a project defaults Area and Iteration to its roots and moves focus to **Move**, so the whole flow is `⇧P` `pay` `↩` `↩`. `Tab` / `⇧Tab` walk the rows and buttons; `↩` on a row opens its picker (§4.5 popover on the target's trees, anchored to the row); `↩` on Move (or `⌘↩` anywhere in the sheet) moves; `Esc` closes the open picker, then the sheet.
- **Consequences line** (`info` 12 `text.tertiary`, `type.small` `text.secondary`): the spec's words for what Azure DevOps will do, filled in: "State resets to New. Child items stay in Storefront." (the second sentence only when the item has children).
- **While the target's types load** (one request, spec FR-24 step 2): Area and Iteration rows show "Checking Payments…" in `text.tertiary`; Move is disabled. **Type missing:** the rows are replaced by "Project Payments has no Bug type." (`circle-alert` 14 `status.warning.fg`, `type.body`), Move stays disabled; choosing another project clears it.
- **Can't move this type** (test-management and hidden types): `⇧P`, the Project cell and the palette command don't open the sheet; the Project cell tooltip and a toast say "Test cases can't be moved to another project" (the type's name); the cell shows no chevron.
- Move is disabled until a different project is chosen ("Choose another project" tooltip).
- On Move the sheet closes, the grid shows the new values with one pending line spanning the grid ("◷ Moving to Payments…"); the item stays selected (spec). A 403 shows the spec's sentence as the failed line: "Moving needs the Move work items out of this project permission in Azure DevOps (Contributors don't have it by default)."

## 5. Description

### 5.1 Rendering

The description renders as rich text through a new `RichText` element in `crates/ui` (shared with comments, §7, and usable by the PR review later). Typography maps HTML or Markdown blocks to tokens; no style, color or size from the source is ever applied.

| Block | Rendering |
|---|---|
| Paragraph | `type.body` `text.primary`, `space.8` between blocks |
| Heading 1 | `type.heading` 15/20 600, `space.16` above |
| Heading 2–6 | `type.body_strong` 13/20, `space.12` above |
| Bold / italic / underline / strikethrough | weight 600 / italic / underline / strikethrough, same color (links are told apart from underlined text by `text.link`) |
| Link | `text.link`, underline on hover and keyboard focus; click opens the system browser; work item links (`#1234` or ADO work item URLs) open in Work items instead, PR links in Pull requests. Tooltip: the URL. |
| Mention | §7.2 rules |
| Bulleted / numbered list | Marker column 16 (`•` / `1.` in `text.tertiary`, tabular), nested levels indent `space.16`; checkboxes (`- [ ]`) render as a read-only `square` / `square-check` 14 |
| Inline code | `type.code_inline` on `bg.code`, padding 0 `space.4`, `radius.xs` |
| Code block | `type.code` on `bg.code` (always opaque, tokens §10.2), padding `space.12`, `radius.sm`, 1 px `border.subtle`; horizontal scroll inside the block, no wrap; no syntax highlighting (spec FR-3), `syntax.variable` text |
| Quote | 2 px `border.strong` bar on the left, `space.12` indent, `text.secondary` |
| Horizontal rule | 1 px `border.subtle`, `space.16` above and below |
| Table | A plain text grid: `type.small`, header row `type.label` `text.secondary`, 1 px `border.subtle` cell lines, padding `space.4` × `space.8`; wider than the prose column → horizontal scroll inside the table. No cell colors or merged-cell styling. |
| Image | Never fetched (spec). A chip in place: `image` 14 + "Image" + alt text if any, `type.small` `text.secondary` on `bg.active`, `radius.xs`, 24 high; click or `↩` opens the item in Azure DevOps. Tooltip "Images open in Azure DevOps  o". |
| Anything else (embedded HTML, unknown tags) | Its text content as a paragraph |

- **Long descriptions:** taller than 24 lines (480 px) clip at 480 with a "Show all · 62 lines" ghost button below (no gradient fade: a fade would need a non-token color stop). `↩` on the button or `d`-then-`Esc` keeps the expansion for this item until the selection changes.
- **Empty:** "No description." `type.body` `text.tertiary`, plus with write access a ghost button "Add a description  d" (spec §States).
- **Read-only token:** rendering unchanged, no Edit button.
- **Repro Steps and Acceptance Criteria** (spec FR-3, Q-10 accepted 2026-10-01): when the item has them, each renders read-only as its own block after Description with the same section head style ("Repro steps", "Acceptance criteria"), no Edit button, the same clip at 480 px and the same blocked-image chips. They are not cursor stops except for their Show all button.

### 5.2 Editor

`d`, `↩` on the description cursor stop, the section head's "✎ Edit `d`" ghost button, or a double-click in the text opens the editor in place of the rendered block (instant, scroll position kept so the first visible line stays put).

```
 Description                                     [ Write | Preview ]  ⌘E
 ┌────────────────────────────────────────────────────────────────────┐
 │ Settlement runs at 00:05 UTC, but merchants in UTC+10 see the      │  type.code, bg.input
 │ previous day's totals.                                             │  min 8 lines, grows to 24,
 │                                                                    │  then scrolls inside
 │ - Repro: set merchant tz to Australia/Sydney                       │
 │ - Expected: totals for the local calendar day                      │
 └────────────────────────────────────────────────────────────────────┘
 Markdown                                                   Draft saved
 [ Save  ⌘↩ ]   [ Cancel  esc ]
```

| Part | Spec |
|---|---|
| Field | `bg.input`, 1 px `border.default`, `radius.sm`, 2 px `border.focus` ring while focused; padding `space.12`; Markdown source in `type.code` (the code font keeps list markers and fences aligned; Q-7). Min 8 lines, grows with content up to 24 lines, then scrolls inside. Soft wrap at the field width. |
| Write / Preview | Segmented control 24 high, right of the section head label; `⌘E` toggles while the editor has focus. Preview shows §5.1 rendering of the current text in the same box (`bg.surface`, same border) and keeps the field's height so the buttons don't jump. |
| Format hint | Left of the line under the field, `type.small` `text.tertiary`, exactly the spec's words: **Markdown fields:** "Markdown". **HTML fields:** "HTML (saved as HTML)" followed by an `info` 12 whose tooltip reads "This description is stored as HTML in Azure DevOps. Write Markdown here; adoken saves it back as HTML." No syntax cheat line: the Preview is the teacher. |
| Draft indicator | Right of the same line: "Draft saved" `type.small` `text.tertiary` after each local draft write (debounced, no animation). Blank until the first change. |
| Buttons | "Save" primary 28 with `⌘↩` keycap; "Cancel" ghost with `esc` keycap. Save disabled (and `⌘↩` no-op) while the text equals the server's. |
| Keys | `⌘↩` saves; `Esc` closes the editor and **keeps the draft** (spec); `⌘E` Write/Preview; `Tab` inserts nothing (moves focus to Save, then Cancel: no focus trap); standard text-input keys. |

**Draft marker.** When a description draft exists and the editor is closed, the section head shows a neutral "Draft" chip after the label, and the Edit button reads "✎ Resume draft  d". The rendered block shows the server's description (the draft is mine, not Azure DevOps'). The chip's tooltip: "Unsaved description draft from 2h ago · d to resume". Saving successfully deletes the draft and the chip.

**Saving.** On `⌘↩` the editor closes, the rendered block shows the new text at once with the pending line under the section head (§9.1). Failure reopens nothing: the rendered block reverts to the server's text, the draft stays, and the inline error under the section offers "Resume draft".

### 5.3 Blocked: formatting adoken can't edit

When the spec determines the HTML can't round-trip (tables, images, inline styles/colors, custom attributes), the editor doesn't open. Pressing `d` (or the Edit button) shows an inline notice under the section head instead, and the Edit button is replaced by "↗ Edit in Azure DevOps  o":

```
 Description                                        ↗ Edit in Azure DevOps  o
 ┌────────────────────────────────────────────────────────────────────┐
 │ ⓘ This description uses formatting adoken can't edit yet:          │  bg.raised, radius.sm,
 │   a table and 2 images. Edit it in Azure DevOps.  [Open  o]         │  info glyph text.tertiary
 └────────────────────────────────────────────────────────────────────┘
 …rendered description (§5.1) as usual…
```

- Copy: "This description uses formatting adoken can't edit yet: {list}. Edit it in Azure DevOps." with {list} naming what was found, joined in plain words: "a table", "2 images", "text colors", "custom formatting". Button "Open" (`o`).
- Surface `bg.raised`, `radius.sm`, padding `space.12`, `info` 14 `text.tertiary`, text `type.small` `text.secondary`. Neutral, not a warning: nothing failed.
- The notice is always visible once the description is known to be blocked (not only after pressing `d`), so the reason precedes the attempt; `d` focuses the notice's Open button.

## 6. Links

### 6.1 Section

```
 Links  6
 Parent
  ✱ #20092  As support I can see why a payment failed       ◉ Active   (KM)
 Children  2
  ✱ #20118  Add tz to settlement job config                 ◌ New      (AL)
  ✱ #20119  Backfill wrong-day totals                       ◌ New
 Related
  ✱ #20146  Settlement totals off for DST change            ◌ New      (MT)    ✕   ← keyboard cursor
 Pull requests
  ⎇ !4295   Fix timezone bug in daily settlement job          Completed
 Commits
  ◦ 3f9c2a1 Use merchant tz for the settlement day   payments-api
  ⛓ Add link…  l
```

| Part | Spec |
|---|---|
| Section head | "Links" + total count (no action in the head: Add link sits after the rows so visual order matches the cursor walk, spec §3.3). |
| Add link row | Last row of the section, 36 high like a link row: `link` 14 `text.secondary` + "Add link…" `type.body` `text.secondary` + keycap `l`; hover/cursor as a link row. Hidden with a read-only token (the token hint covers it, §9.4). |
| Group labels | `type.label` `text.tertiary`, 24 high, with a count when > 1. Fixed order (spec FR-23): Parent · Children · Related · Predecessors · Successors · Duplicates · Duplicate of · Pull requests · Commits · Other links (branches, builds, hyperlinks, attachments). Empty groups are omitted. |
| Work item row | 36 (`size.control.lg`), padding `space.8`, `radius.sm`. Type glyph 14 in `wi.*`; `#id` `type.code_inline` `text.secondary` (fixed 56 column, tabular); title `type.body` `text.primary`, flex, ellipsis; state: category glyph 12 (§3.3) + word `type.small` `text.secondary`, max 96; assignee avatar `size.avatar.sm` 16 (none when unassigned); remove button (§6.3). |
| Unsynced work item | `circle-dashed` 14 `text.tertiary`, `#id`, "Not synced" `text.tertiary` (0004 FR-17 convention). Opens all the same (Work items fetches it). |
| Pull request row | PR glyph tinted by status (tokens §2.8), `!id`, title, status word `type.small` `text.tertiary` (as today). Read-only: no remove button; tooltip on the row "Open !4295 · link or unlink pull requests from the pull request". |
| Commit row | `git-commit-horizontal` 14 `text.tertiary`, short SHA `type.code_inline` `text.secondary`, message `type.body`, repository `type.small` `text.tertiary`. Read-only; opens in Azure DevOps. |
| Other links | Hyperlinks and any relation the spec keeps read-only: `link` 14 `text.tertiary`, the comment or URL host, opens the system browser. |
| Long groups | Children (or any group) beyond 5 rows: the first 5 then a ghost row "Show 7 more" (`↩` expands, stays expanded for this item). |
| Row states | Rest none · hover `bg.hover` · cursor `bg.hover` + focus ring · pressed `bg.active`. `↩` / click opens: work item → its detail (back with `⌘[`), PR → Pull requests, commit/other → browser. |
| Empty section | "No links." `type.body` `text.tertiary`, then the Add link row. |

The old hint line ("Enter from the list, then ↑ ↓ and Enter to open. Esc goes back.") is removed: the cursor walk (§10.3) and the cheat sheet teach it.

### 6.2 Add link

Two steps, both keyboard-only possible: `l` → relation menu → work item picker. Typical: `l` `3` `20146` `↩`.

**Step 1: relation menu** (state-menu frame, §3.3; anchored under the Add link row; title "Add link as…"):

| Key | Row | Notes |
|---|---|---|
| `1` | Parent… | Reads "Change parent…" when one exists |
| `2` | Child… | |
| `3` | Related… | |
| `4` | Predecessor… | |
| `5` | Successor… | |
| `6` | Duplicate… | |
| `7` | Duplicate of… | |

The spec owns which relations are offered; the list above mirrors spec §Links. Each row has a 14 glyph in `text.secondary`: `arrow-up-from-line` (parent), `arrow-down-to-line` (child), `link` (related), `arrow-left` (predecessor), `arrow-right` (successor), `copy` (duplicate of / duplicate).

**Step 2: work item picker.** The 0004 FR-17 picker, unchanged in frame and engine (`Cmd+P` engine and query language scoped to work items; `#1234`, bare ids, pasted links, filters), with these differences:

- Title: "Set the parent of #20101" / "Change the parent of #20101" / "Add a child to #20101" / "Link a related item to #20101" / "Add a predecessor of #20101" / "Add a successor of #20101" / "Mark an item as a duplicate of #20101" / "Mark #20101 as a duplicate of…".
- The item itself and items already linked with the same relation show in `text.disabled` with a right-aligned reason ("This item", "Already a child") and can't be chosen.
- `↩` links; the picker closes. The new row appears in its group at once with the pending line (§9.1). Change parent replaces the old parent row in place (old row leaves when confirmed).

Direct palette commands skip step 1: "Work item: add parent…", "…add child…", "…add related link…" (titles per spec).

### 6.3 Remove link

- Remove button: ghost 24 × 24, `unlink` 14 `text.tertiary` (hover `text.primary` on `bg.hover`), right-anchored overlay like the pins unpin button (pins.md §3.7): appears on row hover **and** under the keyboard cursor. Not a Tab stop. Tooltip "Remove link  ⌫". Label "Remove link to #20146".
- `⌫` on a focused work-item link row, or the button, opens the confirm dialog (the review's confirm frame, 0004): title "Remove link to #20146?", body "#20146 Settlement totals off for DST change stays as it is; only this related link goes.", buttons "Remove" primary `↩` · "Cancel" ghost `esc`. Parent: "…only its parent link goes." 
- Confirmed: the row stays in place with title `text.tertiary` and the pending line "◷ Removing…"; on confirm it leaves (`row-remove` when the pointer did it; instant when the keyboard cursor is in the section, tokens §7.3). Failure restores it with an inline error.

## 7. Discussion

### 7.1 Thread

```
 Discussion  4                                                         
   Show 2 earlier comments                                                    ghost row
 (KM)  Kwame Mensah  Apr 22                                                   name type.body_strong, time type.small
       Seen on 3 merchants this week. @Ada Lindqvist can you take it?         rich text
                                                                              space.16 between comments
 (AL)  Ada Lindqvist  4d · edited                                             
       On it. Root cause is the job's hard-coded UTC day boundary:            
       ┌─────────────────────────────────────────────┐                        
       │ let day = now.date_naive();                  │                        
       └─────────────────────────────────────────────┘                        
 (AL)  You  ◷ Sending…                                                        pending comment
       Fix is in !4295.                                                        body text.secondary
```

| Part | Spec |
|---|---|
| Order | Oldest → newest, composer last (chat order; Q-3). On open, if there are more than 5 comments only the last 5 show, with "Show {n} earlier comments" (ghost row, `↩` expands). |
| Avatar | `size.avatar.lg` 28 in a 28 column, `space.12` gap; top-aligned with the name line. |
| Name line | Name `type.body_strong` `text.primary`; "You" for my comments; time `type.small` `text.tertiary` (relative, tooltip full timestamp); " · edited" `text.tertiary` when the comment was edited. |
| Body | `RichText` (§5.1) at prose width minus the avatar column. Images: the chip. |
| New comments from sync | `row-insert` when the section is on screen and the keyboard cursor isn't in it; otherwise instant. No toast; the inbox handles mentions. |
| Count | Section head count = total comments, `type.numeric`. |
| Empty | "No comments yet." `type.body` `text.tertiary`, then the composer. |

### 7.2 Mentions

- Rendered as the display name with an `@`: "@Dana Kim", weight 560, `text.link`. Not a link (there's no person page); tooltip shows the unique name ("dana@contoso.com").
- Mentions of me: `text.primary` 560 on `accent.wash`, `radius.xs`, padding 0 `space.2`, so my name jumps out of a long thread without a hue-only signal (the wash is a shape; the name is mine).
- Unresolved identities: "@Unknown user" in `text.tertiary` (tooltip: the raw id is never shown; "Not in adoken's people list yet").
- Contrast (§13.3): `text.link` ≥ 5.16 on the hover and selected-muted washes; `text.primary` on `accent.wash` ≥ 13.2.

### 7.3 Composer

```
 (AL)  ┌──────────────────────────────────────────────────────────────┐   collapsed: 36 high
       │ Add a comment…                                               │
       └──────────────────────────────────────────────────────────────┘

 (AL)  ┌──────────────────────────────────────────────────────────────┐   focused: min 3 lines,
       │ Fix is in !4295. @da▌                                        │   grows to 12, then scrolls
       │    ┌──────────────────────────────────────┐                  │
       │    │ (DK) Dana Kim      dana@contoso.com   │ ← mention popover│
       │    │ (DO) Dan Okafor    dan.o@contoso.com  │                  │
       │    └──────────────────────────────────────┘                  │
       └──────────────────────────────────────────────────────────────┘
       Markdown · @ to mention                          [ Comment  ⌘↩ ]
```

| Part | Spec |
|---|---|
| Field | My avatar 28 left (same column as comments); text field `bg.input`, 1 px `border.default`, `radius.md`, focus ring; `type.body` (comments are conversational: UI font, unlike the description source). Placeholder "Add a comment…" `text.tertiary`. |
| Collapsed / focused | Collapsed 36 high one line. Focus (`c`, `⇧C`, click, cursor `↩`) expands to 3 lines (instant) and shows the footer. |
| Footer | Left "Markdown · @ to mention" `type.small` `text.tertiary`; right "Comment" primary 28 with `⌘↩` keycap, disabled while empty or whitespace. |
| Mention popover | Opens when `@` is typed after a space or at the start: `menu-in` above the caret line (below when there's no room), 280 wide, rows 32 with avatar 20 + name + sign-in name, max 6 rows, filtered by the text after `@`. It is the people picker's list (spec FR-26: FR-13's sources, groups included; groups show `users` 16 in place of an avatar). `↑ ↓` move, `↩` or `Tab` inserts a **mention chip**, `Esc` closes the popover only. |
| Mention chip (in the field) | "@Dana Ruiz" as one token: `type.body` 560 `text.link`, no background, `radius.xs`; the caret steps over it as one character and `⌫` deletes it whole. Sent as `@<identity id>` (spec). |
| Draft | `Esc` blurs the composer and keeps the text as a local draft (spec). Collapsed with a draft: shows the first line of the draft in `text.secondary` and a neutral "Draft" chip at the right of the field; the section head is unchanged. Restored when the item is reopened. |
| Posting | `⌘↩` or the button: the composer clears and collapses at once; the comment appears at the end of the thread as pending ("You" + "◷ Sending…" in place of the time, body `text.secondary`). Confirmed: time "just now", body `text.primary`, no other change (no toast; optional `ui.action_done` sound). |
| Failed | The pending comment stays in place with an inline error under it: "Couldn't post your comment: {cause}. Your text is saved. [Retry] [Edit]" (`circle-x` 14 `status.danger.fg`, text `type.small` `text.secondary`). Edit moves the text back into the composer and removes the failed row. |
| Offline | Composer stays editable (drafts save); Comment disabled, tooltip "Offline — reconnect to send". |
| Read-only token | No composer; the header token hint covers it (§9.4). |

## 8. History

```
 History
  ↻  Mei Tanaka changed State  New → Doing                          4d
  ⚇  Mei Tanaka assigned it to Ada Lindqvist                         4d
  @  Kwame Mensah mentioned you                                     Apr 22
  ◉  Kwame Mensah created it                                        Apr 22
   Show all 16                                                                ghost row
   Earlier revisions aren't synced; history shows what sync observed.          type.small text.tertiary
```

- Rows 28: glyph 14 `text.tertiary` (existing mapping: created `circle-dot`, changed `refresh-cw`, assigned `user`, mentioned `at-sign`), sentence `type.body` `text.secondary`, time `type.small` `text.tertiary` right-aligned, tabular.
- Field changes, when the spec provides them, read "{who} changed {Field}  {old} → {new}": the field name `text.primary`, values `text.secondary`, the arrow `text.tertiary`; paths use breadcrumbs (§4.3). My own writes read "You changed …".
- Newest first (history is a log, unlike the discussion), last 5 then "Show all {n}" (`↩` expands).
- Not cursor stops except the Show all row. The section is `Role::List` for screen readers.

## 9. Edit states

Every editable element (title, state, the four fields, description, link rows, comments) uses the same visual grammar. The status line sits directly under the element (in a grid cell, under its value; in the rail, under the row; for title/state, under the chip row; for sections, under the section head), `type.small` 12/16, glyph 12, `space.4` above.

### 9.1 Pending (sending)

```
 │ Assigned to                          │
 │ (DK) Dana Kim                        │   new value shown at once, text.primary
 │ ◷ Sending…                           │   clock 12 + "Sending…", text.tertiary
```

- Principles §3.3: `clock` 12 + "Sending…" `text.tertiary`; the new value is shown in its normal style (it's what I asked for); never a check or a "Saved" word.
- **Queued behind another write on the same item** (one in-flight write per item, spec): the same clock + "Sending…" (the user doesn't need to know about the queue; the order is preserved).
- **Throttled / waiting:** after 3 s pending in the governor's user queue (spec §States), the line reads "◷ Waiting for Azure DevOps…" (the 0004 FR-15 wording); tooltip "Azure DevOps asked adoken to slow down. Your change is kept and sends as soon as it allows." Nothing is dropped.
- The pending line appears on the same frame as the change. On confirm, the line disappears instantly (no success toast, principles §3.3) and the optional `ui.action_done` sound plays; a state move that celebrates (spec 0022) shows its celebration instead ([ken.md](ken.md) §12).
- Other edits stay allowed while one is pending (spec); each pending element has its own line.
- Removing: "◷ Removing…". Moving: "◷ Moving to Payments…" (one line spanning the grid).

### 9.2 Failed

```
 │ Assigned to                                              │
 │ (AL) Ada Lindqvist                                       │   reverted to the server value
 │ ⊗ Azure DevOps didn't let you edit work items in this    │   circle-x status.danger.fg,
 │   area.  Retry · Dismiss                                 │   text text.secondary
 │   {Azure DevOps' own message, one line}                  │   text.tertiary
```

- Template (principles §3.4: what happened + what's kept + what to do): "Couldn't change {Field}: {cause}." + **Retry** (holds my value and re-sends it; `↩` when the line has focus) and **Dismiss**. Title and description add "Your text is saved." and offer **Resume draft** when a draft exists.
- Causes, using the spec's sentences (FR-7): 5xx or network: "Couldn't reach Azure DevOps. Nothing changed." · 403 (permission, not scope): "Azure DevOps didn't let you edit work items in this area." followed by Azure DevOps' own message in `text.tertiary` · other refusals that aren't rules or conflicts: Azure DevOps' message, one line, full text in [Details] · missing scope (401/403 on the work item transport): switches the pane to §9.4 and keeps my value as **Retry** ("Your change is kept.") · 404: the pane-level Not available banner (§12).
- [Details] (principles §3.4) expands status code, `x-vss-activityid` and time in `type.code_inline` with [Copy].
- Errors stay until Retry succeeds, Dismiss, or the item changes on the server. The optional `ui.error` sound plays once.

### 9.3 Conflict (someone else changed the same field)

Most conflicts never show: when the other person changed different fields, adoken resends once on their revision and the user only sees the pending line (spec FR-8). When they changed **my** field, the field shows their value and an inline notice under it, holding my value until I choose.

```
 │ Assigned to                                              │
▌│ (KM) Kwame Mensah                                        │   server value; 2 px status.warning.fg bar on the left edge
▌│ ▲ Changed in Azure DevOps by Dana Ruiz 1m ago.           │   triangle-alert status.warning.fg
▌│   Your edit wasn't saved.                                │
▌│   Yours: (DK) Dana Kim          Apply mine · Keep theirs │   my value text.secondary
```

| Part | Spec |
|---|---|
| Notice | Under the element, on `status.warning.wash` spanning the cell (or rail row, or chip row), `radius.sm`, padding `space.8`; `triangle-alert` 12 `status.warning.fg`; sentence `type.small` `text.primary`; "Yours:" line with my value rendered like a value (avatar, breadcrumb) in `text.secondary`. A 2 px `status.warning.fg` bar on the element's left edge stays until resolved, so the eye finds it in a busy pane (a shape, not only a hue). |
| Copy | "Changed in Azure DevOps by {name} {time}. Your edit wasn't saved." (name and time from `System.ChangedBy` / `System.ChangedDate`, spec FR-8). |
| Actions | **Apply mine** (resends on the new revision; the notice becomes the pending line) · **Keep theirs** (drops my value; the notice leaves). Ghost buttons, `type.small`. |
| Title | Same notice under the title; "Yours:" shows my title, one line with ellipsis, full text in the tooltip. |
| Description (spec FR-19) | Under the Description head: "Changed in Azure DevOps by {name} {time}. Your edit wasn't saved." + **Compare** · **Overwrite with mine** · **Discard mine**. My text stays as the draft until I choose. **Compare** expands, in place of the rendered description, two read-only blocks: "Theirs (now in Azure DevOps)" above, "Yours" below, each with a `type.label` `text.tertiary` caption, rendered rich text, `bg.raised`, `radius.sm`, each clipped at 480 with Show all; `Esc` or Compare again collapses. |
| Motion | The notice appears instantly (it answers my action). |
| Keys | The notice is the next cursor stop after its element; `↩` on it focuses its first action; `←` `→` move between actions. |

### 9.4 Read-only token or access level

```
 ✱ Bug · #20101                                  ↻ Synced 2m ago   ⚲   ↗ Open  o   ⋯
 Timezone bug in daily settlement job
 ◉ Doing    ⌃ Priority 4
 ⚿ Editing work items needs Work Items (Read & write).
```

- One passive hint row under the chip row (spec 0001 FR-9; amended 2026-10-01, owner notes; accepted 2026-10-01): `lock` 14 `text.tertiary`, `type.small` `text.secondary`, no button, no keyboard stop. Copy: "Editing work items needs Work Items (Read & write)." The same copy serves the Read-only access level and a token missing `vso.work_write`. Replacing the token lives in Settings › Account and `⌘P` › Replace token… only.
- Values read like plain values (spec FR-28): no chevrons, no pencil, no hover wash on cells, no Edit/Add link buttons, no composer, no remove buttons; the state chip is a plain chip. Letter keys (`t s a i …`) show the hint as a toast "Editing work items needs Work Items (Read & write)" with no action; `⌘P` lists the edit commands disabled with the same reason as subtitle (command-palette.md §4.3).
- Sync this item, Pin, Open, Copy stay (reads).
- If a write is refused for scope (the token changed under us), the pane switches to this state and the element keeps a **Retry** line holding my value ("Your change is kept."), spec FR-28.

### 9.5 Rule violation (Azure DevOps needs another field)

```
 ◉ Doing ⌄    ⌃ Priority 4
 ┌──────────────────────────────────────────────────────────────────────────────┐
 │ ⓘ Azure DevOps needs Root cause to move to Resolved.                         │  bg.raised, radius.sm
 │   Root cause  [ Configuration                ⌄ ]   [ Move to Resolved  ↩ ]   │  inline field + primary
 │                                                    [ Cancel  esc ]           │
 └──────────────────────────────────────────────────────────────────────────────┘
```

- Shown under the element that tried to change (here the state chip row; in the rail, under the State row), on `bg.raised`, `radius.sm`, padding `space.12`, `circle-alert` 14 `status.warning.fg`, text `type.body` `text.primary`. Not red: nothing broke, a rule asked for more.
- Copy: "Azure DevOps needs {Field} to move to {State}." (state rules) or "Azure DevOps needs {Field} to save this change." (other rules), with ADO's field display name.
- **Known field** (value list, text, or person; spec FR-9): an inline control of the right kind (a menu of allowed values, a text field, or the people picker §4.5), focused on appear; primary "Move to {State}" / "Save" with `↩` (sends the original change and the field in one write); "Cancel" with `esc`.
- **Unknown field:** "Azure DevOps needs {Field} to move to {State}. adoken can't edit {Field} yet." + "Open in Azure DevOps  o" + "Cancel".
- Until resolved, the element shows the server value (the change was refused); the notice keeps my intent. `Esc` or Cancel closes the notice and drops the intent.

### 9.6 Offline

- Values render as usual; edit affordances stay visible (principles §3.5) but pickers, menus and Save/Comment are disabled: activating one shows the tooltip "Offline — reconnect to send" at the control (and speaks it). Title, description and comment editors still open; description and comment drafts save locally.
- Freshness shows the offline variant and Sync this item is disabled (§3.4). The statusbar "Offline" pill owns the global message; no pane banner.
- Nothing is sent offline and nothing queues across restarts (spec FR-10). A write in flight when the connection drops resolves as a failure ("Couldn't reach Azure DevOps. Nothing changed. Retry"), never as a fake done state.

### 9.7 Summary

| State | Glyph + token | Copy (example) | Where |
|---|---|---|---|
| Pending | `clock` `text.tertiary` | "Sending…" / "Removing…" / "Moving to Payments…" | Under the element |
| Waiting (throttled) | `clock` `text.tertiary` | "Waiting for Azure DevOps…" (after 3 s pending) | Under the element |
| Failed | `circle-x` `status.danger.fg` | "Azure DevOps didn't let you edit work items in this area. Retry · Dismiss" / "Couldn't reach Azure DevOps. Nothing changed. Retry" | Under the element |
| Failed (text) | `circle-x` `status.danger.fg` | "Couldn't reach Azure DevOps. Nothing changed. Your text is saved. Resume draft · Dismiss" | Under the section head |
| Conflict | `triangle-alert` `status.warning.fg` on `status.warning.wash` + 2 px left bar | "Changed in Azure DevOps by Dana Ruiz 1m ago. Your edit wasn't saved. Apply mine · Keep theirs" (description: "Compare · Overwrite with mine · Discard mine") | Under the element |
| Read-only token | `lock` `text.tertiary` | "Editing work items needs Work Items (Read & write)." (no button) | Under the chip row |
| Rule violation | `circle-alert` `status.warning.fg` on `bg.raised` | "Azure DevOps needs Root cause to move to Resolved." | Under the element |
| Blocked description | `info` `text.tertiary` on `bg.raised` | "This description uses formatting adoken can't edit yet: a table and 2 images. Edit it in Azure DevOps. Open" | Under the Description head |
| Offline | tooltip only | "Offline — reconnect to send" | At the control |
| Title empty | `circle-alert` `status.warning.fg` | "Title can't be empty" | Title hint line |

## 10. Keyboard and focus

### 10.1 Keys

Context: the Work items view, with **either** the list (the selected row's item) **or** the detail focused, and never while a text field, menu, picker or sheet has the keyboard (key context `(WorkItemList || WorkItemDetail) && !TextInput`, the same rule as the review's single letters, 0004 FR-14). From the list, an editing key opens the editor in the detail; when the editor closes, focus returns to the list. Every key is also a `⌘P` command (spec FR-35 titles, prefixed "Work item: ") and appears in the `?` cheat sheet, in tooltips and in the ⋯ menu.

| Key | Action | `⌘P` command (spec FR-35) | Opens |
|---|---|---|---|
| `t` | Edit title | Work item: Edit title | §3.2 |
| `s` (alias `⇧S`) | Change state | Work item: Change state… | §3.3 |
| `a` | Assign | Work item: Assign… | §4.5 |
| `i` | Assign to me | Work item: Assign to me | pending at once |
| — | Unassign | Work item: Unassign | pending at once |
| `⇧A` | Change area | Work item: Change area… | §4.5 |
| `⇧I` | Change iteration | Work item: Change iteration… | §4.5 |
| `⇧P` | Move to project (slice C, v0.2) | Work item: Move to project… | §4.6 |
| `d` | Edit description (or resume draft) | Work item: Edit description | §5.2 |
| `l` | Add link | Work item: Add link… | §6.2 |
| `⌫` (on a link row) | Remove link | Work item: Remove link… (opens a picker of the item's work item links, then the confirm) | §6.3 |
| `c` (alias `⇧C`) | Comment | Work item: Comment | §7.3 |
| `⌘⇧R` | Sync this work item | Work item: Sync this work item | §3.4 |
| `o` | Open in Azure DevOps | Work item: Open in Azure DevOps | browser |
| `y` | Copy link | Work item: Copy link | toast "Copied link" |
| `⇧Y` | Copy id | Work item: Copy id | toast "Copied #20101" |
| `⌘D` | Pin / unpin (existing, spec 0014) | — | — |
| `⌘K` | Action menu (existing) | — | ⋯ menu |

Inside editors and overlays:

| Where | Keys |
|---|---|
| Title field | `↩` save · `Esc` cancel (discard) |
| Description editor | `⌘↩` save · `Esc` close, keep draft · `⌘E` Write / Preview |
| Composer | `⌘↩` post · `Esc` blur, keep draft · `@` mention popover (`↑ ↓`, `↩`/`Tab` insert, `Esc` close popover) |
| State menu | type to filter · `↑ ↓` · `↩` · `Esc` (clears the filter, then closes) |
| Relation menu | `↑ ↓` · `↩` · `Esc` (no digit picks, spec 0025 FR-3) |
| Pickers | type to filter · `↑ ↓` · `↩` · `Esc` · area tree `→ ←` |
| Move sheet (slice C, v0.2) | `Tab`/`⇧Tab` · `↩` open row picker / move (on Move) · `⌘↩` move from anywhere · `Esc` |
| Confirm dialog | `↩` / `⌘↩` confirm · `Esc` cancel |
| Rule notice | `↩` apply (sends the change with the field) · `Esc` cancel |
| Conflict notice | `↩` focuses its first action · `←` `→` between actions |
| Detail cursor | `↑ ↓` / `j k` move · `↩` edit or open · `⌫` remove link (link rows) · `Esc` back to the list |

### 10.2 Conflict check

Checked 2026-10-01 against `docs/design/command-palette.md` §7 and every `KeyBinding::new` / `*_KEYS` in `crates/app/src/**` and `crates/ui/src/**`.

| Key | Other bindings found | Verdict |
|---|---|---|
| `t` `a` `i` `l` | none anywhere | free |
| `s` | Inbox `SNOOZE_KEYS` (Inbox list context), Today `SNOOZE_KEYS` (Today context) | free in Work items (different contexts) |
| `d` | Today `DONE_KEYS` (Today context); `⌘D` is pins (different chord) | free |
| `c` | Review `COMMENT_KEYS` (PrReview: comment on line), Today `COPY_NAMES_KEYS` | free; same intent as the review ("comment") |
| `o` `y` `⇧Y` | Review list + review `web::OPEN_KEYS`, `COPY_LINK_KEYS`, `COPY_ID_KEYS` | same keys, same meaning (0004 FR-14) |
| `⇧S` | Review `STATUS_KEYS` (status menu) | same intent, alias by design |
| `⇧C` | Review `PR_COMMENT_KEYS` (comment on the PR) | same intent, alias by design |
| `⇧A` | Review `ABANDON_KEYS` (always confirms first) | free in Work items; muscle-memory risk noted (§18) |
| `⇧I` | none | free |
| `⇧P` | Review `DRAFT_KEYS` (draft toggle) | free in Work items; Move always opens a sheet, so a slip never writes |
| `⌘⇧R` | none in app or palette §7 (UxActivity confirmed the Activity drawer uses `⌘⇧A`, not `⌘⇧R`) | free |
| `⌘E` (editor only) | none | free |
| `↑ ↓ j k ↩ Esc` in detail | existing `NextWorkItemLink` / `PreviousWorkItemLink` / `OpenWorkItemLink` / `FocusWorkItemList` | rebound: the walk now covers all stops (§10.3), not only links (spec §3.3) |
| `⌫` on a link row | text input `Backspace` (only inside text fields) | free outside text fields |
| `[` `]` `←` `→` | list path / tab keys (WorkItemList) | untouched |
| Global `?` `/` `⌘P` `⌘R` `⌘D` `⌘K` | — | untouched |

### 10.3 Focus order and the detail cursor

`Tab` reaches, in visual order: the list (one stop) → toolbar buttons (back when stacked, freshness/Sync, Pin, Open, ⋯) → the **detail content** (one stop) → the composer. Inside the detail content, `↑ ↓` / `j k` move one cursor through every actionable stop in visual order:

1. Not available banner (only while shown)
2. Title (then its conflict / failed notice when shown)
3. State chip (then its rule / conflict / failed notice when shown)
4. Assigned to → Project → Area → Iteration (grid: row by row, left to right; wide: rail top to bottom after the title)
5. Description (or its Show all / blocked notice)
6. Each link row (and Show more rows), then Add link (spec §3.3 order)
7. Show earlier comments row (when present), then each comment (`↩` does nothing; the stop lets eyes and screen readers follow the thread, spec §3.3)
8. Composer
9. History Show all row

The read-only token hint is not a stop (it has no button; spec 0001 FR-9).

`↩` on a stop activates it (edit, open menu, open link, expand). The cursor starts on the title when focus enters from the list with `↩`, and on the element just edited when an editor closes. The cursor scrolls its stop into view (instant). `Esc` unwinds one level: open popover/menu → editor → detail cursor → list (stacked: slides back to the list). Focus is never trapped; every overlay returns focus to the element that opened it.

## 11. Motion and sound

| Moment | Motion |
|---|---|
| Editor opens / closes (title, description, composer expand) | instant (`motion.instant`, text input) |
| State menu, relation menu, pickers, mention popover | `menu-in` (140, `ease.out`) / `menu-out` (90, `ease.in`); opened by keyboard: instant |
| Move sheet, confirm dialog | `dialog-in` / exit 130 `ease.in`, scrim with it |
| Value change after my action | instant (I caused it) |
| Pending line appears / clears | instant |
| Value changes arriving from sync while viewing | instant value swap; no flash (Q-4 considered and rejected: a highlight wash on every background update is noise) |
| New comment from someone else while visible | `row-insert` (200, `ease.out`); instant when the cursor is in the discussion |
| Link row removed | `row-remove` by pointer; instant by keyboard |
| Conflict notice | instant (it answers my write); its 2 px bar likewise |
| Inline error / rule notice | instant (they answer my action) |
| Syncing glyph | `glyph-spin` on the shared ~30 fps clock, only while visible |
| Sticky compact header | instant |
| Single ↔ two-column switch | instant, at the end of `pane-resize` |
| Show all / Show more expansions | instant |

**Reduce motion:** everything above snaps; the syncing glyph is a still `loader-circle` with "Syncing…"; nothing else changes.

**Sound** (sound.md; sounds off by default; mute and quiet hours apply):

| Event | Sound | Default within sounds | Notes |
|---|---|---|---|
| Azure DevOps confirmed a field change, link add/remove, comment, move | `ui.action_done` | on (existing event) | Once per confirmed write, not per field in a batch. Not for a state move that celebrates (spec 0022), which plays `ui.celebrate_step` / `ui.celebrate_milestone` instead (amended 2026-10-01, owner notes; accepted 2026-10-01) |
| A write I made failed (incl. rule refusal, conflict) | `ui.error` | on (existing event) | Once per failure |
| Sync this item finished (I pressed it) | `ui.sync_done` | off (existing event) | Amend sound.md trigger (§16) |
| Copy link / copy id | `ui.copy` | off (existing event) | |
| Menus, pickers, cursor moves | silent | | |

**Ken (arcade layer, opt-in):** no new moments in this doc. Work item edits aren't review hits (ken.md §3.1) and the detail stays a calm work surface; closing an item doesn't count either (Q-5, resolved: no for now).

## 12. States

| State | What shows |
|---|---|
| Nothing selected | Existing empty state ("Select a work item", `list-todo`), body updated: "↑ ↓ to move, ← → to switch tabs, ↩ to open, letters to edit (press ? for keys)." |
| Loading (local read) | Nothing (< 150 ms by principles §3.2); unchanged from today |
| Partly synced (title-index row only, spec FR-29/FR-31) | Header, title, state and the grid paint from the index row (fields it lacks show an 8 px `bg.raised` skeleton bar, 60 % of the value width). Description and Links show three skeleton lines each (`bg.raised`, 8 px, widths 100 / 92 / 60 %, `space.8` apart, `skeleton-shimmer`; flat under reduce motion). Freshness "Partly synced". After the selection rests 400 ms adoken fetches it once (freshness "Syncing…"); the skeletons are replaced in place (`motion.instant`). Offline or already tried in the last 10 min: the skeletons give way to "Not synced yet · Sync this item ⌘⇧R" (`type.body` `text.tertiary` + ghost button). |
| Not synced at all (deep link to an unknown id) | Existing empty state "#{id} isn't synced yet" with action [Sync this item ⌘⇧R]; while syncing the button shows its inline spinner. |
| Not available (404 on sync or write) | Pane banner under the header (principles §3.4 banner, `status.danger.wash`, `circle-alert` 14 `status.danger.fg`): "This work item was deleted or you lost access to it." [Open in Azure DevOps `o`]. The last data stays visible below it; every edit affordance is removed as in §9.4; the freshness keeps its last time. Pins of it follow pins.md §3.5. |
| Read failed (local store) | Existing empty state "Couldn't read this work item" + body, plus [Retry]. |
| Stale (≥ 15 min) | Freshness "Updated 18m ago" (`text.secondary`); content unchanged. |
| Syncing this item | Freshness "Syncing…" with spinner; content stays interactive; values update in place when it lands. |
| Sync failed | Freshness "Couldn't sync · Retry" (click retries; cause in the tooltip); the data shown is kept. |
| Offline | §9.6 |
| Throttled | Freshness "Waiting for Azure DevOps…"; writes "Waiting for Azure DevOps…" after 3 s pending (§9.1) |
| Read-only token / access level | §9.4 |
| Demo org | Edits apply to the demo store with the normal pending → done (spec §States, FR-37; fake delay ≤ 300 ms); freshness "Demo data", Sync disabled; Open disabled with the existing reason; Copy link copies the fake link (0004 FR-14). |
| Empty sections | "No description." · "No links." · "No comments yet." (each `type.body` `text.tertiary`, with their add buttons when writable) |
| Pending / failed / conflict / rule / blocked description | §9 |

## 13. Accessibility

### 13.1 Roles and labels

| Element | Role | Label (example) | State / actions |
|---|---|---|---|
| Detail pane | `Role::Region` | "Work item #20101, Timezone bug in daily settlement job" | |
| Back button (stacked) | `Role::Button` | "Back to work items, Escape" | |
| Type · id | `Role::Button` | "Bug 20101. Copy id, Shift Y" | |
| Freshness / Sync | `Role::Button` | "Sync work item 20101. Synced 2 minutes ago" · "…Syncing" · "…Couldn't sync: not found" · offline "…Offline, synced 2 hours ago" (disabled) | live region polite on change: "Work item 20101 synced" / "Couldn't sync work item 20101: not found" |
| Pin toggle | pins.md §11.1 | "Pin work item 20101" | pressed |
| Open | `Role::Button` | "Open work item 20101 in Azure DevOps, O" | disabled + reason in demo |
| More | `Role::Button` | "More actions for work item 20101, Command K" | `aria_expanded` |
| Title | `Role::Heading` (level 1) wrapped in `Role::Button` when writable | "Title: Timezone bug in daily settlement job. Edit, T" | editing: `Role::TextInput` "Title" |
| State chip | `Role::Button` | "State: Doing, in progress. Change state, S" | `aria_expanded`; read-only token: `Role::StaticText` "State: Doing" |
| State menu | `Role::Menu` "Change state" | items `Role::MenuItem` "Code review, in progress, 3"; current: "Doing, current" | |
| Priority, tags | `Role::StaticText` | "Priority 4" · "Tags: infra, settlement" | |
| Property grid | `Role::Group` | "Fields" | |
| Property cell | `Role::Button` | spec FR-36 form: "Assigned to, Ada Lindqvist. Change, A" · "Area, Infra, in Storefront. Change, Shift A" · "Iteration, Sprint 41, September 29 to October 10, current. Change, Shift I" | `aria_expanded` while its picker is open; with a status line its text is appended: "…Sending", "…Azure DevOps didn't let you edit work items in this area", "…Changed in Azure DevOps by Dana Ruiz, your edit wasn't saved" |
| People picker | `Role::Dialog` "Change assignee" | search `Role::SearchInput` "Search people"; list `Role::ListBox` (spec FR-36); rows `Role::ListBoxOption` "Dana Ruiz, dana.ruiz@contoso.com, current"; group labels exposed as option-group names ("From your directory") | `aria_selected` |
| Area / iteration picker | `Role::Dialog` "Change area" | search `Role::SearchInput`; `Role::Tree`, rows `Role::TreeItem` "Payments, in Storefront, level 2, current" | `aria_expanded` on parents, `aria_selected` |
| Move sheet | `Role::Dialog` "Move work item 20101 to another project" | rows as `Role::Button` "Project: Payments. Change" | |
| Meta line | `Role::StaticText` | "Created April 22 by Kwame Mensah. Changed 4 days ago by Mei Tanaka" | |
| Description | `Role::Article` "Description" | rich text exposes headings (`Role::Heading`), lists (`Role::List`), links (`Role::Link` "Repro guide, link"), code (`Role::Code`), images ("Image, opens in Azure DevOps") | |
| Description editor | `Role::TextInput` (multi-line) "Description, Markdown" / "Description, saved as HTML" | Write/Preview `Role::RadioGroup` | |
| Blocked notice | `Role::Note` | full copy | |
| Links section | `Role::List` "Links, 6" | groups `Role::Group` "Children, 2"; rows `Role::ListItem` "Child: work item 20118, Add tz to settlement job config, New, assigned to Ada Lindqvist" | `on_a11y_action`: open, remove |
| Remove link button | `Role::Button` | "Remove link to work item 20146" | |
| Discussion | `Role::List` "Discussion, 4 comments" | comments `Role::ListItem` "Kwame Mensah, April 22: Seen on 3 merchants this week. At Ada Lindqvist can you take it?"; pending "You, sending: …" | |
| Composer | `Role::TextInput` "Add a comment, Markdown, at to mention" | mention popover `Role::List` "People" | |
| History | `Role::List` "History" | rows "Mei Tanaka changed State from New to Doing, 4 days ago" | |
| Conflict notice | `Role::Group` labelled with its sentence | "Apply mine" / "Keep theirs" (description: "Compare", "Overwrite with mine", "Discard mine") buttons inside | Compare `aria_expanded` |
| Token hint | `Role::Note` | full copy, no button | |
| Rule notice | `Role::Group` labelled with its sentence | inline control and buttons inside | |

Announcements (spec FR-36; live region per principles §6): polite for "Sending", the accessible-only "Assigned to Dana Ruiz, saved" (routine writes have no toast, so screen-reader users get this line instead), failures, conflicts, rule notices and "Comment posted"; **assertive** only for the missing-scope error that switches the pane to read-only.

### 13.2 Other

| Concern | Behavior |
|---|---|
| Color independence | State chip: glyph + word; fields: labels; errors: glyph + sentence; conflict: sentence + 2 px left bar; mentions of me: wash shape + name; link states: glyph + word. |
| Reduce motion | §11 |
| Mute | §11; nothing detail-specific |
| Text scale | `font_scale` up to 1.3: the grid drops to one column below 240 px cells (§2.4); the toolbar shortens freshness then Open (§2.4); titles wrap. Primary actions (Save, Comment, Move) never truncate. |
| Targets | Chip-menu 24 high; property cells 52; link rows 36; icon buttons 24 × 24 (`size.hit_min`). |
| Custom backgrounds | Pane `bg.surface` at `surface_opacity` is covered by the legibility pass for every `text.*` token; overlays at `overlay_opacity` ≥ 0.9 with frost; code blocks `bg.code` opaque. |

### 13.3 Contrast (new pairings)

Measured (WCAG 2.x, alpha flattened over `bg.surface`), Dojo Night / Dojo Day / Arcade / Sakura:

| Pairing | Where | Ratios | Need |
|---|---|---|---|
| `text.primary` on `accent.wash` | Mention of me | 13.24 / 16.36 / 16.95 / 14.82 | 4.5 |
| `text.link` on `bg.hover` | Mention / link in a hovered row | 7.50 / 5.37 / 11.30 / 5.66 | 4.5 |
| `text.link` on `bg.selected_muted` | Link text in an open cell | 7.24 / 5.16 / 10.54 / 5.45 | 4.5 |
| `text.secondary` on `status.warning.wash` | Conflict review values | 6.11 / 6.80 / 10.04 / 7.30 | 4.5 |
| `text.primary` on `status.danger.wash` | (reserved) | 13.28 / 15.98 / 16.07 / 14.66 | 4.5 |
| `text.tertiary` on `bg.selected` | "Sending…" under a cursor row | 4.95 / 5.15 / 8.32 / 5.75 | 4.5 |
| `accent.fg` on `accent.wash` | In-progress state chip | 6.68 / 5.01 / 6.95 / 6.32 | 4.5 |
| `accent.fg` on `accent.wash` over `bg.hover` | Hovered in-progress chip | 5.68 / 4.64 / 5.75 / 5.89 | 4.5 |
| `status.warning.fg` on `status.warning.wash` | Conflict glyph | 7.77 / 4.80 / 9.89 / 5.31 | 3 (icon) |
| `border.control` on `bg.surface` | (inputs use `border.default` + focus ring; listed for the Move sheet field outline on hover) | 3.56 / 3.49 / 5.74 / 4.17 | 3 |

Every pairing passes in the four built-ins. User/community themes: the validator's text minimums (tokens §9.3) cover the `text.*` tokens on bases, not on washes [INFERENCE: the margins above make washes safe in practice; the per-theme `work-item-detail` snapshots are the check].

## 14. Gallery stories

In `crates/gallery`, story ids kebab-case; each renders one snapshot per built-in theme (`adoken-dark`, `adoken-light`, `arcade-dark`, `sakura-light`); `work-item-detail` additionally renders every bundled theme (as `pins-expanded` does). Fixtures use the demo seed at `DEMO_NOW`; spinners and shimmer at phase 0. Every story is a single frame: where a design shows two states (picker filtered, Write then Preview, menu then picker), they are separate stories or only the first state is shot. The freshness variants render together as one strip story.

| Story id | Shows | Fixture |
|---|---|---|
| `work-items` (existing, re-snapshot) | List + redesigned detail, default width | Demo seed, #20101 |
| `work-item-detail` | Default width: header, grid, rich description (heading, list, code block, link, image chip), 3 link groups, 2 comments with a mention of me, composer, history | Rich fixture item |
| `work-item-detail-wide` | Two-column with properties rail (pane 1208) | Same |
| `work-item-detail-stacked` | 1024 window, back button, grid | Same |
| `work-item-detail-narrow-scale` | 900 window, `font_scale` 1.3: one-column grid, shortened toolbar | Same |
| `work-item-detail-state-menu` | State menu open, cursor on "Code review" | Same |
| `work-item-detail-assign-picker` | Assignee picker with query "dan": Assign to me, Unassigned, Recent, People, From your directory results (spec FR-37) | People + directory fixture |
| `work-item-detail-area-picker` | Area tree expanded to the current path (single frame; the filtered state is covered by the picker's unit tests) | Area fixture |
| `work-item-detail-iteration-picker` | Current / Next / Recent shortcut groups above the tree | Iteration fixture with dates |
| `work-item-detail-move-sheet` (slice C, v0.2) | Move sheet with the consequences line; second frame "Project Payments has no Bug type" | Two-project fixture |
| `work-item-detail-editing-title` | Title field with 240/255 counter | Long title |
| `work-item-detail-description-editor` | Editor in Write (Markdown hint, "Draft saved") | Markdown field |
| `work-item-detail-description-preview` | Same editor in Preview | Markdown field |
| `work-item-detail-description-html` | Editor on an HTML field ("HTML (saved as HTML)" hint) | HTML field |
| `work-item-detail-description-blocked` | Blocked notice ("a table and 2 images") | HTML with table + images |
| `work-item-detail-draft` | Description "Draft" chip + composer with a draft | Local drafts |
| `work-item-detail-links` | All relation groups, unsynced row, Show 7 more, cursor on a row with remove button | Link fixture |
| `work-item-detail-link-menu` | Add link step 1: the relation menu | Same |
| `work-item-detail-link-picker` | Add link step 2: picker "Add a child to #20101" with disabled "Already a child" rows | Same |
| `work-item-detail-remove-link` | Remove link confirm dialog | Link fixture |
| `work-item-detail-composer-mention` | Composer focused, mention popover | People fixture |
| `work-item-detail-pending` | Assignee "Sending…", state "Sending…", pending comment, link "Removing…" | Injected pending writes |
| `work-item-detail-failed` | Assignee 403 error with Retry/Dismiss; failed comment with Retry/Edit | Mock 403 / 503 |
| `work-item-detail-conflict` | State conflict only: Closed by a teammate, notice with Apply mine / Keep theirs and the warning bar (the multi-field Review frame isn't shot) | Mock conflict |
| `work-item-detail-rule` | Rule notice with the Resolved Reason menu (single frame; the unknown-field variant is covered by unit tests) | Mock rule error |
| `work-item-detail-read-only` | Token hint, no chevrons/composer/Add link | Read-only scopes |
| `work-item-detail-freshness` | One strip story (420 × 380): toolbar freshness variants stacked: just now, 2m, stale 18m, partly synced, syncing, failed, offline, throttled, demo | Injected clock + states |
| `work-item-detail-partly-synced` | Index-only item with skeleton cells and "Not synced yet" sections | Index-only fixture |
| `work-item-detail-offline` | Offline freshness, disabled Comment with tooltip open | Offline flag |
| `work-item-detail-sticky` | Scrolled: sticky compact header | Long discussion |
| `work-item-detail-not-available` | 404 banner over the last data, edit affordances gone | Mock 404 |

## 15. New tokens and icons

Colors, radius, elevation, motion and z-layers: **none** new. New sizes join tokens §5.2.

| New | Kind | Value | Used by |
|---|---|---|---|
| `size.detail_wide_min` | size token | 1000 px: detail pane width at which the properties rail appears | §2 |
| `size.detail_properties` | size token | 280 px: properties rail width | §2.3 |
| `size.detail_prose_max` | size token | 720 px: max width of title, description and comments | §2.1 |
| `size.property_cell` | size token | 52 px: property grid cell height | §4.1 |
| `size.picker_popover` | size token | 320 px: minimum width of anchored pickers | §4.5 |
| `size.sheet_width` | size token (slice C, v0.2) | 480 px: form sheets (Move to project) | §4.6 |
| `pencil` | icon (Lucide, ISC) → `IconName::Pencil` | — | Title hover/cursor, Edit buttons |
| `ellipsis` | icon → `IconName::Ellipsis` | — | ⋯ More button |
| `folder-tree` | icon → `IconName::FolderTree` | — | Area cell, area picker root |
| `calendar-range` | icon → `IconName::CalendarRange` | — | Iteration cell |
| `user-x` | icon → `IconName::UserX` | — | "Unassigned" picker row |
| `triangle-alert` | icon → `IconName::TriangleAlert` | — | Conflict notice |
| `git-commit-horizontal` | icon → `IconName::GitCommitHorizontal` | — | Commit link rows |
| `arrow-up-from-line` / `arrow-down-to-line` | icons → `IconName::ArrowUpFromLine` / `ArrowDownToLine` | — | Relation menu: parent / child |
| `arrow-left` | icon → `IconName::ArrowLeft` | — | Relation menu: predecessor |
| `folder-input` | icon → `IconName::FolderInput` (slice C, v0.2) | — | ⋯ menu: Move to project |

All new files verified present under `assets/icons/lucide/` on 2026-10-01. Existing icons reused, all already in `IconName`: `bug`, `square-check`, `book-open`, `trophy`, `crown`, `circle-alert`, `square`, `circle-dashed`, `circle-dot`, `circle-check`, `circle-check-big`, `circle-slash`, `circle-x`, `chevron-down`, `chevron-up`, `chevron-right`, `chevron-left`, `refresh-cw`, `loader-circle`, `clock`, `wifi-off`, `lock`, `info`, `external-link`, `pin`, `pin-off`, `link`, `unlink`, `copy`, `user`, `users`, `folder-kanban`, `search`, `image`, `at-sign`, `git-pull-request` (+ draft / merge / closed), `chevrons-up`, `minus`, `list-todo`, `arrow-right`, `check`.

New component: `RichText` in `crates/ui` (§5.1), rendering a sanitized block tree from HTML or Markdown with tokens only.

## 16. Amendments to other docs (applied 2026-10-01)

All rows below were applied on acceptance. The slice C parts (`size.sheet_width`, `⇧P`, "work item moved") are recorded in the target docs as v0.2 (slice C).

| Doc | Section | Amendment |
|---|---|---|
| [principles.md](principles.md) | §2 Detail pane | Add: "The work item detail shows a properties rail (280) when the pane is ≥ 1000 px wide (work-item-detail.md §2)." |
| [principles.md](principles.md) | §3.3 Pending writes | Extend the example list "(vote, comment)" to "(vote, comment, work item field, link)". |
| [tokens.md](tokens.md) | §5.2 Layout sizes | Add the six size tokens of §15 (`size.sheet_width` marked v0.2, slice C). |
| [command-palette.md](command-palette.md) | §7.2 Global | Add rows: "`⌘⇧R` · Work items: sync this work item"; and under view-specific keys note "Work items: t s a i ⇧A ⇧I d l c o y ⇧Y (work-item-detail.md §10); ⇧P with slice C in v0.2". |
| [sound.md](sound.md) | §2.1 | `ui.action_done` trigger: add "work item field changed, link added or removed" ("work item moved" with slice C, v0.2). `ui.sync_done` trigger: "A sync you started (`⌘R`, or Sync this item `⌘⇧R`) finished". |
| [pins.md](pins.md) | §6.2 | No change needed (pin stays immediately left of Open); note the work item header's Open is now the compact "Open `o`" button. |
| [activity.md](activity.md) | Sync tab | "You asked" row for per-item sync with the shared copy "Syncing…" / "Synced just now" / "Couldn't sync: {cause}" (agreed with UxActivity; already in activity.md §4.3). |

## 17. Open questions — resolved 2026-10-01

All recommendations accepted by the owner.

- **Q-1 Properties rail on wide panes.** **Resolved:** yes. At ≥ 1000 px detail width (`size.detail_wide_min`) the four fields + state move into a 280 px right-hand rail; below that the grid stays (§2, §2.3; principles §2).
- **Q-2 Chevrons on property cells at rest.** **Resolved:** only on hover and under the keyboard cursor (§4.1).
- **Q-3 Discussion order.** **Resolved:** oldest → newest with the composer last, last 5 shown with "Show {n} earlier comments" (§7.1; spec FR-4).
- **Q-4 Highlight background changes.** **Resolved:** no flash; values swap instantly (§11).
- **Q-5 Ken.** **Resolved:** no for now; work item state changes are not Ken hits. Revisit if dogfooding asks (§11).
- **Q-6 Remove link: confirm or Undo.** **Resolved:** confirm dialog (`⌫` `↩`), matching the review's Unlink (§6.3; spec FR-22, Q-11).
- **Q-7 Description editor font.** **Resolved:** code font, with Preview one key away (`⌘E`) (§5.2).
- **Q-8 Open button.** **Resolved:** the compact "Open `o`"; the full wording stays in the tooltip and the ⋯ menu (§3.1).

## 18. Risks

- **⇧A means Abandon in the PR review and Area here.** Abandon always confirms, so a slip costs one `Esc`, but it's a habit trap; `⌘P` and the cheat sheet show both.
- **Rich text needs the raw source.** Sync currently flattens descriptions and comments to plain text (`crates/sync/src/wire.rs` `html_to_text`); spec FR-34 adds source columns and a one-time refetch. Until that migration lands for an item, §5.1 and §7.1 render the plain projection as paragraphs.
- **HTML round-trip.** The editor on HTML fields depends on the spec's convertibility check; a false "convertible" would lose formatting on save. The blocked state (§5.3) is the safety valve.
- **Picker people source.** Assign quality depends on the people already in the store; the directory search (spec FR-13) covers gaps but only after 3 characters and a 300 ms pause, so a first-time assignee takes a beat.
- **Frame budget.** `RichText` layout of a long description must stay off the hot path (parse off the UI thread, cache per revision); clipping at 480 px (§5.1) bounds the first layout.
