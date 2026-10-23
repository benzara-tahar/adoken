# Command palette (Cmd+P)

Status: v0.1. Owner: ux-designer. Behavior, ranking, and perf acceptance: `docs/specs/0003-*` (command palette / universal search). Tokens: [tokens.md](tokens.md). States: [principles.md](principles.md) §3.

One box for everything: pull requests, work items, pipeline runs, people, repositories, projects, notifications, and every command. Local-first: results come from the store and the search index, < 50 ms at 100k items, with no spinner in the normal case.

## 1. Opening

| Shortcut | Opens with | Context |
|---|---|---|
| `⌘P` | empty query, all scopes | global (also click the titlebar search field) |
| `⌘⇧P` | `>` (commands) | global |
| `⌘O` | `proj:` chip (switch project filter) | global |
| `⌘P` / `Esc` while open | closes (toggle) | palette |

The palette view is created once at startup and kept alive (hidden), so opening is a visibility flip + focus: first frame ≤ 16 ms. The query is cleared on close unless it was closed with an unfinished scope chip within the last 10 s (reopen restores it, selected text, so typing replaces).

## 2. Layout

```
                ┌──────────────────────────────────────────────────────────┐
                │ ⌕  [PRs ×] retry idem author:@me▏                         │ input 52
                │━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━│ 1 px progress (only if slow)
                │ Top hit                                                  │ group header 28
                │ ▌⊙ Retry idempotency keys on payment capture     ✓✓⏳ 2h │ row 44
                │    !4821 · payments-api · feature/retry → main           │
                │ Pull requests                                            │
                │  ⊙ Retry budget for webhook dispatch  Draft      ✓  3d   │
                │    !4799 · webhooks · dana/retry-budget → main           │
                │  …                                                       │
                │  Show all 23 pull requests →                             │
                ├──────────────────────────────────────────────────────────┤
                │ ↩ Open   ⌘↩ Browser   ⌘K Actions   Tab Complete     ?   │ footer 32
                └──────────────────────────────────────────────────────────┘
```

- Width `size.palette_width` = 680; top edge at 14 % of window height (never less than titlebar + 48). Centered horizontally over the whole window.
- Surface: `bg.overlay` (× `overlay_opacity` with custom backgrounds), `radius.lg`, `elevation.3`, `z.palette`. Scrim `bg.scrim` over the window.
- **Input row** (52): search icon 16 `text.tertiary`; scope chips; text input `type.heading` (15/20) `text.primary`, placeholder `text.tertiary` "Search pull requests, work items, pipelines, people…  (> for commands)". Filter tokens render inline in the input with `accent.wash` background, `radius.xs`, and `text.primary` (valid) or `status.warning.wash` + wavy underline (unknown key).
- **Results:** row height `size.palette_row.*` by density: compact 36 (single line: title + trailing, subtitle hidden), default 44 (title `type.body_strong` + subtitle `type.small`), comfortable 52 (same two lines, more padding). Visible rows: max 8.5 (the half row hints scroll). Group headers 28, `type.label` `text.tertiary`, not selectable. Virtualized with GPUI `list` + `ListState` (variable heights because of headers); only visible rows render.
- **Selection:** `bg.selected` + 2 px left bar `accent.solid` (the `▌`), `radius.sm`, inset 6 px. Instant move (`motion.instant`). Scroll keeps the selection with ≥ 1 row of context.
- **Footer** (32): hints with keycaps for the selected row's primary actions (context-specific), right side `?` opens the help list (§4.6). `type.small` `text.tertiary`.
- **Preview pane (wide mode):** when the window is ≥ 1200 wide and `[palette] preview = "auto"` (or `"on"`), the palette is 960 wide: list 560 + preview 400 separated by `border.default`. `⌘.` toggles preview. Preview renders from the store only (§6).
- Height: shrinks to content (min: input + 1 row + footer), animates height changes over `motion.base` only when growing by > 3 rows; otherwise instant (never animate under the user's typing).

Motion: `palette-in` / `palette-out` from [tokens.md](tokens.md) §7.3 (quick 140 ms, exit 90 ms). Reduce motion: snap.

## 3. Query language

### 3.1 Scopes

Typed at the start of the query. A sigil scopes immediately; a keyword becomes a chip when followed by a space (or `Tab`).

| Scope | Sigil | Keywords | Chip label | Default list (empty query) |
|---|---|---|---|---|
| Commands | `>` | `cmd:` | Commands | Recently used commands, then all by category |
| Pull requests | `!` | `pr:` | PRs | Review requested from you, then your open PRs |
| Work items | `#` | `wi:` | Work items | Assigned to you (active states), by changed date |
| People | `@` | `people:` | People | People from your recent PRs |
| Pipelines | — | `p:` `pipe:` | Pipelines | Your recent runs, failed first |
| Repositories | — | `r:` `repo:` | Repos | Repos you touched recently |
| Projects | — | `proj:` | Projects | All synced projects (toggle to filter the app) |
| Notifications | — | `n:` | Inbox | Unread, by priority |
| Pins | — | `pin:` | Pins | Your pins in pin order (spec 0014); `⌘J` opens the palette in this scope |
| Help | `?` | — | Help | Scopes, filters, shortcuts (§4.6) |

- Sigils are ADO's own reference syntax (`!` PR, `#` work item, `@` person), so pasting "see !4821" just works.
- One scope at a time. Typing a second scope keyword replaces the chip.
- `Backspace` in an empty input: first press highlights the chip (2 px `border.focus` outline), second press removes it. `⌘⌫` clears query, chips, and filters at once.

### 3.2 Direct jumps

| Input | Result |
|---|---|
| `!4821` / `pr:4821` | Exact PR as Top hit. If not in the store: row "Open !4821 — not synced yet" (fetches on Enter, spinner in the row). |
| `#1234` | Exact work item, same rules. |
| `4821` (bare digits) | Top hits: PR !4821 and work item #4821 if either exists, then normal fuzzy results. |
| Pasted `https://dev.azure.com/<org>/<project>/_git/<repo>/pullrequest/<id>` | "Open pull request !<id> from link" as the only Top hit. Same for `_workitems/edit/<id>`, `_build/results?buildId=<id>`, and legacy `<org>.visualstudio.com` URLs. Links to another org: "This link is for another organization" (disabled row) + "Open in browser". |

### 3.3 Filters

`key:value` tokens anywhere in the query. Negate with a leading `-` (`-is:draft`). Quote values with spaces (`repo:"Payments API"`). Multiple values OR (`type:bug,task`). Different keys AND. People values accept `@me` or a fuzzy name (`author:@dana`). Dates: `<7d`, `>2w`, `2026-09-01..2026-09-15`.

| Key | Scopes | Values | Autocomplete source |
|---|---|---|---|
| `is:` | PRs | `open` `draft` `merged` `abandoned` `mine` `review-requested` `unread` | fixed |
| `is:` | Work items | `mine` `open` `closed` | fixed |
| `is:` | Inbox | `unread` `snoozed` `done` | fixed |
| `author:` | PRs | person | people in store |
| `reviewer:` | PRs | person | people in store |
| `vote:` | PRs | `approved` `suggestions` `waiting` `rejected` `none` (your vote unless combined with `reviewer:`) | fixed |
| `repo:` | PRs, pipelines | repo name | repos in store |
| `branch:` / `target:` | PRs, pipelines | branch name | branches seen in store |
| `label:` | PRs | label | labels in store |
| `type:` | Work items | `bug` `task` `story` `feature` `epic` `issue` or any custom type name | types in store |
| `state:` | Work items | state name | states in store |
| `assigned:` | Work items | person | people |
| `priority:` | Work items, inbox | `1`–`4` / `urgent` `high` `normal` `low` | fixed |
| `area:` / `iteration:` | Work items | path, `@current` | paths in store |
| `tag:` | Work items | tag | tags in store |
| `status:` | Pipelines | `failed` `running` `succeeded` `partial` `canceled` `queued` | fixed |
| `by:` | Pipelines | person | people |
| `from:` / `source:` | Inbox | person / `pr` `wi` `pipeline` `mention` `nudge` | people / fixed |
| `project:` | all | project name | synced projects |
| `updated:` / `created:` | all item scopes | date expression | — |

Without a scope, filters narrow each group they apply to and hide groups they can't apply to (e.g. `type:bug` hides Pipelines). Unknown key → the token is treated as text and the footer says "Unknown filter `auther:` — did you mean `author:`? Tab to fix" (edit distance ≤ 2).

### 3.4 Autocomplete

A suggestion strip appears **inside the results list, as the first group** ("Suggestions") while the caret is in a scope keyword, filter key, or filter value:
- Typing `au` → suggestions `author:`; typing `author:` → `@me`, then recent people; typing `author:da` → fuzzy people matches.
- `Tab` accepts the first (or selected) suggestion and appends a space. `↓` moves into suggestions like any row; `↩` on a suggestion also accepts it (does not open anything).
- Max 5 suggestions. They disappear when the caret leaves the token.

### 3.5 Free text

Fuzzy, typo-tolerant matching (algorithm and ranking: spec 0003 / `crates/search`). Fields searched per type: PR — title, id, repo, source/target branch, author; work item — title, id, tags, assigned; pipeline — pipeline name, run number, branch; person — name, email; repo — name; command — title, aliases. Matched characters render `text.match` at weight 560 in title and subtitle only (never in trailing meta). Typo matches highlight the matched span without per-char precision.

## 4. Results

### 4.1 Grouping

- **No scope:** `Top hit` (1 row, best score across types) → `Suggestions` (if autocompleting) → fixed group order `Pull requests · Work items · Pipelines · People · Repositories · Projects · Notifications · Commands`. Each group shows ≤ `max_per_group` (default 4) rows plus a "Show all 23 pull requests →" row (↩ on it applies the scope). Empty groups are omitted. Order is fixed for muscle memory; only Top hit is dynamic.
- **With a scope:** one flat list, no headers, up to 500 rows (virtualized), then "Showing 500 of 2,318 — refine your search".
- `show_group_headers = false` removes headers (groups stay ordered, separated by `border.subtle`).

### 4.2 Empty query

- `Pinned` — the user's pins in pin order, max 9 (spec 0014; visuals in [pins.md](pins.md)).
- `Recent` — last `recent_count` (8) items opened from anywhere in the app, any type.
- `Suggested` — up to 3 nudges (spec 0006): "Review !4830 Add retry budget — waiting 2 days on you".
- With a scope chip: that scope's default list (§3.1).

### 4.3 States

| State | What the user sees |
|---|---|
| Typing (normal) | Results replace in place on every keystroke. Previous results stay until new ones arrive: never an empty flash. Selection resets to the first row when the query changes. |
| Slow (> 120 ms since the keystroke) | 1 px indeterminate bar under the input (`accent.solid`, 1400 ms sweep; reduce motion: static 30 % bar). |
| No matches | Empty state inside the list: "No matches for "retr idem"" + rows: **Search Azure DevOps for "retr idem"** (server search, spec 0003) and **Clear filters** (if any filters). |
| Server search running | That row shows a 14 px spinner and "Searching Azure DevOps…". Results arrive as a `From Azure DevOps` group **appended at the bottom**; local results never reorder. |
| Server search failed | Row becomes "Couldn't search Azure DevOps · Retry" (`status.danger.fg` icon). |
| First sync in progress | Footer left shows `◔ Still syncing — results may be incomplete`; the server-search row is always appended as the last row. |
| Offline | Server-search row disabled: "Offline — showing results on this Mac". |
| Unavailable command | Row shown in `text.disabled` with reason as subtitle ("Offline — reconnect to vote", "Read-only token — vote in the browser"); ↩ does nothing but announces the reason. |
| Item not synced (direct jump) | See §3.2. |

### 4.4 Actions

`↩` runs the row's primary action and closes the palette. `⌘K` opens the **action menu** for the selected row: a secondary list anchored to the right edge of the selected row (menu-in motion), 280 wide, fuzzy-filterable (type to filter), each action with its shortcut. `Esc` closes it back to the results.

| Type | ↩ | ⌘↩ | ⌘K actions (v0.1) |
|---|---|---|---|
| Pull request | Open in adoken (Overview) | Open in browser | Open files · Approve · Approve with suggestions · Wait for author · Reject · Reset vote · Copy link `⌘⇧C` · Copy `!id title` `⌘⌥C` · Copy source branch · Copy `git checkout` command · Mute notifications for this PR |
| Work item | Open in adoken | Open in browser | Change state… · Assign… · Assign to me · Comment… · Sync this work item `⌘⇧R` (spec 0015 FR-35: each opens the item in Work items with that editor open) · Copy link · Copy `#id title` · Show linked PRs · Mute notifications for this item |
| Pipeline run | Open in adoken | Open in browser | Open logs in browser · Copy link · Show PR · Show pipeline history |
| Person | Show their open PRs (`pr:` `author:@name`) | Open profile in browser | Show PRs they review · Show work assigned · Copy email |
| Repository | Show open PRs (`pr:` `repo:name`) | Open in browser | Copy clone URL (HTTPS) · Show pipelines |
| Project | Toggle project filter | Open in browser | Only this project · All projects |
| Notification | Open target | Open target in browser | Mark done `e` · Snooze… `s` · Mute source |
| Command | Run | — | Show keybinding |

Vote actions only appear when you're a reviewer (or could add yourself) and the token access level is Review; otherwise hidden (not disabled) from ⌘K, but shown disabled in `>` command search with the reason. Work item edit actions (Change state…, Assign…, Assign to me, Comment…) follow the same rule with `vso.work_write` (spec 0015 FR-28); Sync this work item only needs `vso.work` and is always offered.

### 4.5 Commands (`>`)

Every app action is a command with a stable id, a title in sentence case, optional aliases, a category, and its current keybinding. Examples: `app::Sync` "Sync now" `⌘R`; "Quiet mode: turn on or off" and "Quiet mode for 1 hour" (no sounds, celebrations or Ken surfaces while on; works with Ken off; spec 0025 FR-8); `theme::Select` "Change theme…" (opens a sub-list of themes; ↩ applies, moving the selection doesn't preview: user decision 2026-09-30); `palette::CustomizeRows` "Customize result rows…". Sub-lists (theme, accent, density, project) use the same palette UI with a breadcrumb chip ("Theme ›").

Command families added 2026-10-01:
- **Work item** (spec 0015 FR-35, keys in [work-item-detail.md](work-item-detail.md) §10): "Work item: Edit title", "Change state…", "Assign…", "Assign to me", "Unassign", "Change area…", "Change iteration…", "Edit description", "Add link…", "Remove link…", "Comment", "Sync this work item" `⌘⇧R`, "Open in Azure DevOps", "Copy link", "Copy id". "Move to project…" arrives with slice C in v0.2.
- **Activity** (spec 0016 FR-3, FR-25, [activity.md](activity.md) §6): "Activity: Show sync activity", "Activity: Show network log", "Activity: Hide", "Network log: Export…".
- **Navigation** (spec 0018 FR-15): "Go back" `⌘[`, "Go forward" `⌘]`.
- **Account** (spec 0001 FR-16, spec 0009 FR-9): "Replace token…", "Change access…".
- **Pull request review** (spec 0019 §3, [pr-review-polish.md](pr-review-polish.md)): "Filter files…" `/`, "Files: clear filter" `⌘⌫`, "Only files with threads: off → Turn on" `⇧T`, "Show threads" / "Show files" `t`, "Threads: show active" / "show resolved" / "show all", "Copy pull request title", "Copy source branch", "Copy target branch", "Group reviewers: hidden → Show" / "Group reviewers: shown → Hide".
- **Work items filters** (spec 0008 FR-8): "Work items: Filter by area…", "Work items: Filter by iteration…" (switch to that tab first; `/` on By area / By iteration).
- **Help** (spec 0020 FR-14; aliases "tour", "guide", "walkthrough"): "Tour: Today", "Tour: Inbox", "Tour: Pull request review", "Tour: Work item", "Tour: Command palette", "Tour: This view", "Tours: Show again on first use", "Tours: Turn off first-use tours" (title flips to "Turn on" when off).
- **Global shortcut** (spec 0021 FR-13): "Global shortcut settings…" (opens Settings › Keyboard at the Global shortcut section).
- **Celebrations** (spec 0022 FR-15): "Preview celebration". Turning celebrations off is Settings › Celebrations or Quiet mode (spec 0025 FR-8).
- **Statusbar** (spec 0023 FR-10): "Statusbar: Show or hide memory and CPU" (one toggle, spec 0025 FR-9), "Copy resource usage".

Command rows: leading category icon, title, trailing keycaps (the command's own shortcut, e.g. "Toggle sidebar ⌘B"; no ⌘1…⌘9 row numbers, spec 0025 FR-1). An on/off pair is one toggle command titled by what it does ("Group reviewers: show or hide", "Quiet mode: turn on or off"; spec 0025 FR-9). Themes are picked through "Theme: Choose…", not one row per theme (spec 0025 FR-10).

### 4.6 Help (`?`)

Static list inside the palette: scopes with sigils, filter keys with examples, and the palette keymap (§7). Selecting a filter row inserts it into the query.

## 5. Customizable result rows

### 5.1 Row anatomy

```
[leading]  {title}  [badge] [badge]                    [trailing] [trailing] [trailing]
           {subtitle}
```
- `leading`: 16 px glyph column, 24 wide — `type_icon` (domain icon in its domain color, [tokens.md](tokens.md) §2.8), `status_icon` (state icon: PR status, vote, run result), `avatar` (20 px), or `none`.
- `title`: `type.body_strong` `text.primary`, single line, truncates first.
- `badges`: ≤ 2 inline chips after the title (`type.label`, `radius.xs`, `bg.raised`), e.g. Draft, your vote.
- `subtitle`: `type.small` `text.secondary`, single line, truncates second; `""` = single-line row.
- `trailing`: ≤ 3 right-aligned items, `type.small` `text.tertiary`, tabular numbers, each ≤ 12 characters; never truncates (drops from the left if the row is too narrow).

### 5.2 Config

```toml
[palette]
preview = "auto"            # auto | on | off
show_group_headers = true
max_per_group = 4
recent_count = 8

[palette.rows.pull_request]
leading = "status_icon"
title = "{title}"
badges = ["draft", "my_vote"]
subtitle = "{id} · {repo} · {source_branch} → {target_branch}"
trailing = ["reviewers_summary", "updated"]
```

Template syntax: `{field}` interpolation plus literal text. A separator (` · `, ` → `, ` / `) adjacent to an empty field is dropped, so optional fields never leave dangling dots. Field formatting is fixed per field (relative times, `4m 12s` durations, IDs with sigils).

Validation: unknown field or slot value → warning in Settings › Search › "Row issues", the field renders empty; invalid TOML → keep last good config.

### 5.3 Field catalog and defaults

| Type (`rows.<type>`) | Fields | Default |
|---|---|---|
| `pull_request` | `id` `title` `repo` `project` `author` `author_avatar` `source_branch` `target_branch` `status` `draft` `my_vote` `reviewers_summary` (icons: ✓ approved, ⏳ waiting, ✕ rejected, with counts) `reviewers` `comments` `unresolved` `pipeline_status` `labels` `work_items` `created` `updated` | leading `status_icon` · title `{title}` · badges `draft`, `my_vote` · subtitle `{id} · {repo} · {source_branch} → {target_branch}` · trailing `reviewers_summary`, `updated` |
| `work_item` | `id` `title` `type` `state` `assigned_to` `assigned_avatar` `priority` `area` `iteration` `tags` `parent` `project` `created` `updated` | leading `type_icon` · title `{title}` · badges `state` · subtitle `{id} · {type} · {area}` · trailing `priority`, `assigned_avatar`, `updated` |
| `pipeline_run` | `pipeline` `run` `status` `result` `branch` `triggered_by` `duration` `finished` `repo` `project` | leading `status_icon` · title `{pipeline} {run}` · subtitle `{branch} · {triggered_by}` · trailing `duration`, `finished` |
| `person` | `name` `avatar` `email` `open_prs` | leading `avatar` · title `{name}` · subtitle `{email}` · trailing `open_prs` |
| `repository` | `name` `project` `default_branch` `open_prs` | leading `type_icon` · title `{name}` · subtitle `{project} · {default_branch}` · trailing `open_prs` |
| `project` | `name` `description` `updated` `filtered` | leading `type_icon` · title `{name}` · subtitle `{description}` · trailing `filtered` (✓ when in the app filter) |
| `notification` | `title` `source` `from` `from_avatar` `priority` `received` `unread` `target_id` | leading `from_avatar` · title `{title}` · badges `unread` · subtitle `{source} · {target_id}` · trailing `priority`, `received` |
| `command` | `title` `category` `shortcut` | leading `type_icon` · title `{title}` · subtitle `""` · trailing `shortcut` |

### 5.4 Customize UI

Command "Customize result rows…" opens Settings › Search › Result rows:
- Type selector (segmented, 8 types).
- Leading: dropdown. Title/subtitle: template inputs with field autocomplete on `{`. Badges/trailing: chip lists, reorder by drag or `⌥↑/⌥↓`, add via `+` menu of fields, remove with `⌫`.
- Live preview: 3 real rows from the store at the current density, in both selected and unselected states, updating per keystroke.
- [Reset to default] per type.

## 6. Preview pane (wide mode)

Content per type, from the store only (never triggers a fetch; shows "Not synced yet" if data is missing):
- **PR:** title (`type.heading`), status + draft chips, author avatar/name, `source → target`, reviewers with vote icons, latest pipeline status, linked work items, first 8 lines of description (markdown rendered, images omitted), updated time.
- **Work item:** title, type/state/priority chips, assigned, area/iteration, tags, first 8 lines of description, linked PRs.
- **Pipeline run:** name + run, result, branch, trigger, duration, stage list with result icons.
- **Person:** avatar 28, name, email, their open PRs (≤ 5), PRs awaiting their review (≤ 5).
- **Command:** description + keybinding.

Updates on selection change, debounced 50 ms while a navigation key is held (auto-repeat) so fast scrolling never builds previews it throws away.

## 7. Keyboard map

### 7.1 Palette

| Key | Action | Action id |
|---|---|---|
| `↑` `↓` | Previous / next row (wraps) | `palette::SelectPrev` / `SelectNext` |
| `⌥↑` `⌥↓` | Previous / next group | `palette::SelectPrevGroup` / `SelectNextGroup` |
| `PageUp` `PageDown` | Page | `palette::PageUp` / `PageDown` |
| `↩` | Primary action (or accept suggestion) | `palette::Confirm` |
| `⌘↩` | Open in browser | `palette::OpenInBrowser` |
| `⌘K` | Action menu for selected row | `palette::OpenActions` |
| `Tab` | Accept autocomplete suggestion / turn keyword into chip | `palette::Complete` |
| `⌘D` | Pin / unpin the selected PR, work item or pipeline (spec 0014) | `pins::TogglePin` |
| `⌘⇧C` | Copy web link of selected row | `palette::CopyLink` |
| `⌘⌥C` | Copy `!id title` / `#id title` as markdown link | `palette::CopyReference` |
| `⌘.` | Toggle preview pane | `palette::TogglePreview` |
| `⌘⌫` | Clear query, chips, filters | `palette::Clear` |
| `⌫` (empty input) | Highlight chip, then remove | `palette::RemoveScope` |
| `Esc` | Unwind one level: action menu → suggestion → chip highlight → query → close | `palette::Dismiss` |
| `⌘P` | Close | `palette::Toggle` |
| `⌘⇧P` | Toggle `>` command scope | `palette::ToggleCommands` |

Printable keys always go to the input, even when a row is selected. Mouse: hover does **not** move selection (avoids fighting the keyboard); click runs the primary action; right-click opens the action menu.

### 7.2 Global (app-wide)

| Key | Action |
|---|---|
| `⌘P` / `⌘⇧P` / `⌘O` | Palette / commands / projects |
| `⌘1` … `⌘6` | Today · Inbox · Pull requests · Work items · Pipelines · Search (sidebar order, spec 0025 FR-5) |
| `⌘J` | Palette in `pin:` scope |
| `⌘D` | Pin / unpin the current item (selected row, card, or open detail) |
| `⌥⌘↑` / `⌥⌘↓` | Move the focused pin up / down |
| `⌘[` / `⌘]` | Back / forward through views, restoring each view's filters, selection and scroll (spec 0018); mouse back / forward buttons too. Not while a text field has the keyboard |
| `⌘R` | Sync now |
| `⌘,` | Settings |
| `⌘B` | Collapse / expand the sidebar rail |
| `⌘⇧↩` | Toggle full-width detail (Work items, Pipelines; Pull requests always shows one pane) |
| `/` | Focus the list filter |
| `?` / `⌘/` | Shortcuts cheat sheet (`?` only when no text input is focused); generated from the registered bindings and palette commands, searchable, grouped by screen: Everywhere, Today, Inbox, Pull requests, Pull request review, Work items, Pipelines, Search, Pins, Activity, Settings, then Command palette (unbound commands); widget-internal keys are hidden (spec 0025 FR-7) |
| `↑ ↓` / `j k` | Move in lists |
| `↩` / `⌘↩` | Open / open in browser |
| `⌘K` | Action menu for the selected list row (same menu as §4.4) |
| `⌘⇧C` / `⌘⌥C` | Copy link / copy reference |
| `e` / `s` | Inbox: mark done / snooze menu |
| `⌘⌫` | Clear list filters |
| `⌘⇧A` | Toggle the Activity drawer (sync activity, network log) on its last tab (spec 0016, [activity.md](activity.md)) |
| `F6` | Cycle focus: content → Activity drawer (when open) → toasts |
| `⌘⇧R` | Work items: sync this work item (spec 0015 FR-30; Work items view, list or detail) |
| `⌃⌥⇧⌘A` (configurable, Settings › Keyboard) | Show adoken from any app (system-wide, spec 0021; one shortcut, spec 0025 FR-12) |

View-specific keys (PR review, notification center) are defined in their specs and appear in the `?` overlay and in `>` command results. Work items: `t s a i ⇧A ⇧I d l c o y ⇧Y`, aliases `⇧S ⇧C`, `⌫` on a link row ([work-item-detail.md](work-item-detail.md) §10, spec 0015 §3.3; `⇧P` with slice C in v0.2), in the `(WorkItemList || WorkItemDetail) && !TextInput` context. Activity drawer: `⌃Tab` / `⌃⇧Tab` switch tabs, `⌘⇧↩` maximize / restore, `F` failed only, `T` totals, `/` filter, `⌘C` copy rows, `⌘⌫` clear filters, `End` jump to latest, in the `ActivityPanel && !TextInput` context ([activity.md](activity.md) §6.2).

Windows/Linux: `⌘` → `Ctrl`, `⌥` → `Alt`. All bindings are overridable in `<config>/keymap.toml`:

```toml
[[binding]]
context = "Palette"          # Palette | Workspace | List | PrReview | Inbox | WorkItemList | WorkItemDetail | ActivityPanel
keys = "cmd-k"
action = "palette::OpenActions"
```

## 8. Accessibility

- Palette: `Role::Dialog` labelled "Command palette"; focus moves to the input on open and returns to the previously focused element on close.
- Input: `Role::SearchInput`, label "Search Azure DevOps", placeholder exposed. Scope chip is part of the label ("Search pull requests").
- Results: `Role::List` labelled by the current scope ("Results, 23 pull requests"); each row `Role::ListItem` with `aria_selected` and a full label built from the row fields, independent of the visual template: "Pull request 4821, Retry idempotency keys on payment capture, active, by Dana Kim, payments-api, feature/retry into main, 2 approvals 1 waiting, updated 2 hours ago." Group headers are `Role::Heading`.
- Selection changes announce the row label (polite). Result count changes announce once, 500 ms after typing stops: "23 results".
- Action menu: `Role::Menu` with `Role::MenuItem` entries including shortcuts in the label.
- Match highlighting is visual only; the label contains plain text.
- Contrast: `text.match` passes 4.5:1 on `bg.overlay` and `bg.selected` in built-ins; the theme validator enforces it for user themes.
