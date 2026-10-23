# 0024 — Threads view performance: first frame, dwell timing, list selection after a status change

| Field | Value |
| --- | --- |
| Status | Proposed (2026-10-02) |
| Milestone | v0.1 (Q-4): groups A and B ride with 0019 group B on the cut line; group C ships with 0019 group D, not on the cut line |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/app` (`review/pr/polish.rs`, `review/pr.rs`, `review/mod.rs`, `review/list_status.rs`, `review/data.rs`), `crates/gallery` (`tests/review_polish_smoke.rs`), gpui fork `zeronsh/zui` (A, Q-2) |
| Serves | **Snappy** (`t` paints well inside one frame, also at 120 Hz; excerpts land 150 ms after you stop, not 300 ms) · **Less noise** (the row you just acted on stays under your cursor, so the next command hits it) |
| Depends on | `docs/specs/0019-pr-review-polish.md` (FR-9, FR-10, FR-11, FR-18, FR-19, FR-21, FR-23, AC-B4, AC-B9, AC-S2, §7), `docs/specs/0018-navigation-history.md` (FR-5, restorable list state), AGENTS.md global budgets |
| Design | No new visuals. `docs/design/pr-review-polish.md` §2.2 (cards) and §4 (row chip, "Sending…") apply unchanged |

> Rule: no code without an **Accepted** spec (AGENTS.md). The spec owns behavior, acceptance, and budgets; design docs own visuals, motion, sound, and copy.

Follow-up to 0019, asked for by the owner on 2026-10-02 after the 0019 perf pass (`review_polish_smoke`, L+T fixture). Three groups, each shippable alone:

- **A. First frame after `t`:** a budget with margin, and the work to meet it.
- **B. Dwell timing (FR-11):** excerpts queue 150 ms after scrolling stops, as 0019 says, not about 300 ms.
- **C. Selection after a status change on the list:** the row you acted on stays selected (Q-3).

## 1. Problem

"`t` is instant on my machine, but the harness says it uses 15.5 of 16 ms in a debug build. Any card change pushes it over, and on a 120 Hz display it's already two frames. When I stop scrolling the Threads view, the code under each card shows up late. And when I marked a PR as draft from To review and then copied its branch from `⌘P`, I got another PR's branch, because the one I marked had left the tab and the selection jumped to a different row."

## 2. Users and job to be done

- **Persona:** reviewer coming back to a large PR (0019 B); anyone changing a PR's status from the list (0019 D).
- **Job to be done (A):** "When I press `t` on a big PR, I want the Threads view on the next frame, every time, so the switch never feels like a load."
- **Job to be done (B):** "When I stop scrolling on a card, I want its code to appear as soon as the app knows I'm reading, so I don't wait on a timer that runs twice."
- **Job to be done (C):** "When I change a PR's status from the list, I want my next command to act on that PR, so I never copy or change the wrong one."
- **Smallest delightful slice:** B (one timing fix, measured, RICE 300).
- **Priority (RICE, Q-4):** B = 300 (Reach 60 · Impact 0.5 · Confidence 100% · Effort 0.1 week). C = 135 (Reach 30 · Impact 1 · Confidence 90% · Effort 0.2 week; a wrong-branch paste into a terminal is a correctness bug). A = 37.5 (Reach 60 · Impact 0.5 · Confidence 50% · Effort 0.4 week; it passes 16 ms today, and the release number and the cause of the remaining cost are unknown). Build order: B, C, A.
- **Already in place** (verified 2026-10-02; numbers from the 0019 perf pass, debug build, headless Metal, 1280×800, L+T):
  - `t` → Threads painted: 15.4–15.7 ms (key handling 0.67 ms, draw ≈ 14.7 ms). With `LOOKAHEAD` set to 0 as an experiment: 13.5 ms, so the lookahead costs ≈ 2 ms and the viewport's cards ≈ 12.7 ms of draw. The experiment was reverted.
  - The Threads list is a gpui `list` built with `ListState::new(0, ListAlignment::Top, px(LOOKAHEAD))`, `LOOKAHEAD = 900.0` px (`crates/app/src/review/pr/polish.rs`). gpui's overdraw is fixed at construction: `ListState` at zui rev `18a89af` has no setter (`crates/gpui/src/elements/list.rs`). 900 px is a constant, not "one screen": on a list viewport taller than 900 px, cards in the last part of the screen below are never laid out, so FR-11 never queues them.
  - Dependencies build at `opt-level = 2` in dev (`[profile.dev.package."*"]`, root `Cargo.toml`), so the debug draw cost is mostly adoken's own crates at `opt-level = 0`. The release number may be much lower [INFERENCE]. Nobody has measured it.
  - Dwell: `track_dwell` runs in `PrReview::render` (`review/pr.rs`), before the list lays out. It compares `polish.laid_out`, which holds the item indices the *previous* frame laid out. When scrolling stops, the first timer starts against the last moving frame's set, finds the resting frame's set different at 150 ms, and restarts. Excerpts queue at ≈ 2 × `DWELL` ≈ 300 ms. The comparison uses item indices, so a sync that splices a card above the viewport also restarts the timer. The smoke's `dwell` helper (`crates/gallery/tests/review_polish_smoke.rs`) advances the clock twice to work around this.
  - Selection: a status change from the list goes through `update_listed_pr` → `refilter` (`review/mod.rs`). `refilter` keeps the selection on the same PR only if it is still in the tab. Otherwise the selection moves to the open review's PR, or else to the tab's first row. To review lists only active, non-draft PRs (`data::ListTab::matches`), so Mark as draft (or Abandon…) removes the row at once, and the `⌘P` copy commands (0019 FR-22, FR-23: "the selected row") act on another PR. Seen in the smoke: `fix/inventory-reservation-expiry` copied instead of `feature/retry`. AC-S2 avoids this by switching to All first.

## 3. UX flow

### 3.1 A: `t` on a large PR

1. In Files on L+T, `t`. The next frame shows the Threads view with every card that fits the screen at full fidelity (code excerpt or its placeholder of final height, full conversation).
2. The cards below the screen are laid out over the next frame or two. The screen doesn't change while this happens (no pop-in, no scroll jump).
3. Scrolling, `n`/`p` and `↩` behave as in 0019.

### 3.2 B: reading cards

1. Scroll the Threads view and stop on a card. 150 ms later the files of the cards on screen and one screen below are queued (0019 FR-11). Their excerpts appear as their diffs compute.
2. A repaint that doesn't move the cards (a focus ring, a count, a toast, a sync inserting a card above the screen while the focused card stays put) doesn't push the 150 ms back.

### 3.3 C: status change from the list (Q-3)

1. On **To review**, `!9001` is selected. `⌘K` › Mark as draft (or `⇧P`). On the next frame the row still sits at the same position, still selected, its chip reads Draft with "Sending…". The tab label's count drops by one at once.
2. `⌘P` › "Copy source branch" copies `!9001`'s branch.
3. `⇧P` again publishes it back (the row was never lost).
4. The row leaves To review when the tab is shown again from scratch: switching tab and back, changing the repository filter, or leaving the Pull requests view and returning.

### States

| State | Behavior |
| --- | --- |
| Empty | Unchanged (0019 §3 States). |
| Loading / first sync | A: excerpt placeholders keep their final height (0019 FR-10), so laying out the rest of the cards on later frames moves nothing. B: a file whose list is still loading queues nothing; its cards dwell once they exist. |
| Error | C: a refused status change reverts the row in place and keeps it selected (0019 FR-21's toast). |
| Offline / stale | B: dwell still runs. Uncached files show "Offline" and queue nothing (0019 FR-11). C: status changes are disabled offline (0019), so nothing moves. |
| Throttled by ADO | B: queued files wait behind the governor (0019). Dwell timing is unchanged. |
| Read-only token | A, B unchanged. C: no status changes, so nothing to keep. |

## 4. Functional requirements

### 4.1 Group A: first frame after `t`

- **FR-1 Viewport first.** The frame that first paints the Threads view (after `t`, "Show threads", a scope change, or back to Threads via 0018) lays out only the cards that intersect the list's viewport. The cards in the lookahead region below are laid out on the following frames, without changing anything on screen.
- **FR-2 Lookahead is one screen.** Once filled, the laid-out region below the viewport is one viewport height of the list as currently sized, not a fixed pixel count. It follows window resizes. This is also the "within one screen below" region of 0019 FR-11 (FR-5).
- **FR-3 Cost beyond FR-1, only if needed.** If FR-1 alone doesn't meet the debug gate (AC-A1), a profile of the first frame on L+T names the top costs of the remaining draw before any change is made. Caching shaped card headers (author line, timestamps, file path, chips) across frames and Files↔Threads toggles is the first candidate, used only if the profile shows text shaping of headers at ≥ 2 ms. Any cache counts toward 0019's 10 MB budget and is dropped when the review closes.
- **FR-4 Release measurement.** The first-frame number is measured once in an optimized build (AC-A4) and recorded in §7's "Today" column. It is not gated in CI (disk; see Risks).

**Options considered for A** (recommendation: 1, then 3 only on profile evidence; Q-2):

| # | Option | Expected gain | Cost and risk | Verdict |
| --- | --- | --- | --- | --- |
| 1 | Smaller first-frame lookahead (0 px), grown to one screen on the next frame | ≈ 2 ms debug (measured: 15.5 → 13.5 ms with `LOOKAHEAD` 0) | Needs `ListState::set_overdraw` in the zui fork (one method; no setter at rev `18a89af`). Rebuilding the `ListState` instead loses measured heights and costs more than it saves. The growth frame adds ≈ 2 ms to a ≈ 3 ms frame. | **Do** (FR-1, FR-2) |
| 2 | Lay out lookahead rows after the first frame in chunks spread over several idle frames | Same as 1 | More machinery (frame scheduling, partial-measure state) for a 2 ms job that fits in one frame | Cut; becomes 1's fallback only if AC-A2 fails |
| 3 | Cache shaped card headers | Unknown until profiled. gpui reuses shaped lines only from the previous frame [INFERENCE], so the first frame after `t` shapes every header fresh | Memory (bounded by the 10 MB budget), invalidation on theme, font size and sync edits, ≈ 0.3 week | **Only if** FR-3's profile justifies it |
| 4 | Prewarm the Threads view offscreen while Files shows | Could take the first frame to ≈ 0 | Every review pays for a view most opens never show; frames in Files get slower | Cut |
| 5 | Placeholder conversations on the first frame, filled in later | Large | Visible pop-in; breaks 0019 FR-10's stable heights promise | Cut |

### 4.2 Group B: dwell timing (corrects 0019 FR-11's implementation)

- **FR-5 What dwells.** The dwell set is the set of threads (by thread id, not list index) whose cards intersect the list's viewport, read from the same frame's layout, after the list has laid out.
- **FR-6 When it fires.** The dwell clock starts at the first frame that paints a new dwell set. It restarts only when a later frame paints a different dwell set: scroll, keyboard moves that scroll, resize, scope change, or a sync that changes which cards are on screen. Frames that paint the same set don't touch it: focus changes, counts, toasts, excerpt arrival, FR-1's lookahead growth, a sync splice above or below the screen that FR-9's anchor keeps out of view. When the clock reaches `DWELL` (150 ms), the files of every card intersecting the dwell window (the viewport plus one viewport height below, FR-2) are queued as in 0019 FR-11: same priority, same exclusions (computed, computing, collapsed, unavailable, offline uncached), same ≤ 4 in flight.
- **FR-7 Once per rest.** A dwell fires at most once per resting set. Idle frames after it queue nothing more.

### 4.3 Group C: selection after a status change on the list (Q-3)

- **FR-8 The acted-on row stays.** When a status change made from the Pull requests list (row menu, `⇧S`/`⇧P`/`⇧A`, `⌘P`, 0019 FR-18 and FR-19) makes its PR no longer match the current tab, the row stays listed at its position with its new status and stays selected. Tab counts update at once (as 0019 FR-9 does for cards). A confirmation, a refusal and revert, and a background sync update the row's data in place without removing it. `j`/`k` move off and back onto it like any row.
- **FR-9 When it goes.** A kept row leaves the tab, under the normal rules, when the list is rebuilt for a new view of the tab: switching tab, changing the repository filter, leaving the Pull requests view and returning, or restoring a list state through back/forward (0018 FR-5 restores the selection by id, and a PR no longer in the tab falls back as 0018 says).
- **FR-10 Scope.** Only PRs whose status the user changed from this list, or from a review opened from it, are kept. A change made in the review counts as made on its row: back to the list, the row is in place and selected (0019 AC-D5 still holds: it shows the new status). Rows that leave the tab because of anyone else's change (sync) leave as today.

## 5. ADO API surface and limits

| Purpose | Endpoint (7.1) | Scope | Cost / limit notes |
| --- | --- | --- | --- |
| A | **None.** Layout and paint only. | — | 0 requests. |
| B excerpts | `GET …/git/repositories/{repo}/blobs/{objectId}` (existing, 0004, 0019 FR-11) | `vso.code` | Same set of files as 0019 FR-11, ≤ 2 blobs per file ever, ≤ 4 in flight, user lane. Firing at 150 ms instead of ≈ 300 ms doesn't change the count. FR-2 queues more files on tall windows (one real screen below instead of 900 px). That is FR-11 as written, still bounded by the L+T worst case in 0019 §5 (≤ 300 GETs over a full read). |
| C | `PATCH …/pullrequests/{id}` (existing, 0019 FR-21) | `vso.code_write` | Unchanged: one per user action. Keeping the row adds no request. |

- **Rate limits:** no new TSTU cost. The dwell still ignores fast scrolls (0019 AC-B4), so a fling can't burst into the self-limit (300 requests / 5 min, 0016 FR-8).
- **No push to local apps:** C's kept row shows sync updates within the hot tier (≤ 60 s foreground, 0002), as every row does.
- **PAT scopes:** `vso.code` (B), `vso.code_write` (C), already in the one token (C6). Nothing new.

## 6. Acceptance criteria

Harness: `crates/gallery/tests/review_polish_smoke.rs` (headless Metal, `HeadlessAppContext`, real key dispatch, `FrameRecorder`, counting allocator) and 0019's view-test and mock-server harness with an injected clock. Fixtures S and L+T are 0019 §6's. Shell tests call `adoken_app::background::init(cx)`, or the `⌘P` copy and status commands stay disabled (`front_view` is `None`; `adoken_gallery::init` doesn't call it). In B's ACs the test draws a frame every 16 ms of injected time. "Queued" means a call into `PrReview::compute` for that file, read through a test probe or the mock's blob counter.

**A. First frame**

- **AC-A1 Debug gate** (frame recorder, debug). On L+T at 1280×800, with the first file selected in Files, 20 rounds of `t` (key to painted) then `t` back: p99 ≤ 12 ms (Q-1). On the first Threads frame, the last laid-out card is the last one intersecting the viewport (probe `threads_laid_out()`: no item starts below the viewport's bottom).
- **AC-A2 Lookahead fills, nothing moves** (frame recorder + screenshot). Within 2 frames after AC-A1's first frame, the laid-out region reaches one viewport height below the viewport. Each of those frames is ≤ 8 ms debug. The viewport's pixels in the first frame and in the frame after the fill are identical (image diff: 0 pixels).
- **AC-A3 One screen, not 900 px** (view test). At 1280×1600, after the fill, the laid-out region below the viewport is within one card of one viewport height (> 900 px). After resizing to 1280×800 it shrinks to match on the next fill.
- **AC-A4 Release number** (scripted manual check, reference Mac, run once, ≥ 6 GB free before building): `cargo test --release -p adoken-gallery --test review_polish_smoke` prints `t → threads` p99 ≤ 8 ms. The printed p50/p99 go into §7's "Today" column.
- **AC-A5 No regressions** (same smoke). Threads scroll top to bottom p99 < 16 ms (today 3.9 ms); `↩` from a computed card paints Files with the thread visible within one frame; the view's heap after reading every card stays < 10 MB (today +6.28 MB), including any FR-3 cache.

**B. Dwell timing**

- **AC-B1 Rest** (injected clock, L+T, no diffs computed but the selected file's). Scroll with the wheel and stop. The last frame that moves the cards is at t₀. No file is queued at t₀ + 149 ms. By t₀ + 182 ms (150 ms + 2 frames), exactly the files with a card intersecting the viewport or the one-screen window below are queued, minus the exclusions in FR-6.
- **AC-B2 Opening.** `t` with nothing cached: the same timing, measured from the first Threads frame. FR-1's lookahead growth frames don't delay it.
- **AC-B3 Pauses shorter than the dwell.** Scroll, rest 140 ms, scroll on: nothing is queued for the first rest. 0019 AC-B4's 1 s top-to-bottom scroll queues nothing.
- **AC-B4 Repaints that don't move the cards** (injected clock + mock sync). During a rest, at t₀ + 60 ms: focus a card with `n` without scrolling (the card is already on screen), and apply a sync that inserts a thread above the viewport while the focused card stays in place (0019 AC-B8). The dwell still fires by t₀ + 182 ms. A sync that inserts a card inside the viewport restarts it (fires 150 ms + ≤ 2 frames after that frame).
- **AC-B5 Once.** After the dwell fires, 2 s of idle frames queue nothing more. The mock sees ≤ 2 blob GETs per file over the whole run.
- **AC-B6 Harness.** `review_polish_smoke`'s `dwell` helper advances the clock once by 200 ms (its two-round loop and comment are gone), and the smoke passes.

**C. Selection after a status change**

- **AC-C1 Row stays** (view test + mock server, clipboard read back). To review lists `!1`, `!2`, `!3`, with `!2` selected. `⌘K` › Mark as draft: on the next frame `!2` is still second, selected, chip Draft with "Sending…". The tab label's count dropped by one. `⌘P` › "Copy source branch" puts `!2`'s source branch on the clipboard. The mock returns 200: the row is still there and selected. `⇧P` publishes it back.
- **AC-C2 Survives sync and moves.** With `!2` kept, a mock list poll that reports `!2` as draft keeps it listed and selected. `j` selects `!3`, `k` selects `!2` again.
- **AC-C3 Leaves on a fresh view of the tab.** Each of these drops `!2` from To review: Mine then To review; changing the repository filter; leaving to Today and back; `⌘[` then `⌘]` across the list (0018). After each, the selection follows the normal rules.
- **AC-C4 Abandon, review, refusal.** On Mine, `⇧A` `↩` on the selected row: the row stays, chip Abandoned, selected. From To review, `↩` opens `!2`, `⇧P` marks it draft, `Esc` back: `!2` is in place, selected, chip Draft. A 403 on Mark as draft: the row reverts in place and stays selected, with 0019 FR-21's toast.
- **AC-C5 Others' changes still leave.** A sync that changes `!3` (not acted on) so it no longer matches To review removes it on that update, and the selection on `!2` doesn't move.
- **AC-C6 Smoke.** AC-S2 in `review_polish_smoke` runs Mark as draft from To review (no switch to All first), then `⌘P` › "Copy source branch" reads `feature/retry`.

## 7. Performance budget

| Interaction | Budget | Today (2026-10-02) | Measured how |
| --- | --- | --- | --- |
| `t` → Threads painted, L+T 1280×800, debug | p99 ≤ 12 ms (≥ 25% under one 60 Hz frame; Q-1) | 15.4–15.7 ms (13.5 ms with lookahead 0) | Frame recorder (AC-A1) |
| `t` → Threads painted, L+T, release | p99 ≤ 8 ms (one 120 Hz frame) | to measure | Smoke under `--release`, once (AC-A4) |
| Frames laying out the lookahead after `t`, debug | each ≤ 8 ms; ≤ 2 frames | — | Frame recorder (AC-A2) |
| Scroll Threads view, L+T, debug | p99 < 16 ms | p50 2.9 · p99 3.9 · max 4.4 ms | Frame recorder (AC-A5) |
| `↩` from a computed card | ≤ 1 frame | 5.1–6.1 ms | Frame recorder (AC-A5) |
| Rest → excerpt files queued | 150 ms ≤ t ≤ 150 ms + 2 frames | ≈ 300 ms | Injected clock (AC-B1) |
| Heap added by the Threads view, L+T | < 10 MB, caches included | +6.28 MB | Counting allocator (AC-A5) |
| Status change from the list → row repainted | next frame, row in place | next frame, row gone | View test (AC-C1) |
| Cold start | no change (< 500 ms) | — | Launch timer |

Global rules (AGENTS.md) always apply: frame < 16 ms, Cmd+P < 50 ms on 100k items, cold start < 500 ms. Regressions are bugs.

## 8. Success signal (telemetry-free)

- **Harness:** `review_polish_smoke`'s `t → threads` line stays ≤ 12 ms debug on every run through the v0.1 freeze. The release number is recorded in §7.
- **Dogfood:** on the owner's 120 Hz display, no "hitch on `t`" or "code shows up late" note in the dogfood log over a week of returning-to-a-PR sessions.
- **Support signal:** no "I copied the wrong branch" or "my PR vanished from the list after I drafted it" from beta users.

## 9. Risks

- **The debug gate may need more than FR-1.** FR-1 is measured at ≈ 13.5 ms, above 12. Meeting 12 needs FR-3's profile-led work, whose gain is unknown. Mitigation: Q-1's fallback. The release number (AC-A4) decides whether debug is the right thing to optimize.
- **A fork patch.** FR-1 needs `ListState::set_overdraw` in `zeronsh/zui`. adoken and zeron pin the same rev (root `Cargo.toml`), so the patch moves both pins or adoken carries a `[patch]` until zeron follows. One small method, but a fork change all the same.
- **Pop-in on the growth frame.** If the user scrolls during frame 1, unmeasured cards below could shift the scrollbar. Mitigation: AC-A2's pixel check, and gpui's existing estimated heights for the scrollbar.
- **Disk for the release run.** A release build of the gallery test and its deps takes several GB, with ≈ 9 GB free. Mitigation: one manual run after checking ≥ 6 GB free, never in CI, never `cargo clean`.
- **A list that briefly disagrees with its tab** (C). A draft sits in To review until the tab is refreshed. Mitigation: its chip says Draft, the count already excludes it, and 0019 FR-9 already does the same for cards.
- **Debug numbers depend on the machine.** Gates are read on the reference Mac. Other machines report but don't gate.

## 10. Open questions (for the user)

- **Q-1 Debug gate for `t` → Threads painted.** Recommendation: p99 ≤ 12 ms in debug on L+T (25% headroom under 16 ms, so the next card feature doesn't tip it), and ≤ 8 ms in release. Fallback, decided by data, not by a new question: if after FR-1 and FR-3's profile pass debug is still > 12 ms but release is ≤ 8 ms, gate debug at 14 ms and record why in §7.
- **Q-2 How to cut the first frame.** Recommendation: option 1, viewport-only first frame, lookahead grown to one screen on the next frame, via a one-method zui patch (`ListState::set_overdraw`). Cache shaped card headers (option 3) only if a profile shows header shaping ≥ 2 ms and the gate still fails. Options 2, 4 and 5 are cut (§4.1).
- **Q-3 Selection after a status change on the list.** Marking a draft from To review moves the PR out of the tab and the selection to another row, so a following `⌘P` › Copy source branch copies the wrong row's branch. Recommendation: (a) the acted-on row stays in place, selected, with its new status, until the tab is shown again from scratch (tab switch, repository filter, leaving the view); counts update at once (FR-8–FR-10). Alternatives: (b) keep today's behavior and make the copy commands act on the last acted-on PR (hidden state, surprising elsewhere); (c) follow the PR into the tab it now belongs to (the list jumps under you); (d) drop the selection to nothing (the next command is disabled, which is safe but a dead end).
- **Q-4 Slot.** Recommendation: v0.1. Groups A and B join 0019 group B on the cut line (item 3): if the Threads view moves to v0.2, they go with it. Group C is not on the cut line, because it fixes a wrong-target bug in 0019 group D, which isn't on the line either. Build order by RICE: B (300), C (135), A (37.5).

## 11. Out of scope

- The palette's `palette.latency` of 170–318 ms in the headless run. It includes the test's settle loop and a debug build, so it isn't a valid measure. Palette latency stays with 0003's harness (Cmd+P < 50 ms rule).
- The real demo-app smoke (`ADOKEN_DEMO=1` binary, screencapture, `pbpaste`, `⌘K` items, `⌘Q`) and 0019's uncovered measurements (AC-B4's mock-counted blob requests, §7's 2,000-row group-filtering bench) stay 0019's own follow-ups, not this spec.
- Release-build perf gating in CI: cut (disk). One manual run (AC-A4).
- Prewarming the Threads view offscreen, and placeholder conversations on the first frame: cut (§4.1, options 4 and 5).
- Keeping rows that others' changes (sync) remove from a tab: unchanged (FR-10) → `docs/ideas.md` if asked.
- Any change to what the Threads view shows or to the dwell length (150 ms): unchanged from 0019.

## 12. References

- Pull Requests — Update (`isDraft`, `status`): https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/update?view=azure-devops-rest-7.1
- Blobs — Get Blob: https://learn.microsoft.com/en-us/rest/api/azure/devops/git/blobs/get-blob?view=azure-devops-rest-7.1
- Rate limits: https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/rate-limits?view=azure-devops
- gpui `list` and `ListState` (overdraw fixed in `ListState::new`): `zeronsh/zui` rev `18a89af`, `crates/gpui/src/elements/list.rs`.
