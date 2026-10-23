# 0022 — Celebrations for finished work

| Field | Value |
| --- | --- |
| Status | Accepted (2026-10-01, owner: all recommendations; overrides noted) |
| Milestone | v0.1 (Q-9). Group A (calm celebrations, Reduce Motion fallback, sounds, settings) on the cut line right after the Ken layer; group B (arcade style) ships only with the Ken layer and is cut with it |
| Owner | product-manager (spec) · gpui-engineer (build) · ux-designer (visuals, seal asset, sound recipes) |
| Crates | `crates/ken` (pure, clock-injected celebration rules next to `sparks.rs`), `crates/app` (`ken/layer.rs` overlay, `workitems/writes.rs` and review completing triggers, Settings › General, palette), `crates/sound` (two `ui` events and their recipes), `crates/gallery` |
| Serves | **Snappy:** the moment Azure DevOps confirms a finished piece of work is felt on that frame, so there is no glance back at the chip or the browser to check it landed. **Less noise:** only forward moves into review or done, and your own merges, celebrate; the dozens of other writes in a day stay silent, and bursts are rate-limited |
| Depends on | `docs/specs/0015-work-item-editing.md` (FR-6 optimistic writes, FR-7 server wins, FR-8 conflicts, FR-9 rule violations, FR-11 state menu, FR-33 type metadata cache), `docs/specs/0004-pr-review.md` (FR-10 asynchronous Complete), `docs/specs/0002-sync.md` (FR-15 writes, event stream), `docs/specs/0005-notification-center.md` (quiet hours), `docs/specs/0007-theming-backgrounds-sounds.md` (FR-1 tokens, FR-7 sounds, FR-8 reduce motion), `docs/specs/0009-settings.md` (`settings.toml`, Settings › General), `docs/specs/0017-today-redesign.md` (its static "Merged today" glyph, contract C3) |
| Design | `docs/design/ken.md` §12 Celebrations (styles, confetti, seal, static fallback, copy, assets) and §3.3 (K.O. handoff); `docs/design/sound.md` §2.1, §4, §7 (events, quiet-hours matrix, recipes); existing tokens only (`docs/design/tokens.md` §2.4, §2.5, §2.9, §7) |

> Rule: no code without an **Accepted** spec (AGENTS.md). The spec owns behavior, acceptance, and budgets; design docs own visuals, motion, sound, and copy.

## 1. Problem

Owner, 2026-10-01 dogfood notes: "For some actions, we need a very special effect that gives the sensation of accomplishment and pushes more dopamine to the developer. Whenever we complete a PR, when we click on the button, we should have a confetti effect. The actions I'm thinking about are when we turn a work item to Done, Code Review, or Committed. Each one has a different level of accomplishment: Code Review means we already created the PR, so that is something; Committed or Done means it passed review and QA testing, so we get a much stronger effect." Today, finishing work in Azure DevOps feels like filling a form: a merge or a move to Done ends with a page reload. In adoken it ends with a pending clock quietly disappearing (0015 FR-6) or, only with the opt-in Ken layer, a K.O. that arrives a few seconds later on the next ingest poll (`crates/app/src/ken/layer.rs` `POLL` = 2 s) and only for PRs you authored. Work item transitions have no moment at all.

## 2. Users and job to be done

- **Persona:** a developer who moves their own work items through the board during the day and completes PRs (their own, or ones they review and are allowed to merge).
- **Job to be done:** "When I finish something real (my PR merges, my work item reaches review or done), I want adoken to mark the moment, scaled to how big it is, so my day feels like progress, without slowing me down or getting in my way."
- **Smallest delightful slice:** in the work item detail, `s`, "done", `↩`: about 300 ms later Azure DevOps confirms, and on that frame a confetti burst rises from the state chip. "Code Review" gets a small puff. Complete on a PR: confetti from the status chip when Azure DevOps confirms the merge.
- **Priority (RICE, `docs/ideas.md` scoring):**

  | Group | Scope | Reach | Impact | Confidence | Effort | RICE |
  | --- | --- | --- | --- | --- | --- | --- |
  | **A** | Calm celebrations (work items, PRs), tiers, Reduce Motion fallback, rate limits, two sounds, settings, palette | 80 | 1 | 70% | 1 wk | **56** |
  | **B** | Arcade style for Ken users (seal stamp, sparks), K.O. handoff | 20 | 1 | 60% | 0.5 wk | **24** |

- **Already in place:** the Ken overlay layer, pointer-transparent and focus-free (`crates/app/src/ken/layer.rs`); the particle system, ≤ 48 sparks (`crates/ken/src/sparks.rs`, ken.md §4.4); surface gating by reduce motion, quiet hours and calm mode (`crates/ken/src/settings.rs` `KenSettings::surfaces`); the K.O. for my merged PRs (`crates/ken/src/engine.rs` `Effect::Ko`, ken.md §3.3); the confirmed-write hook (`WiEdits::answered`, `crates/app/src/workitems/writes.rs:1301`, which already plays `ui.action_done`); the "completing" mark set by my accepted Complete and cleared when the PR leaves Active or after 10 min (`crates/sync/src/writes.rs:897`, `crates/sync/src/engine.rs` `COMPLETING_STALE_MS`); the completing re-reads at 2 s, then 4 s and 8 s later (`crates/sync/src/scheduler.rs:59-61`).
- **Cut:** celebrations for other people's changes or anything sync observes; Today rows (they get 0017's static glyph, contract C3); auto-complete merges (Q-4); Ken scoring of work item moves (Q-5); a per-type state editor UI (v0.2, Q-7); Dock bounce, OS banners, haptics, full-screen overlays outside the Ken K.O.

## 3. UX flow

Visuals, motion, copy and assets: `docs/design/ken.md` §12. Sounds: `docs/design/sound.md` §2.1, §7.

### 3.1 Tiers and styles

| Tier | Moments (defaults, FR-2) | Calm style (default) | Arcade style (Ken arcade, FR-7) | Static fallback (Reduce Motion, rate limit) |
| --- | --- | --- | --- | --- |
| **Step** | A work item of mine moves forward into a review state ("Code Review", "In Review", "Ready for Review", "Review") or any state in the Resolved category | Confetti puff from the state chip, ≈ 0.7 s | 12 sparks from the chip | `sparkles` glyph beside the new state for 3 s |
| **Milestone** | A work item of mine moves forward into "Committed", "Done", or any state in the Completed category; a PR I completed in adoken is confirmed merged | Confetti burst from the chip, ≈ 1.4 s | Seal stamp on the chip plus 32 sparks | Glyph for 3 s plus a toast |
| (K.O.) | A PR **I authored** and completed in adoken merges, with Ken's K.O. on | none: the K.O. is the celebration (FR-6) | K.O. overlay (ken.md §4.2) | K.O. toast (ken.md §4.2) |

How the defaults land on the stock processes (categories from Microsoft Learn, §12):

| Process · type | Step | Milestone | No celebration |
| --- | --- | --- | --- |
| Agile · User Story, Bug | Active → Resolved | → Closed | → New, → Active, → Removed |
| Agile · Task | — | Active → Closed | → New, → Active |
| Scrum · Product Backlog Item, Bug | — | Approved → Committed (Q-3), → Done | → New, → Approved, → Removed |
| Scrum · Task | — | In Progress → Done | → To Do, → In Progress |
| Basic · Issue, Task | — | Doing → Done | → To Do, → Doing |
| CMMI · Requirement, Bug | Active → Resolved | → Closed | → Proposed, → Active |
| Owner's process (custom) | Active → Code Review | Code Review → Committed, → Done (once per item per 24 h, FR-3) | backward moves |

### 3.2 Happy path: work item

1. Work items, `↓` to "Timezone bug in daily settlement job" (Active). `s`, type "done", `↩`.
2. The chip shows Done with the pending clock and "Sending…" (0015 FR-6). No effect yet.
3. Azure DevOps answers 200 (≈ 300 ms). On the frame the pending clock clears, a confetti burst rises from the chip and settles in ≈ 1.4 s. With sounds on, `ui.celebrate_milestone` plays instead of `ui.action_done`.
4. Keys and clicks keep working during the burst: `↓` moves to the next item at once.

### 3.3 Happy path: pull request

1. In the review of !4821, Complete, `↩` in the sheet. Azure DevOps answers 2xx with `mergeStatus: queued`. The status chip reads "Completing…" (0004 FR-10). This is the click's instant feedback; nothing celebrates yet (Q-2).
2. The first re-read (2 s after the Complete) sees `completed`. On the frame that store change is applied, the chip reads "Completed" and a confetti burst rises from it.
3. If !4821 is mine and Ken's K.O. is on, the K.O. plays on that frame instead (no confetti, ken.md §3.3).

### 3.4 Alternate paths

- **Rule refusal, conflict, failure:** nothing celebrates until a write succeeds. After filling a required field (0015 FR-9) or **Apply mine** (FR-8), the confirmed write celebrates once.
- **Backward or sideways:** Done → Active, Code Review → Active, Closed → Resolved: no effect, `ui.action_done` as today.
- **Bounce:** Code Review → Active → Code Review on the same day: the second entry doesn't celebrate (24 h item memory, FR-3). Committed → Done after Committed already celebrated: no second milestone.
- **I moved away:** the item or PR isn't on screen when the confirmation lands (another view, closed review): a milestone shows its toast and bursts from it; a step shows nothing extra.
- **adoken in the background** (window not key or minimized) when the confirmation lands: no effect, nothing queued for later.
- **Many at once** (closing a sprint's tasks): the first full effects play; inside 1.5 s of the previous one, or past 5 in 10 min, the static glyph shows instead (FR-11).
- **Merge fails** after the 2xx (rejected by policy, conflicts): no celebration; the existing "Merge rejected by policy" notice shows (0004 FR-10).
- **Demo org:** same path through the demo writes (`apply_local`, ≤ 300 ms delay); good for the gallery and posts.

### States

| State | Behavior |
| --- | --- |
| Empty | Not applicable. |
| Loading / first sync | Nothing celebrates: sync-observed changes never do (FR-1). |
| Error | A failed write never celebrates; its existing error and `ui.error` show. |
| Offline / stale | No writes are sent offline (0015 FR-10), so nothing celebrates. A PR merge seen after reconnect celebrates only if the completing mark is still set (≤ 10 min after the click). |
| Throttled by ADO | The celebration waits for the confirmation however long the governor holds the write (0015 §States); the PR celebration waits for the re-read. |
| Reduce Motion | Static fallback (FR-9). |
| Quiet hours | Calm style only; sounds per `docs/design/sound.md` §4 (FR-10). |
| Calm mode *(Amended 2026-10-02 by 0025 FR-8: now Quiet mode)* | No celebration at all (FR-10). |

## 4. Functional requirements

### Triggers

- **FR-1 Own confirmed actions only.** A celebration has exactly two triggers:
  - **Work item:** a state change I made in adoken (state menu, the palette's `⌘K` "Change state…", FR-8 **Apply mine**, FR-9 resend with the required field) answered with success by Azure DevOps (`WiEdits::answered` with `Ok(WiOutcome::Applied)` for a `WiChange::State`).
  - **Pull request:** a PR whose Complete I pressed in adoken, confirmed `completed` by any sync pass while this install's completing mark is still set (≤ 10 min after the accepted Complete, `COMPLETING_STALE_MS`).

  Never: changes sync observes (other people's, mine made in the browser, first sync, backfill, rebuild), work items Azure DevOps transitions itself when a PR completes (the Complete sheet's "transition linked work items"), auto-complete merges (Q-4), pipelines, votes, comments.
- **FR-2 Tier of a state.** Resolved from the confirmed state (the answer's state, not the requested one; server wins, 0015 FR-7) and its category from the type metadata (0015 FR-33: `states[].category` of `workitemtypes/{type}`). Only when that metadata isn't cached, the category falls back to `StateCategory::of_state` (`crates/model/src/lib.rs:373`), which only knows stock names. In order:
  1. Category Proposed or Removed: no tier, whatever the lists say.
  2. Name in `milestone_states`, or category Completed: **Milestone**.
  3. Name in `step_states`, or category Resolved (Q-8): **Step**.
  4. Otherwise: no tier.

  Names match case-insensitively after trimming. A name in both lists counts as Milestone.
- **FR-3 Forward moves only, once per item.** A state's rank is (category order, tier), compared category first: Removed < Proposed < In Progress < Resolved < Completed, then none < Step < Milestone. A work item write celebrates only when all hold:
  - the new state has a tier;
  - its rank is higher than the rank of the state the write started from (the write's base state);
  - its tier is higher than any tier already celebrated for that item in the last 24 h (in memory, per session).
- **FR-4 Per-surface switches.** `pull_requests = false` turns off PR celebrations, `work_items = false` turns off work item celebrations, `enabled = false` turns off both. Off means no effect, glyph, toast or celebration sound; the normal write feedback (`ui.action_done`, the pending clock clearing) is unchanged. Ken's K.O. has its own switch (`[ken] ko`) and isn't affected.

### Timing and no double celebration

- **FR-5 Timing.** The effect starts on the frame that applies the confirmation: for a work item, the frame where the pending clock clears; for a PR, the frame where the store change showing `completed` is applied, whatever view is open. Never on the click, and never on a PR's 2xx (`mergeStatus: queued` isn't a merge; principles §1.4). The instant feedback for the Complete click stays "Completing…" (0004 FR-10; Q-2).
- **FR-6 One celebration per moment.**
  - When Ken's K.O. applies to a merge (ken.md §3.3: Ken on, `ko` on, I authored the PR, closed within 24 h), the K.O. is the celebration. It starts on FR-5's frame instead of the next Ken ingest poll. 0022 then shows no confetti, glyph, toast or celebration sound. Ken's later ingest of the same merge scores it once and shows nothing again.
  - A write that celebrates plays its celebration sound instead of `ui.action_done`, never both.
  - A PR completion that also transitions linked work items celebrates once, for the PR (FR-1).

### Effects

- **FR-7 Style.** **Arcade** when Ken is enabled with intensity `arcade` and `particles` on, Reduce Motion is off, quiet hours aren't active and calm mode is off. Otherwise **calm**. Ken `subtle` uses calm. Visuals per ken.md §12. *(Amended 2026-10-02 by 0025 FR-8: "calm mode" here reads Quiet mode)*
- **FR-8 Anchor and placement.** The effect comes from the element showing the new state: the work item's state chip, else its visible list row; the PR's status chip in the review header, else its visible PR list row. With no visible anchor, a Milestone shows its toast (ken.md §12.5) and bursts from it; a Step shows nothing extra. Effects paint in the Ken overlay layer (`z.ken`, present with Ken off), never take focus, never intercept the pointer or keys, never dim, block or shake the content, and contain no flashes. When adoken's window isn't key or is minimized at FR-5's frame, nothing shows and nothing is queued.
- **FR-9 Reduce Motion.** With Reduce Motion on (0007 FR-8, `[accessibility] reduce_motion`), no particle, seal or chip animation runs and no animation frame is requested. Instead, the `sparkles` glyph (14 px, `status.success.fg`, the same glyph as Today's "Merged today" row, contract C3) appears beside the new state in the anchor for 3 s, appearing and disappearing without a transition. A Milestone also shows its toast. The celebration sound still plays (sound isn't motion).
- **FR-10 Quiet hours and calm mode.** During quiet hours (0005), effects use the calm style; sounds follow sound.md §4's `ui` column (`quiet_hours.ui_sounds`). Calm mode (ken.md §7) shows no celebration at all and plays `ui.action_done` as usual. The calm mode commands work with Ken off (Q-6). *(Amended 2026-10-02 by 0025 FR-8: calm mode is replaced by Quiet mode ("Quiet mode: turn on or off", "Quiet mode for 1 hour"), which also silences every sound, so no `ui.action_done` plays)*
- **FR-11 Rate limits.** Per session, in memory, clock-injected:
  - a full effect starts at most once per 1.5 s;
  - at most 5 full effects per rolling 10 min.

  A trigger over either limit gets the static fallback (FR-9) and `ui.action_done` instead of a celebration sound. The 24 h item memory (FR-3) applies before the limits.
- **FR-12 Budgets.** Every effect ends within 1.6 s. At most 64 particles are live window-wide, shared with Ken's sparks (Ken alone stays ≤ 48, ken.md §4.4); a burst that would exceed the cap is trimmed. Animation frames are requested only while a particle, seal, glow or glyph timer is live: no frames, timers or wakeups otherwise. Nothing is allocated, rasterized or decoded before the first full effect of the session, and nothing at all while `enabled = false`.

### Sound, settings, palette

- **FR-13 Sounds.** Two `ui` events (sound.md §2.1): `ui.celebrate_step` and `ui.celebrate_milestone`, on by default once sounds are enabled (sounds stay off after install). One sound per celebration, replacing `ui.action_done` (FR-6). When a K.O. replaces the celebration, only `ken.ko` plays. Both built-in packs (`dojo`, `arcade`) carry original synthesized recipes (sound.md §7). *(Amended 2026-10-02 by 0025 FR-13: one pack, Dojo, plays them; Arcade sounds are Ken-only)*
- **FR-14 Settings.** `settings.toml`, listed by 0009 FR-1:

  ```toml
  [celebrations]
  enabled = true            # Q-1
  pull_requests = true
  work_items = true
  step_states = ["Code Review", "In Review", "Ready for Review", "Review"]
  milestone_states = ["Committed", "Done"]
  ```

  Per-key fallback and reporting per 0009 FR-5 (a non-list value restores that key's default; non-string entries are dropped). Live apply and hot reload per 0009 FR-3 and FR-4. Settings › General, under Motion: a "Celebrations" toggle, "Pull requests I complete" and "Work item state changes" (disabled while the master is off), and a read-only summary of the two lists with "Edit in settings file" (copy: ken.md §12.5). The lists are edited in the file only in v0.1 (Q-7).
- **FR-15 Palette.** `⌘P` commands: "Turn celebrations off" / "Turn celebrations on" (one command, labeled by the current state; writes `enabled`) and "Preview celebration" (plays a Milestone in the current style at the window's center, honoring Reduce Motion and the sound settings, ignoring the master switch and the rate limits). No default shortcuts; `⌘K` keeps its row-action meaning (`crates/app/src/pin_actions.rs`). *(Amended 2026-10-02 by 0025 FR-8: "Turn celebrations off" / "Turn celebrations on" are removed; Settings › Celebrations is the switch; "Preview celebration" stays)*
- **FR-16 Ken interplay.** Work item moves aren't Ken hits: they don't extend combos or streaks and aren't stored in `ken_hits` (Q-5). Completing someone else's PR keeps its Merge Wave hit (ken.md §3.1); its celebration is the Milestone at the anchor.
- **FR-17 Accessibility.** Particles, seal and glow are accessibility-hidden. A Milestone announces politely ("Moved work item 4821 to Done", "Pull request 4821 merged into main"); a K.O. keeps ken.md §10's announcement, so a merge is announced once. A Step adds no announcement. The static glyph is never the only signal: the state label carries the meaning.
- **FR-18 Tokens, gallery, originality.** Colors only from existing tokens: calm confetti from `accent.fg`, `status.success.fg`, `status.info.fg`, `status.warning.fg`; arcade from `ken.*`. Gallery stories: `celebrate-step`, `celebrate-milestone`, `celebrate-pr`, `celebrate-arcade-milestone`, `celebrate-reduced`, `settings-celebrations`, each snapshotted per built-in theme at fixed effect times with a seeded generator. The seal asset and sound recipes are original and pass ken.md §8.

### Cross-spec changes (accepted 2026-10-01; applied the same day)

- **0015 FR-6 / `docs/design/work-item-detail.md` §11:** "No success toast for routine writes; the only success sound is `ui.action_done`" gains "except the celebrations of spec 0022 (a Milestone's toast in the static fallback, `ui.celebrate_*`)".
- **0004 §3 step 7:** "Success plays the optional Ken merge moment" becomes "Success (the confirmed merge) plays the celebration of spec 0022, or Ken's K.O. for my own PR".
- **`docs/design/principles.md` §1.1, §1.3, §1.7:** per Q-1.
- **`docs/design/tokens.md` §7.4:** add "celebrations show the static glyph (spec 0022 FR-9)".
- **0009 FR-1 and General tab:** `[celebrations]` with the FR-14 defaults.
- **Ken calm mode commands** (`crates/app/src/ken/mod.rs:79`): renamed and available with Ken off per Q-6 (code change, built with this spec; `docs/design/ken.md` §7). *(Amended 2026-10-02 by 0025 FR-8: replaced by the two Quiet mode commands)*

## 5. ADO API surface and limits

| Purpose | Endpoint (api-version) | Scope | Cost / limit notes |
| --- | --- | --- | --- |
| Work item state write (existing) | `PATCH {org}/_apis/wit/workitems/{id}?$expand=relations&api-version=7.1` | `vso.work_write` | 0015 FR-11; its 200 is the trigger. No extra request. |
| Type states and categories (existing cache) | `GET {org}/{project}/_apis/wit/workitemtypes/{type}?api-version=7.1` | `vso.work` | 0015 FR-33, cached 24 h; already fetched to open the state menu. |
| Complete (existing) | `PATCH …/pullrequests/{id}?api-version=7.1` | `vso.code_write` | 0004 FR-10; answers `mergeStatus: queued`, not the merge. |
| Completing re-read (existing) | `GET …/pullrequests/{id}?api-version=7.1` | `vso.code` | 2 s after the Complete, then 4 s and 8 s later (±10% jitter); its `completed` is the trigger. |

- **Rate limits:** zero new requests and zero added TSTU. Celebrations ride existing answers.
- **No push to local apps:** the PR celebration's latency is bounded by the completing re-reads (a merge that takes 1–2 s is seen at the 2 s re-read; at worst 14 s through the 3 re-reads). A slower merge is seen by the My pull requests lane (≤ 1 min foreground, 0002 FR-4) and celebrates if the completing mark is still set (≤ 10 min). No webhook exists for a desktop app.
- **PAT scopes:** none new. Writes need `vso.work_write` and `vso.code_write`, already in the single onboarding token (contract C6). Without them there are no writes, so nothing celebrates.

## 6. Acceptance criteria

Harness: a pure decision function (trigger, state metadata, settings, conditions, memory, now) → effect, unit-tested with an injected clock; the 0015 fake writer / mock server for work item writes; 0002's mock server and scheduler test clock for the completing re-reads; a frame-counter test hook on the overlay layer (frames requested, timers owned, buffers allocated); gallery snapshots with a seeded generator.

- **AC-1 Tier table.** With fresh memory, the decision function returns, for each (type metadata, from → to): every cell of §3.1's process table; plus a custom "Ready for QA" with category Resolved → Step; "done " (trailing space, lower case) → Milestone; a custom "Code Review" whose metadata category is Proposed → none; Active → Removed: none, even when "Removed" is in `milestone_states`. With no metadata cached, "Closed" resolves through `of_state` to Milestone and "Code Review" (In Progress by the fallback) to Step via the list. (unit)
- **AC-2 Backward and sideways.** Done → Active, Code Review → Active, Closed → Resolved, New → Approved, Committed → Resolved: no celebration; with sounds on, exactly one `ui.action_done` per write and no `ui.celebrate_*`. (unit + fake writer)
- **AC-3 Item memory.** Injected clock, #42: Active → Done at T celebrates a Milestone; Done → Active at T+1 min none; Active → Done at T+2 min none; Active → Code Review at T+3 min none (a Milestone was celebrated within 24 h); Active → Done at T+24 h+1 min celebrates again. Owner's flow on #43 with fresh memory: Active → Code Review (Step), Code Review → Committed (Milestone), Committed → Done (none). (unit)
- **AC-4 Only my confirmed writes.** A sync pass applies #42 Active → Done by Dana, #43 Active → Done made by me in the browser, and the first sync stores 30 Done items: zero celebrations and zero celebration sounds. (mock server + unit)
- **AC-5 Confirmation gate.** A state write answered 400 `TF401320` (0015 AC-7): no celebration; filling the field and resending → 200 → exactly one. A conflict (0015 AC-6): none; **Apply mine** → 200 → one. A network error: none; Retry → 200 → one. (fake writer)
- **AC-6 Work item timing.** The fake writer answers 300 ms after `↩`: the overlay's first effect frame is the frame that clears the pending clock (same frame index); before it, no particle exists. (frame hook + fake writer, injected clock)
- **AC-7 Pull request timing and failures.**
  - Complete answered 2xx `mergeStatus: queued`: no celebration while "Completing…" shows.
  - The 2 s re-read returns `completed`: exactly one Milestone, starting on the frame that store change is applied.
  - The re-read returns `rejectedByPolicy`: none, and "Merge rejected by policy" shows.
  - The review is closed (Work items open) when the re-read lands: the Milestone toast shows and the burst comes from it.
  - The merge is first seen 11 min after the Complete (mark cleared): none.

  (mock server + scheduler test clock + frame hook)
- **AC-8 Linked work items.** Complete with "transition linked work items"; the next pass shows #42 and #43 Done: exactly one celebration (the PR's) and none for the work items. (mock server)
- **AC-9 No double with the K.O.**
  - (a) Ken arcade, `ko` on, my PR: the K.O. overlay starts on the confirming frame, zero confetti particles, exactly one sound (`ken.ko`), no `ui.celebrate_*` and no `ui.action_done`. The next Ken ingest poll shows no second K.O. or toast; `ken_kos` holds one row.
  - (b) Ken subtle: one K.O. toast, no celebration toast.
  - (c) Ken off: a calm Milestone.
  - (d) Ken arcade, someone else's PR: an arcade Milestone; the Merge Wave hit counts once.

  (unit + frame hook)
- **AC-10 Reduce Motion.** With `reduce_motion = "on"`, a Milestone: 0 particles, 0 animation frames requested; the `sparkles` glyph is present from the confirming frame until exactly 3 s later (injected clock), then gone; the toast is present; with sounds on, `ui.celebrate_milestone` plays once. A Step: glyph only, no toast. (unit + story `celebrate-reduced`)
- **AC-11 Rate limits.** Milestones on distinct items at T, T+1 s, T+2 s: full, static, full. Seven Milestones on distinct items 2 s apart: the first 5 full, the 6th and 7th static with `ui.action_done`. A Milestone at T+10 min+1 s after the first: full. (unit, injected clock)
- **AC-12 Quiet hours and calm mode.** Calm mode on: a Milestone shows no particle, glyph or toast, and `ui.action_done` plays. Quiet hours on with Ken arcade: calm style (no seal); with `quiet_hours.ui_sounds = false`, silent. Calm mode commands run with `[ken] enabled = false` and suppress celebrations (Q-6). (unit) *(Amended 2026-10-02 by 0025 FR-8, AC-7: read calm mode as Quiet mode; nothing plays, including `ui.action_done`)*
- **AC-13 Settings.** No `[celebrations]` table: FR-14's defaults. `enabled = false`: zero particles, glyphs, toasts and celebration sounds, and `ui.action_done` plays. `work_items = false`: work items none, PRs still celebrate. Editing `step_states = ["QA"]` in the file: "QA" celebrates as a Step within one hot-reload poll, without restart. `milestone_states = "Done"`: that key falls back to its default and is reported. "Done" in both lists: Milestone. (unit: settings parser + hot reload)
- **AC-14 Background window.** The confirmation lands while the window isn't key: no effect then, and none after refocusing. (unit: window-active condition)
- **AC-15 Never in the way.** During a live Milestone burst over the detail, `s` opens the state menu on the next frame, a click on a link row under the particles opens it, and focus never moves to the overlay (the layer has no focus handle; hit-testing passes through). (story + unit)
- **AC-16 Idle cost.** Celebrations enabled, no trigger, 10 s simulated: the layer requests 0 animation frames, owns 0 timers and has allocated no particle buffer. After a Milestone, the last requested frame is within one frame of the last particle's death (and of the glyph timer ending under Reduce Motion). With `enabled = false` and a trigger: no allocation, no seal raster. (frame hook + allocation counter)
- **AC-17 Frame budget.** Scripted perf check (perf harness, M1-class Mac, 1440 × 900 and 5K windows, 6000 × 4000 background at blur 30): three Milestones 2 s apart plus an overlapping K.O.; every frame < 16 ms (frame recorder); celebration simulation + paint p99 ≤ 2 ms at 64 live particles (bench). (scripted check `perf: celebrate`, recorded in `docs/perf/`)
- **AC-18 Palette.** "Turn celebrations off" sets `enabled = false` in `settings.toml` (other keys kept, 0009 AC-1) and the command then reads "Turn celebrations on". "Preview celebration" plays a Milestone with `enabled = false`, shows the static fallback under Reduce Motion, and isn't counted by the rate limits. Neither binds a key; `⌘K` behavior is unchanged. (unit + palette test) *(Amended 2026-10-02 by 0025 FR-8: the celebrations on/off command is removed; the "Preview celebration" part stands)*
- **AC-19 Sounds.** Sounds on: a Step plays `ui.celebrate_step` once and no `ui.action_done`. Sounds off: the audio device is never opened (sound.md §1). Both new events validate in `dojo` and `arcade` (`Pack::validate`, durations ≤ 2.0 s). (unit: `adoken-sound`)
- **AC-20 Tokens and snapshots.** Every FR-18 story × every built-in theme matches its snapshot at effect times 0, 200 and 600 ms with a fixed seed. The 0007 AC-1 grep finds no color literal in the celebration code. Overriding `accent.fg` in a theme file changes the calm confetti colors in the snapshot. (snapshot + token dump)
- **AC-21 Originality.** The seal asset, its kanji and both recipes pass ken.md §8's checklist, signed off by ux-designer in the PR that adds them; the kanji passes the native-speaker review ken.md §3.2 requires. (named manual check: "ken §8 review")
- **AC-22 Demo org.** In the demo, `s` → Done celebrates after the demo delay (≤ 300 ms); the mock server records zero requests. (unit + story)

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Confirmation applied → first effect frame | Same frame (≤ 16 ms) | Frame hook (AC-6, AC-7) |
| Click → work item celebration | Azure DevOps' write round trip + 1 frame (demo ≤ 300 ms + 1 frame) | Fake writer latency |
| Click → PR celebration | The re-read that sees `completed` + 1 frame: 2 s (±10%) for a merge done by then; ≤ 14 s through the 3 re-reads | Scheduler test clock |
| Frame during an effect | < 16 ms; celebration simulation + paint p99 ≤ 2 ms at 64 particles | Frame recorder + bench (AC-17) |
| Idle | 0 frames, 0 timers, 0 wakeups | Frame hook (AC-16) |
| Memory | ≤ 256 KB while an effect is live; 0 while `enabled = false` | Allocation counter |
| Cold start | +0 ms: nothing loaded before the first effect; the table is read with the existing settings read | Launch timer A/B |
| Sound | < 50 ms trigger → audible (0007 §7) | Audio loopback test |

Global rules (AGENTS.md) always apply: frame < 16 ms, Cmd+P < 50 ms on 100k items, cold start < 500 ms. Regressions are bugs.

## 8. Success signal (telemetry-free)

- **Dogfood:** after two working weeks, the owner still has `[celebrations] enabled = true` and records, per celebration that felt wrong, one line in the dogfood notes ("too much", "missed one", "wrong tier for state X"). Target: no "too much" line after the first week.
- **Build-in-public:** a 10 s clip of `s` → Done and a PR merge celebrating gets "how do I get this" replies rather than "how do I turn it off".
- **Support:** no question asking how to disable confetti (one `⌘P` command answers it).

## 9. Risks

- **Persona fit.** Celebrations don't make anything faster or quieter on their own; they are an owner decision. Mitigations: zero idle cost, zero requests, one switch, routine writes stay silent.
- **Fatigue and noise.** Mitigations: two tiers, forward moves only, 24 h item memory, rate limits (FR-11), calm mode, master and per-surface switches. *(Amended 2026-10-02 by 0025 FR-8: calm mode is now Quiet mode)*
- **Process variety.** Custom states may miss the default lists, or a review state may live in another category. Mitigations: category rules cover stock and most inherited processes; lists are editable; real categories come from type metadata, not the stock-name fallback.
- **Stock Scrum "Committed"** means "work started" there, so stock Scrum users get a Milestone at the start of work (Q-3).
- **False celebration on asynchronous merges.** Mitigated by celebrating only the confirmed `completed` (FR-5).
- **Principle conflict.** Default-on effects contradict "calm by default" and "playful is opt-in" (Q-1).
- **Frame cost over backgrounds.** Shared particle cap and AC-17 on a blurred background.

## 10. Open questions — resolved 2026-10-01

All recommendations accepted by the owner (Q-2 confirmed explicitly).

- **Q-1 Default on or off?** **Resolved:** on by default in the calm style, sounds off, one switch in Settings › General and `⌘P` (FR-14, FR-15, AC-13). `docs/design/principles.md` §1.1, §1.3 and §1.7 name "a short celebration of your own finished work (spec 0022)" as the one default-on effect.
- **Q-2 Celebrate the PR on the click, or on the confirmed merge?** **Resolved:** on the confirmed merge, usually about 2 s after the click; "Completing…" stays the click's instant feedback (FR-5, AC-7).
- **Q-3 "Committed" for stock Scrum.** **Resolved:** keep the owner's default, Committed = Milestone; the list is editable; revisit after beta feedback (FR-2, FR-14, AC-1).
- **Q-4 Auto-complete merges.** **Resolved:** no celebration in v0.1 (FR-1); parked in `docs/ideas.md` (RICE ≈ 6: Reach 20 · Impact 0.5 · Confidence 60% · 1 wk).
- **Q-5 Do work item moves count in Ken?** **Resolved:** no; they aren't hits and don't extend combos or streaks (FR-16).
- **Q-6 Calm mode with Ken off.** **Resolved:** the commands become "Calm mode for 1 hour", "Calm mode until tomorrow", "End calm mode", available with Ken off, and calm mode hides celebrations (FR-10, AC-12; `docs/design/ken.md` §7). RICE 200, v0.1. *(Amended 2026-10-02 by 0025 FR-8: the three calm mode commands became Quiet mode)*
- **Q-7 Per-type state editor in Settings.** **Resolved:** v0.2 (`docs/roadmap.md`); v0.1 edits the two lists in `settings.toml` and shows a read-only summary (FR-14). RICE 5.
- **Q-8 Resolved category as Step.** **Resolved:** yes, by default (FR-2, AC-1).
- **Q-9 Milestone.** **Resolved:** v0.1. Group A (RICE 56) on the cut line right after the Ken layer; group B (RICE 24) ships only with the Ken layer and is cut with it (`docs/roadmap.md`).

## 11. Out of scope

- Celebrations for others' actions or anything sync observes (FR-1); approvals, comments, pipelines, inbox zero (Ken's badges cover clean sweep).
- Today rows: 0017's static "Merged today" glyph (contract C3), no effect.
- Auto-complete merges (Q-4, `docs/ideas.md`).
- Ken scoring of work item moves (Q-5).
- Per-type state editor UI (Q-7, v0.2).
- Bulk edit (0015 §11); boards drag and drop (0008 §11).
- Dock bounce, OS banners, haptics, sharing a celebration; full-window effects other than Ken's K.O.

## 12. References

- Workflow states and state categories (Proposed, In Progress, Resolved, Completed, Removed): https://learn.microsoft.com/en-us/azure/devops/boards/work-items/workflow-and-state-categories?view=azure-devops
- Agile process workflow: https://learn.microsoft.com/en-us/azure/devops/boards/work-items/guidance/agile-process-workflow?view=azure-devops
- Scrum process workflow (New, Approved, Committed, Done): https://learn.microsoft.com/en-us/azure/devops/boards/work-items/guidance/scrum-process-workflow?view=azure-devops
- Customize the workflow of an inherited process (custom states such as Code Review): https://learn.microsoft.com/en-us/azure/devops/organizations/settings/work/customize-process-workflow?view=azure-devops
- Work Item Types – Get (`states[].category`): https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-item-types/get?view=azure-devops-rest-7.1
- Pull Requests – Update (complete): https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/update?view=azure-devops-rest-7.1
- WCAG 2.2, 2.3.1 Three flashes or below threshold: https://www.w3.org/TR/WCAG22/#three-flashes-or-below-threshold
- Rate and usage limits: https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/rate-limits?view=azure-devops
