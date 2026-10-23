# 0017 — Today redesign: day summary with a mood, readable PR rows, a side column for what's next

| Field | Value |
| --- | --- |
| Status | Accepted (2026-10-01, owner: all recommendations; overrides noted) |
| Milestone | v0.1 (Q-8). Replaces the Today layout of 0006 before the public beta; nudge logic is untouched. Only the crossfade (FR-29) and the narrow layout's polish (FR-24) sit on the v0.1 cut line |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/nudge` (day summary, mood, per-PR activity, store reads), `crates/store` (one migration: commit counts per iteration and `work_item.state_since_ms`), `crates/sync` (`includeCommits` on the existing iterations request; `state_since` from the already-fetched `StateChangeDate`), `crates/app` (Today view, illustration loading), `crates/ui` (stage bar, new Lucide glyphs), `crates/gallery`, `assets/images/today/` (8 mood illustrations, already embedded) |
| Serves | **Less noise** (one honest sentence and a picture answer "how is my day going?"; full PR titles, one next action, nothing duplicated) · **Snappy** (precomputed off the UI thread, zero added requests, the art never blocks a frame) |
| Depends on | `docs/specs/0006-smart-nudges.md` (Focus choice FR-10, Waiting on you FR-11, stage and blockers FR-12, triage, AC-9), `docs/specs/0002-sync.md` (hot set, freshness), `docs/specs/0004-pr-review.md` (FR-1 iterations request), `docs/specs/0016-statusbar-activity.md` (FR-10 per-lane last success), `docs/specs/0019` (C1 `[review] show_group_reviewers`), `docs/specs/0018` (back/forward, C4), `docs/specs/0020` (tour anchors), `docs/specs/0022` (celebration effects, C3), `docs/specs/0014-pins.md` (`⌘D` on a Today row) |
| Design | `docs/design/today.md` (rewritten for this spec: layout, tokens, copy, motion, illustrations, gallery). Assets: `assets/images/today/*.webp`, served as `adoken_ui::images::TODAY_*` (C2); originals in `output/today-redesign-2026-10-01/images/01..08-*.png` (owner-generated, `assets.json` checksums) |

> Rule: no code without an **Accepted** spec (AGENTS.md). The spec owns behavior, acceptance, and budgets; design docs own visuals, motion, sound, and copy.

## 1. Problem

The owner's words (dogfood notes, 2026-10-01): "the progress bar displayed for the PRs is not pretty at all. For one, the PR title is not displayed entirely because the progress bar takes a lot of space… It's very thin… the thin progress bar with a blue circle is very, very ugly. It also contains the list of policy groups." And: "Currently, it's just text, numbers, statuses, and progress bars." Today shows what happened as a timeline of dots that has to be decoded, truncates the thing you recognize a PR by (its title), lists group reviewers like "React PR reviewers required" as if they were people, and doesn't say what changed on a PR today, which is what matters ("the comments on this PR and also the commits"). Nothing tells you at a glance whether today went well, is stuck, or hasn't started.

## 2. Users and job to be done

- **Persona:** developer on Azure DevOps who opens adoken in the morning and between tasks; author of 1–10 open PRs and reviewer of others.
- **Job to be done:** "When I open adoken, I want to see in one glance how my day is going and what changed on my PRs today, so I can pick the next thing without reading lists."
- **Smallest delightful slice:** a day-summary card (honest headline, one sentence, a few counts, a mood illustration that changes visibly with the day) above full-title PR rows with today's commits and comments; Up next, Waiting on you and Loose ends beside them.
- **Priority (RICE):** Reach 100 (every user, every day; Today is `⌘1` and the first screen) · Impact 2 · Confidence 80% · Effort 2 weeks → **80**, above 0016 A (32) and 0014 pins. Slot and cut order: Q-8.
- **Cut from the notes and the reference (and why):** the "Recently updated" sort dropdown in `references/today-layout.png` (the owner fixed the order; a control nobody needs is noise, Q-7); the footer "All activity shown for today" (says nothing); a sync label in the header (the statusbar owns sync state, 0016; Today shows a staleness label only when data is old); confetti or any effect on Today (0022 owns effects, C3).
- **Already in place:** `adoken_nudge::today` (`crates/nudge/src/today.rs:424`) builds a `Day` (Focus, waiting cards, tracks, loose ends, ribbon) on a background executor after each store commit (debounced 500 ms) and every 5 min (`crates/app/src/today/mod.rs:73-75`), from one `Snapshot` (`crates/nudge/src/store.rs:38`). Today's window is local midnight (`WorkingHours::day_start`, `crates/nudge/src/hours.rs:97`). `Snapshot.merged_today`, my votes' `voted_at`, `Track.updated`, `Track.blocker`, `Person.is_group`, `work_item.terminal_since_ms` (migration 0008) and hot-set threads with comments already exist. Commit counts and per-PR activity do not.

## 3. UX flow

Visuals, copy, tokens and motion: `docs/design/today.md`. Keys were checked against `crates/app/src/today/mod.rs:77-95` (all kept) and `crates/app/src/pin_actions.rs` (`⌘K` row actions, not bound on Today; untouched).

### 3.1 Happy path

1. `⌘1` (or the sidebar) opens Today. The header reads "Today" and the date ("Thursday, October 1").
2. The **day-summary card** spans the width: on the left a headline ("Something shipped"), one honest sentence ("1 PR merged today · 1 conflict to resolve") and counts ("1 merged · 3 reviews · 5 commits"); on the right the mood illustration. The card has the same size and arrangement in every mood.
3. Below it, two columns. **Your PRs** (left): one row per active PR of mine and each of mine merged today, most recently active first. Each row: the full title on its own line, a compact segmented stage bar with its state ("In review", "Changes requested", "Merged today" with a static sparkles glyph on a green row), reviewer faces, and a line of today's activity ("!66231 · 2 commits today · 1 new comment").
4. The right column: **Up next** (the one next action, 0006 FR-10, with its button "Review now ↩"), **Waiting on you** (reviews I owe with how long they've waited), **Loose ends** (my in-progress items a nudge flags, with their age).
5. `j`/`k` move within a column, `h`/`l` switch columns, `↩` opens the selected PR review, work item or failing run; `s` / `d` / `x` triage the selected row's nudge; `e` hides Up next until tomorrow. `⌘[` after opening something returns to Today with the same row selected (0018).

### 3.2 Alternate paths

- **Narrow window** (Today's own width < 960 px, e.g. 900 px window with the sidebar expanded): one column in the order summary, Up next, Your PRs, Waiting on you, Loose ends. Keys: `j`/`k` and `h`/`l` both walk that single list.
- **Mixed day:** a merge and a conflict show the shipped art and both facts in the sentence; a blocker never replaces an accomplishment (FR-8).
- **Group reviewers:** hidden by default (C1); Settings › Pull requests › "Show group reviewers" brings them back as initials after the people.
- **Parked rows:** a snoozed / done / muted row stays in place, dimmed with its glyph (0006 FR-8); it no longer drives the mood.

### States

| State | Behavior |
| --- | --- |
| Loading (first open, before the first `Day`) | Nothing for 150 ms, then skeletons (principles §3.2): the card frame with two text bars and an empty art box, three PR row bars, three right-column row bars. Shimmer off under Reduce Motion. |
| No account ("me" unknown) | Empty state "Today fills in after the first sync" (unchanged). No card. |
| First sync incomplete (`initial_sync_done = false`) | Card: fresh-start art, headline "Getting your day ready", sentence "First sync in progress · counts appear when it finishes", counts "–". Rows that have synced show (they are facts); Up next reads "Up next appears after the first full sync". No mood claim (FR-8 row 1). |
| Empty org / nothing of mine | Card computed normally (fresh start or clear). Your PRs: "No open pull requests" + "Your pull requests show here while they're open and on the day they merge." Right sections show one quiet line each ("No reviews waiting on you", "No loose ends"). |
| Error (store read or pass failed) | Pane banner (principles §3.4): "Couldn't load Today. {reason}. [Retry]"; Retry = "Today: Refresh". The last good `Day`, if any, stays below the banner. |
| Offline / stale (FR-10) | Everything renders from the store. Header shows the staleness label "Updated 18m ago · Refresh ⌘R". Absence moods (stalled, clear) are not claimed: the card shows the waiting art, "Can't see the latest", "Offline · counts are from 14:02". |
| Throttled by ADO | Same as stale once the warm lane's last success is older than 15 min; before that, nothing changes. |
| Demo mode | Demo store, demo clock; moods computed normally. |

## 4. Functional requirements

### 4.1 Today's window and data

- **FR-1 Today's window.** "Today" is `[local midnight, now]`, local midnight from `WorkingHours::day_start` with the offset Settings › Nudges uses (`crates/nudge/src/hours.rs:97`), the same window as 0006 FR-13 and `Snapshot.merged_today`. Every count and "today" label uses it, so a 07:30 merge or a 22:00 push counts. Working hours (Settings › Nudges, default Mon–Fri 09:00–18:00) are used only for the mood's time phases (FR-9) and Up next's visibility (0006 AC-9).
- **FR-2 Recompute.** The `Day` keeps its triggers (store commit debounced 500 ms, every 5 min, triage) and adds one timer for the earliest of: the next local midnight, the end of the fresh window, the half-day mark (FR-9). At midnight + 1 s, yesterday's merges and counts are gone without waiting for the 5-minute pass.
- **FR-3 Counts and their sources.** All reads are local; zero added requests (0006 FR-7 holds).

  | Metric | Counts | Store source |
  | --- | --- | --- |
  | merged | My PRs (author = me), status completed, `closed_ms` in today | `pull_request.status`, `author_id`, `closed_ms` (`Snapshot.merged_today`, `crates/nudge/src/store.rs:94-105`, filtered to author = me). Abandoned never counts. |
  | reviews | PRs I don't author where my vote is non-zero and `voted_ms` is in today; one per PR | `pr_reviewer.vote`, `voted_ms` (logic of `ribbon()`, `crates/nudge/src/today.rs:595-614`). `voted_ms` is when sync saw the vote change (`crates/sync/src/engine.rs:2526-2530`), at most one hot interval after Azure DevOps. |
  | discussions | Distinct threads on PRs involving me where (a) I posted a comment today (`is_system = 0`, `is_deleted = 0`, `created_ms` in today), or (b) the thread is resolved (`ThreadStatus::is_resolved`: fixed, won't fix, closed, by design) with `updated_ms` in today, and either the PR is mine or I commented in that thread at any time | `comment.author_id`, `created_ms`, `is_system`, `is_deleted`; `pr_thread.status`, `updated_ms`. Threads are the hot set's, already in `Snapshot.threads` (`store.rs:71`); the read is extended to my PRs merged today. |
  | commits | New commits in pushes to my PRs (active or merged today) whose iteration `created_ms` is in today; iteration 1 counts only when the PR was created today | `pr_iteration.created_ms` + the new `commit_count` (FR-4) |
  | done | Work items assigned to me, state category Completed, whose terminal transition is in today | `work_item.assigned_to_id`, `state_category = 'Completed'`, `terminal_since_ms` (migration 0008; first observed closure, refined by `ClosedDate`/`StateChangeDate`), `is_deleted = 0`. Removed doesn't count. The WIQL baseline covers all my items in every state (`engine.rs:1240`). |
  | opened (no slot) | My PRs with `created_ms` in today | `pull_request.created_ms` |
  | loose-end age (no slot) | Days the item has been in its current state, shown on Loose ends rows (FR-22) | `work_item.state_since_ms` (new, filled by sync from `Microsoft.VSTS.Common.StateChangeDate`, a field the work item request already fetches: `crates/sync/src/engine.rs:85`, 0 added requests), falling back to `changed_ms` when NULL |

  Attention inputs, from the existing `Day` (`today.rs:463-526`): **conflicts** and **failed builds** = my active PRs whose `Track.blocker` is `Conflict` / `BuildFailed` and not parked (a turned-off type never blocks, 0006 FR-12); **owed** = unparked Waiting-on-you rows + unparked Loose ends + my active PRs whose unparked blocker is `ChangesRequested`; **waiting on reviewers** = my active non-draft PRs at `Stage::Review` whose blocker is none or `WaitingOnReviewers`. Approved, not yet completed PRs are none of these.

- **FR-4 Commits, not pushes.** An Azure DevOps PR iteration is one push ("Iterations are created as a result of creating and pushing updates", Pull Request Iterations – List). adoken counts commits:
  1. The existing iterations request (`crates/sync/src/pr_files.rs:201-210`, sent when a PR's file list is due) adds `includeCommits=true`. Same request, same lane, larger body.
  2. Sync stores per iteration `commit_count` = commits of iteration N whose id is not in iteration N−1's list (correct whether Azure DevOps lists the push's commits or all commits so far, [INFERENCE: verify which on a recorded fixture, AC-14]) and `commits_truncated` when `hasMoreCommits` is true. Store migration: two nullable columns on `pr_iteration`.
  3. Display rule: known → "2 commits today"; truncated or unknown (`commit_count` NULL: iteration synced before the migration) → that iteration counts as its known number, or 1, and the total shows "N+"; the PR's head moved today (`last_push_ms` in today) but its iteration list hasn't synced yet (`pr_file_sync.source_commit ≠ last_source_commit`) → "New commits today", no number, until it does. Numbers are never guessed.
  4. "Today" is push time (the iteration's `createdDate`), not the commit's author date (Q-1).
- **FR-5 Per-PR activity.** Each PR row carries: `commits_today` (FR-4), `comments_today` = non-system, non-deleted comments created today by anyone but me on that PR, `opened_today`, `merged_at` (when merged today), and `last_activity` = max(`pull_request.updated_ms`, newest iteration `created_ms`, newest non-system comment `created_ms`, `closed_ms`).
- **FR-6 One computation.** The summary, mood and per-PR activity are computed inside the existing background `Day` pass (`crates/app/src/today/model.rs:30-57`) from one `Snapshot`; rendering only lays out the result. The mood and sentence are pure functions of (`Snapshot`, `Settings`, `Pass`, `Ledger`, data health, `now`), testable with an injected clock; strings come from `crates/nudge/src/copy.rs`.

### 4.2 Day summary and mood

- **FR-7 Moods.** Eight moods, each with one illustration (C2) and one headline:

  | Mood id | Art | Headline | Meaning |
  | --- | --- | --- | --- |
  | `fresh-start` | `01-fresh-start` | A fresh start | The day is beginning |
  | `finding-flow` | `02-finding-flow` | Finding your flow | Commits, new PRs or finished work items today |
  | `helping-grow` | `03-helping-grow` | Helping things grow | Reviews or discussions today |
  | `waiting` | `04-waiting` | Waiting to bloom | My PRs wait on reviewers; neutral |
  | `blocked` | `05-blocked` | Blocked | Conflicts or failed builds and nothing done yet |
  | `shipped` | `06-shipped` | Something shipped | A PR of mine merged today |
  | `clear` | `07-clear` | Clear for now | Nothing needs attention |
  | `stalled` | `08-stalled` | Nothing moved today | No recorded progress by mid-day with work owed |

  Two variants reuse an art without claiming its mood: **first sync** (fresh-start art, "Getting your day ready") and **stale** (waiting art, "Can't see the latest").

- **FR-8 Mood selection** (first match wins). Let A = merged + reviews + discussions + commits + done + opened; growth = reviews + discussions; flow = commits + opened + done; B = conflicts + failed builds.

  | # | Condition | Mood |
  | --- | --- | --- |
  | 1 | First sync incomplete | first-sync variant (fresh-start art) |
  | 2 | merged ≥ 1 | `shipped` |
  | 3 | A ≥ 1 and growth > flow | `helping-grow` |
  | 4 | A ≥ 1 | `finding-flow` (ties go here) |
  | 5 | B ≥ 1 | `blocked` |
  | 6 | Workday and in the fresh window (FR-9) | `fresh-start` |
  | 7 | Workday, past the half-day mark, owed ≥ 1, data fresh | `stalled` |
  | 8 | waiting on reviewers ≥ 1 | `waiting` |
  | 9 | owed = 0 and data fresh | `clear` |
  | 10 | Data stale and rule 7 or 9 would have matched | stale variant (waiting art) |
  | 11 | Otherwise (owed ≥ 1 before the half-day mark, or a non-workday) | `fresh-start` |

  Consequences the owner asked for: a blocker never erases accomplishments (rows 2–4 precede 5; the sentence still names it, FR-11); waiting is neutral (lavender art, no attention tone); an inactive day (`stalled`) is distinct from a fresh morning (row 6), an incomplete sync (row 1) and stale data (row 10); a non-workday is never `stalled`. Rule 10 applies to rows 7 and 9 only: observed activity and blockers are facts and stay claimable on stale data.

- **FR-9 Time phases.** Let elapsed = working time between today's working-hours start and `now` (`WorkingHours::working_between`, `hours.rs:127`; 0 before the start). **Fresh window**: elapsed < 2 h (default 09:00–11:00; any time before 09:00). **Half-day mark**: elapsed ≥ half of the working day (default 13:30). After working hours end, elapsed is the full day. Both thresholds are constants, not settings (Q-3, Q-4).
- **FR-10 Fresh vs stale data.** Data is stale when any of: the app is offline (principles §3.5); the token is rejected; the last successful pass of the My pull requests or Work items lane is older than max(15 min, 2 × that lane's interval in Settings › Sync) (0016 FR-10 per-lane last success; a lane set to Manual only or paused by the user never makes data stale by age). This replaces `crates/app/src/today/model.rs:70-78`, which reads tiers as always fresh. Stale data shows the header staleness label (principles §3.3).
- **FR-11 The sentence.** One line, at most 3 parts joined by " · ", singular/plural as written:
  1. **Lead** by mood: `shipped` "{n} PR(s) merged today"; `helping-grow` "{n} review(s) done today" (reviews ≥ 1) else "{n} discussion(s) today"; `finding-flow` "{n} commit(s) pushed today" ("{n}+" per FR-4) else "{n} PR(s) opened today" else "{n} work item(s) done today"; `waiting` "{n} PR(s) waiting on reviewers"; `stalled` "No reviews, commits or merges yet today"; `blocked` and `fresh-start` have no lead.
  2. **Attention**, in order, while parts < 3: "{n} conflict(s) to resolve", "{n} failed build(s) to fix", "{n} review(s) wait on you" ("1 review waits on you").
  3. Fixed sentences replace 1–2: `clear` "Nothing needs your attention right now"; first sync "First sync in progress · counts appear when it finishes"; stale "Offline · counts are from {HH:mm}" or "Sync delayed · counts are from {HH:mm}" (time of the older of the two lanes' last success). `fresh-start` with no parts: "Nothing needs you yet".

  The owner's example holds verbatim: one merge and one conflict read "1 PR merged today · 1 conflict to resolve".
- **FR-12 Counts row.** Slots in fixed order: merged, reviews, discussions, commits, done. Every non-zero metric shows; while fewer than 3 slots show, zero-valued merged, then reviews, then commits are added (quiet tone, `text.tertiary`); slots always display in the fixed order. A missing scope hides its slot (`done` without `vso.work`). First sync shows merged, reviews, commits as "–". Labels singular at 1 ("1 review"). The row's height is fixed so the card never changes size between moods.
- **FR-13 Header.** "Today", the local long date below it, and on the right the staleness label only when FR-10 says stale. The old header's fired-nudges counter ("🔔 3/3") is dropped: the inbox shows nudges.

### 4.3 Your PRs

- **FR-14 Rows and order.** One row per active PR I authored and per PR of mine merged today; abandoned and older merges never show. Order: `last_activity` (FR-5) descending, then id. This replaces 0006 FR-12's order (blocked first, merges last): blockers show on the row, not by position. Parked rows keep their place, dimmed with their glyph (0006 FR-8).
- **FR-15 Row content.** Line 1: the **full title**, wrapping (never truncated before 3 lines; beyond 3, an ellipsis with the full title in the tooltip and accessible label), with "Updated 3m ago" (`last_activity`, principles §5 relative times) at the right. Line 2: the stage bar (FR-16), its state glyph and label, reviewer faces at the right (FR-18). Line 3: `!id` then the activity line (FR-19). `↩` or a click opens the PR review (0004); `⌘D` pins it (0014); `c` copies reviewer names on an N4 row (0006 FR-9).
- **FR-16 Stage bar.** Four equal segments Draft · Review · Approved · Merged; the segments up to the PR's stage (0006 FR-12 `Stage::of`) are filled, the rest empty. The fill tone and label come from the row's state; a blocker's tone overrides the stage's (an approved PR with a failed build fills 3 segments in the danger tone):

  | State (first that applies) | Filled | Label |
  | --- | --- | --- |
  | Merged today | 4 | Merged today (sparkles glyph) |
  | Merge conflict (N5) | by stage | Merge conflict |
  | Build failed (N3) | by stage | Build failed |
  | Changes requested (N2) | 2 | Changes requested |
  | Waiting on reviewers (N4, past threshold) | 2 | No reviews · {age} |
  | Approved | 3 | Approved |
  | In review | 2 | In review |
  | Draft | 1 | Draft |

  No dot, no circle, no hairline: segments are filled bars (sizes and tokens in the design doc §4.2). Color is never alone: the label and glyph carry the state.
- **FR-17 Merged today.** Only PRs completed today get the success row wash and the static sparkles glyph (C3: no effect, nothing animates). A PR merged yesterday is not on Today.
- **FR-18 Reviewers (C1).** Faces of reviewers other than me who haven't declined, required first, at most 3 then "+N". With `[review] show_group_reviewers = false` (default, spec 0019), reviewers whose identity `Person.is_group` is true are not shown and not counted in "+N"; they still count in the stage (`Stage::of` reads every reviewer) and in N4's names. With it true, groups show as initials after the people. Each face shows its vote glyph badge for non-zero votes; a no-vote face is dimmed. A reviewer whose identity hasn't synced yet counts as a person.
- **FR-19 Activity line.** After `!id`, the first matching form:
  1. Merged today: "Merged today at {HH:mm}".
  2. Today's parts, in order, those that apply: "Opened today"; commits (FR-4: "2 commits today", "1 commit today", "3+ commits today", "New commits today", "1 push today"); comments "1 new comment" / "{n} new comments" (FR-5, from others).
  3. No activity today: the blocker with how long it has held ("Conflict for 5h", "Build failed 2h ago", "Changes requested by Kim · 1d", "Waiting on Kim (required), Sam · 2d"); else "All reviewers approved" when approved; else "No activity today".

### 4.4 Right column

- **FR-20 Up next.** The Focus of 0006 unchanged in choice (FR-10), visibility (AC-9: workdays from the start of working hours) and `e` (hide until tomorrow), shown as the Up next panel: face or nudge glyph, headline, `!id` and title, meta, the action button with `↩`, and the triage hints. Hidden, before working hours or on a non-workday, the panel is omitted and Waiting on you moves up. The age ring is dropped (FR-23).
- **FR-21 Waiting on you.** 0006 FR-11's rows and order (when I was asked, oldest first; parked last), one line each: title (one line, ellipsis, full in tooltip), the author's face, a shield when I'm required, and the age (cool / warm / hot heat tones of 0006 FR-11). No cap: the page scrolls.
- **FR-22 Loose ends** (Q-2, owner override 2026-10-01). 0006's loose ends (N6 any age, N7 past threshold; turned-off types excluded), one line each: type glyph, title, and an age pill "Open {N}d", where N = whole days (floor) of `now − state_since`: how long the item has been in its current state. `state_since` is a new per-work-item value in ms, stored in `work_item.state_since_ms` by a new store migration and filled by sync from `Microsoft.VSTS.Common.StateChangeDate`, which the work item request already fetches (0 added requests). When it is unknown (NULL: synced before the migration, or Azure DevOps sent no date), the age falls back to the item's changed date (`changed_ms`). The nudge's own reason stays in the tooltip ("Active for 3 days · PR !901 merged 2 days ago" for N6, "Active for 8 days · no change for 8 days" for N7). The earlier "Merged {age}" / "Quiet {age}" wording is not used. `↩` opens the item (0015 detail).
- **FR-23 What leaves, what stays.**
  - **Ribbon: dropped.** Its reviews and merges are the counts row; failed builds on my PRs are row blockers and the sentence; mentions belong to the Inbox (0005). A second timeline repeats the card and costs a full row. The Ken streak flame stays in the statusbar (ken.md §4.3), where it already shows. 0006 FR-13 and AC-15 are superseded; `Ribbon` and `Moment` leave `crates/nudge/src/today.rs` (clean cutover), the review counting moves into the summary.
  - **Age rings: dropped.** In one-line rows a ring around a 20 px face is unreadable; the age number with its heat tone carries the same signal (0006 FR-11 heat thresholds unchanged), and color is not alone because the number is the information.
  - **Triage keys: kept**, on any selected row: `s`, `d`, `x`, `e`, `c`, ⌘P "Nudge: Turn off this nudge type" (0006 FR-8). A PR row's nudge is its top condition (`Track.nudge`).

### 4.5 Layout, states, keyboard, accessibility, motion

- **FR-24 Responsive layout.** Today's own width ≥ 960 px: two columns, Your PRs (flex, min 520 px) and a 320 px right column (Up next, Waiting on you, Loose ends). Below 960 px: one column, order summary, Up next, Your PRs, Waiting on you, Loose ends. The whole page scrolls as one; the summary card spans the width in both. The switch happens on the frame the width crosses 960 px, with the selection kept. Content is capped at 1280 px wide and centered.
- **FR-25 States** as in §3 States, using `EmptyState`, `SkeletonList`, `Banner` and `StalenessLabel` (principles §3).
- **FR-26 Keyboard.** One selection across the page (the focus ring of principles §4). Two-column: `↓`/`↑`/`j`/`k` move within the column (the right column is one list: Up next, then Waiting on you rows, then Loose ends rows), `→`/`←`/`l`/`h` switch to the other column's row nearest vertically. One column: both pairs walk the single list. `Home`/`End` first/last row of the page. `↩` runs Up next's action or opens the row. `s`/`d`/`x`/`e`/`c` as FR-23. The summary card is not selectable. Every action is in ⌘P with its key (existing commands, `crates/app/src/today/mod.rs:98-141`); no new keys. `⌘K` is not bound on Today.
- **FR-27 Navigation and tours.** Today's restorable state for back/forward (0018) is the selected row's subject and the scroll offset. Tour anchors for 0020: `today.summary` (card incl. art), `today.prs` (PR column), `today.pr-stage` (first PR row's stage bar), `today.up-next`, `today.waiting`, `today.loose-ends`.
- **FR-28 Accessibility.** The card is a region labeled "Day summary" whose label reads headline, sentence and counts ("Something shipped. 1 PR merged today, 1 conflict to resolve. 1 merged, 3 reviews, 5 commits."); the illustration is decorative (no role, no label; the headline carries the mood). Each section is a `List` labeled with its title and count. Rows are `ListItem`s with full labels (design §7), e.g. "Pull request !66231 Fix session timeout on reconnect. In review, stage 2 of 4. 2 commits today, 1 new comment. Updated 3 minutes ago. Reviewers: Jane Doe approved, Mike Smith no vote." Mood changes are not announced (no live region).
- **FR-29 Motion.** The only animation on Today: when the mood changes while Today is visible, the new illustration crossfades over the old at `motion.base`. Under Reduce Motion it swaps in one frame. Selection, row inserts and reorders are instant. No sounds.

### 4.6 Illustrations (C2)

- **FR-30 Assets** (C2). The eight mood illustrations (owner-generated originals, transparent, no text; checksums of the PNG originals in `output/today-redesign-2026-10-01/assets.json`) are already embedded in the binary as WebP files at 2× the display box (528 × 352): `assets/images/today/*.webp`, served by `adoken_ui::Assets` under the paths `adoken_ui::images::TODAY_FRESH_START` … `TODAY_STALLED`, and recorded in `assets/manifest.toml` with `ship = true`, kind `illustration` and their provenance (generated by the owner from `image-generation-prompts.txt`). They are not re-added or re-encoded. Each ≤ 350 KB, all eight ≤ 2.5 MB (measured 2026-10-01: 0.59 MB as WebP, largest 112 KB). `onboarding-welcome` (`ONBOARDING_WELCOME`, 720 × 480) is 0001's.
- **FR-31 Loading.** An illustration is decoded off the UI thread (the pattern of `crates/app/src/avatar/mod.rs:447` and `background/pipeline.rs`) the first time its mood is needed; until it's ready the art box paints the card surface only (no skeleton, no spinner) and the text paints immediately. At most two decoded illustrations are held (current and previous, for the crossfade); the previous is dropped after the crossfade. Nothing is decoded before the first frame.
- **FR-32 Themes.** The art is painted unmodified (no tint, filter, opacity or frame) on the card surface in every theme, flush to the card's right edge and clipped by its radius; text never overlaps it. One art set serves dark and light themes (checked 2026-10-01 on Dojo Night and white surfaces: clean cutouts, no halos); the gallery snapshots in the four built-ins are the check for every change.

### 4.7 Amendments to other specs (accepted 2026-10-01; applied the same day)

- **0006:** §3 item 1's layout, FR-11's age ring clause, FR-12's order clause, FR-13 (ribbon) and AC-15 are superseded by this spec; AC-14 keeps its blocker labels, not its order. Nudge types, caps, Focus choice, triage and every other AC are unchanged.
- **0004 FR-1:** the iterations request adds `includeCommits=true` and stores per-iteration commit counts (FR-4).
- **ADR 0001 / store:** one migration adding `pr_iteration.commit_count INTEGER`, `pr_iteration.commits_truncated INTEGER` (cache table; rebuilt with it) and `work_item.state_since_ms INTEGER` (nullable; filled by the next work item sync, FR-22).
- **Assets:** the eight illustrations are embedded as `assets/images/today/*.webp` (C2, FR-30).

## 5. ADO API surface and limits

| Purpose | Endpoint (api-version) | Scope | Cost / limit notes |
| --- | --- | --- | --- |
| Commit counts | `GET {project}/_apis/git/repositories/{repo}/pullRequests/{id}/iterations?includeCommits=true` (7.1), the request sync already sends when a file list is due (≤ 5 PRs per hot pass, `FILE_LIST_BUDGET` 2 TSTU / 5 min, `crates/sync/src/engine.rs:66-67`) | `vso.code` | **0 added requests.** Body grows by one `GitCommitRef` per commit (≈ 0.5 KB each; `hasMoreCommits` marks truncation). [INFERENCE] negligible TSTU change; the 0016 Network log measures it on the real-org recording of AC-14. |
| Everything else | none: store reads only | — | 0 TSTU |

- **Rate limits:** no new traffic; the iterations request keeps its existing budget and back-off.
- **No push to local apps:** counts are as fresh as the lanes: reviews and PR state ≤ one hot interval (default 1 min), commits ≤ a hot pass after the push is seen (≤ 5 file lists per pass; "New commits today" bridges the gap), comments ≤ the hot set's thread pass, done items ≤ one warm interval. Today never claims absence on stale data (FR-8 row 10).
- **PAT scopes:** `vso.code` (PRs, iterations, threads), `vso.work` (done items, loose ends), `vso.build` (failed builds). C6 mints all of them at onboarding; with a pasted token missing `vso.work`, the done slot and Loose ends are hidden; missing `vso.build`, failed builds are never counted.

## 6. Acceptance criteria

Harness: `crates/nudge` unit tests with hand-built `Snapshot` fixtures and an injected clock (`FixedClock`). Unless stated: Thursday 2026-10-01, UTC offset +02:00, working hours Mon–Fri 09:00–18:00, all times local, me = `me`, every nudge type on, data fresh, first sync done, nothing parked. "Mood" is the computed mood id; "sentence" is the FR-11 string.

**Moods**

- **AC-1 Fresh start (morning).** Given now 09:40, no activity since midnight, one review asked of me yesterday, no blockers, when the day is computed, then mood `fresh-start`, headline "A fresh start", sentence "1 review waits on you", counts "0 merged · 0 reviews · 0 commits" (quiet tone). At 10:59 still `fresh-start`. Given instead one work item done at 09:05, the mood is `finding-flow` and the counts read "0 merged · 0 reviews · 1 done".
- **AC-2 Fresh start (first sync).** Given `initial_sync_done = false` at 15:30 and a fixture that holds a merge today and a conflict, then the first-sync variant (fresh-start art), headline "Getting your day ready", sentence "First sync in progress · counts appear when it finishes", counts "–"; never `shipped`, `blocked` or `stalled`.
- **AC-3 Finding flow.** Given now 11:00 and my PR !100 with iteration 2 at 09:10 (`commit_count` 3) and iteration 3 at 10:20 (2), plus a review I voted yesterday, then mood `finding-flow`, sentence "5 commits pushed today", counts "0 merged · 0 reviews · 5 commits", and !100's activity line "!100 · 5 commits today".
- **AC-4 Helping grow, and the tie.** Given votes today on !200 (10:00, approve) and !201 (10:30, wait for author), my comment at 10:31 in a thread of !201, and one commit pushed to my !100, then reviews 2, discussions 1, commits 1, mood `helping-grow`, sentence "2 reviews done today". Given instead one vote and one commit, mood `finding-flow`.
- **AC-5 Waiting.** Given now 14:00, my non-draft !300 in review with no votes, created yesterday at 10:00 (13 working hours ago, under N4's 24), nothing owed, no activity, then mood `waiting`, headline "Waiting to bloom", sentence "1 PR waiting on reviewers", and no attention tone anywhere on the card.
- **AC-6 Blocked.** Given now 14:00, my !400 with `mergeStatus = conflicts`, no activity, then mood `blocked`, sentence "1 conflict to resolve". After snoozing !400's N5, mood is no longer `blocked` (the fixture then reads `clear`).
- **AC-7 Shipped.** Given now 15:00 and my !500 completed at 14:32 today, then mood `shipped`, sentence "1 PR merged today", !500's row is the merged-today style with the sparkles glyph and "!500 · Merged today at 14:32", 4 filled segments. Given !501 completed yesterday at 23:50, !501 is not listed and not counted.
- **AC-8 Mixed day.** Given AC-7's !500 and AC-6's !400, then mood `shipped` and sentence exactly "1 PR merged today · 1 conflict to resolve".
- **AC-9 Clear and stale.** Given now 14:00, nothing owed, no PR of mine waiting, no blockers, no activity, then mood `clear`, sentence "Nothing needs your attention right now". Given the same with the Work items lane's last success at 13:20 (40 min old), then the stale variant: waiting art, "Can't see the latest", "Sync delayed · counts are from 13:20", and the header shows the staleness label. Offline: "Offline · counts are from …".
- **AC-10 Stalled, with its thresholds.** Given two reviews owed (asked yesterday) and no activity: at 13:31, mood `stalled`, sentence "No reviews, commits or merges yet today · 2 reviews wait on you"; at 13:29, `fresh-start` (rule 11); at 20:00, `stalled`. On Saturday 2026-10-03 at 15:00 with the same fixture, `fresh-start` (never stalled on a non-workday).
- **AC-11 Today's window.** Given my PR completed at 2026-09-30 23:30 UTC (01:30 local on Oct 1), it counts as merged today. Given a vote at 23:55 local yesterday, it doesn't count. With the clock moved from 23:59:59 to 00:00:01, the midnight timer recomputes within 1 s and every count reads 0 (FR-2).

**Counts and sources**

- **AC-12 Count rules.** One fixture, one assertion per row of FR-3: an abandoned PR of mine today is not merged; two votes by me on the same PR today count 1 review; my own comment on my PR counts as a discussion, a system comment and a deleted comment don't; a thread resolved today on my PR counts with no comment of mine, on another's PR only if I commented in it; a work item Completed at 10:00 today counts 1 done, one Removed today counts 0, one completed yesterday and edited today counts 0; iteration 1 of a PR created yesterday doesn't count, of one created today does.
- **AC-13 Commit display (FR-4).** Iterations today with `commit_count` 2 and NULL → "3+ commits today"; `commits_truncated` → "N+"; head moved today with the file sync behind → "New commits today"; after the list syncs → the number.
- **AC-14 includeCommits (mock server).** A hot pass over a PR with a new push sends the iterations request once with `includeCommits=true` and no other new request (request count equals the pre-change recorded suite); iterations recorded with 3 then 5 cumulative commits store `commit_count` 3 and 2; the same with per-push lists (3 then 2) stores 3 and 2. A real-org recorded fixture settles which form Azure DevOps returns (scripted check, recorded in §13).
- **AC-15 Zero requests.** Computing Today after a full mock sync issues zero requests (extends 0006 AC-10).

**Your PRs**

- **AC-16 Order.** Given !1 updated 09:00 with a comment from Kim at 11:00, !2 updated 10:30, !3 merged at 10:45, and !4 with a conflict updated 08:00, the order is !1, !3, !2, !4; "Updated" reads from `last_activity`.
- **AC-17 Title.** A 140-character title wraps to 2 lines with no ellipsis at 520 px; a 400-character title shows 3 lines with an ellipsis, the full title in the tooltip and the accessible label. (unit: layout + story)
- **AC-18 Stage bar.** One fixture per FR-16 row asserts filled count, tone token and label, including an approved PR with a failed build (3 segments, danger tone, "Build failed") and N4 below its threshold ("In review"). (unit)
- **AC-19 Activity line.** Fixtures produce exactly: "Opened today · 1 commit today", "2 commits today · 1 new comment" (my own comment excluded), "Conflict for 5h", "Changes requested by Kim · 1d", "Waiting on Kim (required), Sam · 2d", "All reviewers approved", "No activity today", "Merged today at 14:32".
- **AC-20 Group reviewers (C1).** Given !600 with reviewers Kim (person, approved), "React PR reviewers" (group, required, no vote), with `show_group_reviewers = false`: one face (Kim), no "+N", stage Review (the group still blocks Approved); with `true`: two faces, the group as initials after Kim. Toggling applies on the next frame (0009 FR-3).

**Right column, layout, keyboard**

- **AC-21 Up next unchanged.** 0006 AC-9 and AC-12 pass against the Up next panel; `e` hides it and Waiting on you moves to the top of the right column.
- **AC-22 Rows.** Waiting rows read title, face and age in the heat tone of 0006 FR-11 (cool at 2 h, warm at 30 working h, hot at 80 working h), parked last and dimmed. Given now 2026-10-01 15:00, a loose end (N6) whose `state_since` is 2026-09-28 09:00 reads "Open 3d", one (N7) at 2026-09-23 10:00 reads "Open 8d", and one with `state_since` NULL and `changed_ms` 2026-09-29 12:00 reads "Open 2d"; no row reads "Merged" or "Quiet". (unit)
- **AC-23 Breakpoint.** At 959 px the page is one column in the FR-24 order; at 960 px two columns with the right column 320 px; crossing it keeps the selected row. (unit: layout)
- **AC-24 Keyboard.** In two columns with the second PR row selected, `l` selects the right-column row nearest vertically, `j` the next right-column row, `h` returns to the nearest PR row; `↩` on a PR row emits `OpenPr`, on a loose end `OpenWorkItem`; `s` on a waiting row snoozes its N1; `c` on an N4 PR row copies the names. In one column, `l` acts as `j`. Every key's command is in ⌘P. (unit)
- **AC-25 Back.** Open !2 from Today with `↩`, then `⌘[`: Today shows with !2 selected and the same scroll offset (0018). (scripted smoke run)

**States, accessibility, motion, illustrations, gallery, performance**

- **AC-26 States.** Stories and unit checks for: loading (nothing before 150 ms, skeletons after), no account, first sync, empty org, error banner with Retry rerunning the pass, stale label. (unit + story)
- **AC-27 Accessibility.** The a11y tree has the "Day summary" region with the FR-28 label, no node for the illustration, one `List` per section, and row labels as in FR-28 and design §7 for a PR, a merged PR, Up next, a waiting row and a loose end. (unit: a11y tree)
- **AC-28 Reduce Motion.** With Reduce Motion on, a mood change swaps the art in one frame (no intermediate opacity); with it off, the crossfade lasts `motion.base` and no frame exceeds 16 ms. Nothing else on Today animates under either setting. (unit + frame recorder)
- **AC-29 Assets.** Exactly eight mood illustrations ship, one per FR-7 id, each 528 × 352 RGBA with transparent pixels, ≤ 350 KB, total ≤ 2.5 MB, each listed in `assets/manifest.toml` with `ship = true`; the mood → asset map is total (unit over every mood and both variants).
- **AC-30 Decode off the UI thread.** Switching through all eight moods decodes each image off the UI thread (UI-thread decode counter = 0), holds at most two decoded images, and paints the card's text in the first frame of each switch. (unit + frame recorder)
- **AC-31 Gallery.** Stories, one snapshot per built-in theme (Dojo Night, Dojo Day, Arcade, Sakura): `today-mood-fresh-start`, `today-mood-finding-flow`, `today-mood-helping-grow`, `today-mood-waiting`, `today-mood-blocked`, `today-mood-shipped`, `today-mood-clear`, `today-mood-stalled`, `today-busy-day` (the mixed day of AC-8 with every section filled), `today-first-sync`, `today-stale`, `today-loading`, `today-no-account`, `today-empty-org`, `today-error`, `today-narrow`, `today-up-next-hidden`, `today-group-reviewers-shown`, `today-long-titles`. The former `today-focus-hidden`, `today-clear-day` and `today-new-user` stories and their snapshots are removed. (snapshot test)
- **AC-32 Bench.** `cargo bench -p adoken-nudge --bench evaluate` over its corpus extended with 3,000 iterations (today and older), 20,000 thread comments, 200 completed work items and 100 merges today: evaluate + `today` (incl. summary, mood, per-PR activity) p99 < 20 ms. (bench)
- **AC-33 Smoke.** `today_smoke` drives the real shell on the demo store: `⌘1`, `j`/`k`/`h`/`l`, `↩` on a PR row, `⌘[`, resize from 1280 to 900 px wide, a store commit that changes the mood; frame p99 < 16 ms; screenshots in `target/today-smoke/`. (scripted smoke run)
- **AC-34 state_since from sync (FR-22).** Given the mock work item batch returns `Microsoft.VSTS.Common.StateChangeDate` = 2026-09-28T07:00:00Z for #48213, when a work item pass stores it, then `work_item.state_since_ms` equals that instant, the pass's request count equals the pre-change recorded suite (0 added requests), and a payload without the field leaves `state_since_ms` NULL. A store migrated from the previous release keeps every row with `state_since_ms` NULL until the next sync. (integration: `crates/sync` mock + store migration test)

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| evaluate + Today (summary, mood, activity) on the extended corpus | p99 < 20 ms, off the UI thread (unchanged budget) | `crates/nudge/benches/evaluate.rs` (AC-32) |
| Added store reads (iterations today, comments for merged-today PRs, done items) | < 5 ms on a temp store holding the bench corpus | Criterion bench in `crates/nudge` |
| Today paint from a computed `Day` (30 PR rows, 20 right rows) | < 16 ms frame; layout < 4 ms | Frame recorder |
| Breakpoint crossing, mood change | ≤ 1 frame to the new layout; crossfade frames < 16 ms | Frame recorder (AC-23, AC-28) |
| Illustration decode | Off the UI thread, < 30 ms each on the reference Mac; never awaited by a frame | Bench + decode counter (AC-30) |
| Memory | ≤ 2 decoded illustrations (≤ 1.5 MB); `Day` growth < 200 KB on the corpus | Heap profile |
| Binary size | ≤ 2.5 MB added (AC-29) | Release build size diff |
| Cold start | Unchanged (< 500 ms); no illustration decoded before the first frame | Launch timer |
| Added ADO cost | 0 requests; iterations body growth only | Mock suite request count (AC-14) |

Global rules (AGENTS.md) always apply: frame < 16 ms, Cmd+P < 50 ms on 100k items, cold start < 500 ms. Regressions are bugs.

## 8. Success signal (telemetry-free)

- **Dogfood:** for ten working days the owner writes one line per day in the dogfood notes: the mood Today showed at 17:00 and whether it matched their own read of the day. Target ≥ 9 of 10 matches, and zero "it said nothing moved but I…" lines.
- **Dogfood:** the next notes contain no complaint about truncated PR titles, the stage bar, or group reviewers on Today.
- **Behavior:** the owner opens Today (`⌘1`) as the first view on ≥ 4 of 5 mornings instead of going straight to the PR list (self-reported in the notes).
- **Build-in-public:** a post with the eight moods side by side gets replies about the art and the honesty of the sentence, not "what does the bar mean?".

## 9. Risks

- **A wrong mood erodes trust faster than no mood.** Mitigations: no claims on partial or stale data (rows 1, 10), a blocker never hides accomplishments, `stalled` only after half the working day with work owed and never on non-workdays, unit fixtures per rule and boundary (AC-1–AC-11).
- **Negative moods can feel judgmental** (a meeting-heavy day reads "Nothing moved today"). Mitigations: no-blame copy, "yet", the threshold Q-3, and the owner's notes are the judge (§8).
- **Attribution gaps.** Resolved threads don't record who resolved them; `voted_ms` and `last_push_ms` are observation times; done items count whoever closed them while assigned to me. All are named in FR-3 and the copy never says "you resolved".
- **Iteration commit semantics are undocumented** (per-push vs cumulative). Mitigation: the N vs N−1 difference works for both (AC-14); the real-org fixture records which.
- **Commit counts lag a push** by up to a few hot passes on busy hot sets (5 file lists per pass). Mitigation: "New commits today" bridges it honestly.
- **Asset weight and provenance.** ~0.6 MB of WebP in the binary; AI-generated art must be original and free of text. Mitigations: 2× downscale, size caps (AC-29), provenance in `assets/manifest.toml`, no third-party style references in the prompts file.
- **Dark art on light themes and vice versa.** Mitigation: one checked set, cutouts without halos, snapshots in four themes on every change (FR-32, AC-31).

## 10. Open questions — resolved 2026-10-01

All recommendations accepted by the owner, except Q-2 (owner override).

- **Q-1 Commits "today" by push time or authored time?** **Resolved:** push time, the iteration's `createdDate` (FR-4 item 4, AC-3, AC-14).
- **Q-2 Loose-end age label.** **Resolved (owner override):** the pill reads "Open {N}d", days in the work item's current state, from a new `state_since` (ms) filled from the state change date (`Microsoft.VSTS.Common.StateChangeDate`, already fetched, 0 requests), stored in `work_item.state_since_ms` by a new migration; unknown `state_since` falls back to the changed date. "Merged 3d" / "Quiet 8d" are not used (FR-3 table, FR-22, AC-22, AC-34).
- **Q-3 When does "Nothing moved today" start?** **Resolved:** at half the working day (13:30 by default), only on workdays and only when something is owed (FR-8 row 7, FR-9, AC-10).
- **Q-4 How long is a fresh morning?** **Resolved:** the first 2 working hours (until 11:00 by default) and any time before working hours (FR-9, AC-1).
- **Q-5 Drop the ribbon timeline?** **Resolved:** dropped (FR-23).
- **Q-6 Drop the age rings around faces?** **Resolved:** dropped; the age number in its heat tone stays (FR-21, FR-23, AC-22).
- **Q-7 Sort control on Your PRs?** **Resolved:** no control, most recently active first (FR-14, AC-16).
- **Q-8 Milestone.** **Resolved:** v0.1, RICE 80, before the public beta. If v0.1 slips, only the crossfade (FR-29) and the narrow layout's polish (FR-24) are cut, never the mood rules (`docs/roadmap.md` v0.1 cut line).
- **Q-9 A switch to hide the illustration?** **Resolved:** no switch in v0.1; `[today] illustration` waits in `docs/ideas.md` until dogfood finds the art noisy (FR-29, FR-32).

## 11. Out of scope

- Celebration effects for merges or work item transitions (0022, C3); Today has a static glyph only.
- Back/forward history mechanics (0018, C4) and the guided tour content (0020).
- Group reviewer setting and its PR list / review header behavior (0019, C1).
- Per-user weighting of moods, mood history, streak-like mood stats: no "snappy / less noise" job; `docs/ideas.md` if asked.
- A setting to hide the illustration (`[today] illustration`, Q-9): `docs/ideas.md`, only if dogfood finds it noisy.
- Team or org-wide day summaries (Later: team features).
- A sort or filter control on Your PRs (Q-7).
- Showing others' commits on PRs I review: Today shows my PRs only; review deltas are 0004's iteration compare (v0.2).
- Illustration variants per theme or per season; user-supplied mood art.

## 12. References

- Pull Request Iterations – List (`includeCommits`, `commits`, `hasMoreCommits`, `createdDate`; "Iterations are created as a result of creating and pushing updates to a pull request"): https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-iterations/list?view=azure-devops-rest-7.1
- Pull Request Threads – List (thread status): https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-threads/list?view=azure-devops-rest-7.1
- Pull Requests – Get Pull Requests (`closedDate`, `mergeStatus`, reviewers' votes): https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/get-pull-requests?view=azure-devops-rest-7.1
- Owner notes and assets: `output/today-redesign-2026-10-01/corrected-notes.txt` (lines 29–57), `references/today-layout.png` (layout only), `images/01..08-*.png`, `assets.json`.
