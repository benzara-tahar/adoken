# Guided tours

Status: v0.1, **Accepted** (2026-10-01, owner: all recommendations; spec 0020 Accepted the same day). Owner: ux-designer. Behavior, triggers, acceptance and budgets are in [`docs/specs/0020-guided-tours.md`](../specs/0020-guided-tours.md) ("the spec" below). This doc owns the coach mark, the spotlight, placement, motion and **every step's copy** (single source; the code's tour definitions mirror §5). Tokens: [tokens.md](tokens.md). Principles: [principles.md](principles.md) §1, §5, §6.

## 1. Concept

A tour is 3–5 **coach marks**, each pointing at one region of the view with one or two sentences and the keys that act there. It is a guide beside the work, not a gate in front of it:

- **No scrim.** The window isn't dimmed; the region is outlined (the **spotlight**) and everything stays clickable (spec Q-4).
- **Any other input ends it, and still happens.** Pressing `j` on step 2 ends the tour and moves to the next file (spec FR-6).
- **Keys are live.** Keycaps in the copy come from the live keymap through the cheat sheet's formatter (spec FR-12); the strings below show the current bindings for reading convenience.

```
┌ review ───────────────────────────────────────────────────────────────────────────┐
│ ‹ Pull requests  !4821 Retry idempotency keys …            Vote… ⇧V  Complete… ⇧M │
├──────────────────────────────┬────────────────────────────────────────────────────┤
│╭────────────────────────────╮│                                                    │
││ Files 14          3/14 viewed│   ◀ ┌───────────────────────────────────────┐      │
││ ▾ src                        │     │ Files                           2 of 5│      │
││   ✓ capture.rs        +12 −3 │     │ [j] and [k] move between files. [v]   │      │
││     retry.rs          +40 −8 │     │ marks one viewed and moves on.        │      │
││   …                          │     │                                       │      │
│╰────────────────────────────╯│     │ All shortcuts [?]  Skip tour Back [Next]│    │
│  spotlight: 2 px border.focus │     └───────────────────────────────────────┘      │
└──────────────────────────────┴────────────────────────────────────────────────────┘
```

## 2. Coach mark

### 2.1 Anatomy

```
 ← size.coach_mark 320 (× font_scale) ─────────────────────────────→
 ┌─────────────────────────────────────────────────────────────────┐  ↑ space.16
 │ {Title}                                               {n} of {N}│  title row
 │ {Body with [keycaps] inline}                                    │  space.6 above
 │                                                                 │  space.12
 │ All shortcuts [?]                     Skip tour   Back   [Next] │  footer, control.sm 24
 └─────────────────────────────────────────────────────────────────┘  ↓ space.16
```

| Part | Spec |
|---|---|
| Surface | `bg.overlay` (× `overlay_opacity` and frosted with custom backgrounds, tokens §10.2), `radius.md`, `elevation.2`, padding `space.16`, z-layer `z.dialog`. |
| Arrow | 12 × 6 triangle in `bg.overlay` with a 1 px `border.default` edge, on the side facing the anchor, centered on the anchor's center projected onto that side, clamped ≥ 16 px from the corners. Omitted when the mark sits inside its anchor (§3, rule 4). |
| Title | `type.body_strong`, `text.primary`, one line (≤ 32 characters, spec AC-7). |
| Counter | "2 of 5", `type.small` + `type.numeric`, `text.tertiary`, right-aligned on the title row. |
| Body | `type.body`, `text.secondary`, wraps, max 3 lines at `font_scale` 1.0. Keycaps are the standard keycap component (principles §4), baseline-aligned inline. |
| Footer left | "All shortcuts" `type.small` `text.tertiary` + keycap `?`. Not a button: pressing `?` ends the tour and opens the cheat sheet (spec FR-6). |
| Skip tour | Ghost button, `size.control.sm`, label `text.tertiary` (hover `text.primary` on `bg.hover`). |
| Back | Ghost button, `size.control.sm`. Disabled on step 1 (`text.disabled`, not a Tab stop). |
| Next / Done | Primary button, `size.control.sm`, `accent.solid` / `accent.on`. Label "Next", "Done" on the last step. Focused when the step shows (focus ring per principles §4). |

### 2.2 Spotlight

- A 2 px `border.focus` outline drawn `space.4` outside the anchor's bounds, radius = the anchor's own radius + 4 (`radius.md` for panes with square corners). Nothing is filled, nothing is dimmed.
- When the inflated outline would leave the window content area, it is clamped `space.4` inside it (pane-sized anchors like the diff).
- Paints on `z.dialog`, never intercepts the pointer.

## 3. Placement

Each step names a preferred side (§5). The coach mark is placed by these rules, in order:

1. Try the preferred side, then the opposite side, then the two perpendicular sides. A side fits when the mark plus `space.8` gap stays inside the window content area (titlebar and statusbar excluded) with `space.8` margin.
2. Along the chosen side, center on the anchor, then slide to fit.
3. Recompute every frame the anchor's bounds change (spec FR-9); the mark jumps to its new place (`motion.instant`): it tracks the layout, it doesn't travel.
4. If no side fits (pane-sized anchors: the diff, the PR list column at 900 px), the mark sits **inside** the anchor, at its top-right corner inset `space.16`, without the arrow. This is the only case where the mark covers part of its anchor (spec FR-16).
5. At the 900 × 600 minimum window and at `font_scale` 1.3, every step of every tour must place by rules 1–4 (stories `tour-narrow`, `tour-font-scale`).

## 4. Motion

| Moment | Motion |
|---|---|
| Tour starts | Coach mark `menu-in` (quick 140, opacity 0→1, translate 4 → 0 toward the anchor); spotlight appears with it (opacity, same timing). |
| Step change | Spotlight moves instantly; the old mark disappears instantly and the new one runs `menu-in`. Never a sliding mark across the window. |
| Re-place after resize / layout | Instant. |
| Tour ends | `menu-out` (90, opacity) for mark and spotlight together. Focus returns at once, not after the fade. |
| Reduce motion | Everything snaps (tokens §7.4). |
| Loops, pulses, glows | None, in any mode. |
| Sound | None. |

Input during an entrance jumps it to its end state (tokens §7.3).

## 5. Tours and copy

Key references are written `[key]` here and are action references in the code (spec FR-12). At most 5 per step; a range `[1]–[5]` counts as one (spec AC-7). Rows marked **with 0019** / **with 0018** join the tour when that spec ships; until then the sentence is absent (the definition is updated in the change that ships the spec, spec FR-10).

### 5.1 Pull request review (`review`, story `review-open`)

| # | Anchor | Side | Title | Body |
|---|---|---|---|---|
| 1 | `review.header` | below | This pull request | Status, reviewers and branches at a glance. [⇧Y] copies the id, [Y] the link. **with 0019:** Click the title or a branch to copy it. |
| 2 | `review.file-tree` | right | Files | [j] and [k] move between files. [v] marks one viewed and moves on. **with 0019:** [/] filters by path, [⇧T] keeps files with threads. |
| 3 | `review.diff` | inside | The diff | [c] comments on the cursor line. [⌘⇧D] switches between unified and side by side. **with 0019:** [t] shows only the threads. |
| 4 | `review.thread` | left | Threads | [n] and [p] jump between threads. [r] replies, [⇧R] resolves. |
| 5 | `review.actions` | below | Vote and finish | [⇧V] votes, [⇧C] comments on the whole pull request, [⇧M] completes. [⇧S] publishes, marks as draft or abandons. |

Actions: `review::NextFile`, `PreviousFile`, `MarkViewed`, `CommentOnLine`, `ToggleDiffLayout`, `NextThread`, `PreviousThread`, `ReplyToThread`, `ToggleResolved`, `OpenVoteMenu`, `CommentOnPullRequest`, `OpenCompleteSheet`, `OpenStatusMenu`, `CopyPrId`, `CopyPrLink` (`crates/app/src/review/pr.rs`, `web.rs`). Step 4 is skipped on a PR without inline threads (spec AC-5). Read-only token: step 5 body becomes "Voting, commenting and completing need Code (Read & write)." (0004 §3 item 8).

### 5.2 Command palette (`palette`, story `palette-my-work`)

| # | Anchor | Side | Title | Body |
|---|---|---|---|---|
| 1 | `palette.input` | below | Find anything | Type part of a title, an id like !4821 or #1234, or paste an Azure DevOps link. |
| 2 | `palette.input` | below | Narrow it down | Start with ! for pull requests, # work items, @ people, > commands. Add filters like author:@me or is:draft. |
| 3 | `palette.results` | right | Act on a result | [↩] opens it, [⌘↩] opens it in Azure DevOps, [⌘K] shows its actions: vote, copy, assign. |
| 4 | `palette.footer` | below | Every action is here | Type > to run anything by name, these tours included. Start with ? to see every scope and filter. |

`!`, `#`, `@`, `>`, `?` and the filters are literal query text, rendered in `type.code_inline`, not keycaps. Actions: `palette::Confirm`, `palette::ConfirmBrowser`, `pin_actions::OpenRowMenu`.

### 5.3 Inbox (`inbox`, story `inbox`)

| # | Anchor | Side | Title | Body |
|---|---|---|---|---|
| 1 | `inbox.list` | inside | Only what's addressed to you | Your pull requests, work items, pipelines and mentions, one row per item. [↩] opens it. **with 0018:** [⌘[] brings you back to this row. |
| 2 | `inbox.filters` | below | Filter with one key | [1]–[5] show one source, [⇧1]–[⇧4] one priority. [⌘⌫] shows everything again. |
| 3 | `inbox.footer` | above | Triage without opening | [e] marks done, [s] snoozes, [m] mutes this item. [z] undoes. |
| 4 | `inbox.settings` | below | Make it quieter | Priority rules, quiet hours and mutes live here. [⌘⇧,] |

Actions: `inbox::Open`, `ToggleSource*`, `TogglePriority*`, `ClearFilters`, `MarkDone`, `Snooze`, `MuteSubject`, `Undo`, `ToggleRules` (`crates/app/src/inbox/mod.rs`); 0018's back action.

### 5.4 Work item (`work-item`, story `work-item-detail`)

| # | Anchor | Side | Title | Body |
|---|---|---|---|---|
| 1 | `workitem.state` | below | State in one key | [s] changes the state, [t] edits the title. Changes go to Azure DevOps as you make them. |
| 2 | `workitem.fields` | below | Fields | [a] assigns, [i] assigns it to you, [⇧A] sets the area, [⇧I] the iteration. |
| 3 | `workitem.description` | left | Description and links | [d] edits the description. [l] links another work item. |
| 4 | `workitem.discussion` | above | Discussion | [c] writes a comment. Type @ to mention someone. |
| 5 | `workitem.freshness` | below | Is this current? | When adoken last confirmed this item. [⌘⇧R] syncs just this one. **with 0018:** [⌘[] goes back to where you came from. |

Actions: `ChangeWorkItemState`, `EditWorkItemTitle`, `AssignWorkItem`, `AssignWorkItemToMe`, `ChangeWorkItemArea`, `ChangeWorkItemIteration`, `EditWorkItemDescription`, `AddWorkItemLink`, `CommentOnWorkItem`, `SyncWorkItem` (`crates/app/src/workitems/detail/mod.rs`). Read-only token: steps 1–4 keep their titles; bodies become "Editing needs Work items (Read & write). [o] opens it in Azure DevOps." (0015 read-only state, [work-item-detail.md](work-item-detail.md) §9.4).

### 5.5 Today (`today`, ships with 0017; story `today-busy-day`)

| # | Anchor | Side | Title | Body |
|---|---|---|---|---|
| 1 | `today.summary` | below | Your day at a glance | What moved today and what needs you. The picture follows the day: shipped, waiting, blocked or clear. |
| 2 | `today.prs` | right | Your pull requests | Most recently updated first; the bar shows how close each is to merged. [j] and [k] move, [↩] opens. |
| 3 | `today.up-next` | left | Up next | The one thing worth doing now. [e] hides it until tomorrow. [h] and [l] switch columns. |
| 4 | `today.waiting` | left | Waiting on you | Reviews and replies others need from you. [s] snoozes, [d] marks done, [x] stops it for this item. |
| 5 | `today.loose-ends` | left | Loose ends | Things you started that went quiet, with how long they've been open. |

Actions: `today::SelectDown`, `SelectUp`, `Activate`, `DismissFocus`, `SelectPrev`, `SelectNext`, `SnoozeNudge`, `MarkNudgeDone`, `NotForThisItem` (`crates/app/src/today/mod.rs`; column semantics per 0017). In single-column (narrow) Today, step 3 drops the column sentence. Anchors agreed with 0017's author on 2026-10-01; `today.pr-stage` (the first row's stage bar) is reserved for a later step and unused in v1.

## 6. Other copy

| Where | Copy |
|---|---|
| Buttons | "Back" · "Next" · "Done" · "Skip tour" |
| Counter | "{n} of {N}" |
| Footer hint | "All shortcuts" + `?` keycap |
| Palette commands (category Help) | "Tour: Today" · "Tour: Inbox" · "Tour: Pull request review" · "Tour: Work item" · "Tour: Command palette" · "Tour: This view" · "Tours: Show again on first use" · "Tours: Turn off first-use tours" / "Tours: Turn on first-use tours" |
| Disabled reasons (palette subtitle) | "Open a pull request first" · "Select a work item first" · "No tour for this view" · "Nothing to show yet" |
| Settings › General | Switch "Show tours the first time you open a view" · Button "Show tours again" · its toast "Tours will show again the first time you open each view" |

## 7. Accessibility

| Element | Role | Label (example) | Notes |
|---|---|---|---|
| Coach mark | `Role::Dialog` | "Pull request review tour, step 2 of 5: Files" | Body is the description; focus moves to Next on every step, which announces it. |
| Keycaps in body | text | spoken form: "j and k move between files. v marks one viewed…"; modifiers spelled ("Shift V", "Command Shift D") | |
| Spotlight | none (decorative) | — | Not in the a11y tree. |
| Buttons | `Role::Button` | "Back", "Next" / "Done", "Skip tour" | Back is disabled (not hidden) on step 1. |

- Contrast: only `text.primary` / `text.secondary` / `text.tertiary` on `bg.overlay` and `accent.on` on `accent.solid`, pairs the theme validator already enforces; the spotlight is `border.focus` (≥ 3:1 vs every background, tokens §2.2).
- No traps: `Esc` always ends; `Tab` cycles only the three buttons while the tour shows (it is short-lived and any other key leaves it).
- Color independence: the spotlight is a shape (outline), not a tint.

## 8. Gallery stories

In `crates/gallery`, one snapshot per built-in theme, reduce motion on, clocks injected, demo seed at `DEMO_NOW`. Story names in words only (gallery naming test).

| Story | Content |
|---|---|
| `tour-review-files` | Step 2 of `review` over `review-open` at 1440 × 860 |
| `tour-review-done` | Step 5 (Done) |
| `tour-palette` | Step 2 of `palette` inside `palette-my-work` |
| `tour-inbox-filters` | Step 2 of `inbox` |
| `tour-work-item-state` | Step 1 of `work-item` |
| `tour-today-summary` | Step 1 of `today` (with 0017) |
| `tour-narrow` | Step 2 of `review` at 900 × 600 (flipped placement) |
| `tour-font-scale` | Step 3 of `review` at `font_scale` 1.3 (inside-anchor placement) |

## 9. New tokens

| Token | Value | Notes |
|---|---|---|
| `size.coach_mark` | 320 (× `font_scale`, max window width − 16) | `size::COACH_MARK`. Coach mark width. |

Colors, radius, elevation, motion and z-layers: **none** new (`z.dialog` for marks and spotlight).
