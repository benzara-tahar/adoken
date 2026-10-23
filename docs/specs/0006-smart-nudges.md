# 0006 — Smart nudges: stale PRs and my active work, at most a few times a day

| Field | Value |
| --- | --- |
| Status | Draft |
| Milestone | v0.0.5 → v0.1.0 |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/notify` (nudge engine), `crates/app` (Today card), `crates/store` |
| Serves | Less noise (replaces "did anyone look at my PR?" pings and standup archaeology) |
| Depends on | `docs/specs/0002-sync.md` (state + events), `docs/specs/0005-notification-center.md` (delivery, quiet hours, snooze) |
| Design | `docs/design/today.md` (Today; rewritten for spec 0017, accepted 2026-10-01), `docs/design/principles.md`, `docs/design/ken.md` (optional playful copy) |

## 1. Problem

PRs rot. A review request sits for two days because it scrolled out of an email inbox; an author doesn't notice a "wait for author" vote; a PR sits with a failed build nobody looked at; work items stay "Active" for weeks after the code merged. ADO shows all of this only if you go looking, hub by hub. Teams compensate with Slack pings and standup questions, which is more noise.

## 2. Users and job to be done

- **Persona:** reviewer who forgets requests; PR author waiting on others; dev preparing for standup.
- **JTBD:** "When something I own or owe is stuck, I want one gentle, well-timed reminder with the next action, so nothing rots and nobody has to chase me."
- **Smallest delightful slice:** a morning **Today** card plus a small set of precise, rate-capped nudges computed from local data.

## 3. UX flow

> **Today layout superseded (accepted 2026-10-01, spec 0017):** item 1's layout (ribbon, Focus card, card grid, age rings, track lines and their order: FR-11's age ring, FR-12's order clause, FR-13, AC-15) is replaced by [`0017-today-redesign.md`](0017-today-redesign.md); nudge logic (types, thresholds, caps, Focus choice FR-10, waiting order FR-11, stages and blockers FR-12, triage) is unchanged. Item 1 below is kept as the 2026-09-30 history.

1. **Today ("my day").** *Revised by user decision 2026-09-30* (feedback: the old card of summary lines plus lists was "a list of lists"; layout and visuals in [`docs/design/today.md`](../design/today.md)). Today is a calm, glanceable surface of faces, glyphs and numbers, top to bottom:
   - **Ribbon**: a thin timeline of today's working hours (stretched to cover earlier or later events and now) with a "now" marker and a glyph per event: my votes on others' PRs, my PRs that merged, failed builds I requested or on my PRs, mentions of me. Counts beside it ("👁 3 ⎇ 1 ✕ 2 @ 4"), plus the Ken review streak while the Ken layer shows it.
   - **Focus**: one hero card with the single most important next action (FR-10), e.g. "Mara is blocked on you · !4821 Fix login throttle · 3 files · 2d" with her avatar in an age ring and the nudge's action on `↩`; "Clear. Nothing waits on you." when nothing does. This is the Today card of AC-9: it shows on workdays from the start of working hours and `e` hides it until tomorrow.
   - **Waiting on you**: reviews I owe as cards (author avatar in an age ring whose color warms with time, short title, age, changed files, active discussions), blocked-longest first (FR-11).
   - **Your PRs**: each of my active PRs as a Draft → Review → Approved → Merged track with a dot at its position, reviewer avatars by vote, and one blocker glyph; my PRs merged today sit at the end (FR-12).
   - **Loose ends**: my in-progress work items with N6 (merged, still open) or a due N7.
   Nudges are not listed separately: they pick the Focus and order the cards, and each due nudge is attached to its subject's card, so `s` snooze, `d` done, `x` not for this item (and ⌘P "Turn off this nudge type", `c` copy names for N4) work on the selected card. Arrows (`↑↓←→`, `hjkl`) move between cards across sections; `↩` opens the PR review, work item, or failing run.
2. **Nudges** arrive as P2 notifications (0005) with one primary action:
   - "Review requested 26 working hours ago: !912 Add login throttle — **Review now**"
   - "!870 has a wait-for-author vote from Kim since yesterday — **Open**"
   - "!870 build failed 3 h ago and nobody has pushed — **Open failing run**"
   - "#48213 is Active but its PR !901 merged 2 days ago — **Open in ADO** to close it"
   - "#47001 has been Active with no linked PR or change for 7 working days — **Still on it?**"
3. Every nudge has `Cmd+K` actions: Snooze (0005 menu), "Not for this item", "Turn off this nudge type".
4. **Settings → Nudges:** per nudge type on/off + threshold; working days and hours (used for all "working hours" math and by 0005 snooze presets); daily cap.

### States

| State | Behavior |
| --- | --- |
| Nothing stuck | Focus reads "Clear. Nothing waits on you."; no nudges. |
| First sync incomplete | No nudges until hot + warm tiers have completed once (avoid false "stale" from partial data); Focus reads "Syncing" and makes no claim. |
| Stale data (tier failing) | Nudges that depend on that tier are suppressed until fresh. |
| Quiet hours / weekend | Nudges are held by 0005 rules; stale timers pause outside working hours. |

## 4. Functional requirements

- **FR-1 Nudge types (v0.1).**

  | Id | Nudge | Trigger (all times in working hours) | Default threshold |
  | --- | --- | --- | --- |
  | N1 | Review waiting on me | I'm a reviewer with vote 0 on an active, non-draft PR for ≥ T since I was added | 24 working h |
  | N2 | My PR: waiting for author | Someone voted −5 or −10 on my PR and I haven't pushed or replied since | 4 working h |
  | N3 | My PR: build failing | Latest policy build on my PR failed and no push since | 2 working h |
  | N4 | My PR: no reviewer activity | My active non-draft PR has no votes and no human comments from others for ≥ T | 24 working h |
  | N5 | My PR: merge conflicts | `mergeStatus = conflicts` on my PR | immediate (once) |
  | N6 | Work item: merged but still active | Work item assigned to me in an in-progress state category, linked PR completed ≥ T ago | 24 working h |
  | N7 | Work item: quiet active item | Work item assigned to me in an in-progress state category, no change and no linked PR activity for ≥ T | 5 working days |

- **FR-2 "Since I was added" (N1).** Use the reviewer-added system thread timestamp when threads are synced (hot set) `[INFERENCE: ADO emits a system thread for reviewer changes; verify property names on fixtures]`; otherwise fall back to PR creation date.
- **FR-3 State categories.** "In progress" uses the process's state category (`InProgress`) rather than state names, so custom processes work. Read from work item type states metadata (cached, cold tier).
- **FR-4 Rate caps.** Each nudge fires at most once per subject per 24 h; at most **3 nudges per day** total by default (configurable 0–10), prioritized N5 > N3 > N2 > N1 > N6 > N4 > N7, then by age. Excess nudges are not queued; they stay visible on Today, on their subject's card.
- **FR-5 Working-time math.** Thresholds count only configured working days/hours in the user's local time zone (default Mon–Fri 09:00–18:00). Public holidays are out of scope.
- **FR-6 Resolution.** A nudge auto-resolves (marked done in 0005) when its trigger condition stops being true (e.g. I voted, author pushed).
- **FR-7 Local computation.** Nudges are computed from the store after each warm-tier pass; they cause zero additional ADO requests.
- **FR-8 Opt-outs** are remembered per subject ("Not for this item") and per type; a type turned off no longer picks the Focus or shows as a track blocker or loose end. A snoozed, done or muted subject keeps its card on Today, dimmed with a glyph and sorted last, and is never the Focus.
- **FR-9 N4 names who to ping.** An N4 nudge lists the reviewers with no vote, required reviewers first (e.g. "Waiting on Kim (required), Sam"). adoken never messages them; the only actions are Open, Copy names, Snooze, opt-outs.
- **FR-10 Focus choice** (user decision 2026-09-30). The Focus is the first due nudge in FR-4 order that isn't snoozed, done or muted; if none is due, the review waiting on me the longest (shown as not yet due); otherwise "Clear". Before the first full sync it reads "Syncing". Its face is the PR author for N1 and the reviewer whose wait-for-author or reject vote is oldest for N2; other kinds show the nudge's glyph.
- **FR-11 Waiting on you** (user decision 2026-09-30). One card per N1 condition (regardless of threshold), ordered by when I was asked (FR-2), oldest first; parked cards last. Age heat: cool under the N1 threshold, warm from 1× to 3×, hot from 3×. (The age ring that filled at 3× is dropped by spec 0017 FR-23; the age number keeps the heat tone.)
- **FR-12 PR tracks** (user decision 2026-09-30). Stage: Merged when completed; Draft while draft; Approved when at least one reviewer approved (10 or 5), every required reviewer approved, and nobody voted −5 or −10 (declined reviewers and I don't count); otherwise Review. One blocker, highest first: merge conflict (N5), build failed (N3), changes requested (N2), waiting on reviewers (N4, only once past its threshold), with how long it has held; turned-off types don't block. Row order is spec 0017 FR-14's (most recently active first; blockers show on the row, not by position), superseding the 2026-09-30 order "blocked first by blocker priority then oldest block, then by last update; parked after; PRs merged today last".
- **FR-13 Ribbon** (user decision 2026-09-30). **Superseded by spec 0017 FR-23 (accepted 2026-10-01):** the ribbon is dropped; its reviews and merges are Today's counts row, failed builds are row blockers, mentions stay in the Inbox. Zero additional requests still holds (FR-7).

## 5. ADO API surface and limits

- **No additional endpoints.** Inputs come from 0002: PR list rows (votes, `mergeStatus`, draft, creation date), hot-set threads (reviewer-added times, human comments), builds for PR branches, "mine" work items with `System.State` and links, work item type state categories (`GET /{project}/_apis/wit/workitemtypes/{type}/states`, `vso.work`, cold tier, cached).
- **Rate limits:** zero incremental cost.
- **No push:** a nudge can be late by at most one warm interval; thresholds are hours, so irrelevant.
- **PAT scopes:** `vso.code`, `vso.work`, `vso.build` (read). N6/N7 only link to ADO to close/update items because v0.1 does not request `vso.work_write`.

## 6. Acceptance criteria

Harness: store fixtures + injected clock + fake notification sink.

- **AC-1** Given I was added as reviewer Monday 10:00 with working hours Mon–Fri 09:00–18:00 and threshold 24 working h, N1 fires Wednesday 16:00 (not Tuesday 10:00).
- **AC-2** Given the N1 subject gets my vote before the threshold, no nudge fires; given it fires and I vote afterward, the nudge is marked done within one warm pass.
- **AC-3** Given 6 eligible nudges on one day and the default cap of 3, exactly 3 fire, chosen by priority order N5 > N3 > N2 > N1 > N6 > N4 > N7, and next morning Today shows all six, each on its subject's card.
- **AC-4** The same subject never produces two nudges within 24 h, including across app restarts.
- **AC-5** Given a work item in a custom state "Doing" whose category is `InProgress`, N7 treats it as in progress.
- **AC-6** Given "Not for this item" on !912, no N1–N5 nudge fires for !912 again; other PRs are unaffected.
- **AC-7** Nudges firing during quiet hours are held by 0005 and appear in the inbox without banner (verified via the fake sink).
- **AC-8** With the warm tier failing for 3 intervals, no nudge depending on it fires.
- **AC-9** The Focus card (the Today card) appears once per workday on first foreground after working-hours start; `e` hides it until the next workday.
- **AC-10** Nudge evaluation causes zero requests on the mock ADO server.
- **AC-11** Given my PR !870 eligible for N4 with reviewers listed by ADO as Sam (optional, vote 0) then Kim (required, vote 0), the N4 nudge text reads "Waiting on Kim (required), Sam" (required first), and producing it triggers zero write requests to the mock server.
- **AC-12** Given due nudges N5 on !873 and N2 on !870, the Focus is !873 ("Your PR has a conflict"); after snoozing !873 (and the N3 on !871) it is !870 ("Kim asked for changes", Kim's face). Given only a review asked 1 h ago, the Focus is that review, not due; after marking it done, the Focus reads "Clear".
- **AC-13** Given reviews asked Tuesday 10:00, Monday 10:00 (snoozed) and Wednesday 09:00, Waiting on you lists Tuesday, Wednesday, then the snoozed Monday card.
- **AC-14** Given my PRs with a conflict, a failed build, a wait-for-author vote, and no reviewer activity for 24 working hours, their blockers read Conflict, Build, Changes, No reviews; on Monday noon the last one has no blocker yet. (Row order is spec 0017 AC-16's.)
- **AC-15** Superseded by spec 0017 (the ribbon is dropped, FR-13); Today's counts are tested by 0017 AC-3–AC-12.

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Full nudge evaluation, 1k PRs + 5k work items | < 20 ms off main thread | Bench |
| Today render (from the precomputed day) | < 16 ms | Frame recorder |
| Memory overhead | < 5 MB | Heap profile |

## 8. Success signal (telemetry-free)

- Dogfood: builder's median review-request age at vote drops week over week (local-only stat "Median time to first vote" in Settings → About).
- Build-in-public: "adoken told me my PR was blocked before my lead did" replies.
- The daily cap is never raised by the builder (if it is, nudges are too timid or too noisy — both are learnings).

## 9. Risks

- **False positives erode trust fast.** Hence caps, no nudges on partial data, and auto-resolution.
- **Team norms differ** (24 h may be rude or lax). Thresholds are per type and easy to change.
- **Reviewer-added timestamps** may be unavailable outside the hot set → fall back to creation date (can over-nudge on old PRs where I was added late). Mitigation: N1 requires the PR to be in the hot set, which it always is when I'm a reviewer (0002 FR-2).

## 10. Open questions — resolved 2026-09-25

All recommendations accepted by the user.

- **Q-1** Default daily cap? **Resolved:** 3 nudges per day (configurable 0–10); the Today card carries the rest (FR-4).
- **Q-2** Should N4 suggest who to ping? **Resolved:** yes — list reviewers with no vote, required first; no auto-messaging (FR-9, AC-11).
- **Q-3** Add `vso.work_write` in v0.1 for one-key "Close work item"? **Resolved:** no; N6/N7 link to ADO. Revisit with work item quick actions in v0.2 (`docs/roadmap.md` v0.2 item 5). **Superseded 2026-10-01:** spec 0015 edits work items in v0.1, and the Review token holds `vso.work_write` (0001 §5.2).

## 11. Out of scope

- Nudging *other people* (posting comments, pinging reviewers) — adoken never acts on the user's behalf.
- Team-level stale PR dashboards (Later: team features).
- Holiday calendars, per-repo working hours.
- Pipeline approval nudges (needs approvals API + `vso.build_execute`; `docs/ideas.md`).

## 12. References

- Pull Requests — Get Pull Requests (votes, `mergeStatus`, draft): https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/get-pull-requests?view=azure-devops-rest-7.1
- Work Item Type States — List (state categories): https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-item-type-states/list?view=azure-devops-rest-7.1
- Pull Request Threads — List: https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-threads/list?view=azure-devops-rest-7.1
