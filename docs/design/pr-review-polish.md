# PR review polish: visuals, copy, tokens

Status: v0.1, **Accepted** (2026-10-01, owner: all recommendations; spec 0019 Accepted the same day). Owner: ux-designer. Behavior, acceptance and budgets: `docs/specs/0019-pr-review-polish.md`. Tokens: [tokens.md](tokens.md). States and recurring components: [principles.md](principles.md) §3, §4. Keys checked against [command-palette.md](command-palette.md) §7 and every `KeyBinding::new` in `crates/app/src/review/**`.

Groups follow the spec: **A** file filter and "only files with threads", **B** Threads view, **C** group reviewers, **D** status actions, **E** copy title and branches.

No new tokens. No animation: every change in this document is `motion.instant`, so Reduce Motion changes nothing. No sound except the existing optional `ui.copy` on a copy (0004 FR-14).

## 1. Review body layout

```
┌ header (0004) ─────────────────────────────────────────────────────────────────────┐
│ ‹ Pull requests  ⊙ !4821  Retry idempotency keys on payment capture   Copy link Y … │
│ [Active ▾ ⇧S]  Dana Ruiz  feature/retry → main  Updated 2h  Iteration 3              │
│ ✓ Dana Ruiz Approved   ◌ Tom Ito No vote   [Waiting on 1 required group]              │
│ [Approve] [Vote… ⇧V] [Comment ⇧C] [Complete… ⇧M]                                      │
├ tree (300 px) ─────────────┬ diff pane ──────────────────────────────────────────────┤
│ Files 7 of 300   2/7 viewed │ [Files | Threads 8]  t   src/pay/retry.rs   ☐ Viewed v │
│ [⌕ auth -test        ⌘⌫ ×] │                                                         │
│ [◫ Only with threads ⇧T]   │  …diff rows…                                            │
│ ▸ src/                      │                                                         │
│   ▸ auth/                   │                                                         │
│     token.rs        ●2  +40 │                                                         │
└─────────────────────────────┴─────────────────────────────────────────────────────────┘
```

### 1.1 Tree header (A)

Three rows, top to bottom; the tree list keeps the remaining height.

| Row | Height | Content |
| --- | --- | --- |
| Counts | `size.control_lg` (existing) | Left "Files 300", or "Files 7 of 300" while filtered. Right "2/7 viewed" (follows the filter); tooltip "41 of 300 viewed in this pull request" while filtered. `type.label`, `text.secondary`. |
| Filter field (`review.tree-filter`) | `size.control_md` | `bg.input`, `radius.sm`, 1 px `border.default` (focus: `border.focus` ring per principles §4). Leading `search` icon 14 `text.tertiary`. Placeholder "Filter files" `text.tertiary`; trailing keycap `/` while empty and unfocused. With text: trailing clear button (`x`, 14, tooltip "Clear filter ⌘⌫"). Inset `space.8` from the pane edges. |
| Threads toggle (`review.threads-only`) | `size.control_sm` | Ghost toggle button: `message-square` icon + "Only files with threads", trailing keycap `⇧T`. Pressed: `bg.selected`, `accent.fg` icon, `aria-pressed`. In Threads mode it reads pressed, is disabled, tooltip "Always on in Threads". |

Matched characters in file names and directory rows render in `text.match` at weight 560, as in the palette (command-palette.md §3.5). Ancestor directories of matching files stay visible at `text.tertiary` when only their descendants match. A file whose own name doesn't match but whose directory path does shows no highlight in its name.

Filtered to nothing (principles §3.1): in the list area, `text.secondary` "No files match "auth -test"", below it a Ghost button "Clear filter ⌘⌫". With the threads toggle on and no text: "No files with active threads" and "Show all files ⇧T".

### 1.2 Diff pane bar: Files | Threads switch (`review.mode`)

A two-segment control at the start of the existing file bar (0004), height `size.control_sm`: "Files" and "Threads 8" (count = threads in the current status scope, tabular numbers). Selected segment `bg.selected` + `text.primary`; the other `text.secondary`, hover `bg.hover`. Keycap `t` after the control. In Files mode the rest of the bar is unchanged (path, Viewed, layout toggle). In Threads mode the rest of the bar is the status scope (§2.1).

## 2. Threads view (B)

### 2.1 Bar

`[Files | Threads 8]  t   Active 8 · Resolved 12 · All 20`

The scope is a three-segment control, same styling as §1.2, each label with its count. Default **Active**. The Active count includes pending threads.

### 2.2 List

One virtualized vertical list on `bg.code`, full diff-pane width, content max width 880 px, left-aligned with `space.16` padding.

```
 Pull request · 2 comments                                    (Q-6 group, no excerpt)
 ┌───────────────────────────────────────────────────────────────┐
 │ [Active]  General comment                            3h        │
 │ ◍ Dana Ruiz 3h  Can we split the migration?                   │
 │ Reply r · Resolve ⇧R                                           │
 └───────────────────────────────────────────────────────────────┘
 src/pay/retry.rs · 3 threads                       Open in diff ↩
 ┌───────────────────────────────────────────────────────────────┐
 │ [Active]  Lines 40–42                                2h        │
 │ ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄ │
 │  37   let backoff = base * 2;                                  │  excerpt
 │  38 − let id = Uuid::new_v4();                                 │
 │  39 + let id = key.reuse();                                   ▌│  anchored rows
 │  40 + retry(id, backoff)?;                                    ▌│
 │ ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄ │
 │ ◍ Tom Ito 2h   Should the key survive a restart?              │
 │ ◍ Dana Ruiz 1h Yes, it's persisted with the order.            │
 │ Reply r · Resolve ⇧R · Open in diff ↩                         │
 └───────────────────────────────────────────────────────────────┘
```

- **Group header** per file: path in `type.code_inline` `text.primary` (directory part `text.secondary`), " · 3 threads" `text.tertiary`; trailing Ghost button "Open in diff" (opens the group's first card). Height `size.control_md`, `space.16` above. Not sticky.
- **Card:** the inline thread card of 0004 (same component: border `border.default`, focused `border.focus`; `bg.raised` flattened over `bg.code`; `radius.md`; status chip, location, age, comments, footer). The excerpt sits between the card's top line and its comments.
- **Excerpt:** unified rows (always unified, whatever the diff layout), code font, `diff.*` washes and gutters, syntax highlighting as in the diff (plain first). The anchored rows carry a 2 px `diff.comment` bar at the right edge of the code area. Separated from the card's top line and its comments by 1 px `border.subtle`. Row height = the diff row height (code font size). Horizontal overflow clips, like the diff.
- **Excerpt placeholder** while the file's diff is loading: the same number of rows as `Skeleton` bars of varying width (flat `bg.raised` blocks under Reduce Motion), so nothing moves when code arrives.
- **No excerpt** notes, `type.small` `text.tertiary`, one line in place of the excerpt:
  - older code: "Code from iteration 2 isn't shown here."
  - collapsed file: "Generated file: code not loaded." / "Binary file." / "Large file: code not loaded."
  - unavailable: "Couldn't load this file's code." with Retry `⇧L`.
  - path not in the pull request: "This file is no longer changed in this pull request." The card has no "Open in diff".
- **Last groups:** "Comments on older code" cards stay inside their file's group, after the file's other cards, with location "Lines 12–14 · iteration 2". Threads on paths no longer in the PR form a final group "Files no longer in this pull request".
- **Resolved within Active scope** (spec FR-9): the card stays, its chip changes to its resolved status; no motion.

### 2.3 Empty states

| Case | Title (`type.body_strong`) | Body / action |
| --- | --- | --- |
| No human threads at all | "No comments on this pull request yet" | — |
| Active scope empty, resolved exist | "No active threads" | Ghost button "Show all 12" |
| Filter or scope hides all | "No threads match "auth"" | "Clear filter ⌘⌫" |

## 3. Group reviewers (C)

- **Hidden** (default): no group avatar, chip or row.
- **Pending required groups indicator**, only while a hidden group is required and has no vote (spec FR-16, Q-4):
  - **List row:** after the reviewer groups, a `users` icon 14 + count, `text.tertiary`, at `opacity.dimmed` like the existing no-vote group. Tooltip: "Waiting on 2 required groups: React PR reviewers, Service reviewers". Names drop the `[Project]\` prefix.
  - **Review header:** at the end of the reviewers row, a Neutral `Chip` with `users` icon: "Waiting on 1 required group" / "Waiting on 2 required groups"; same tooltip.
- **Shown** (setting on): groups render as today (initials avatar, `Avatar::person` with `.group(true)`, 0012 FR-3), no indicator.
- **Settings › General › Pull requests:** switch "Show group reviewers", sub-label "Teams and groups added by branch policies, such as "[Project]\React PR reviewers". Hidden groups still count toward approvals." (0009 owns the row's styling.)

## 4. Status actions (D)

- **Header status control** (`review.status`): the existing chip plus `chevron-down` becomes a bordered control: 1 px `border.default`, `radius.sm`, `space.4` inner padding, chip + chevron + keycap `⇧S` always visible (not only in the tooltip). Hover `bg.hover`; open `bg.selected`. Without valid actions it is the plain chip (unchanged).
- **Publish button:** on a draft with write access, the actions row starts with "Publish" (`git-pull-request` icon, keycap `⇧P`), Primary variant when I'm the author, default variant otherwise. Approve stays where it is, after it.
- **Row action menu** (⌘K / right-click), the "Pull request" menu of `crates/app/src/review/mod.rs` `open_row_menu`, in this order: Open `↩` · Open in browser `O` · Copy link `Y` · Copy PR id `⇧Y` · Copy title · Copy source branch · Pin/Unpin `⌘D` · separator · the valid status items from: Publish `⇧P` / Mark as draft `⇧P`, Abandon… `⇧A`, Reactivate. Items with an invalid status are absent, not disabled (command-palette.md §4.4).
- **Abandon confirmation over the list:** the same confirmation dialog as the review's (status_links `Confirming::Abandon`), centered over the content column, scrim `bg.scrim`.
- **List row while sending:** the status chip shows the new status with a 12 px `clock` glyph and `text.tertiary` "Sending…" (principles §3.3), gone on confirmation.
- **Refusal in the list:** toast "Status of !4821 not changed. <reason>" with a `circle-x` icon in `status.danger.fg`, 5 s, pausing on hover (principles §4). The review keeps its header notice line (0004).

## 5. Copy title and branches (E)

- **Title:** the header title keeps its ellipsis and width. Hover `bg.hover` wash over the text box, `radius.sm`, pointer cursor, tooltip "Copy title". `Role::Button`, label "Copy title: <title>".
- **Branches:** source and target names are separate click targets with the same hover wash; the arrow between them isn't. Tooltip "Copy feature/retry". Label "Copy source branch feature/retry" / "Copy target branch main".
- **Toasts** (0004 FR-14 toast, 5 s, no motion): "Copied title", "Copied feature/retry" (a branch name longer than the toast truncates in the middle: "Copied feature/very…/name").

## 6. Copy table

| Where | Text |
| --- | --- |
| Filter placeholder | Filter files |
| Threads toggle | Only files with threads |
| Threads toggle tooltip (Threads mode) | Always on in Threads |
| Mode switch | Files · Threads {n} |
| Scope | Active {n} · Resolved {n} · All {n} |
| Group header | {path} · {n} thread / threads |
| Open in diff | Open in diff |
| Group indicator | Waiting on {n} required group / groups |
| Command: palette disabled reason | Select a pull request first |
| Toasts | Copied title · Copied {branch} · Status of !{id} not changed. {reason} |

## 7. Accessibility

- Filter field: `Role::SearchInput` (as the Activity drawer's and the pickers' search fields), label "Filter files". Result count announced politely after typing settles (300 ms): "7 of 300 files".
- Threads toggle: `Role::Button` with `aria_pressed`. Mode and scope controls: `Role::RadioGroup` with one `Role::RadioButton` per segment, `aria_selected` on the chosen one.
- Threads list: `Role::List`; each card `Role::ListItem`, label "Thread on src/pay/retry.rs lines 40 to 42, active, 2 comments, last by Dana Ruiz 1 hour ago". Group headers are `Role::Heading`.
- Group indicator: label equals its tooltip.

## 8. Gallery

One snapshot per built-in theme for each: tree filtered with highlights; tree filtered to nothing; threads-only on; Threads view (Active, All, empty Active with resolved, excerpt loading, older code, collapsed file, read-only); header with the status control and with Publish on a draft; header and list row with hidden groups and the pending indicator; with groups shown; list row menu with status items (active, draft, abandoned); abandon confirmation over the list; copy toasts.
