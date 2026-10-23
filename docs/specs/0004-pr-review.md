# 0004 — PR review view: read, discuss, vote, merge without the web UI

| Field | Value |
| --- | --- |
| Status | Accepted (write path, user decision 2026-09-30) |
| Milestone | v0.0.5 (Review) → v0.1.0 |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/app` (views), `crates/ui` (diff, file tree, markdown), `crates/ado` (PR endpoints), `crates/store` (blob cache) |
| Serves | Snappy (primary), Less noise (only threads that matter, local "viewed" state) |
| Depends on | `docs/specs/0001-onboarding.md` (Review vs Read-only scopes), `docs/specs/0002-sync.md` (PR rows, threads, events) |
| Design | `docs/design/principles.md`, `docs/design/tokens.md` (`diff.*`, `vote.*`, `pipeline.*` tokens), `docs/design/ken.md` (review combos, merge K.O.) |

## 1. Problem

Reviewing in the ADO web UI is a sequence of waits: the Files tab loads the tree, then each file's diff loads on click, big PRs stutter while scrolling, and the Overview mixes system noise ("Build succeeded", "Policy updated") with real discussion. Keyboard support is thin. Reviewers put off reviews because each one feels expensive, and PRs go stale.

## 2. Users and job to be done

- **Persona:** reviewer with 3–20 open review requests; PR author responding to feedback.
- **JTBD:** "When a review is requested, I want to read the change, leave comments, and vote in one fluid keyboard flow, so reviewing takes minutes, not a context switch."
- **Smallest delightful slice:** open a PR instantly from the inbox, move through files with `j`/`k`, see existing threads inline, reply/resolve, add a line comment, approve — all without a spinner on already-synced PRs.

## 3. UX flow

Layout, shortcuts and motion: `docs/design/principles.md` + tokens in `docs/design/tokens.md`. Keymap is registered as commands (0003) so everything is also in `Cmd+K`/`Cmd+P`.

1. **Open** from inbox (0005), `Cmd+P` (0003), a pasted URL, or a notification. The header (title, status, draft badge, source → target, author, reviewers with vote chips, required-policy/check status, linked work items) paints from the store immediately.
2. **Overview tab:** description (Markdown, 400-char list truncation replaced by full fetch), then discussion. **System threads** (votes, pushes, policy updates, ref updates) are collapsed into one quiet timeline line by default; human threads are shown in full. Toggle "Show system activity".
3. **Files tab:** file tree (left) with change type, `+/-` counts, comment count, and a local **viewed** checkbox; diff (right).
   - Unified or side-by-side (`Cmd+Shift+D` toggles). Default: side-by-side when the window is ≥ 1400 px wide, unified below; once the user toggles, their choice persists and overrides the width rule (Q-3).
   - `j`/`k` next/previous file, `n`/`p` next/previous thread, `v` mark viewed and advance, `Enter` expand collapsed context.
   - Syntax highlighting (async; plain text first), intraline word diff, unchanged regions collapsed with expanders, whitespace-only changes toggle.
   - Binary, generated (`*.lock`, `*.min.*`, configured globs) and files > 1 MB or > 20k lines: collapsed by default with "Load anyway" / "Open in browser".
   - File tree filter (`/`), "Only files with threads" (`⇧T`), the Threads view (`t`) and `n`/`p` continuing across files: spec 0019 FR-1–FR-13 (amended 2026-10-01, owner notes; accepted 2026-10-01).
4. **Threads inline:** existing file threads anchored to their line/side; status (active, fixed, won't fix, closed, by design, pending) as a chip. Actions: reply (`r`), resolve/reactivate (`Shift+R`), open in browser.
5. **New comment:** select a line on either side (click gutter or `c` on the cursor line) → inline composer (Markdown, `@` mention autocomplete from known identities, `Cmd+Enter` post). PR-level comment from Overview.
6. **Vote:** `Cmd+K → Approve / Approve with suggestions / Wait for author / Reject / Reset`, also buttons in the header. Required-reviewer badge where applicable.
7. **Complete (merge):** when the user is allowed and policies are satisfied, "Complete" opens a sheet: merge strategy (defaults to the branch's allowed/default strategy), delete source branch, transition linked work items, custom merge message. Also "Set auto-complete". When Azure DevOps confirms the merge of a PR I completed in adoken, it gets one celebration (spec 0022), or Ken's K.O. instead when the Ken layer is on (`docs/design/ken.md` §3.3) (amended 2026-10-01, owner notes; accepted 2026-10-01).
8. **Read-only level:** vote, comment, resolve, complete are hidden behind a single passive hint, "Review actions need Code (Read & write)" (0001 FR-9): no button, link or token action (amended 2026-10-01, owner notes; accepted 2026-10-01).
9. **Links** (user decision 2026-09-30, FR-14): for the open PR and for the selected row of the PR list, **Copy PR id** (`⇧Y`, copies `!4821`; clicking the header's `!4821` does the same), **Copy link** (`Y`, the canonical web URL) and **Open in Azure DevOps** (`O`, the system browser). Same keys in the list and the review (single letters: off while a text field, menu or sheet has the keyboard); each is also a `Cmd+P` command. The header's title row ends with "Copy link" and "Open" buttons showing their keys. A copy is confirmed by a brief toast ("Copied !4821" / "Copied link", bottom-right, 5 s, no motion; the optional `ui.copy` sound). The demo org copies its fake `contoso` link; Open is disabled there with the reason in its tooltip (as in work items), and `O` shows the reason as the toast. List rows have no right-click menu: there is no context-menu component yet.

### 3.1 Pull requests list

The Pull requests view lists synced PRs (newest activity first) in tabs **Mine** (active, authored by me), **To review** (active, not draft, I'm a reviewer who hasn't declined and my vote is none or wait-for-author) and **All**.

**One pane at a time** (user decision 2026-09-30): the view shows either the full-width list or the full-width review of one PR, never side by side, so long titles stay readable. The former list-beside-review layout and its full-width toggle (`⌘⇧↩`, "Toggle pull request list") are gone from this view.

- **Open:** `↩` or a click on a row opens its review. Opening from elsewhere (`Cmd+P`, the inbox, Today, a notification) goes straight to the review and selects that PR's row when it's listed.
- **Back:** `Esc` (also `Cmd+P` › "Back to pull request list") or the **Pull requests** back button at the start of the review header's title row returns to the list with the same row selected and the scroll position kept; its tooltip reads "Back to the list · Esc". `Esc` goes back only when no composer, menu or sheet has the keyboard (their own `Esc` closes them first). `⌘[` / `⌘]` are history back / forward across views (spec 0018): after opening the review from the list, `⌘[` returns to the list exactly as `Esc` does; after opening it from the Inbox, Today, a pin or the palette, `⌘[` returns there. While the review is still opening, isn't synced, or failed, a bar with the same back button stands in for the header. (amended 2026-10-01, owner notes; accepted 2026-10-01)
- **Keyboard:** the list has the keyboard whenever it's shown (entering the view, after going back, after a click on the list): `↑`/`↓` or `j`/`k` move, `Home`/`End` jump to the first/last row, `PageUp`/`PageDown` move by the rows that fit, `↩` opens, `←`/`→` switch tabs (wrapping; also `Cmd+P` › "Pull requests: next/previous tab"). The selection never wraps.
- **Rows** (`space.8` padding, two title lines + one meta line): status icon; the **full title**, wrapping to 2 lines, then an ellipsis; the age at the right. Meta line: `!id · repository · author avatar + name · source → target`, then badges: the status chip (**Draft**, **Active**, **Completed**, **Abandoned**, or **Completing…** while my Complete merges), **Merge conflict**, **Auto-complete** (set in adoken), the count of active human comment threads, the CI status (worst latest run of each pipeline built for the PR, from the store; hidden when none), and reviewer avatars grouped by vote in the order approved (check ring), approved with suggestions, waiting for author, rejected (red ring), each group led by its vote icon and ringed in its `vote.*` color, at most 3 avatars per group then "+n"; required reviewers who haven't voted come last at `opacity.dimmed`; optional reviewers without a vote and declined reviewers aren't shown. Hovering an avatar names the reviewer and vote.
  Group reviewers (policy groups such as "[Project]\React PR reviewers") are hidden by default in the rows and the review header, behind `[review] show_group_reviewers`; the row action menu gains status changes and copy title / source branch: spec 0019 FR-14–FR-18, FR-22–FR-24 (amended 2026-10-01, owner notes; accepted 2026-10-01).

**Repository filter** (user decision 2026-09-30, FR-13):

- Combined with the tabs; default **All repositories**. `r` in the list, the header's Repository button, or `Cmd+P` › "Filter pull requests by repository…" opens a picker under the list title: a query field (focused) over "All repositories" and the repositories that have PRs in the current tab, each with its count, sorted by count then name. Typing narrows by name (case-insensitive substring); `↑`/`↓` + `↩` choose, `Esc` or a click outside closes.
- While a repository is chosen, a chip "Repository: <name>" with **Clear** sits under the tabs; `⌘⌫` in the list or `Cmd+P` › "Pull requests: all repositories" clears it. Tab counts follow the filter.
- The selection stays on the same PR when it's still listed, else the open PR, else the first row.
- When the filter hides every PR of the tab: "No pull requests in <repo>" with an **All repositories** button (`⌘⌫`).
- The choice persists per organization in the org's store (`review_pref`, beside the diff layout preference): the store is already one file per org, so no org key is needed, and it's view state rather than a user-edited setting. A saved repository that no longer exists falls back to All repositories (and the saved value is cleared).

### States

| State | Behavior |
| --- | --- |
| Not yet synced PR (deep link) | Header skeleton ≤ 1 frame, then network fetch at user priority. |
| Loading file content | Diff area shows the file header, then (only after 150 ms: a fast load never flashes) a spinner with "Loading <file name>…"; after 3 s a second line says what the wait is on: "Downloading 2 files from Azure DevOps…", "Waiting for Azure DevOps (sync in progress)…", "Azure DevOps asked adoken to slow down. Waiting…", or offline. Static under reduce motion. No layout shift when content arrives (user decision 2026-09-30, FR-15). |
| Loading the file list (opened before it was synced, or after a push) | After 150 ms the tree header shows "Loading files…"; with no stored list the tree shows skeleton rows. A failure shows the reason and **Retry** (`⇧L`, also `Cmd+P` › "Retry loading files"); a stale list stays below it (user decision 2026-09-30, FR-15). |
| A file's contents couldn't be fetched | The reason and **Retry** (`⇧L`). |
| Offline | Everything cached is viewable; composer disabled with "Offline"; unsent draft kept locally. |
| PR updated while viewing (new push) | Non-modal banner "2 new commits — Reload diff"; never swaps content under the cursor. |
| Merge conflicts | Header badge; Complete disabled with reason. |
| Throttled | Composer/vote queue with countdown; nothing lost. |

## 4. Functional requirements

- **FR-1** Diffs are computed **locally** from base and target blob contents (ADO REST returns per-file change lists and blobs, not hunks). Base = merge-base per the iteration's `commonRefCommit` / `$compareTo=0`; target = latest iteration's source commit. The iterations request carries `includeCommits=true` (same request, larger body), and sync stores each iteration's new commit count for Today (spec 0017 FR-4; amended 2026-10-01, owner notes; accepted 2026-10-01).
- **FR-2** Blobs are fetched by `objectId` and cached content-addressed in the store; a blob is never fetched twice. Cache cap 500 MB, LRU eviction.
- **FR-3** Prefetch: on opening Files, fetch blobs for the first 5 files in tree order and the neighbors of the selected file (governor user priority, max 4 in flight).
- **FR-4** Diff view is virtualized: layout cost proportional to visible lines, not file size.
- **FR-5** Thread anchoring uses `threadContext` (`filePath`, `rightFileStart/End`, `leftFileStart/End`) and, when the thread's iteration differs from the displayed one, `pullRequestThreadContext.trackingCriteria`/iteration context to map lines; unmappable threads appear in a "Comments on older code" section on that file.
- **FR-6** New line comments are created with the current iteration context (`iterationContext` first 1, second = displayed iteration) **and the file's `changeTrackingId`** from that iteration's changes (`changeEntries[].changeTrackingId`), so they anchor correctly in the web UI and ADO tracks them across later pushes. Without `changeTrackingId`, ADO leaves the comment on the old line number after a push (verified live 2026-09-30).
- **FR-7** "Viewed" state is local-only, keyed by `(pr, path, objectId)`: a file changed by a new push becomes un-viewed automatically.
- **FR-8** Optimistic writes per 0002 FR-14; unsent composer drafts persist across restarts.
- **FR-9** Vote values: Approve 10, Approve with suggestions 5, No vote 0, Wait for author −5, Reject −10.
- **FR-10** Complete uses `PATCH` with `status: completed`, `lastMergeSourceCommit` = displayed source commit (so a newer push is not merged unseen), and `completionOptions`. Auto-complete sets `autoCompleteSetBy` = me; cancel sends the all-zero GUID. Completion is **asynchronous**: a successful `PATCH` answers `status: active, mergeStatus: queued, closedBy: me` and the PR completes 1–2 s later (verified live 2026-09-30). adoken shows "Completing…" and blocks Complete until a follow-up refresh reports `completed`, or shows the merge failure reason and offers Complete again.
- **FR-11** Mentions in rendered comments (`@<id>`) are resolved to display names via the identity cache (0002).
- **FR-12 Auto-complete guard.** ADO auto-complete merges whatever is on the source branch once policies pass, including commits pushed after it was set. When I set auto-complete, adoken records the source commit I reviewed. If a later `PrPushed` event (0002) arrives before completion, adoken raises a P1 notification (0005) "New commits on !912 after you set auto-complete", with a one-key **Cancel auto-complete** action (`PATCH` clearing `autoCompleteSetBy`) and **Review changes**. The sheet that sets auto-complete says this explicitly. Server-side protection ("reset votes on new pushes" branch policy) is shown in the sheet when the branch has it.
- **FR-13 List repository filter.** The list filters by one repository or all (§3.1); filtering runs in memory over the loaded list (≤ 2,000 PRs), never a store query per keystroke.
- **FR-14 PR links** (user decision 2026-09-30). The web URL is `{org URL}/{project}/_git/{repo}/pullrequest/{id}`, built from the stored org URL (`https://dev.azure.com/{org}/`; a legacy `https://{org}.visualstudio.com/[collection/]` keeps its host and path) and the synced project and repository **names**, each percent-encoded as a path segment (`Mobile App` → `Mobile%20App`). The names are read off the UI thread with the list and with the opened review. Until they are synced, Copy link and Open are disabled with the reason. Pasting a copied link into `Cmd+P` opens the same PR (0003 FR-7).
  Copy title and copy source / target branch (header click, `Cmd+P`, row menu): spec 0019 FR-22–FR-24 (amended 2026-10-01, owner notes; accepted 2026-10-01).
- **FR-15 Loading order and indicators** (user decision 2026-09-30). The selected file's blobs are requested before the prefetch set's (FR-3): prefetch starts once the selection's blobs are local or failed, so its downloads never queue behind prefetch. Every review request runs at the governor's user priority, which is admitted ahead of queued sync requests and has a slot of its own (0002 FR-7). Indicators follow §3 States; diff and highlighting never run on the UI thread.
- **FR-16 Status changes** (user decision 2026-09-30). With write access, the header's status chip is a menu (`⇧S`, or a click) offering only the changes valid now: an active PR offers Mark as draft (Publish when it's a draft) and Abandon…; an abandoned PR offers Reactivate (a draft stays a draft); a completed PR, or one Azure DevOps is completing, offers none. `⇧P` publishes / marks as draft, `⇧A` asks to abandon; each is also a `Cmd+P` command. Each is a `PATCH …/pullrequests/{id}` (`isDraft`, `status: abandoned|active`); the header and the list's row change on the next frame and revert with Azure DevOps' reason (a 403 names the Contribute to pull requests permission). Abandon always asks first.
  Discoverability (status items in the list's row menu, `⇧S`/`⇧P`/`⇧A` and these commands on the list's selected row, the header control's visible `⇧S` keycap, a Publish button on drafts), same behavior as above: spec 0019 FR-18–FR-21 (amended 2026-10-01, owner notes; accepted 2026-10-01).
- **FR-17 Work item links** (user decision 2026-09-30). The header's Work items row lists the PR's linked work items (type icon, `#id`, title, state; unsynced ones by id): the store's at once, then Azure DevOps' list (`GET …/pullRequests/{id}/workitems`, fetched when the review opens, stored when their project is synced), which wins. A click opens the item in Work items. "Link work item…" (`⇧W`) opens a picker over the synced work items using the `Cmd+P` engine and query language scoped to work items (`#1234`, bare ids, filters, pasted work item links; an unsynced id links all the same); `↩` links with a JSON Patch `add` of an `ArtifactLink` relation to the PR (`vso.work_write`). Unlink (the row's button or `Cmd+P` › "Unlink work item…") asks first, then removes that relation guarded by the item's revision. A token without `vso.work_write` (Review level, 0001), or one Azure DevOps refuses with 401/403, shows the passive reason instead of Link, with no token action (0001 FR-9; amended 2026-10-01, owner notes; accepted 2026-10-01).
  The work item picker is a shared component: spec 0015 FR-21 (Add link in the work item detail) reuses it with the same behavior (amended 2026-10-01).
- **FR-18 Overlays** (fixed 2026-09-30). Every overlay (vote and status menus, Complete sheet, confirmations, work item picker) closes on `Esc` and on a click outside it, and the keyboard returns to the review.

## 5. ADO API surface and limits

| Purpose | Endpoint (7.1 unless noted) | Scope | Notes |
| --- | --- | --- | --- |
| PR detail | `GET /{project}/_apis/git/repositories/{repo}/pullrequests/{id}` | `vso.code` | Full description. |
| Iterations | `GET …/pullRequests/{id}/iterations?includeCommits=true` | `vso.code` | `includeCommits` adds one `GitCommitRef` per commit to the body, 0 added requests (spec 0017 FR-4) |
| Changed files | `GET …/pullRequests/{id}/iterations/{iter}/changes?$top=2000&$skip=…&$compareTo=…` | `vso.code` | Default page 100, **max 2,000** per page. |
| File content | `GET /{project}/_apis/git/repositories/{repo}/blobs/{objectId}?$format=octetstream` | `vso.code` | Content-addressed → perfect cache. |
| Threads | `GET/POST …/pullRequests/{id}/threads`, `PATCH …/threads/{threadId}` | read `vso.code`; write `vso.code_write` (or `vso.threads_full`) | |
| Reply | `POST …/threads/{threadId}/comments` | `vso.code_write` | |
| Vote | `PUT …/pullRequests/{id}/reviewers/{reviewerId}` | `vso.code_write` | |
| Complete / auto-complete | `PATCH …/pullrequests/{id}` | `vso.code_write` | |
| Checks | `GET /{project}/_apis/policy/evaluations?artifactId=vstfs:///CodeReview/CodeReviewId/{projectId}/{prId}` (7.1-preview.1) | `vso.code` | Required vs optional policies, build status. |
| Statuses | `GET …/pullRequests/{id}/statuses` | `vso.code` | External checks. |
| Linked work items | `GET …/pullRequests/{id}/workitems` | `vso.code` | |

- **Rate limits:** opening a synced PR costs 0 requests for the header, then ~3 (iterations, changes, policy evaluations) + 2 blobs per viewed file. A 50-file PR fully reviewed ≈ 105 requests spread over minutes — within budget, all at user priority. Re-opening costs ~3 thanks to the blob cache.
- **No push:** new pushes are detected by the hot tier (≤ 60 s foreground); the banner prevents silent content swaps.
- **PAT scopes:** reading needs `vso.code`; every write needs `vso.code_write` (high privilege). Read-only users get a complete reading experience.

## 6. Acceptance criteria

Harness: mock ADO server with fixture PRs (small: 3 files; large: 300 files / 40k changed lines; with 60 threads incl. system threads, outdated threads, binary files, a 25k-line generated file); frame recorder; fake clock.

- **AC-1** Given a PR already in the store, header + file tree + first diff (blob cached) render within 100 ms of `Enter` in the inbox.
- **AC-2** Given an uncached PR at 80 ms RTT, the first file's diff is visible within 600 ms of open.
- **AC-3** Scrolling a 20k-line diff at maximum trackpad velocity never produces a frame > 16 ms.
- **AC-4** Given a blob already cached by `objectId`, no network request for that blob is made on re-open (mock asserts zero hits).
- **AC-5** Given 60 threads of which 25 are system threads, Overview shows 35 human threads and one collapsed system-activity line; toggling shows all 60.
- **AC-6** Given a thread anchored on right lines 40–42 of iteration 2 and the view showing iteration 3 where those lines moved to 55–57, the thread renders at 55–57; a thread whose lines were deleted appears under "Comments on older code".
- **AC-7** A new line comment created in adoken on right line 12 of `src/a.rs` round-trips: the mock receives `threadContext.filePath = "/src/a.rs"`, `rightFileStart.line = 12`, the iteration context of the displayed iteration, and that file's `changeTrackingId`.
- **AC-8** `j`/`k` move file selection; `v` marks viewed and advances; after a new push changes that file's `objectId`, it shows as un-viewed.
- **AC-9** Approve sends `PUT …/reviewers/{me}` with `vote: 10`; header updates on the next frame; on 403 it reverts and names `vso.code_write`.
- **AC-10** Complete sends `lastMergeSourceCommit` equal to the displayed source commit; if the server reports a newer commit (HTTP 409 `TF401192` `GitPullRequestStaleException`, verified live), the sheet shows "New commits since you reviewed" and does not retry automatically. Any other 4xx shows Azure DevOps' own message. A 2xx with `mergeStatus: queued` shows "Completing…" until sync confirms `completed`.
- **AC-11** With Read-only scopes, no vote/comment/resolve/complete control is focusable; the explanatory hint is present.
- **AC-12** Files > 1 MB, binaries, and configured generated globs render collapsed and cost zero blob requests until expanded.
- **AC-13** Composer text typed while offline persists across app restart and posts after reconnect only when the user presses `Cmd+Enter` again (no silent auto-post).
- **AC-14** Syntax highlighting never blocks: with highlighting artificially delayed by 500 ms, plain text renders first and frame time stays < 16 ms.
- **AC-15** Given I set auto-complete on !912 at source commit `A` and the mock reports a new source commit `B` before completion, exactly one P1 "New commits after you set auto-complete" notification is created; its Cancel action sends a `PATCH` clearing `autoCompleteSetBy`. With no new push, no such notification is created.
- **AC-16** With no saved preference, opening a PR in a 1440 px-wide window shows side-by-side and in a 1200 px-wide window shows unified; after the user toggles to unified at 1440 px, reopening any PR at 1440 px shows unified (persists across restart).
- **AC-17** Given PRs in repositories `api` (3) and `web` (2) in All, choosing `web` shows only its 2 PRs in every tab and the tab counts count only `web`; the picker lists `api 3` before `web 2`; the choice survives a restart; after `web` is deleted from the store, the list opens on All repositories.
- **AC-18** With !4821 open in project "Mobile App", repository "ios client", `Y` copies `https://dev.azure.com/contoso/Mobile%20App/_git/ios%20client/pullrequest/4821` and `⇧Y` copies `!4821`, each confirmed by a toast; with the list focused, the same keys copy the selected row's PR instead. Pasting that link into `Cmd+P` offers !4821. In the demo org, `O` opens nothing and says why.
- **AC-19** At 80 ms RTT with sync requests queued on the same governor, an uncached PR's file list is stored within 2 RTT + 20 ms and the selected file's blobs within 3 RTT + 30 ms of open (measured 2026-09-30: 164 ms and 261 ms, vs 429 ms and 694 ms with the former first-come queue). A file whose load takes < 150 ms shows no indicator; one that takes > 3 s names its wait; the file list's failure offers Retry and `⇧L` refetches it.
- **AC-20** (§3.1, user decision 2026-09-30) On entering the Pull requests view, `↓` `↓` selects the third row without a click; `End`/`Home`/`PageDown` move as specified. `↩` on a row scrolled below the first page shows only that PR's review (no list beside it) and `↓` then moves the diff cursor, not the list; `Esc` (or `⌘[` when the review was opened from the list, or the header's back button) shows only the list again with the same row selected, the same scroll offset, and `↑` working at once. Opening a PR from `Cmd+P` shows its review directly; `Esc` selects its row in the list, and `⌘[` returns to where the palette was opened (spec 0018 FR-3, AC-3; amended 2026-10-01, owner notes; accepted 2026-10-01). A row with a 300-character title shows two lines ending in an ellipsis.
- **AC-21** (FR-16) On an active PR the status menu lists exactly Mark as draft and Abandon…; `⇧P` makes it a draft in the header, the list row and the store, `⇧P` again publishes it; `⇧A` then `↩` abandons it; Reactivate makes it active again. A refused change reverts and shows the reason.
- **AC-22** (FR-17) Typing a synced work item's id in the Link picker and pressing `↩` lists it in the header and links it in the store; Unlink work item… `↩` `↩` removes it from both. With a token lacking `vso.work_write`, the Work items row shows the reason, no Link button and no token action (amended 2026-10-01, owner notes; accepted 2026-10-01).
- **AC-23** (FR-18) Clicking Vote… then clicking outside the menu closes it and `⇧V` works at once; `Esc` closes it too; the same holds for the Complete sheet, status menu, confirmation and picker.

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Open synced PR → header + tree | < 50 ms | UI trace |
| Open synced PR → first diff (cached blobs) | < 100 ms | UI trace |
| Next file (`j`) with cached blobs | < 16 ms to first diff frame | Frame recorder |
| Next file uncached, 80 ms RTT | < 300 ms | Mock latency |
| Local diff compute, 5k-line file | < 20 ms off main thread | Bench |
| Scroll any diff | 60 fps | Frame recorder |
| Memory, 300-file PR open | < 300 MB RSS | Heap profile |

## 8. Success signal (telemetry-free)

- Dogfood: builder completes ≥ 80% of review requests in adoken for a week (self-count in the weekly demo).
- Local-only stat: "Reviews this week" and "median time from open to vote" in Settings → About; also feeds the optional Ken review combo counter.
- Build-in-public: the 300-file PR scroll video vs the web UI.

## 9. Risks

- **Line mapping across iterations** is the hardest correctness problem; ADO's own tracking criteria are only partially documented. Mitigate with fixtures from real PRs and the "Comments on older code" escape hatch.
- **Syntax highlighting breadth vs binary size** (tree-sitter grammars add MBs). Start with the top ~15 languages seen in dogfood repos.
- **Merge strategy policy details** (allowed strategies per branch policy) may need extra policy-configuration reads (`vso.code`); scope to "default + allowed list" only.

## 10. Open questions — resolved 2026-09-25

All recommendations accepted by the user.

- **Q-1** Complete / auto-complete in v0.1? **Resolved:** yes, in v0.1, behind the confirmation sheet, with protection against merging commits the reviewer hasn't seen: Complete pins `lastMergeSourceCommit` to the displayed commit (FR-10, AC-10), and auto-complete warns on later pushes (FR-12, AC-15).
- **Q-2** Sync "viewed" state to ADO? **Resolved:** no; local-only in v0.1 (FR-7). No documented REST API exists.
- **Q-3** Default diff layout? **Resolved:** side-by-side at ≥ 1400 px window width, else unified; user override persists (flow step 3, AC-16).

## 11. Out of scope (deferred to v0.2 unless noted)

- **Iteration compare / "changes since my last review"** — first v0.2 item (`docs/roadmap.md`; RICE 96 in `docs/ideas.md`).
- Multi-line range comments and `suggestion` blocks.
- Commits tab, per-commit diff.
- Checking out the PR branch locally (`docs/ideas.md`).
- Editing PR title/description, adding reviewers, creating PRs.
- AI summaries (`docs/ideas.md`, low priority).

## 12. References

- Pull Requests — Get Pull Request: https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/get-pull-request?view=azure-devops-rest-7.1
- Pull Requests — Update (complete, auto-complete): https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/update?view=azure-devops-rest-7.1
- Pull Request Iterations — List: https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-iterations/list?view=azure-devops-rest-7.1
- Pull Request Iteration Changes — Get (max 2,000): https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-iteration-changes/get?view=azure-devops-rest-7.1
- Blobs — Get Blob: https://learn.microsoft.com/en-us/rest/api/azure/devops/git/blobs/get-blob?view=azure-devops-rest-7.1
- Pull Request Threads — Create (`vso.code_write` or `vso.threads_full`): https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-threads/create?view=azure-devops-rest-7.1
- Pull Request Reviewers — Create Pull Request Reviewer (vote): https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-reviewers/create-pull-request-reviewer?view=azure-devops-rest-7.1
- Policy Evaluations — List: https://learn.microsoft.com/en-us/rest/api/azure/devops/policy/evaluations/list?view=azure-devops-rest-7.1
