# 0018 — Navigation history: back and forward across views

| Field | Value |
| --- | --- |
| Status | Accepted (2026-10-01, owner: all recommendations; overrides noted) |
| Milestone | v0.1 (Q-5): on the cut line between 0016 group A (sync activity) and pins. RICE 96 |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/app` (`shell`: history, titlebar buttons, key and mouse handling; `today`, `inbox`, `review`, `workitems`, `pipelines`: capture and restore of view state), `crates/gallery` (stories, `nav_history` smoke test) |
| Serves | **Snappy** (one key returns to exactly where I was, instead of re-finding a PR, a file, a filter and a row) · **Less noise** (no "where was I?" detours through the sidebar, the palette and the list) |
| Depends on | `docs/specs/0003-command-palette.md` (targets, commands), `docs/specs/0004-pr-review.md` (§3.1 list and review, FR-8 drafts, FR-17 work item links), `docs/specs/0005-notification-center.md` (filters, open), `docs/specs/0006-smart-nudges.md` (Today targets), `docs/specs/0008-work-items-pipelines.md`, `docs/specs/0014-pins.md` (pin opens), `docs/specs/0015-work-item-editing.md` (detail links, FR-20 drafts), `docs/specs/0016-statusbar-activity.md` (drawer opens), spec 0019 (PR review Files / Threads modes, same batch: its review-local state is restorable here, FR-5) |
| Design | `docs/design/principles.md` §2 (titlebar already places `‹ ›` after the traffic lights), `docs/design/command-palette.md` §7.2 (`⌘[` / `⌘]` already reserved for back / forward). No new design doc: this spec adds two titlebar buttons and their tooltip copy (§3.3) |

> Rule: no code without an **Accepted** spec (AGENTS.md). The spec owns behavior, acceptance, and budgets; design docs own visuals, motion, sound, and copy.

## 1. Problem

Owner dogfood note (2026-10-01): "If we are displaying a PR and then we click on the related work item, it's quite difficult to get back to the PR. … If we are navigating from the notification to the work item or to the PR or to the pipeline, it'd be nice to be able to go back to the notification list and to the exact same state, like with the exact selected filter and the exact selected row."

Today every cross-view open replaces where you were. `Shell::show` assigns `self.nav = nav` and keeps nothing (`crates/app/src/shell/mod.rs:917`); `open_target` (`:935`) switches section and opens the target, so the origin is gone. The only "back" is local: in a PR review, `Esc` / `⌘[` return to the PR list (`crates/app/src/review/mod.rs:97-133`), even if you came from the Inbox. The Inbox keeps its filter but remembers its selection as an index (`crates/app/src/inbox/mod.rs:316`, `refresh` at `:566` clamps it), and opening a notification marks it read, so under the Unread filter the row you opened disappears and the cursor lands on a different one. The design promised `⌘[` / `⌘]` (command-palette.md §7.2, work-item-detail.md §6 "back with `⌘[`"); nothing implements them.

## 2. Users and job to be done

- **Persona:** developer on Azure DevOps who triages from the Inbox and Today, reviews PRs, and hops to linked work items and runs many times a day.
- **Job to be done:** "When I follow a link from a PR, a notification or a card to check something, I want one key to bring me back to exactly where I was, so I can finish what I started without re-finding it."
- **Smallest delightful slice:** PR → linked work item → `⌘[` lands on the same PR, same file, same scroll, same cursor. Inbox (filtered) → PR, work item or run → `⌘[` lands on the Inbox with the same filter and the same notification selected.
- **Priority (RICE):** Reach 90 (every session crosses views) · Impact 2 · Confidence 80% · Effort 1.5 weeks (shell history plus capture/restore in six views) = **96**.
- **Cut:** a history menu on the back button (Q-4), persistence across restarts (Q-1), two-finger trackpad swipe (Q-6), per-pane histories. Each adds surface without changing the two owner scenarios.

## 3. UX flow

### 3.1 Happy path A: PR → work item → back

1. In the Pull requests view, the review of !4821 is open on `src/payments/retry.rs`, scrolled to line 120, the line cursor on 124.
2. The header's Work items row lists #20101; a click (or `↩` on it) opens Work items with #20101 selected and its detail shown.
3. `⌘[` (or the mouse back button, or the titlebar `‹`, whose tooltip reads "Back to !4821 Retry idempotency keys · ⌘[") shows the review of !4821 on `src/payments/retry.rs`, line 120 at the same viewport offset, cursor on 124, the file tree scrolled as before, the keyboard in the diff.
4. `⌘]` returns to #20101 in Work items, as it was.

### 3.2 Happy path B: Inbox → item → back

1. The Inbox is filtered to Source = Pull requests, State = Unread. The fourth row, a mention on !4821, is selected; the list is scrolled.
2. `↩` opens it (0005: it becomes read) in its canonical view.
3. `⌘[` shows the Inbox with the same chips, the same notification selected (shown read, still listed although it no longer matches Unread, FR-6), at the same viewport offset, the keyboard on the list. `↓` moves to the row that was below it.
4. The same holds when the notification opens a work item or a pipeline run, and when notifications arrived meanwhile (selection by id, not index).

### 3.3 Affordances

- **Titlebar:** two ghost icon buttons right after the traffic lights (principles §2 sketch: `● ● ●   ‹ ›   Contoso…`): `chevron-left` and `chevron-right`, `size.control.sm`, icon `size.icon.sm`, `text.secondary`; `text.disabled` and not clickable when there is nothing in that direction; hover `bg.hover`. Tooltips (sentence case): "Back to {destination} · ⌘[" and "Forward to {destination} · ⌘]". Disabled: "Nothing to go back to" / "Nothing to go forward to".
- **Destination labels** (FR-9): "Today"; "Inbox", or "Inbox · Unread · Pull requests" when filters are set; "Pull requests"; "!4821 Retry idempotency keys"; "Work items", or "#20101 Fix login redirect" when an item is selected; "Pipelines", or "payments-ci #20261001.4" when a run is selected; "Search". Titles truncate at 48 characters with an ellipsis.
- **Keys:** `⌘[` back, `⌘]` forward. Mouse back / forward buttons. `Esc` in a PR review keeps its meaning "up to the list" (FR-12).
- **Palette:** "Go back" (`⌘[`) and "Go forward" (`⌘]`).

### 3.4 Alternate paths

- **Back across several hops:** Inbox → !4821 → #20101 → its parent #20090 → `⌘[` `⌘[` `⌘[` walks #20101, !4821, Inbox, each restored.
- **New open after going back:** from !4821 (after one `⌘[`), opening run 512 drops the forward entries (#20101, #20090), as in a browser.
- **Target gone:** the PR left the store (abandoned and aged out), the work item was deleted, the run purged: back still lands there and the view shows its own unavailable state, with back still working (FR-7).
- **Row gone:** the notification was marked done elsewhere or expired: the Inbox restores the filter exactly and selects the row now at that position (FR-7).
- **Same place:** opening the PR that is already shown, or `⌘3` while in Pull requests, adds no entry.

### States

| State | Behavior |
| --- | --- |
| Empty | At launch the history holds one entry, the first section shown; both buttons disabled. |
| Loading / first sync | Entries record what is shown; restoring into a view still loading applies the state when its data lands (FR-8). |
| Error | A view's own error state is restored as the location; nothing is skipped. |
| Offline / stale | No effect: history reads and writes memory only. |
| Throttled by ADO | No effect: history sends no request (FR-13). |

## 4. Functional requirements

### 4.1 Model

- **FR-1 One history per main window.** An ordered list of entries and a cursor, in memory. At launch it holds one entry for the first section shown. It is cleared (back to one entry) on sign-out, on switching between demo and connected mode, and when the window closes. It is not persisted (Q-1). Opening the Settings window, the palette, the Activity drawer, menus, sheets or pickers never changes it.
- **FR-2 Entry.** An entry is a **location** plus the **view state** of that location (FR-5). Locations: `Section(Today | Inbox | PullRequests | WorkItems | Pipelines | Search)`, and `Review(pr id)` for a PR review (it replaces the PR list, 0004 §3.1). In Work items and Pipelines the selected item is view state of the section, because list and detail show side by side (0008 §3).
- **FR-3 What pushes an entry.** An **open** pushes a new entry after the cursor:
  1. every route through the shell's target routing (`Shell::open_target`): palette results and pasted links (0003), Today cards (0006), Inbox `↩` (0005), pins (`⌃1`…`⌃9`, sidebar click, rail peek; 0014), Activity drawer items (0016), OS notification banners (0005); *(Amended 2026-10-02 by 0025 FR-2: no `⌃1`…`⌃9`; pins open by sidebar click, rail peek or ⌘P)*
  2. links inside a view that change the subject: a PR review's work item links (0004 FR-17), a work item detail's PR, parent, child and related links (0015), a run detail's linked PR (0008);
  3. `↩` (or a click) on a PR list row, which opens its review;
  4. `↩` on a thread card in the PR review's Threads mode, which switches to Files on that thread (spec 0019);
  5. a section switch: `⌘1`…`⌘6`, a sidebar or rail click, a palette section command. While the keyboard walks the sidebar with `↑` / `↓` (`Nav::step`), each step **replaces** the entry the walk created, so one walk leaves one entry.
- **FR-4 What doesn't push.** Selection moves, tab changes, filters, typing in filter fields, scrolling, expanding or collapsing, layout toggles, the PR review's `t` (Files / Threads), `⇧T`, `/` and thread status scope (spec 0019), opening in the browser, copying, writes. They change the **current** entry's view state (FR-5). An open whose location and subject equal the current entry's (same PR review; same section with the same selected item) pushes nothing.
- **FR-5 Restorable view state.** Captured into the current entry whenever the cursor is about to leave it (before a push, a back or a forward), so that forward also returns to the exact state. Identity, never index:

  | View | State |
  | --- | --- |
  | Today | selected card (kind + target id), scroll anchor |
  | Inbox | filter (sources, priorities, state), selected `NotificationId`, scroll anchor |
  | Pull requests list | tab, repository filter, selected PR id, scroll anchor |
  | Review(pr) | the list state above (for `Esc`), body mode Files / Threads, path filter text, threads-only toggle, thread status scope, focused thread id (spec 0019), selected file path, file tree scroll anchor, diff anchor (file path + side + line, or thread id), line cursor (side + line), keyboard in tree or diff |
  | Work items | tab, area and iteration path (spec 0008 FR-8), selected work item id, list scroll anchor, detail scroll anchor, detail cursor stop (0015 §3.3), keyboard in list or detail |
  | Pipelines | tab, definition and branch filters, selected definition or run id, list and detail scroll anchors |
  | Search | none |

  A scroll anchor is the id of the first visible row plus its pixel offset, not a raw offset. Not part of an entry: preferences that persist on their own (diff layout, 0004; Inbox filter persistence, 0005), unsent text (already kept as drafts by 0004 FR-8 and 0015 FR-20), open menus, pickers and the palette.
- **FR-6 Opened rows stay put.** The Inbox entry's selection is the notification being opened, captured before opening changes its state (0005 marks it read). When back restores the Inbox, that notification stays listed and selected even if it no longer matches the filter (it became read under Unread), until the selection leaves it or a filter changes; then the filter applies as usual. The same holds for list rows of the other views that an open changed so they no longer match (e.g. a work item whose state changed under a state-filtered tab).
- **FR-7 Missing subjects are shown, never skipped.**
  - The location's subject is gone (PR not in the store, work item deleted or outside the loaded set, run purged): back lands on that location and the view shows its own unavailable state (0004 §3 States "isn't synced"; 0008 §3 States), with the titlebar buttons and `⌘[` still working.
  - A selected row is gone: filters and tab are restored exactly; the selection goes to the row now at the old position in sort order, else the last row, else the empty state.
  - A PR file is gone (a new iteration dropped it): the file at the old tree position is selected and its diff shows from the top. A diff anchor line is gone: the nearest earlier line of the same file.
- **FR-8 Restoring into loading views.** When the restored view's data is still loading (a PR whose review entity was replaced and reopens through 0004's open path; a list still reading the store), the state is applied when the data lands. Any user input to that view before then cancels the pending restore: the user wins. A pending restore older than 5 s is dropped.
- **FR-9 Bounded depth.** At most 50 entries; pushing the 51st drops the oldest. A push drops every entry after the cursor.

### 4.2 Interaction

- **FR-10 Keys.** `⌘[` goes back, `⌘]` forward, in the `Workspace && !TextInput` key context (a text field with the keyboard keeps `⌘[` / `⌘]` for itself and for its own `Esc` flow). Overridable in `keymap.toml` (command-palette.md §7). On Windows and Linux (v0.3): `Alt+←` / `Alt+→`, the platform convention.
- **FR-11 Mouse.** The mouse back and forward buttons (GPUI `MouseButton::Navigate(Back | Forward)`, which the fork also emits for horizontal swipe events from mice and three-finger trackpad swipes: `gpui_macos/src/events.rs:211-236` at rev `18a89af`) go back / forward anywhere in the main window, also while a text field has the keyboard (drafts are kept, FR-5). Two-finger trackpad swipes are not handled (Q-6).
- **FR-12 `Esc` in a PR review stays "up".** `Esc` returns from a review to the PR list (0004 §3.1). When the entry before the cursor is the PR list (`Section(PullRequests)`), `Esc` is a back (the cursor moves; forward returns to the review). Otherwise it pushes `Section(PullRequests)` with the list state the review entry holds. `⌘[` is no longer bound to "Back to pull request list" (§4.4).
- **FR-13 No network.** Back, forward and capture issue no Azure DevOps request. A view that is still alive is re-shown from memory. A PR review that was replaced reopens through 0004's open path, which paints from the local store first; requests that path already makes on any open (0004 FR-3, FR-15, FR-17) are unchanged and attributed to "You: open PR" (0016).
- **FR-14 Titlebar buttons and tooltips.** As §3.3. The tooltip's destination is the label of the entry the button goes to (§3.3 destination labels), computed when the tooltip opens. The buttons are hidden during onboarding (no shell).
- **FR-15 Palette.** Commands "Go back" (`⌘[`) and "Go forward" (`⌘]`). With nothing in that direction they do nothing (the titlebar shows why).
- **FR-16 Focus and accessibility.** After back or forward the keyboard is where it was in that entry (list, detail, tree or diff). The titlebar buttons are buttons labeled "Back to {destination}" / "Forward to {destination}", disabled when unavailable, reachable by `Tab` in visual order (principles §6). No extra announcement: screen readers announce the restored focus.
- **FR-17 Motion.** History adds no animation. Views that animate a list ↔ detail change (work-item-detail.md "slides back") keep their own reduce-motion handling.

### 4.3 Key conflicts checked

| Binding | Where | Result |
| --- | --- | --- |
| `⌘[` onboarding Back (`crates/app/src/onboarding/mod.rs:62`) | `Onboarding` context; the shell isn't active during onboarding | No conflict |
| `⌘[` / `⌘]` Settings tabs (`crates/app/src/settings/mod.rs:54-55`, 0009 §3) | Settings window, its own context | No conflict: history is per main window |
| `⌘[` "Back to pull request list" (`crates/app/src/review/mod.rs:100,133`) | `PrReview` and `PullRequest` contexts, which would shadow the workspace binding | Removed; `Esc` keeps it (FR-12, §4.4) |
| `[` / `]` work item paths (`crates/app/src/workitems/mod.rs:76-77`) | Unmodified keys | No conflict |
| `⌘K` row actions (`crates/app/src/pin_actions.rs`) | — | Untouched |

### 4.4 Amendments to other specs (accepted 2026-10-01; applied to 0004 and command-palette.md the same day)

- **0004 §3.1, line 48. Before:** "- **Back:** `Esc` or `⌘[` (also `Cmd+P` › "Back to pull request list") or the **Pull requests** back button at the start of the review header's title row returns to the list with the same row selected and the scroll position kept. `Esc` goes back only when no composer, menu or sheet has the keyboard (their own `Esc` closes them first). While the review is still opening, isn't synced, or failed, a bar with the same back button stands in for the header."
  **After:** "- **Back:** `Esc` (also `Cmd+P` › "Back to pull request list") or the **Pull requests** back button at the start of the review header's title row returns to the list with the same row selected and the scroll position kept; its tooltip reads "Back to the list · Esc". `Esc` goes back only when no composer, menu or sheet has the keyboard (their own `Esc` closes them first). `⌘[` / `⌘]` are history back / forward across views (spec 0018): after opening the review from the list, `⌘[` returns to the list exactly as `Esc` does; after opening it from the Inbox, Today, a pin or the palette, `⌘[` returns there. While the review is still opening, isn't synced, or failed, a bar with the same back button stands in for the header. (amended 2026-10-01, owner notes; accepted 2026-10-01)"
- **0004 AC-20. Before:** "… `Esc` (or `⌘[`, or the header's back button) shows only the list again with the same row selected, the same scroll offset, and `↑` working at once. Opening a PR from `Cmd+P` shows its review directly; back selects its row. …"
  **After:** "… `Esc` (or `⌘[` when the review was opened from the list, or the header's back button) shows only the list again with the same row selected, the same scroll offset, and `↑` working at once. Opening a PR from `Cmd+P` shows its review directly; `Esc` selects its row in the list, and `⌘[` returns to where the palette was opened (spec 0018 FR-3, AC-3). … (amended 2026-10-01, owner notes; accepted 2026-10-01)". The existing smoke `crates/gallery/tests/pr_list_nav.rs:126-128` (list → review → `⌘[` → same row) keeps passing unchanged.
- **docs/design/command-palette.md §7.2:** the row "`⌘[` / `⌘]` | Back / forward" gains "through views (spec 0018); mouse back / forward buttons too". §4.5 commands: "Go back", "Go forward".
- **docs/design/work-item-detail.md §6** ("back with `⌘[`") is fulfilled as written; no change.

## 5. ADO API surface and limits

| Purpose | Endpoint (api-version) | Scope | Cost / limit notes |
| --- | --- | --- | --- |
| — | **None.** History lives in memory. | none | 0 TSTU. Reopening a replaced PR review uses 0004's open path unchanged (FR-13). |

- **Rate limits:** no added requests. A user stepping back and forth between two PRs whose reviews keep replacing each other repeats 0004's per-open requests; 0004 FR-15 already runs them at user priority under the governor.
- **No push to local apps:** not applicable; a restored view shows the store's current data, which may be newer than when the entry was captured (selection by id keeps the place).
- **PAT scopes:** none.

## 6. Acceptance criteria

Harness: the gallery shell tests that drive the real shell with real key dispatch (`crates/gallery/tests/pr_list_nav.rs` pattern) on the demo store, plus unit tests of a pure history model (`push`, `back`, `forward`, `replace`, capacity) with fake view states. New smoke file: `crates/gallery/tests/nav_history.rs`.

- **AC-1 Owner scenario A (PR → work item → back).** Given the demo store, the review of a PR with ≥ 3 files and a linked work item, the third file selected, the diff scrolled so line L is the first visible row at offset y, the line cursor on L+4 and the tree scrolled: activating the linked work item in the review header shows Work items with that item selected; `⌘[` shows the same PR's review with the third file selected, first visible diff row L at offset y (± 1 px), line cursor L+4, the same tree scroll anchor, and the keyboard in the diff (`↓` moves the line cursor to L+5). `⌘]` shows the work item again, selected, keyboard where it was. The same with the mouse back button and with a click on the titlebar `‹`. (smoke, `nav_history.rs`)
- **AC-2 Owner scenario B (Inbox → item → back).** Given the Inbox filtered to Source = Pull requests and State = Unread, the fourth row (notification N) selected and the list scrolled so N is at offset y: `↩` opens N's target; `⌘[` shows the Inbox with exactly those chips, N selected and listed although now read, N at offset y (± 1 px), keyboard on the list; `↓` selects the row that followed N before. Repeated with a work item notification (opens Work items) and a pipeline run notification (opens Pipelines): same result. Variant: while away, the demo feed inserts 2 newer notifications above N: back still selects N (now 6th) at offset y. (smoke + unit on the inbox view model)
- **AC-3 Push rules.** Each FR-3 source (palette result, pasted link, Today card, Inbox `↩`, `⌃1` pin, Activity drawer item, PR list `↩`, review work item link, work item parent link, run linked PR, `⌘1`…`⌘6`, sidebar click) pushes exactly one entry; each FR-4 action (`j`/`k`, tab switch, filter chip, typing in a filter, scroll, layout toggle, `t`, open in browser) pushes none and is restored by a later back. Walking the sidebar with `↓` four times then `↩` leaves one entry for the walk. Opening the PR already shown pushes nothing. (unit + smoke) *(Amended 2026-10-02 by 0025 FR-2: drop the `⌃1` pin source)*
- **AC-4 Forward truncation and depth.** After A → B → C, back to B, opening D gives A, B, D with forward disabled. After the launch entry plus 60 pushes the history holds exactly the newest 50 entries; 49 backs reach the oldest kept entry, and a 50th back does nothing. (unit)
- **AC-5 Forward restores captured state.** On entry B, moving the selection from row 2 to row 5 and then going back and forward shows row 5 selected, not row 2. (unit)
- **AC-6 Missing subjects.** Back to a PR deleted from the store shows the review's unavailable bar with the back button, and `⌘[` continues to the previous entry; back to an Inbox whose selected notification was marked done elsewhere restores the chips exactly and selects the row now at that position; with the filtered list empty it shows the filtered-empty state; a PR file dropped by a new iteration selects the file at the old tree position with its diff at the top. (smoke + unit)
- **AC-7 Deferred restore.** Given a review entry whose PR review entity was replaced (A → B via links), back to A with the file list delayed 300 ms applies the selected file and diff anchor when it lands; pressing `j` in the tree before it lands cancels the restore and the selection follows the key. A restore still pending after 5 s (injected clock) is dropped. (unit with injected clock)
- **AC-8 `Esc` in a review.** List → review → `Esc` shows the list and forward is enabled (back, not a push); Inbox → review → `Esc` shows the list (one push) and `⌘[` then returns to the review, and `⌘[` again to the Inbox. (smoke)
- **AC-9 Key contexts.** With a comment composer focused, `⌘[` does nothing and the draft is untouched; the mouse back button goes back and the draft is restored on forward (0004 FR-8). In the Settings window `⌘[` / `⌘]` still switch tabs and the main window's history is unchanged. During onboarding `⌘[` is onboarding's Back. No binding of `⌘[` remains in the `PrReview` or `PullRequest` contexts (keymap assertion). (unit: keymap + smoke)
- **AC-10 Titlebar.** At launch both buttons are disabled with tooltips "Nothing to go back to" / "Nothing to go forward to"; after AC-1's first open the back tooltip reads "Back to !{id} {title truncated at 48} · ⌘[", and after AC-2's open "Back to Inbox · Unread · Pull requests · ⌘[". The buttons expose button roles with "Back to …" / "Forward to …" labels and are in the `Tab` order. Gallery story of the titlebar (both disabled, back enabled, both enabled) with one snapshot per built-in theme. (unit + snapshot)
- **AC-11 Palette.** "Go back" and "Go forward" are listed with `⌘[` / `⌘]` and do what the keys do; with nothing to go back to, "Go back" changes nothing. (unit)
- **AC-12 No network, no leaks.** Running AC-1…AC-8 in connected mode against the mock Azure DevOps server with the review entity kept alive records zero requests from back / forward; the history holds ≤ 50 entries and its memory stays ≤ 100 KB with 50 review entries (size assertion). (mock server + unit)
- **AC-13 Clearing.** Sign-out and switching between demo and connected mode leave one entry with both buttons disabled. (unit)

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Capture of the current view state | < 0.2 ms on the UI thread, no allocation proportional to list size | Criterion bench over each view's capture with 5,000 work items / 2,000 PRs / 1,000 notifications loaded |
| `⌘[` / `⌘]` to a live view | Restored frame painted within 1 frame (< 16 ms), selection and scroll included | Frame recorder in `nav_history.rs` |
| Back to a replaced PR review | First paint per 0004 §7 open budget; restore applied in the same frame as the file list / diff | Frame recorder |
| History memory | ≤ 50 entries, ≤ 2 KB each (ids, paths, anchors; no row data) | Size assertion (AC-12) |
| Cold start | No change (< 500 ms): one entry created after the first frame | Launch timer |

Global rules (AGENTS.md) always apply: frame < 16 ms, Cmd+P < 50 ms on 100k items, cold start < 500 ms. Regressions are bugs.

## 8. Success signal (telemetry-free)

- **Dogfood:** for one working week, every PR → work item → PR and Inbox → item → Inbox detour in the builder's notes ends with `⌘[` or the mouse back button, and no note says "lost my place". The owner's 2026-10-01 note is retested verbatim (AC-1, AC-2) and closed.
- **Support signal:** beta questions of the form "how do I get back to …" stop appearing.

## 9. Risks

- **Restore correctness across six views.** Each view captures and restores its own state; a forgotten field silently loses context. Mitigation: one `ViewState` per view type with an exhaustive capture/restore pair, AC-5 per view, and the two owner scenarios as smoke tests.
- **Shadowed bindings.** A view-level `⌘[` binding wins over the workspace one in GPUI's dispatch. Mitigation: AC-9's keymap assertion fails if any context below `Workspace` binds `⌘[` / `⌘]`.
- **Surprising forward truncation.** Users may expect forward to survive a new open. That is the browser convention every user already knows; kept.
- **History noise from section switches.** If every section switch pushes, back may feel slow to reach the PR. Mitigation: sidebar walks collapse to one entry (FR-3.5); same-location opens push nothing (FR-4). The owner accepted section switches as entries (Q-2).
- **Stale selection after long absences.** The store changes while away; restoring by id keeps the place, FR-7 covers removals.

## 10. Open questions — resolved 2026-10-01

All recommendations accepted by the owner.

- **Q-1 Persist history across restarts?** **Resolved:** no, session only; launch keeps opening the first section (FR-1).
- **Q-2 Do section switches (`⌘1`…`⌘6`, sidebar clicks) create entries?** **Resolved:** yes, like a browser; sidebar walks collapse to one entry (FR-3 item 5, AC-3).
- **Q-3 `Esc` in a PR review: back when the list is the previous entry?** **Resolved:** yes; otherwise `Esc` pushes the list (FR-12, AC-8).
- **Q-4 History menu on `‹`?** **Resolved:** cut to `docs/ideas.md` (RICE ≈ 16: Reach 20 · Impact 1 · Confidence 80% · Effort 1 week) (§11).
- **Q-5 Milestone.** **Resolved:** v0.1, on the cut line between 0016 group A and pins (cut after sync activity, before pins) (`docs/roadmap.md`).
- **Q-6 Two-finger trackpad swipe?** **Resolved:** not in this spec; mouse buttons, three-finger swipes, `⌘[` / `⌘]` and the titlebar cover it (FR-11). Parked in `docs/ideas.md`; revisit if a beta user asks.

## 11. Out of scope

- History menu on the back button (Q-4, `docs/ideas.md`).
- Persisting history across restarts (Q-1).
- Two-finger trackpad swipe (Q-6, `docs/ideas.md`).
- Per-pane histories (e.g. inside the Activity drawer or the palette) and a Settings window history.
- Guided tours (spec 0020) reference this spec and create no entries.

## 12. References

- GPUI fork (`zeronsh/zui` rev `18a89af04cfe079d55275b111ac349d5640d9cda`): `crates/gpui/src/interactive.rs` (`MouseButton::Navigate`, `NavigationDirection`), `crates/gpui_macos/src/events.rs:140-236` (buttons 3 / 4 and `NSEventTypeSwipe` mapped to back / forward).
