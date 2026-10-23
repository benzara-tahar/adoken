# 0019 — PR review polish: file filter, Threads view, group reviewers, status actions, copy

| Field | Value |
| --- | --- |
| Status | Accepted (2026-10-01, owner: all recommendations; overrides noted) |
| Milestone | v0.1 (Q-8): groups C, E, D, A; group B (Threads view) on the v0.1 cut line, cut before the items already on it |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/app` (`review/pr.rs`, `review/pr/status_links.rs`, `review/pr/compose.rs`, `review/mod.rs`, `review/data.rs`, `review/web.rs`, `review/writes.rs`, `settings/`), `crates/ui` (`file_tree`, `diff`), `crates/settings` (`[review]` table), `crates/gallery` |
| Serves | **Snappy** (jump to any file or thread in a 300-file PR in a few keystrokes; copy what you need without the browser) · **Less noise** (only the files and threads that matter; policy groups out of the reviewer list) |
| Depends on | `docs/specs/0004-pr-review.md` (§3 steps 3–4, §3.1, FR-2, FR-3, FR-5, FR-7, FR-14, FR-15, FR-16, FR-18), `docs/specs/0003-command-palette.md`, `docs/specs/0009-settings.md` (FR-1, FR-3, FR-4), `docs/specs/0012-avatars.md` (group avatars), `docs/specs/0017-today-redesign.md` (Today's reviewer display, C1), `docs/specs/0018-navigation-history.md` (FR-3, FR-4, FR-5: restorable review state, C4), `docs/specs/0001-onboarding.md` (one token, C6) |
| Design | `docs/design/pr-review-polish.md` (ux-designer): tree header, filter field, mode switch, Threads view cards and excerpts, group indicator, status control, copy targets, copy table, accessibility, gallery. Context: `docs/design/principles.md` §3–§4, `docs/design/command-palette.md` §4.4, §7 |

> Rule: no code without an **Accepted** spec (AGENTS.md). The spec owns behavior, acceptance, and budgets; design docs own visuals, motion, sound, and copy.

Five groups, each shippable or cuttable alone:

- **A. File filter** (owner note line 3, first half of line 5): type to filter the file tree by path; one key to show only files with threads.
- **B. Threads view** (line 5): every thread of the PR in one list, each with its code, and one key to open it in the diff with its file selected.
- **C. Group reviewers** (line 20, contract C1): policy groups hidden by default, with a setting.
- **D. Status actions you can find** (line 21): draft, publish, abandon, reactivate from the list and the header.
- **E. Copy title and branches** (line 23).

## 1. Problem

"On a big PR I `j` through 80 files to find the one I care about. When I come back to a PR, I can't see where the discussion is without opening every file with a comment badge, and I can't read the open threads in one pass. The reviewer list at the top is full of 'React PR reviewers required', 'Copilot reviewers required', 'Service reviewers required': policy groups, not people, and they push the real reviewers out of sight. I couldn't find how to turn a PR into a draft, publish it or abandon it. And I can't copy the title or the branch name I'm looking at." The web UI has a file filter and a comments pane, but each costs a hub load; adoken has the data in memory and shows none of it this way.

## 2. Users and job to be done

- **Persona:** reviewer on large PRs (50–300 files) coming back to a PR between pushes; PR author working through feedback; anyone whose org adds required review groups by branch policy.
- **Job to be done (A):** "When a PR has many files, I want to narrow the tree to the paths or the discussed files I care about, so I get to them without scrolling or `j`-walking."
- **Job to be done (B):** "When I come back to a PR or answer feedback, I want to read every open thread with its code in one list and jump into the diff at any of them, so I can work through the discussion without opening files one by one."
- **Job to be done (C):** "When I look at who reviews a PR, I want to see people, not policy groups, so I see at a glance who approved and who I'm waiting on."
- **Job to be done (D):** "When my PR isn't ready, is ready, or is dead, I want to change its status where I already am, so I don't open the browser for a two-second change."
- **Job to be done (E):** "When I need the PR's title or branch elsewhere (chat, terminal), I want to copy it from where I see it."
- **Smallest delightful slice:** `/` `auth` `↓` opens the first auth file of a 300-file PR in one frame; `t` shows every active thread with its code, `↩` lands in the diff on it.
- **Priority (RICE, Q-8):** C = 270 (Reach 90 · Impact 1 · Confidence 90% · Effort 0.3 week). E = 200 (Reach 40 · Impact 0.5 · Confidence 100% · Effort 0.1 week). A = 144 (Reach 80 · Impact 1 · Confidence 90% · Effort 0.5 week). D = 90 (Reach 30 · Impact 1 · Confidence 90% · Effort 0.3 week; the writes exist). B = 47 (Reach 60 · Impact 2 · Confidence 70% · Effort 1.8 weeks).
- **Already in place** (verified 2026-10-01):
  - The tree is a `uniform_list` over `LoadedPr::tree` with no filter; `j`/`k` follow `LoadedPr::order` (`crates/app/src/review/pr.rs` `render_tree`, `step_file`; `review/data.rs` `LoadedPr`).
  - `LoadedPr::threads` holds the PR's file threads with at least one human comment; PR-level threads are dropped (`data.rs` `build`), so a PR-level comment posted with `⇧C` (`pr/compose.rs`, thread without anchor) shows nowhere in adoken (0004's Overview tab isn't built). `FileEntry::thread_count` counts a file's threads of any status; `n`/`p` step through the thread rows of the selected file only (`pr.rs` `step_thread`).
  - Thread cards render through `PrReview::render_thread` with reply/resolve footers (`pr.rs`, `pr/compose.rs`).
  - `Person.is_group` is mapped from the wire's `isContainer` (`crates/sync/src/wire.rs`); neither the list (`review/mod.rs` `render_reviewers`, `data::reviewer_groups`) nor the header (`pr.rs` `render_header`) checks it.
  - Publish / Mark as draft / Abandon… / Reactivate exist with confirmation, optimistic apply and revert (`pr/status_links.rs`, `review/writes.rs` `status_actions`, `apply_status_action`, `revert_status`), bound to `⇧S`/`⇧P`/`⇧A` only in the `PrReview` key context. Their `Cmd+P` commands are listed in `review/mod.rs` `palette_commands` but are handled only by an open review, so they do nothing from the list. The list's row menu (`review/mod.rs` `open_row_menu`) has no status items. The header chip is a menu trigger whose only hints are a small chevron and a hover tooltip.
  - Copy PR id and Copy link (0004 FR-14, `review/web.rs`) with a 5 s toast. The title and branches are plain text.

## 3. UX flow

Visuals and copy: `docs/design/pr-review-polish.md`. Keys checked against `docs/design/command-palette.md` §7 and every `KeyBinding::new` in `crates/app/src/**` on 2026-10-01: in the `PrReview` context `t`, `⇧T` and `/` are unbound; in the `PrList` context `⇧S`, `⇧P` and `⇧A` are unbound (`⌘⇧A` is the global Activity drawer, a different chord). `/` matches the global "Focus the list filter" convention (command-palette.md §7.2) and the Activity drawer's `/`. `⌘F` is left free for a future find-in-diff. `⌘K` stays the row action menu (`crates/app/src/pin_actions.rs`).

### 3.1 Keys added

| Key | Context | Action | `Cmd+P` command |
| --- | --- | --- | --- |
| `/` | review | Focus the file filter | "Filter files…" |
| `⌘⌫` | review | Clear the file filter and "only files with threads" | "Files: clear filter" |
| `⇧T` | review | Toggle "Only files with threads" | "Only files with threads: off → Turn on" / "…: on → Turn off" |
| `t` | review | Toggle Files / Threads | "Show threads" / "Show files" |
| — | review (Threads) | Scope | "Threads: show active", "Threads: show resolved", "Threads: show all" |
| `⇧S` `⇧P` `⇧A` | list (selected row) | Status menu / Publish or mark as draft / Abandon… | existing commands, now routed (FR-19) |
| — | list, review | Copy title, source branch, target branch | "Copy pull request title", "Copy source branch", "Copy target branch" |
| — | anywhere | Toggle group reviewers | "Group reviewers: hidden → Show" / "Group reviewers: shown → Hide" |

Single-letter keys follow 0004's rule: off while a text field, menu or sheet has the keyboard (the filter field included).

**Review tour (0020 FR-10).** The change that ships a group of this spec also turns on that group's clauses in the PR review tour (`docs/design/tours.md` §5.1, marked "with 0019"): E's step 1 "Click the title or a branch to copy it.", A's step 2 "[/] filters by path, [⇧T] keeps files with threads.", B's step 3 "[t] shows only the threads.". Tour anchors: `review.tree-filter`, `review.threads-only`, `review.mode`, `review.status` (design §1, §4). 0020 AC-7 fails if a cited key is unbound.

### 3.2 Happy path (A: file filter)

1. In a 300-file review, `/` puts the keyboard in the filter field above the tree. Type `auth`. On each keystroke the tree shows only files whose path contains "auth" and their folders; the header reads "Files 7 of 300 · 2/7 viewed". The diff doesn't change.
2. `↓` selects the first match and its diff shows; `↓`/`↑` keep moving through matches. `↩` hands the keyboard back to the review; `j`/`k`/`v` now move only through the 7 files.
3. `auth -test` hides the test files. `Esc` in the field clears the text; `Esc` again leaves the field. `⌘⌫` in the review clears everything.
4. `⇧T` (or the toggle under the field) shows only files with active threads. It combines with the text.

### 3.3 Happy path (B: Threads view)

1. `t` (or the **Threads 8** segment in the diff pane's bar) replaces the diff with the Threads view: every active thread in the PR, grouped by file in tree order, each card showing a few lines of code and the full conversation. The tree keeps its filter and shows only files with threads in scope; its selection follows the focused card.
2. `n`/`p` (or `↓`/`↑`) move card to card across files; `j`/`k` jump to the next/previous file's first card. `r` replies, `⇧R` resolves, as inline.
3. `↩` on a card (or **Open in diff**) shows Files with that file selected and the diff scrolled to the thread, focused, so `r` replies there. `t` alone goes back to Files on the focused card's file.
4. The scope **Active 8 · Resolved 12 · All 20** switches what's listed.

### 3.4 Alternate paths

- **C:** reviewer lists show people only. A required group that hasn't voted shows as "Waiting on 1 required group" in the header and a dimmed group count on the list row. Settings › General › Pull requests › "Show group reviewers" (or `Cmd+P` › "Group reviewers: hidden → Show") brings them back.
- **D:** on the list, `⌘K` or a right-click on a row offers Publish / Mark as draft / Abandon… / Reactivate (whichever apply); `⇧P`/`⇧A`/`⇧S` work on the selected row; the `Cmd+P` commands too. In the review, the status control shows its `⇧S` keycap, and a draft shows a **Publish** button.
- **E:** click the header title: "Copied title". Click the source branch: "Copied feature/retry". Also `Cmd+P` and the row menu.

### States

| State | Behavior |
| --- | --- |
| Empty | A: "No files match "x"" + Clear filter `⌘⌫`; threads-only with none: "No files with active threads" + Show all files `⇧T`. B: "No comments on this pull request yet"; Active empty with resolved ones: "No active threads" + Show all. |
| Loading / first sync | File list loading (0004 States): filter field and toggle stay usable and apply when the list arrives. B: excerpts show placeholder rows of their final height until the file's diff is computed. |
| Error | A file whose blobs couldn't be fetched: its excerpt says so with Retry `⇧L` (0004). D: a refused status change reverts with Azure DevOps' reason (header notice in the review, toast in the list). |
| Offline / stale | A, B, C, E work from memory. Excerpts of uncached files say "Offline" instead of loading. D: disabled with "Offline — reconnect to send" (principles §3.5). |
| Throttled by ADO | Excerpt loads wait behind the governor and show 0004's wait reason after 3 s; nothing else is affected. |
| Read-only token | A, B (no reply/resolve), C, E unchanged. D: no status items, no Publish button, commands disabled with the read-only reason. |

## 4. Functional requirements

### 4.1 Group A: file filter

- **FR-1 Filter field.** The tree header gains a filter field (always visible above the tree list). `/` in the review, a click, or `Cmd+P` › "Filter files…" focuses it. While it has the keyboard, the review's single-letter keys are off.
- **FR-2 Matching.** The query is split on whitespace into terms. A file is listed when its full display path (`FileEntry::path`) contains every plain term and none of the `-`-prefixed terms (a lone `-` is ignored), case-insensitive, ASCII and Unicode simple case folding. An empty query lists every file. Matching runs on the UI thread over the in-memory list, never a store query, and never changes the selection (no load per keystroke). The matched ranges of plain terms are highlighted in file and folder names (Q-1).
- **FR-3 Tree while filtered.** Folders are listed when they contain a listed file. The header reads "Files {listed} of {total}"; the viewed count counts listed files only ("2/7 viewed"), with the PR-wide count in its tooltip. The tree's comment badge is unchanged (all human threads, 0004).
- **FR-4 Navigation.** `j`/`k` and `v`'s advance (0004 §3 step 3) move through listed files in tree order. If the selected file isn't listed, its diff stays on screen, no tree row is selected, and `j`/`k` go to the first listed file after/before its tree position. In the field, `↓`/`↑` select the next/previous listed file (its diff loads as on `j`/`k`), `↩` selects the first listed file when the selection isn't listed and returns the keyboard to the review, `Esc` clears non-empty text, `Esc` on empty text returns the keyboard to the review. `⌘⌫` in the review or field clears the text and turns FR-5's toggle off.
- **FR-5 Only files with threads.** A toggle under the field (`⇧T`, `Cmd+P`) lists only files with at least one thread in the current thread status scope (FR-9; default Active). It combines with the text (AND). In Threads mode it is forced on and disabled.
- **FR-6 Lifetime.** Filter text, toggle, Files/Threads mode and thread scope belong to the open review: kept while it's open (including across a new push, re-applied to the new file list), cleared when the review closes. They are part of the review's restorable state for back/forward (0018 FR-5).

### 4.2 Group B: Threads view

- **FR-7 Mode.** The diff pane's bar starts with a two-state switch **Files | Threads {n}**, `n` = threads in the current scope. `t` or `Cmd+P` › "Show threads" / "Show files" toggles it. Threads mode replaces the diff pane only; the header and tree stay.
- **FR-8 Contents and order.** One card per human thread (at least one non-system, non-deleted comment, as `data::build`), grouped:
  1. **Pull request** (threads without a file anchor), newest activity first (Q-6);
  2. one group per file in tree order (`LoadedPr::order`), listing only files that pass the tree filter (FR-2); within a file, threads by anchored line (right side line, else left), then its "Comments on older code" threads (0004 FR-5) by original line;
  3. **Files no longer in this pull request** (anchored to a path absent from the file list), by path, then line.
  Ordering is fixed (Q-5).
- **FR-9 Status scope.** **Active** (status active, pending, or unknown), **Resolved** (fixed, won't fix, closed, by design), **All**; default Active; each label carries its count. The scope applies to the list and to FR-5. A thread resolved or reactivated from this view keeps its card in place (new status shown) until the scope changes, the view is left, or the review reloads; counts update at once. Threads that arrive or change by sync while the view is open are inserted or updated in place; the focused card keeps its screen position.
- **FR-10 Excerpt.** Each file card shows the code its thread is anchored to as read-only unified diff rows of the displayed iteration (whatever the diff layout): the last `min(anchor lines, 9) + 3` unified rows ending at the anchor's last line (right side line for right anchors, left side for left-only anchors), so the excerpt's height is known before the diff is. Anchored lines are marked (design §2.2); syntax highlighting as in the diff. Without an excerpt: older-code threads ("Code from iteration N isn't shown here."), collapsed files (generated, binary, large: no blob request, 0004 AC-12), unavailable files (reason + Retry `⇧L`), offline uncached files, and files no longer in the PR.
- **FR-11 Excerpt loading.** An excerpt needs its file's diff (0004 FR-1). Files whose cards are visible, or within one screen below, for ≥ 150 ms are queued for diff computation at user priority through the existing path (`PrReview::compute`: cached blobs first, missing blobs fetched, max 4 in flight, FR-15's order: the review's selected file before these). Scrolling past cards faster than that queues nothing. A diff computed here is the same diff Files mode uses: nothing is computed or fetched twice (0004 FR-2). (follow-up: 0024)
- **FR-12 Keys and actions in Threads mode.** `n`/`↓` and `p`/`↑`: next/previous card across groups (no wrap); `j`/`k`: first card of the next/previous group; `Home`/`End`: first/last card; `↩`: Open in diff (FR-13); `r`: reply; `⇧R`: resolve/reactivate; `t`: back to Files on the focused card's file at its last diff position. A click on a tree file scrolls the list to that file's group and focuses its first card. The tree selection follows the focused card's file without changing Files mode's diff state until Files mode shows. `v`, `c`, `Enter` (expand) and the line cursor are Files-only. With a read-only token, reply and resolve are absent (0004 AC-11).
- **FR-13 Open in diff, and `n`/`p` across files.**
  - `↩` (or the card's / group's Open in diff) switches to Files, selects the thread's file (scrolling the tree to it, clearing the text filter only if it hides that file), scrolls the diff so the thread card is fully visible with its anchored line on screen, and sets the thread cursor on it so `r`/`⇧R` act on it. If the diff isn't computed yet, it does so when the diff arrives, unless the user moved (selection, scroll, keys) meanwhile. Not offered for PR-level threads and files no longer in the PR. Back from there returns to the Threads view on the same card (0018 FR-3).
  - In Files mode (per Q-3), `n` on the selected file's last thread (or with none after the cursor) selects the next listed file that has a thread in scope and lands on its first thread; `p` symmetrically lands on the previous such file's last thread. Within a file, `n`/`p` are unchanged.

### 4.3 Group C: group reviewers (contract C1)

- **FR-14 Setting.** `[review] show_group_reviewers` in `settings.toml` (bool, default `false`; 0009 FR-1 lists it). Settings › General › **Pull requests** has the switch "Show group reviewers" (copy in design §3; searchable by group, team, policy, reviewers, required). `Cmd+P` › "Group reviewers: hidden → Show" / "Group reviewers: shown → Hide" writes the same key. Changes apply on the next frame and on external edits within one poll (0009 FR-3, FR-4).
- **FR-15 Display.** A reviewer is a group when its identity's `Person.is_group` is true (Azure DevOps `isContainer`); an identity missing from the people map counts as a person. While the setting is `false`, groups are not drawn in: the list row's reviewer avatars (`render_reviewers`), the review header's reviewer row (`render_header`), and Today's PR reviewer lists (0017 renders it with this rule). The palette preview and pins follow the same rule wherever they draw reviewer names or avatars.
- **FR-16 Pending required groups.** While groups are hidden and at least one hidden group is required (`is_required`), not declined and has no vote, the list row ends its reviewers with a dimmed group glyph and the count, and the header's reviewer row ends with the chip "Waiting on N required group(s)"; both carry the groups' names in a tooltip and accessible label. Optional groups and groups that have voted leave no trace (Q-4).
- **FR-17 Display only.** Hiding never changes any computation: tab membership (To review), vote grouping and counts used for stages, approval or policy state, Complete's blocker, pins' headline (`crates/app/src/pins/status.rs`), Today's stage bars and nudges all count hidden groups exactly as before.

### 4.4 Group D: status actions you can find

- **FR-18 List.** The PR list's row action menu (`⌘K`, right-click) adds, after Pin, the status changes valid for that row now (`review::writes::status_actions`): Publish or Mark as draft (`⇧P`), Abandon… (`⇧A`), Reactivate. Invalid ones are absent; read-only tokens and completed or completing PRs get none. In the list, `⇧S` opens the same status menu as the review's, anchored to the selected row's status chip; `⇧P` and `⇧A` act on the selected row.
- **FR-19 Commands.** "Change pull request status…", "Publish or mark as draft", "Abandon pull request…" and "Reactivate pull request" act on the open review, else, while the Pull requests list is showing, on its selected row. Anywhere else, or with nothing selected, they are listed disabled with "Select a pull request first"; with a read-only token, disabled with the read-only reason (command-palette.md §4.3).
- **FR-20 Header affordance.** The review's status control shows its `⇧S` keycap at all times, not only in the tooltip. A draft with write access gets a **Publish** button (`⇧P`) at the start of the actions row.
- **FR-21 Same behavior everywhere.** Every path runs 0004 FR-16's single implementation: only valid changes, Abandon always asks first (in the list: the same confirmation dialog over the list), `PATCH …/pullrequests/{id}` (`isDraft`, `status`), the row and (if open) the review show the change on the next frame, Azure DevOps' answer replaces it, a refusal reverts it and names the reason (a 403 names the Contribute to pull requests permission). One status change per PR in flight. Feedback in the list: the row's chip shows "Sending…" until confirmed; a refusal shows the toast "Status of !{id} not changed. {reason}". No "Renew token" prompt (C6).

### 4.5 Group E: copy title and branches

- **FR-22 Title.** Clicking the header's title copies the PR title with leading and trailing whitespace trimmed, nothing else; toast "Copied title". `Cmd+P` › "Copy pull request title" and the row menu's "Copy title" copy the open review's, else the list's selected row's.
- **FR-23 Branches.** Clicking the source or the target branch name in the header copies that branch's name as displayed: the ref without `refs/heads/` (`adoken_model::short_branch`); a ref outside `refs/heads/` is copied whole. Toast "Copied {name}". `Cmd+P` › "Copy source branch" / "Copy target branch" and the row menu's "Copy source branch" do the same for the open review, else the selected row.
- **FR-24 Copy mechanics.** Same as 0004 FR-14: system clipboard, the 5 s toast without motion, the optional `ui.copy` sound; works in the demo org. No new keyboard shortcuts (Q-7).

## 5. ADO API surface and limits

| Purpose | Endpoint (7.1) | Scope | Cost / limit notes |
| --- | --- | --- | --- |
| A, C, E | **None.** In-memory data (`LoadedPr`, list, people map with `isContainer` already synced). | — | 0 requests. |
| B excerpts | `GET …/git/repositories/{repo}/blobs/{objectId}` (existing, 0004) | `vso.code` | Only for files with in-scope threads whose cards dwell on screen (FR-11); ≤ 2 per file, ever (content-addressed cache); user priority, ≤ 4 in flight. No new endpoint. |
| B PR-level threads | none (already synced with the PR's threads, 0002) | — | 0 requests. |
| D from the list | `PATCH …/pullrequests/{id}` (existing, 0004 FR-16) | `vso.code_write` | One per user action. |

- **Rate limits:** A, C, E add nothing. B's worst case is the whole Threads view of the L+T fixture read top to bottom: 150 files with threads → ≤ 300 blob GETs, the same a full review of those files costs (0004 §5), spread over the reading time and admitted by the governor's user lane. The dwell rule keeps a fast scroll from bursting into the self-limit (300 requests / 5 min, 0016 FR-8 state 5).
- **No push to local apps:** threads and statuses are as fresh as the hot tier (≤ 60 s foreground, 0002) plus the review's own fetches.
- **PAT scopes:** `vso.code` for A–C and E; `vso.code_write` for D (already in the Review scope set, 0001 §5.2). With one token minted with every shipped scope (C6), a refusal names Azure DevOps' reason and offers no renew prompt.

## 6. Acceptance criteria

Harness: 0004's mock ADO server and gallery `TestAppContext` view tests with real key dispatch, injected clock, frame recorder (`crates/gallery/examples/review_frames.rs` style). Fixtures:

- **S** (5 files): `docs/AUTH.md` (1 pending thread), `src/auth/token.rs` (2 active: lines 10 and 40–42), `src/auth/token_test.rs` (1 fixed), `src/pay/ledger.rs` (1 active on deleted older code), `src/pay/retry.rs` (none); plus 1 active PR-level thread and 1 active thread on `src/old.rs`, which isn't in the file list. Active = 6, Resolved = 1, All = 7.
- **L+T:** 0004 §6's large fixture (300 files / 40k changed lines) with 500 human threads on 150 files (380 active, 20 pending, 100 resolved; 30 on older code; 10 on paths not in the PR), 25 PR-level threads, 60 system threads.

`[A]`…`[E]` = group; each group's ACs pass with the other groups absent.

**A. File filter**

- **AC-A1 Matching** (unit, pure function from tree + query + toggle + scope to listed rows). On S (tree order `docs/AUTH.md`, `src/auth/token.rs`, `src/auth/token_test.rs`, `src/pay/ledger.rs`, `src/pay/retry.rs`, `crates/ui/src/file_tree.rs` `build`): `auth` lists `docs/AUTH.md`, `src/auth/token.rs`, `src/auth/token_test.rs` and the folder rows `docs`, `src`, `auth` (not `pay`); `auth -test` drops `token_test.rs`; `AUTH` equals `auth`; a blank query and `-` list all 5; `zzz` lists none. Highlight ranges cover `auth` in the `auth` folder row and `AUTH` in `AUTH.md`.
- **AC-A2 Keyboard** (view test). In the review on S with `token.rs` viewed: `/` focuses the field; typing `jkv` puts "jkv" in it and moves nothing; after replacing it with `auth` the header reads "Files 3 of 5" and "1/3 viewed"; `↓` selects the first listed file and its diff loads; `↩` returns the keyboard, then `j` moves only among the 3; `/` `Esc` clears the text, `Esc` again returns the keyboard; `⌘⌫` with text and toggle on clears both.
- **AC-A3 Selection outside the filter** (view test). With `src/pay/retry.rs` selected, typing `auth`: no new diff is computed or requested (mock: 0 requests), no tree row is selected, the diff still shows `retry.rs`; `k` selects `src/auth/token_test.rs` (the last listed file before it). `v` on the last listed file marks it viewed and keeps the selection.
- **AC-A4 Threads only** (unit + view test). On S with `⇧T`: Active lists `docs/AUTH.md`, `src/auth/token.rs`, `src/pay/ledger.rs`; with scope All, also `src/auth/token_test.rs`; with `auth` typed, Active lists `AUTH.md` and `token.rs`. In Threads mode the toggle is on and disabled; leaving Threads restores its previous state. Empty states and their buttons appear as in §3 States and clear as labeled.
- **AC-A5 Lifetime** (view test). With `auth` and the toggle on, a push that adds `src/auth/session.rs` shows it listed after the reload, filter intact; back to the list and reopening the PR shows an empty filter and the toggle off.
- **AC-A6 Performance** (bench + frame recorder). On L+T, each keystroke of `src/auth -test` repaints the tree within one frame (< 16 ms frame time); the matching function takes < 1 ms at 300 files and < 5 ms at 5,000 (criterion bench).

**B. Threads view**

- **AC-B1 Contents** (view test). On S, `t` shows groups in order: Pull request (1 card), then the file groups in `LoadedPr::order` that have active threads (`AUTH.md` 1, `token.rs` 2 in line order 10 then 40–42, `ledger.rs` 1 under its older-code label), then "Files no longer in this pull request" (`src/old.rs` 1); the switch reads "Threads 6"; scope reads "Active 6 · Resolved 1 · All 7". `t` again shows Files.
- **AC-B2 Scope** (view test). Resolved shows only `token_test.rs`'s card; All shows 7. In Active, `⇧R` on `token.rs` line 10 sends the resolve (mock), the card stays with its Fixed chip, counts read "Active 5 · Resolved 2"; switching scope and back removes it from Active.
- **AC-B3 Excerpt** (unit on the excerpt function + snapshot). For the right anchor 40–42 with a computed diff: exactly 6 unified rows ending at new line 42, anchored rows 40–42 marked; a 20-line anchor yields 12 rows; a left-only anchor on deleted line 7 ends at old line 7. Before the diff is computed the card is the same height (layout assertion before/after: 0 px change). `ledger.rs`'s older-code card, a generated file's card and `old.rs`'s card show their notes and cause 0 blob requests.
- **AC-B4 Excerpt loading** (mock server, injected clock). On L+T with no blobs cached: opening the view and staying 150 ms requests blobs only for files whose cards are on screen or within one screen below (each file's ≤ 2 blobs, ≤ 4 in flight); scrolling from top to bottom in 1 s (cards on screen < 150 ms each) requests nothing more; reopening after all excerpts loaded requests 0 blobs; offline shows "Offline" in place of uncached excerpts and requests nothing. (follow-up: 0024)
- **AC-B5 Keys** (view test). On S in Threads: `n` ×3 from the first card lands on `token.rs` 40–42; `p` goes back; `j` from the Pull request card lands on `AUTH.md`'s card; `End` lands on `old.rs`; the tree selection follows (`token.rs` highlighted when its card is focused); clicking `ledger.rs` in the tree focuses its card; `r` opens the reply composer in the focused card; `v` and `c` do nothing. Read-only: no reply/resolve controls, `r` does nothing.
- **AC-B6 Open in diff** (view test + mock). `↩` on `token.rs` 40–42 with its diff computed: in the same frame Files shows, `token.rs` is selected and in view in the tree, the thread row is within the diff's visible range and is the thread cursor; `r` replies to that thread. With the diff uncached: the same holds once the diff applies; if `j` was pressed meanwhile, nothing jumps. With `pay` typed in the filter, `↩` on a `token.rs` card clears the text so the file is listed. The Pull request and `old.rs` cards offer no Open in diff and `↩` does nothing.
- **AC-B7 `n`/`p` across files** (view test, per Q-3). In Files on S with `AUTH.md` selected and its thread the cursor, `n` selects `token.rs` and lands on line 10; `n` → 40–42; `n` → `ledger.rs`'s older-code thread; `n` at the last does nothing; `p` from `token.rs` line 10 lands on `AUTH.md`'s thread.
- **AC-B8 Live updates** (view test). With `token.rs` 40–42 focused and on screen, a sync adding a thread on `AUTH.md` inserts its card above without moving the focused card's screen position (± 1 px) and updates counts.
- **AC-B9 Performance** (frame recorder + heap). On L+T: `t` paints the view within one frame; scrolling top to bottom at maximum trackpad velocity keeps frame p99 < 16 ms; `↩` on a card whose diff is computed paints Files within one frame; the view adds < 10 MB RSS.

**C. Group reviewers**

- **AC-C1 Display** (view tests + snapshot). A PR with reviewers Dana (person, approved), `[Contoso]\React PR reviewers` (group, required, no vote), `[Contoso]\Copilot reviewers` (group, optional, approved), Tom (absent from the people map, no vote, required). Setting `false`: the list row shows Dana and Tom and a dimmed group count "1" with tooltip "Waiting on 1 required group: React PR reviewers"; the header lists Dana and Tom and the chip "Waiting on 1 required group". Setting `true`: both groups appear in both places, no count, no chip.
- **AC-C2 Indicator rules** (unit). The pending set is empty when the only hidden group is optional, has voted (any non-zero vote), or has declined; it counts 2 for two required no-vote groups ("Waiting on 2 required groups").
- **AC-C3 Display only** (unit). For a fixture of 50 PRs with groups in every vote state, the To review tab's rows, `reviewer_groups`' vote counts, the pins headline, Complete's blocker and Today's stage are identical with the setting `true` and `false`.
- **AC-C4 Setting** (unit + view test). The Settings switch and the `Cmd+P` command each write `show_group_reviewers` and the list repaints on the next frame; an external edit of `settings.toml` applies within one poll; a missing key reads `false`.
- **AC-C5 Real org** (scripted manual check). On the owner's org, a PR whose required group was approved by a member shows that member as a reviewer with the setting `false`; recorded in §13 with the payload's `votedFor` (sanitized as 0014 §5).

**D. Status actions**

- **AC-D1 Menu** (view test). On the list: an active PR's row menu lists exactly Open, Open in browser, Copy link, Copy PR id, Copy title, Copy source branch, Pin, Mark as draft, Abandon…; a draft's lists Publish instead of Mark as draft; an abandoned PR's lists Reactivate only among status items; a completed or completing PR's and a read-only token's list none.
- **AC-D2 List keys and writes** (mock server). With an active row selected, `⇧P`: the row chip reads Draft with "Sending…" on the next frame; the mock receives `PATCH` with `isDraft: true`; on 200 "Sending…" goes. `⇧P` again publishes. On a 403 the chip reverts and the toast reads "Status of !{id} not changed." plus the reason naming Contribute to pull requests. `⇧A` shows the confirmation; `Esc` sends nothing; `⇧A` `↩` sends `status: abandoned`. `⇧S` opens the status menu at the row. A second `⇧P` while one is in flight sends nothing.
- **AC-D3 Commands** (view test). With the list showing and a row selected, `Cmd+P` › "Publish or mark as draft" does what `⇧P` does; from Today the four status commands are listed disabled with "Select a pull request first"; with a read-only token, with the read-only reason.
- **AC-D4 Header** (snapshot + view test). The status control's `⇧S` keycap is in the unhovered snapshot; a draft with write access shows Publish first in the actions row and `⇧P` or a click publishes it; read-only shows neither.
- **AC-D5 One truth** (view test). Mark as draft from the list, then open the PR: the review shows Draft; Abandon from the review, then back: the row shows Abandoned.

**E. Copy**

- **AC-E1 Title** (view test, clipboard read back). Title `"  Fix auth regression "`: a click on the header title puts `Fix auth regression` on the clipboard and shows "Copied title"; the same from `Cmd+P` in the review, and from the list's row menu and `Cmd+P` for the selected row.
- **AC-E2 Branches.** Source `refs/heads/feature/fix-auth` → `feature/fix-auth`, toast "Copied feature/fix-auth"; target `refs/heads/main` → `main`; a source `refs/pull/12/merge` copies `refs/pull/12/merge`. `Cmd+P` › "Copy target branch" from the list copies the selected row's target.

**Shared**

- **AC-S1 Gallery** (snapshot test). Every story in design §8, one snapshot per built-in theme.
- **AC-S2 Smoke** (scripted run, `crates/gallery/tests/review_polish_smoke.rs`). The real shell on the demo store with an L+T-shaped demo PR, real key dispatch: `/` `auth` `↓` `↩` `j`, `⇧T`, `t`, `n` ×5, `↩`, `r` `Esc`, back to the list, `⌘K` › Mark as draft, `Cmd+P` › "Copy source branch"; frame p99 < 16 ms throughout; screenshots in `target/review-polish-smoke/`.
- **AC-S3 Keymap** (unit). No two bindings in `PrReview`, `PrList` or their parent contexts share a keystroke, and none shadows `⌘K` (`pin_actions`), `⌘[`/`⌘]`, `⌘⇧A` or `⌘P`.

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Filter keystroke → tree repainted (300 files) | ≤ 1 frame (< 16 ms); match < 1 ms | Frame recorder + criterion (AC-A6) |
| Filter match at 5,000 files | < 5 ms | Criterion |
| `⇧T`, scope change | ≤ 1 frame | Frame recorder |
| `t` → Threads view painted (L+T) | ≤ 1 frame (follow-up: 0024) | Frame recorder (AC-B9) |
| Scroll Threads view (L+T) | Frame p99 < 16 ms; only visible cards laid out | Frame recorder |
| `↩` from a card, diff computed | ≤ 1 frame to Files with the thread visible | Frame recorder (AC-B6) |
| `↩` from a card, uncached file, 80 ms RTT | 0004's "next file uncached" < 300 ms | Mock latency |
| Excerpt requests | Only dwelled cards' files; ≤ 2 blobs per file ever; ≤ 4 in flight | Mock request counter (AC-B4) |
| Memory added by the Threads view (L+T) | < 10 MB RSS | Heap profile |
| Group filtering in list rows | No extra allocation per row per frame; < 0.1 ms per row | Bench over 2,000 rows |
| Cold start | No change (< 500 ms): nothing loads before a review opens | Launch timer |

Global rules (AGENTS.md) always apply: frame < 16 ms, Cmd+P < 50 ms on 100k items, cold start < 500 ms. Regressions are bugs.

## 8. Success signal (telemetry-free)

- **Dogfood:** in the owner's next notes, none of lines 3, 5, 20–21 or 23 comes back; over a week, the builder works through returning-to-a-PR sessions with `t` instead of `j`-walking, noted as "used Threads view" lines in the dogfood log.
- **Public:** a build-in-public clip "300-file PR: `/auth` `↩`, `t`, `↩`" gets "how do I get this in the web UI?" replies, not "where is X?".
- **Support signal:** beta questions "how do I abandon / make it a draft in adoken?" and "what are these 'reviewers required' badges?" don't appear.

## 9. Risks

- **Keymap creep in the review.** Three new letters (`/`, `t`, `⇧T`). Mitigations: every one is also a visible control with its keycap and a `Cmd+P` command; AC-S3 guards conflicts; no shortcuts for copy (Q-7).
- **Excerpt fetch bursts.** A Threads view over 150 discussed files could fan out blob requests. Mitigations: 150 ms dwell, one-screen look-ahead, user lane ≤ 4 in flight, content-addressed cache, no fetch for collapsed files (FR-11, AC-B4).
- **Group detection.** Relies on `isContainer` in reviewer payloads (`crates/sync/src/wire.rs`). An identity missing from the people map shows as a person until synced (FR-15), the safe side. Whether a member's vote on behalf of a group always surfaces the member as a reviewer is [INFERENCE] from the `votedFor` field's documentation; AC-C5 verifies it on a real org.
- **Hidden blocker.** Hiding a required group that hasn't voted could make a blocked PR look ready. Mitigation: FR-16's indicator and FR-17 (no computation changes).
- **Two paths for status writes.** The list and the review could drift. Mitigation: FR-21 requires one implementation; AC-D5.
- **Scope creep** toward a comment search, thread sorting, unread tracking per thread, or "changes since my last review". All stay out (§11); the last is v0.2's first item.

## 10. Open questions — resolved 2026-10-01

All recommendations accepted by the owner.

- **Q-1 Path matching.** **Resolved:** substring terms, space = AND, `-term` excludes (FR-2). Fuzzy matching waits in `docs/ideas.md` until asked.
- **Q-2 Default thread scope.** **Resolved:** Active, with pending and unknown counted as active (FR-9).
- **Q-3 `n`/`p` across files in Files mode.** **Resolved:** continue to the next/previous file with an in-scope thread (FR-13, AC-B7).
- **Q-4 What stays visible of hidden groups.** **Resolved:** (c), only required groups that haven't voted, as a dimmed count in the list and "Waiting on N required groups" in the header (FR-16, AC-C1, AC-C2).
- **Q-5 Threads view order.** **Resolved:** file order only in v0.1 (FR-8); "recent first" waits in `docs/ideas.md` for real demand.
- **Q-6 PR-level threads in the Threads view.** **Resolved:** included as the first group, "Pull request", with reply and resolve and no excerpt (FR-8, AC-B1).
- **Q-7 Shortcuts for copy title / branches.** **Resolved:** none; click, `Cmd+P` and the row menu (FR-24).
- **Q-8 Milestone.** **Resolved:** v0.1 for C (RICE 270), E (200), A (144) and D (90); B (47, 1.8 weeks) in v0.1 on the cut line, cut before the items that were already there (`docs/roadmap.md`).

## 11. Out of scope

- Searching comment text, per-thread unread state, sorting threads by activity (Q-5), fuzzy path matching (Q-1) → `docs/ideas.md` if asked.
- Changes since my last review / iteration compare → v0.2 item 1 (`docs/roadmap.md`).
- Excerpts of older iterations' code (needs older blobs and iteration mapping) → with iteration compare.
- Collapsible folders in the tree, file-type facet chips, saved filters → cut: the text filter covers them.
- Adding or removing reviewers, editing title or description (0004 §11).
- `git checkout` command copy (command-palette.md §4.4 lists it; not asked) → stays with the palette's ⌘K actions.
- A separate Settings tab for pull requests: one switch lives in General › Pull requests.

## 12. References

- Pull Request Threads — List (thread `status` values, `threadContext`): https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-threads/list?view=azure-devops-rest-7.1
- Pull Requests — Update (`isDraft`, `status`): https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/update?view=azure-devops-rest-7.1
- Pull Request Reviewers — List (`IdentityRefWithVote`: `isContainer`, `isRequired`, `hasDeclined`, `votedFor`): https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-reviewers/list?view=azure-devops-rest-7.1
- Blobs — Get Blob: https://learn.microsoft.com/en-us/rest/api/azure/devops/git/blobs/get-blob?view=azure-devops-rest-7.1
- Rate limits: https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/rate-limits?view=azure-devops
