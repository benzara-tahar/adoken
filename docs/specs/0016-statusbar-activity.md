# 0016 — Statusbar activity: sync activity and network log

| Field | Value |
| --- | --- |
| Status | Accepted (2026-10-01, owner: all recommendations) |
| Milestone | v0.1. On the cut line after 0015 slice B, the Ken layer and calm celebrations (0022 group A; re-ordered 2026-10-01): the network log (group B) is cut before sync activity (group A) (Q-5, `docs/roadmap.md`). The resource meter slot (FR-5, FR-28) ships with spec 0023 |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/sync` (ledger, governor, scheduler, engine: attribution, pass records, lane state), `crates/store` (per-lane last success), `crates/app` (statusbar items, Activity drawer, palette commands, settings), `crates/ui` (virtualized table row), `crates/gallery` |
| Serves | **Less noise** (one glance answers "is it fresh, is it working?" instead of guessing, refreshing or opening the browser) · **Snappy** (the drawer opens in one frame from memory; a slow or failing endpoint is visible instead of felt) |
| Depends on | `docs/specs/0002-sync.md` (§3 status pill, FR-4/FR-4a lanes, FR-7–FR-10 governor and ledger, FR-11 events, FR-14/FR-15 writes), `docs/specs/0009-settings.md` (Sync tab, `settings.toml`), `docs/specs/0014-pins.md` (FR-11 pinned refresh), `docs/specs/0004-pr-review.md` (user-priority requests, writes), `docs/specs/0012-avatars.md` (avatar requests), `docs/specs/0003-command-palette.md` (commands) |
| Design | `docs/design/activity.md` (ux-designer): statusbar items (§2), drawer (§3), Sync tab (§4), Network tab (§5), keyboard (§6), states (§7), motion (§8), accessibility (§9), gallery (§10), tokens (§11). Layout context: `docs/design/principles.md` §2, §3.3, §3.5 |

Groups A and B are implemented in the working tree. Status remains **Accepted** until AC-B12's 10-minute real-org run is recorded. The 2026-10-01 attempt waited on the macOS Keychain authorization prompt that a rebuilt dev binary raises, which needs the owner's approval.

> Rule: no code without an **Accepted** spec (AGENTS.md). The spec owns behavior, acceptance, and budgets; design docs own visuals, motion, sound, and copy.

Two feature groups, each shippable or cuttable alone (§4):

- **A. Sync activity** (owner note 3): click the statusbar sync item to see what is syncing now and what the last passes brought.
- **B. Network log** (owner note 2): a compact statusbar indicator of outgoing requests and, on click, a log of every request with its status.

They share one drawer (FR-1–FR-5). If only one group ships, the drawer has one tab and no tab strip.

## 1. Problem

"The statusbar says *Synced 2 min ago*, but my colleague's comment isn't there. Is it syncing? Did something fail? Was it throttled? I can't tell, so I hit ⌘R and hope, or open the browser to check." Sync is invisible by design (0002 §2), and that only works while it works. Today the statusbar shows one line and nothing is clickable. 0002 §3 promised a popover with per-tier times; it never shipped. When something goes wrong (a 403 on one project, a 429, a slow endpoint, a token without `vso.build`), the only evidence is the `ADOKEN_SYNC_LOG` file, which you must set before launch and read in a terminal. A local-first app that says "nothing leaves your Mac except Azure DevOps calls" also can't show you those calls.

## 2. Users and job to be done

- **Persona:** developer on Azure DevOps who relies on adoken's freshness all day and needs to trust it. Group B also serves the power user or admin who wants to check adoken's request rate against Azure DevOps' Usage page, or see what an app with no telemetry actually sends.
- **Job to be done (A):** "When the data looks stale or something seems off, I want to see each kind of data's sync state and what the last syncs changed, so I can trust what I see, or know why it's late and what to do."
- **Job to be done (B):** "When adoken feels slow or something failed, I want to see the requests it sends, their status, cost and timing, so I can tell a slow Azure DevOps from a failing token, scope or endpoint without a terminal."
- **Smallest delightful slice (A):** click "Synced 2 min ago". One row per kind of data: state, last synced, next sync. Below that, the recent passes ("3 PRs updated · 2 work items changed · 1 run finished") with the items one keystroke away.
- **Smallest delightful slice (B):** a quiet statusbar indicator that turns red when requests fail, and opens a live, filterable log in which every row says what the request was for ("PR threads"), who caused it ("My pull requests" sync or "You: vote"), and what happened.
- **Priority (RICE):** A = 32 (Reach 80 · Impact 1 · Confidence 80% · Effort 2 weeks including the shared drawer). B = 19 (Reach 40 · Impact 1 · Confidence 70% · Effort 1.5 weeks; the recorder already exists). A first; B is cut first if v0.1 slips (Q-5).
- **Already in place:** `crates/sync/src/ledger.rs` `Recording<T>` wraps all three production transports (`crates/app/src/sync.rs` `run`: main, work-item writes, avatars) and records method, host, templated path, status, bytes, latency and rate headers in a 10,000-entry ring. It has no timestamp, origin, purpose or in-flight state, and each transport has its own ring. The scheduler publishes `cadence::Activity` (last success and smoothed cost per lane) for Settings › Sync. The engine returns `PassReport { events, lane_costs }`. The governor keeps a one-hour `(at, tier, cost)` ledger (`GovSnapshot::ledger`, 0002 FR-10). Nothing renders any of it.

## 3. UX flow

Visuals, copy and motion: `docs/design/activity.md`. Shortcuts were checked against `docs/design/command-palette.md` §7 and every `KeyBinding::new` / `*_KEYS` in `crates/app/src/**`: `⌘⇧A` is unbound globally (only onboarding binds it, inside its own `Onboarding` context, which is never active with the shell), and `⌃Tab` is unbound.

### 3.1 Placement (Q-1)

One **Activity drawer** docks at the bottom of the content column, above the statusbar. The content above it shrinks; the sidebar or rail keeps its full height, so the pins height budget (0014 §3.1: 7 rows at 600 px) is unchanged. It has two tabs, **Sync** (A) and **Network** (B).

| Option | Room at 900 × 600 | Cost | Verdict |
| --- | --- | --- | --- |
| **A. Bottom drawer in the content column, two tabs** | 668 px wide next to the expanded sidebar (844 px beside the rail) × 40% of the height (≈ 215 px at 600 px) | Content gets shorter while it's open | **Recommended**: room for a 10k-row table; you can watch it while you work; one pattern for both groups |
| B. Popover anchored to each statusbar item | ≈ 420 × 360, over the content | Too small for the log's columns; closes when focus leaves, so you can't watch it while you work | Rejected |
| C. Separate window | Any size | Window management; easy to lose behind the main window; a second key window to keep in sync | Rejected (could come back as "pop out", §11) |

### 3.2 Happy path (A: Sync activity)

1. The statusbar reads "Synced 2 min ago". Click it, or press `⌘⇧A`, or run `⌘P` › "Activity: Show sync activity". The drawer opens on **Sync** and the focus moves into it.
2. Six rows: My pull requests · All active pull requests · Work items · Pipeline runs · Search index and projects · Pinned items. Each shows its state ("Synced 40s ago · next in 20s", "Syncing · 3 of 12 pull requests", "Manual only", "Failed: Azure DevOps answered 503 · retrying in 30s").
3. Below them, **Recent passes**, newest first: "14:02:11 · All active PRs, Work items · 1.2 s · 9 requests · 0.03 TSTU — 3 PRs updated · 2 work items changed". `↓` to a pass, `→` (or a click) expands it to the changed items. `↩` on an item opens it in its canonical view (principles §1.5); the drawer stays open.
4. The drawer's actions: **Sync now** (`⌘R`), **Pause** (for 1 hour / until resumed) or **Resume**, **Sync settings…** (opens Settings on the Sync tab).
5. `Esc` closes the drawer, and the focus returns to where it was.

### 3.3 Happy path (B: Network log)

1. The statusbar's network indicator shows the number of requests in flight ("2"), then the last result's status. A failure turns it to the danger tone with a count ("2 failed"). Glyphs and copy: design §2.3.
2. Click it, or `⌘P` › "Activity: Show network log" (`⌘⇧A` reopens the last tab). The drawer opens on **Network**: a live table, newest at the bottom, following the tail.
3. Each row: time, method, status, purpose ("PR threads"), origin ("My pull requests" / "You: open PR" / "Avatars"), latency (and queue wait), size, TSTU.
4. `F` toggles **Failed only**. `/` focuses the filter (purpose, route, origin). `↩` on a row expands its detail: host, templated route, attempt, the rate-limit headers, `x-vss-activityid`, the error text, and the item it was about (in-app only, Q-2).
5. **Totals** (`T`, or the toolbar toggle) groups by endpoint for the last hour and since launch: requests, failures, p50/p95 latency, bytes, TSTU, and the share of Azure DevOps' 200 TSTU limit.
6. `⌘C` copies the selected rows, `⌘P` › "Network log: Export…" saves the whole log; both use the redacted export format (FR-24).
7. Opening the Network tab marks the failures as seen, and the indicator calms down.

### 3.4 Alternate paths

- **Switch tabs:** `⌃Tab` / `⌃⇧Tab`, a click on the tab, or the palette commands.
- **From a pass to its requests** (when both groups ship): "Show requests" on a pass opens Network filtered to that pass.
- **From a request to its pass:** the row detail names the pass ("Pass 14:02:11 · All active PRs") and opens it.
- **Keyboard into the statusbar:** the sync item and the network indicator are buttons at the end of the Tab order (principles §6: Tab order = visual order); `↩`/`Space` opens their tab.
- **Resize:** drag the drawer's top edge. The height persists as `[activity] panel_height`.
- **Hide the indicator:** Settings › Sync › "Show network activity in the statusbar" (Q-3). The log stays reachable by `⌘⇧A` and the palette.

### States

| State | Statusbar sync item | Statusbar network indicator | Drawer |
| --- | --- | --- | --- |
| Empty (no projects) | "No projects" (unchanged) | Idle (requests may still run: identity, projects) | Sync: rows say "No projects selected" with Settings › Projects. Network: whatever ran. |
| Loading / first sync | "First sync…" | In-flight count | Sync: each row shows first-sync progress; Recent passes fill as passes land. Network: live. |
| Error (a lane failed: 5xx, DNS, decode) | "▲ Sync delayed — retrying in 30s" (principles §3.3) | Danger tone + count | Sync: the failing rows show the reason and retry time; the others stay normal. Network: the failed rows are marked. |
| Offline / stale | "Offline — last synced 3 min ago" | Becomes the **Offline** pill (principles §3.5) | Sync: every row "Offline · last synced …", no next time. Network: no new rows; earlier rows stay. |
| Throttled by ADO | "Throttled by Azure DevOps — resuming in 40s" | Warning tone | Sync: every row "Waiting for Azure DevOps · resuming in 40s" plus the `X-RateLimit-Resource` text. Network: the 429 / delayed rows carry their headers. |
| Paused by the user | **New:** "Sync paused · until 15:00" / "Sync paused" (FR-9) | Idle | Sync: rows "Paused" with the resume time; Resume is the primary action. |
| Token rejected or expired / no token | "Token rejected" (or "Token expired" when `validTo` is known and past) / "No token in Keychain" | Danger (rejected, expired) / hidden (no token: nothing is sent) | Sync: every row says so, and the Sync tab's empty state offers **Replace token…** (0001 FR-15; amended 2026-10-01, owner notes; accepted 2026-10-01). Network: the 401 rows. |
| Demo mode | "Demo data" (unchanged) | Hidden | Sync: rows "Demo data · not synced"; Recent passes lists demo-feed ticks as "Demo feed". Network: empty state "Demo mode sends no requests". Nothing is faked (Q-6). |

## 4. Functional requirements

### 4.1 Shared: the Activity drawer

- **FR-1 Drawer.** One Activity drawer per main window, docked at the bottom of the content column, above the statusbar. The sidebar or rail keeps its full height. Default height 40% of the window, min 160 px, max 70%, and never so tall that less than 120 px of content remains (at 600 px: max 418 px). The top edge is a splitter: drag it, or with it focused `↑`/`↓` 24 px, `⇧↑`/`⇧↓` 96 px, `Home`/`End` max/min. `⌘⇧↩` inside the drawer maximizes / restores it (the same "make this pane big" meaning as the global full-width toggle). The height persists as `[activity] panel_height`. It stays open across section changes (⌘1…⌘6) until closed. It never covers the statusbar or the sidebar.
- **FR-2 Tabs.** "Sync" (group A) and "Network" (group B). With one group shipped, there is no tab strip. The last tab persists as `[activity] tab`.
- **FR-3 Open and close.** `⌘⇧A` toggles the drawer on its last tab. A click or `↩` on the statusbar sync item opens Sync; on the network indicator, Network. Palette commands: "Activity: Show sync activity", "Activity: Show network log", "Activity: Hide". `Esc` (when no filter field or expanded row has the keyboard) closes it, and the focus returns to the element that had it before. `⌃Tab` / `⌃⇧Tab` switch tabs inside the drawer. `F6` cycles the focus content → drawer → toast region (principles §4 already promises `F6` for toasts; nothing binds it today). Single-letter keys (`F` failed only, `T` totals, `j`/`k`) and `/` work only while the drawer has focus and no text field does (key context `ActivityPanel && !TextInput`), so they never shadow the lists' keys elsewhere. All bindings are overridable in `keymap.toml` and appear in the `?` cheat sheet.
- **FR-4 Keyboard and accessibility.** The drawer is a region labeled "Activity"; the network log is a table (not a live log region), so rows are never announced. Its lists move with `↑`/`↓`/`j`/`k`, `Home`/`End`, `PageUp`/`PageDown`. Every row's accessible label carries its full content ("My pull requests, synced 40 seconds ago, next in 20 seconds"; "14:02:11, GET, 200, PR threads, My pull requests, 182 milliseconds, 4 kilobytes, 0.004 TSTU"). The statusbar items are buttons, labeled with their state. Screen readers hear, politely: sync state changes (to failed, throttled, offline, recovered), at most one per 10 s; and network failures once when unseen danger failures go from 0 to ≥ 1, then at most one per 30 s. Never one per request. Every pointer action has a keyboard path (principles §1.2).
- **FR-5 Statusbar order.** Left: sync item, then the org. Center: quiet hours / snooze (unchanged). Right: network indicator, then Ken combo (if enabled), offline pill, then the resource meter (if enabled, spec 0023 FR-1) and the demo pill. At 900 px the indicator shortens first (design §2.5); the sync label truncates last. The resource meter drops its CPU part right after the notice truncates and hides after the org hides (0023 FR-8). (amended 2026-10-01, owner notes; accepted 2026-10-01)
- **FR-6 Live updates without storms.** Network and sync activity reach the UI as snapshots published at most **4 times a second** (trailing edge, so the last change always shows). The statusbar repaints only when its rendered text or tone changes. While the drawer is closed, a request costs the UI thread nothing beyond that throttled snapshot check; while it's open, the drawer pulls only entries newer than the last one it holds (by sequence number), never a full copy. A hidden or minimized window does no activity repaints.

### 4.2 Group A: Sync activity

- **FR-7 Rows.** One row per user-facing kind. Each maps to scheduler lanes (`crates/sync/src/cadence.rs`):

  | Row | Lanes | Cadence shown |
  | --- | --- | --- |
  | My pull requests | `MyPrs` (hot) | Settings › Sync "My pull requests" |
  | All active pull requests | `AllPrs` (warm) | "All active pull requests" |
  | Work items | `WorkItems` (warm) | "Work items" |
  | Pipeline runs | `LiveRuns` (hot) + `DoneRuns` (warm) | "Pipeline runs"; the row shows the sooner next pass and the worse state of the two |
  | Search index and projects | `Index` (cold) | fixed 30 min / 60 min in the background |
  | Pinned items | The pinned-refresh steps inside `AllPrs`, `WorkItems`, `LiveRuns`, `DoneRuns` (0014 FR-11) | no own cadence: "refreshed with All active PRs, Work items and Pipeline runs"; shows the pin count and "N not available" (0014 FR-13) |

- **FR-8 Row state.** Each row shows the first state that applies, in this order:
  1. **Token rejected** (governor unauthorized; "Token expired" when `validTo` is past) → the Sync tab's empty state with **Replace token…** (0001 FR-15; amended 2026-10-01, owner notes; accepted 2026-10-01).
  2. **Needs a scope**: the kind's scope is missing (`vso.work`, `vso.build`, 0001 §5.2; 0014 FR-1) → names the scope; no button (0001 FR-9).
  3. **Offline** (0002 FR-5) → "last synced …", no next time.
  4. **Waiting for Azure DevOps**: `Retry-After`, 429 backoff or `X-RateLimit-Delay` holds the host (governor `Hold::Paused`) → countdown, and the last `X-RateLimit-Resource` text verbatim (Microsoft says to show it to a human, §12).
  5. **Waiting for adoken's budget**: the self-limit of 300 requests / 50 TSTU per 5 min is spent (`Hold::Budget`) → countdown.
  6. **Syncing now**, with progress when the total is known before the step starts: "3 of 12 pull requests" (hot-set threads due), "project 2 of 5" (per-project lists), "400 of 1,150 work items" (after WIQL, batches of 200), "timelines 2 of 4". Otherwise a count so far without a total ("index: 3,400 items so far"). No fake percentages.
  7. **Failed**: the last attempt failed. The reason comes from the error (HTTP status + Azure DevOps' error code and message, "Couldn't reach dev.azure.com", "Unexpected response, adoken may need an update" for decode errors), plus when the backoff retries ("retrying in 30s", 0002 §3 States).
  8. **Paused** (FR-4a pause) → "until 15:00" or "until resumed".
  9. **Paused in the background** (background = Paused) → "resumes when adoken is in front".
  10. **Slowed by rate pressure** (`Pressure::Slow`: warm/cold ×4; `HotOnly`: everything but hot lanes deferred) → shows the remaining share ("Azure DevOps rate limit 38% left") and when pressure resets.
  11. **Manual only** → "only with Sync now ⌘R"; last synced.
  12. **Synced**: "Synced 40s ago · next in 20s" (next = the scheduler's planned due time, jitter included, adjusted for background, pause and pressure).
  13. **Not synced yet.**
- **FR-9 Paused in the statusbar.** While a pause is active (0002 FR-4a) and no pass runs, the statusbar sync item says it is paused and until when, in a quiet tone. It no longer says "Synced 2 min ago" as if nothing were held. Unauthorized, throttled and offline still take precedence; a ⌘R pass during a pause shows "Syncing…".
- **FR-10 Last success survives restarts.** The last successful pass per lane is stored locally (one row per lane in the org's store, local table, never synced), so after a restart, or while paused or offline at launch, every row shows its real last-synced time, not "Not synced yet". It is written in its own transaction right after the transaction that commits the lane's last page, so the stored time can lag the data but never claim fresher data (amended 2026-10-01: each lane ends in a different step, threads, file lists, timelines or comments, so there is no single last-page transaction).
- **FR-11 Pass records.** Every pass (scheduled, Sync now, wake/reconnect catch-up, a write's re-read (0002 FR-15), a completing PR's re-read (0004 FR-10), "load older runs", first sync, and a single work item's sync (spec 0015 FR-30/FR-31), listed as "You: Sync #20101") produces one record: start and end time, lanes, trigger, outcome (ok / failed with reason / deferred by pressure / canceled by offline or sleep), requests, TSTU, the events it persisted (0002 FR-11) and what changed (FR-12). A pass that sends nothing and changes nothing (e.g. deferred) is folded into the row state, not listed.
- **FR-12 What changed.** A change is a stored row whose content differs from before the pass: inserted, updated (any field the app shows) or removed / left the active set. Re-writing an identical row is not a change. Counted per kind: pull requests (new, updated, completed/abandoned), PR comments, work items (new, changed, removed), work-item comments, pipeline runs (started, finished with result, updated), index entries, projects and repositories. Counts are exact. PRs, work items and runs also keep their ids: up to **20 per kind per pass**, then "+N more". Index entries, projects and repos are counts only. The engine counts from the old-vs-new comparison it already makes for events (0002 FR-11), inside the same page transaction.
- **FR-13 Recent passes.** The Sync tab lists the pass records, newest first, kept in memory: the last **200 passes or 2 hours**, whichever is fewer. They are not persisted; after a restart the list starts empty while FR-10 times remain (Q-4). Each pass summarizes its changes in one line ("3 PRs updated · 2 work items changed · 1 run finished · 2 notifications"). Consecutive successful passes of the same lanes that changed nothing are shown as one line ("10:38–10:40 · No changes · 4 passes"); this is display only, and each still counts toward the 200. Expanding a pass lists the changed items with their current titles from the store; `↩` opens an item's canonical view (0004 review, 0008 work item / run detail) through the same routing as palette targets. A changed item that's gone from the store shows its id, not openable. "2 notifications" opens the inbox.
- **FR-14 Actions.** **Sync now** sends `Control::RefreshNow` (the 10 s coalescing and "just synced" hint of 0002 §3 apply). **Pause** offers "For 1 hour" and "Until resumed"; while paused, **Resume**. Both edit `[sync]` exactly like Settings › Sync and the existing palette commands (`crates/app/src/sync.rs` `set_pause`). **Sync settings…** opens Settings on the Sync tab. The drawer offers no cadence controls; Settings owns them (0009).
- **FR-15 Demo.** The demo org sends nothing, so rows read "Demo data · not synced". Demo-feed ticks (`crates/app/src/shell/mod.rs` `demo_feed`) are listed as passes labeled "Demo feed" with what they added. Sync now, Pause and Resume are disabled with the reason "Demo org: nothing syncs".

### 4.3 Group B: Network log

- **FR-16 Coverage.** Every HTTP request adoken makes to Azure DevOps after the shell starts is logged: sync passes, user-priority reads (PR files and blobs, 0004 FR-1–FR-3; work-item links, 0004 FR-17; load older runs; 0015's per-item syncs, conflict re-reads, type, field and classification metadata and directory people search), every write (0004 FR-14–FR-17; 0015 work item field and link PATCHes and comment POSTs), avatars (0012), and the bootstrap reads (connection data, identities, projects). One log per app session, merging all three transports (`crates/app/src/sync.rs` `run`: main, work-item writes, avatars) in send order. Requests made before the shell starts (onboarding, PAT verification and minting in 0001) are **not** logged, and the PAT-create response is never readable by the recorder (§11).
- **FR-17 Entry.** Each request records:

  | Field | Meaning | In-app | Export |
  | --- | --- | --- | --- |
  | `seq` | Monotonic per session; orders and links entries | — | yes |
  | `at` | Wall-clock time the request went out (ms) | local `HH:MM:SS.mmm` | ISO 8601 UTC |
  | `method` | GET / POST / PUT / PATCH (`POST` query vs write kept apart internally) | yes | yes |
  | `host` | `dev.azure.com`, `vssps.dev.azure.com`, … | detail | yes |
  | `route` | Templated path from the route table (FR-19) | detail | yes |
  | `purpose` | Human label from the route table: "PR threads", "Work items batch", "Build timeline", "Vote", "Avatar" … | yes | yes |
  | `origin` | Who caused it (FR-18) | yes | yes |
  | `pass` | The pass record it belongs to, if any (FR-11) | detail, links | pass start time |
  | `subject` | The item it was about (PR !4821, #1234, run 77), as a typed id resolved from the store for display | in-app only (Q-2) | **never** |
  | `attempt` | 1, 2, 3 … for the governor's retries after 429 / `Retry-After` (each try is its own entry) | detail | yes |
  | `queued_ms` | Time held by the governor before sending (slots, `Retry-After`, budget) | yes, beside latency | yes |
  | `elapsed_ms` | Send → response complete (or failure) | yes | yes |
  | `status` | HTTP status; or **Failed** (no response: offline, DNS, TLS, timeout) or **Canceled** (abandoned before a response, e.g. pass canceled by sleep). UI strings use sentence case and Azure DevOps' US spelling, as the pipeline "Canceled" state does | yes | yes |
  | `bytes` | Response body size | yes | yes |
  | `rate` | `X-RateLimit-Cost`, `-Limit`, `-Remaining`, `-Reset`, `-Resource`, `-Delay`, `Retry-After` as received (0002 FR-8) | TSTU column + detail | yes |
  | `activity_id` | `x-vss-activityid` response header when present (Azure DevOps' request correlation id; principles §3.4 shows it in error details) | detail | yes |
  | `error` | Failure kind + Azure DevOps error code (`TF400733`, `VS402335` …) + Azure DevOps' message | detail | kind + code only |

  An entry is created **when the request goes out** (in-flight) and completed when it ends. A request whose future is dropped completes as **Canceled**, so the in-flight count can't leak.
- **FR-18 Origin.** Every request carries one origin, set where the work starts and inherited by every request it makes:
  - **Sync lane:** My pull requests · All active pull requests · Work items · Pipeline runs · Search index and projects, plus **pinned** when the step is a pinned refresh (0014 FR-11), plus **Sync now** when the pass came from ⌘R.
  - **You:** open PR (files, blobs), vote, comment, reply, resolve, status change, complete, link work item, load older runs, and each write's **re-read** (0002 FR-15, 0004 FR-10); from spec 0015: work item field edits (state, assign, title, area, iteration, description), link add / remove, work item comment and its item GET, Sync #id (FR-30, including FR-31's auto-fetch of a partly synced item), conflict re-reads, and the metadata and directory reads a picker or rule error triggers.
  - **Avatars** (0012).
  - **Startup** (connection data, my identity, project list before the first pass).

  A request with no origin is a bug: debug builds assert, release builds log "Unattributed".
- **FR-19 Route table and redaction.** One table in `crates/sync` lists every Azure DevOps route adoken calls: method, path pattern, purpose label, which segments are fixed vocabulary. The ledger templates a path by matching it against the table:
  - The org is `{org}`, in the path and in a legacy `{org}.visualstudio.com` host (the parser accepts those URLs, `crates/ado/src/lib.rs`); project, repository, team or definition segments are `{project}` / `{repo}` / `{name}`; GUIDs are `{id}`; git object ids are `{sha}`; numbers are `{n}`; any other non-vocabulary segment is `{name}`. Today's `path_template` works by position and shape only: it leaked the project name on `GET {org}/_apis/projects/{name}` (0014 §5) and git object ids on `…/blobs/{objectId}` until both were patched on 2026-10-01 (`{project}`, `{sha}`). Any new route with a name after `_apis` would leak again, which the table prevents.
  - A path that matches no route is logged as purpose "Other request" with every segment after `_apis` that isn't a known word replaced by `{name}`, and a debug assert fires.
  - **Never recorded**, in memory, on screen, in the export or in `ADOKEN_SYNC_LOG`: query strings, fragments, request bodies (WIQL text, comment text, JSON patches), response bodies, any header other than the seven rate headers and `x-vss-activityid`, the credential, user info in URLs.
  - Error text never contains a URL. In-app it may contain Azure DevOps' message, which can name a repository or project (local only, like the store); the export keeps only the error kind and code.
- **FR-20 Status classes.** Success: 2xx, 304. Warning: 429, any response with `Retry-After` or `X-RateLimit-Delay` > 0 ("delayed"), other 4xx (404 included: a pinned item's "Not available" is expected but still visible). Danger: no response (Failed), 5xx, 401, 403, 203 sign-in page. Quiet: Canceled. Design maps these to the existing status tokens (design §5.3).
- **FR-21 Statusbar indicator.** In connected mode, when `[activity] network_indicator` is on (default, Q-3): the in-flight count while > 0 (shown only once a request has been in flight for 150 ms, principles §3.2), else the last request's status ("200", "Failed", "Canceled") in the quiet tone. The tone is danger while there are unseen danger failures from the last 5 minutes (with their count), warning for unseen warnings, otherwise quiet. Failures of background requests on optional transports (avatars, 0012 FR-9: their 401/404 never reject the token) are logged and listed by Failed only, but don't change the tone or count unless the user caused the request (Q-10). Opening the Network tab marks every current failure as seen. Offline: it becomes the Offline pill (principles §3.5). Hidden in demo mode and with no token. No animation per request; reduce motion → static glyphs only (design §8).
- **FR-22 Table.** A virtualized table, newest at the bottom. It follows the tail while the selection is on the last row; scrolling up or selecting stops following, and "Jump to latest" (`End`) resumes. Entries that arrive while you're not following are counted ("12 new"), and the scroll position doesn't move.
- **FR-23 Filters.** **Failed only** (danger + warning), origin (multi-select), purpose / endpoint (multi-select from the route table), status class, and free text over purpose, route and origin. Filters combine with AND. `⌘⌫` clears them. The filter applies to the live tail too. The filter state lasts for the session.
- **FR-24 Totals (realizes 0002 FR-10).** A totals view groups by endpoint (method + route), and alternatively by origin and by governor tier, over **the last hour** or **since launch**. Each group shows: requests, failures, p50 / p95 latency, p95 queue wait, bytes, summed TSTU. The header shows the last 5 minutes' TSTU and its share of Azure DevOps' 200 TSTU limit (the same measured value as Settings › Sync, `Activity::window_cost`). Since-launch totals are counters kept apart from the ring, so they stay exact after old entries are evicted. Per-tier last-hour numbers equal the governor ledger's (`GovSnapshot::ledger`).
- **FR-25 Copy and export.** `⌘C` copies the selected rows; "Network log: Export…" (palette and toolbar) writes the whole log (or the filtered rows, if a filter is on) to a file the user chooses. Both use one format: JSON Lines, one object per entry with the "Export" fields of FR-17, plus a first line with adoken's version, the export time and the redaction note (Q-9). The export runs off the UI thread. It never contains the subject, Azure DevOps' free-text messages, or anything FR-19 forbids.
- **FR-26 Retention.** Entries live in memory only: one ring of **10,000 entries** per session (at the governor's cap of 300 requests / 5 min, that's ≥ 2.7 hours), oldest dropped first, cleared when adoken quits (Q-4). `ADOKEN_SYNC_LOG=<file>` stays for long captures and appends the same JSON Lines as the export.
- **FR-27 Privacy line.** The Network tab always shows one line, worded by design (§5.8), that states the facts of FR-19 and FR-26: requests are recorded on this Mac only, without query strings, bodies, headers or the token, and cleared when adoken quits.

### 4.4 Settings

- **FR-28 Settings.** A new `[activity]` table in `settings.toml`: `panel_height` (px, clamped to FR-1's range), `tab` (`"sync"` / `"network"`), `network_indicator` (bool, default `true`), `resource_meter` (bool, default `false`; spec 0023 FR-11, its switch lives in Settings › General). Settings › Sync gains the switch "Show network activity in the statusbar" and the link "Open sync activity ⌘⇧A". Changes apply on the next frame (0009 FR-3). (amended 2026-10-01, owner notes; accepted 2026-10-01)

### 4.5 Amendments to other specs (applied 2026-10-01)

Each amendment applies with the group in brackets; `[S]` applies with either.

- **[A] 0002 §3 item 1:** "Click opens a popover with per-tier last-success times…" → "Click opens the Activity drawer's Sync tab (spec 0016)." States table: add "Paused by the user" to the statusbar copy (FR-9).
- **[B] 0002 FR-10 Debug ledger:** superseded by 0016 FR-24 (Network › Totals). It is no longer hidden behind a setting; per-tier last-hour counts and TSTU stay, per-endpoint and per-origin groups are added. **0002 §8:** "Local debug ledger (FR-10)" → "Network › Totals (0016 FR-24)".
- **[S] 0009 FR-1:** add `[activity]` (`panel_height`, `tab`; `[B]` `network_indicator`). **0009 §3 Sync tab:** `[A]` the "Open sync activity ⌘⇧A" link; `[B]` the "Show network activity in the statusbar" switch (FR-28).
- **`docs/design/principles.md`** §2 statusbar line: `[A]` "sync state (button, opens Activity › Sync), org name"; center unchanged; `[B]` right side gains the network indicator (opens Activity › Network; becomes the offline pill when offline) before the Ken combo counter and the demo pill. `[S]` Window anatomy: the Activity drawer spans the content column above the statusbar. `[A]` §3.3: add the paused statusbar state. `[B]` §3.5: the offline pill is the network indicator's offline form.
- **`docs/design/command-palette.md`** §7.2: `[S]` `⌘⇧A` Activity drawer; inside it `⌃Tab` / `⌃⇧Tab`, `⌘⇧↩` maximize / restore, `F6` focus cycle (content → drawer → toasts); `[B]` `F` failed only, `T` totals, `/` filter, `⌘C` copy rows, `⌘⌫` clear filters, `End` jump to latest. §4.5 commands: `[S]` "Activity: Hide"; `[A]` "Activity: Show sync activity"; `[B]` "Activity: Show network log", "Network log: Export…".
- **[B] `ADOKEN_SYNC_LOG` file format** (`crates/sync/src/ledger.rs`): the line `METHOD host path status bytes ms` becomes the FR-25 JSON Lines record. The only consumer is the builder (`sync_probe` reads entries in memory, not the file). The doc comments in `crates/app/src/sync.rs` and `crates/app/src/lib.rs` and the CHANGELOG wording follow. The project-name and object-id leaks in today's `path_template` were fixed on 2026-10-01 independently of this spec (FR-19).
- **[A] `docs/adr/0001-local-store.md`:** a new **local** table holds each lane's last successful pass (FR-10), added by store migration 0010; it survives cache rebuilds. Pass records and the network log stay in memory (FR-13, FR-26) and add no table.
- **From spec 0015 (accepted the same day):** FR-11, FR-16 and FR-18 cover 0015's per-item syncs, work item writes, comments and metadata reads as user-initiated requests ("You: …").

## 5. ADO API surface and limits

| Purpose | Endpoint (api-version) | Scope | Cost / limit notes |
| --- | --- | --- | --- |
| — | **None.** The drawer and the indicator read local state only. | none | 0 TSTU. Sync now uses the existing ⌘R pass (0002 §3, coalesced within 10 s). |
| Headers shown | `Retry-After`, `X-RateLimit-Resource`, `-Delay`, `-Limit`, `-Remaining`, `-Reset`, `-Cost` on any response (documented, §12) | — | `X-RateLimit-Resource` "may vary over time and without warning": shown verbatim, never parsed (FR-8). |
| Header shown (unverified) | `x-vss-activityid` | — | Not on any Microsoft Learn page found (§12); recorded only when present. The AC-B12 real-org probe records whether it arrives. |

- **Rate limits:** zero added requests. Showing measured TSTU per request and per endpoint makes the FR-9 budget (≤ 10 TSTU / 5 min steady, 50 hard cap) visible. The user can compare it with Azure DevOps' Usage page (Organization settings › Usage, Project Collection Administrators only), which lists requests by user agent (adoken sends `adoken/<version>`).
- **No push to local apps:** the drawer shows freshness honestly as the last successful poll and the next one. It can't show changes Azure DevOps has and adoken hasn't polled yet.
- **PAT scopes:** none added. Missing optional scopes (`vso.work`, `vso.build`) show up as the "Needs a scope" row state, and 401/403 rows name the cause, as in 0004 FR-14.

## 6. Acceptance criteria

Harness: 0002's recorded-fixture mock ADO server with latency and scripted headers, injected clock, temp store. `[A]` / `[B]` / `[S]` = group A, group B, shared. Each group's ACs pass with the other group absent.

**Shared**

- **AC-S1 Open, close, focus.** With focus on a PR list row, `⌘⇧A` opens the drawer on its last tab; `Esc` closes it and the same row has focus. A click on the sync item opens Sync, a click on the network indicator opens Network; `⌃Tab` switches tabs. Opening and closing doesn't change the sidebar's height (layout assertion). (unit + story)
- **AC-S2 Persistence.** Resizing to 300 px and switching to Network, then restarting, reopens at 300 px on Network; `panel_height = 5000` clamps to 70% of the window and is reported (0009 FR-5). (unit: settings)
- **AC-S3 No storms.** Given 200 recorded requests and 40 progress updates within 1 s, the shell's activity notifications are ≤ 5 and the statusbar repaints ≤ 5 times; given 60 s with no requests and no state change, zero repaints come from activity. (unit, injected clock + repaint counter)
- **AC-S4 Accessibility.** Each statusbar item exposes a button role with its state in the label; drawer rows expose the labels of FR-4; the splitter is reachable and moves 24 / 96 px per key, never leaving < 120 px of content; `F6` cycles content → drawer → toasts. A lane going Failed → Synced produces exactly two polite announcements; 200 requests with 3 failures within 30 s produce exactly one. (unit: a11y tree)
- **AC-S5 Gallery.** Stories, one snapshot per built-in theme: statusbar (idle, in flight, failures, paused, throttled, offline pill, token rejected, demo, indicator off); drawer at 900 × 600 with sidebar and with rail; Sync (idle, first sync with progress, throttled, one failed lane, paused, missing scope, demo, empty history, pass expanded); Network (live tail, failed only, totals, row detail, empty, demo, 10,000 rows). (snapshot test)
- **AC-S6 Smoke.** `activity_smoke` (`crates/gallery/tests/activity_smoke.rs`) drives the real shell with real key dispatch: ⌘⇧A, a ⌘R pass, expand a pass, `↩` opens a changed PR, switch to Network, `F`, export to a temp file; frame p99 < 16 ms throughout; screenshots in `target/activity-smoke/`. (scripted smoke run) Amended 2026-10-01: no mock-server harness exists for the whole shell (the mock Azure DevOps server lives in `crates/sync/tests/support`), so the smoke runs on the **demo store plus a fixture activity hub**: a network log of 10,000 synthetic requests (37 failures) and recorded passes, with "a ⌘R pass" pushed into the pass tracker as the scheduler would record it. Real request recording over the mock server is covered by the AC-B ACs in `crates/sync`.

**A. Sync activity**

- **AC-A1 Row states.** A pure function from (scheduler activity, governor snapshot, prefs, scopes, now) to rows is unit-tested with one fixture per FR-8 state, including the precedence of every adjacent pair (e.g. offline beats failed, a ⌘R pass beats paused, budget wait beats syncing). (unit)
- **AC-A2 Next time.** With My PRs at 1 min and foreground, the row reads "next in" the scheduler's planned due time (± 1 s); switching to the background (Slower) moves it to ×5; Manual only shows no next time. (unit, injected clock)
- **AC-A3 Progress.** With 12 hot-set PRs due and the mock holding the 4th thread response, the My pull requests row reads "3 of 12"; during the index pass it shows a running count without a total. (mock server)
- **AC-A4 What changed.** Given a warm + hot pass in which the mock changes 3 PRs (one vote, one push, one completed), 2 work items (state, title) and finishes 1 run, while 40 other PRs come back identical: the pass record counts exactly 3 / 2 / 1, lists those ids, and lists none of the 40. A second identical pass lists no changes. With 25 changed work items, it lists 20 and "+5 more". The store-write budget of 0002 §7 (< 20 ms per 1,000 changed rows) still holds (bench). (mock server + bench)
- **AC-A5 Open from history.** `↩` on a listed PR opens the review (0004), on a work item the Work items detail, on a run the run detail; the drawer stays open. (unit + smoke)
- **AC-A6 Pinned row.** With 3 pins, one answering 404, the Pinned items row shows 3 pins, "1 not available", and the time of the last warm pass whose pinned steps all ran. (mock server)
- **AC-A7 Actions.** Sync now in the drawer sends one `RefreshNow` (twice within 10 s: still one); Pause › For 1 hour writes `[sync]` like Settings and the statusbar reads paused until +1 h; Resume clears it; Sync settings… opens Settings on Sync. (unit)
- **AC-A8 Restart.** Given passes at T, a restart with pause on (nothing sent) shows every row's last-synced time from T, and Recent passes is empty. (unit: store + scheduler)
- **AC-A9 Throttled and failing.** A 429 with `Retry-After: 40` and `X-RateLimit-Resource: Core` turns every row to "Waiting for Azure DevOps", with the countdown and "Core"; the rows recover after 40 s. A 503 on the work-items WIQL marks only Work items as Failed with the status and retry time. (mock server)
- **AC-A10 Scopes and token.** A token without `vso.build`: Pipeline runs reads "Needs a scope: vso.build" with no button, and the other rows are normal (amended 2026-10-01, owner notes; accepted 2026-10-01). A 401: every row "Token rejected". (unit)
- **AC-A11 Demo.** In demo mode the rows say demo data, a demo-feed tick appears as a "Demo feed" pass, and Sync now / Pause are disabled with the reason; the mock server sees zero requests. (unit + story)

**B. Network log**

- **AC-B1 Entry fields.** One mock pass yields, for every request: `at` from the injected clock, method, host, route, purpose, origin, attempt 1, queue wait, latency, status, bytes and `X-RateLimit-Cost` as served; entries from all three transports appear in one sequence in send order. (unit + mock server)
- **AC-B2 Redaction canaries.** With org `canaryorg`, project `CanaryProject`, repo `canary-repo`, my email `canary@example.com`, a PAT `canary-pat-0000`, a comment text `canary-body`, and a pinned work item in the unselected project `CanaryOther` (forcing `GET {org}/_apis/projects/{name}`): after a full first sync, a vote, a comment, an avatar fetch and a 404 whose Azure DevOps message names `CanaryProject`, no canary string appears in any entry's host, route, purpose or origin, in the export file, or in the `ADOKEN_SYNC_LOG` file (the in-app error detail may show the 404 message); the project-by-name route reads `/{org}/_apis/projects/{project}`. (mock server + file scan in CI, extends 0002 AC-15)
- **AC-B3 Route coverage.** Every request issued across the mock suites (sync, review writes, files, avatars, pins) matches a route-table entry: zero "Other request" entries. (test over the suite's recorded ledger)
- **AC-B4 Origin.** A scheduled warm pass tags its requests with their lanes, and the pinned PR GET-by-id is also tagged pinned; a ⌘R pass is tagged Sync now; opening a PR tags files and blobs "You: open PR"; a vote is "You: vote" and its re-read within 5 s is "You: vote · re-read"; avatars are "Avatars". No entry is unattributed. (mock server)
- **AC-B5 Retries and holds.** The mock answers 429 + `Retry-After: 2` and then 200: two entries, attempts 1 and 2, the second with queue wait ≥ 2 s (injected clock). A request held by the budget shows a queue wait equal to the hold. (mock server)
- **AC-B6 In-flight and canceled.** With the mock holding 3 responses, the indicator shows 3 in flight; dropping the pass (sleep) completes them as Canceled and the count returns to 0. (unit)
- **AC-B7 Indicator tone.** One 503 → danger with "1 failed"; opening the Network tab → quiet; a 404 alone → warning; an avatar 404 in a background fetch → logged, tone unchanged; 5 minutes after the last unseen failure → quiet; offline → the Offline pill; demo → hidden; `network_indicator = false` → hidden, and `⌘⇧A` still opens the log. (unit + story)
- **AC-B8 Ring and totals.** After 10,001 requests the oldest is gone and 10,000 remain; since-launch totals still count 10,001; the last-hour totals per tier equal `GovSnapshot::ledger` for the same window. (unit)
- **AC-B9 Filters and tail.** On a 10,000-entry fixture with 37 failures, Failed only shows 37 rows; adding origin "Work items" narrows to their intersection; new entries that match join the tail, others don't; scrolled up, new entries leave the viewport where it was and show "N new". (unit)
- **AC-B10 Export.** Exporting the canary fixture writes JSON Lines whose first line carries version, time and the redaction note, one line per entry with exactly the FR-17 export fields; the export runs off the UI thread (no frame > 16 ms during it). (unit + frame recorder)
- **AC-B11 Performance.** On the 10,000-entry fixture: opening the Network tab paints within one frame; scrolling from top to bottom at 60 fps keeps frame p99 < 16 ms; a filter change paints within one frame; totals compute in < 5 ms. (frame recorder + bench)
- **AC-B12 Real org.** A read-only run of the app against the builder's org with the drawer open for 10 minutes records, in §13 of this spec: the request count and TSTU of the window vs. the Network totals, whether `x-vss-activityid` arrived, and the Usage page's view of user agent `adoken/<version>` if the builder has access. (scripted manual check, sanitized like 0014 §5)

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Recorder overhead per request | < 20 µs on the sync thread, no work on the UI thread per request | Criterion bench in `crates/sync` |
| Statusbar activity repaints | ≤ 4 per second under any request rate; 0 when nothing visible changed; < 1 ms layout each | Repaint counter (AC-S3) + frame recorder |
| `⌘⇧A` / click → drawer painted | ≤ 1 frame (< 16 ms) with 10,000 entries and 200 pass records | Frame recorder |
| Scroll 10,000 rows | Frame p99 < 16 ms (virtualized: only visible rows laid out) | Frame recorder (AC-B11) |
| Filter or totals change | ≤ 1 frame; totals over 10,000 entries < 5 ms | Bench + frame recorder |
| Live tail during the 100k cold backfill (0002 AC-14) with the drawer open | Frame p99 < 16 ms | Perf harness |
| Pass-record bookkeeping | Within 0002 §7's store-write budget (< 20 ms per 1,000 changed rows, off the UI thread) | Criterion bench (AC-A4) |
| Memory | Ring ≤ 4 MB at 10,000 entries; pass records ≤ 1 MB | Perf harness |
| Export of 10,000 entries | < 200 ms, off the UI thread | Bench |
| Cold start | No change (< 500 ms): per-lane last success is read in the existing store-open pass; the drawer's view is created on first open | Launch timer |

Global rules (AGENTS.md) always apply: frame < 16 ms, Cmd+P < 50 ms on 100k items, cold start < 500 ms. Regressions are bugs.

## 8. Success signal (telemetry-free)

- **Local stat the user can see:** the Totals header ("Last 5 min: 0.4 TSTU · 0.2% of your Azure DevOps limit · 41 requests") is the evidence for the "budgeted polling" claim (roadmap principles), visible to every user.
- **Dogfood:** over a working week, the builder diagnoses every "is this stale?" moment from the drawer, without `ADOKEN_SYNC_LOG`, a terminal or the browser. Each such moment gets one line in the dogfood notes: what the drawer showed, and whether it was enough.
- **Support signal:** "is it syncing?" / "why is X missing?" questions from beta users come with a drawer screenshot or a redacted export instead of a description.
- **Public replies:** a build-in-public post showing the network log ("every request adoken makes, on your Mac, nowhere else") gets privacy or trust replies, not "what does it send?" questions.

## 9. Risks

- **Noise.** An always-on network indicator that flickers contradicts "calm by default" (principles §1.3). Mitigations: ≤ 4 Hz, no animation, quiet tone unless failures are unseen, failures calm down once seen, a switch to hide it (Q-3).
- **Redaction regressions.** Every new endpoint can leak a name through its path; shape-based templating already leaked project names on `projects/{name}` and git object ids on `blobs/` (both fixed 2026-10-01). Mitigations: an allow-list route table (FR-19), canary scan (AC-B2), route coverage test (AC-B3), and export without free text.
- **Attribution plumbing.** Origin must reach ~30 call sites in `engine.rs`, plus `writes.rs`, `pr_files.rs` and `avatars.rs`. If it's tagged per call, a new call can forget it. Mitigation: set the origin where the work starts (pass, lane step, user action) and inherit it; unattributed requests assert in debug (FR-18, AC-B4).
- **Change counting cost.** Counting changes inside page transactions could slow big backfills. Mitigation: index entries are counted, never listed; ids capped at 20 per kind; reuse the old-vs-new comparison events already make; bench (AC-A4).
- **Vertical space.** At 600 px tall a 40% drawer leaves ≈ 320 px of content. Mitigations: min height 160 px, resizable, never covers the sidebar, `Esc` closes.
- **Numbers that alarm.** TSTU figures and red 4xx rows (e.g. an expected 404 for a deleted pin) can look worse than they are. Mitigations: TSTU shown as a share of the 200 limit; 404 is warning, not danger; the Pinned row explains "not available".
- **`x-vss-activityid` is undocumented.** It may be absent or renamed. Mitigation: optional field, verified by AC-B12; nothing depends on it.
- **Scope creep toward a dev tool** (bodies, headers, replay, HAR). Every one of those either breaks the redaction promise or serves no "snappy / less noise" job. They stay out (§11).

## 10. Open questions — resolved 2026-10-01

All recommendations accepted by the owner.

- **Q-1 Placement.** **Resolved:** one bottom drawer in the content column with Sync and Network tabs (§3.1, FR-1, FR-2, AC-S1). Popover and separate window stay rejected ("pop out" possibly later, §11).
- **Q-2 Item ids in the network log.** **Resolved:** yes, in the app only, resolved from the local store for display ("PR threads · !4821 Retry idempotency keys"). Copy, export and `ADOKEN_SYNC_LOG` never include ids, names or Azure DevOps' free-text messages (FR-17 `subject`, FR-19, FR-25, AC-B2, AC-B10).
- **Q-3 Network indicator visibility.** **Resolved:** visible by default in connected mode, quiet unless there are unseen failures, with a switch in Settings › Sync to hide it (FR-21, FR-28, AC-B7).
- **Q-4 Retention.** **Resolved:** memory only: the last 10,000 requests and the last 200 passes or 2 h, cleared on quit; only each lane's last successful sync time is stored (FR-10, FR-13, FR-26, AC-A8, AC-B8). `ADOKEN_SYNC_LOG` and Export cover long captures.
- **Q-5 Milestone and cut order.** **Resolved:** v0.1, both groups on the cut line right after the Ken layer: network log cut before sync activity, then the existing items (pins …). With 0015 Q-1 (slice B first, ahead of the Ken layer), the full order is in `docs/roadmap.md`: (1) 0015 slice B, (2) Ken layer, (3) network log, (4) sync activity, (5) pins, then the rest.
- **Q-6 Demo mode.** **Resolved:** honest empty states: no fake requests or sync rows; demo-feed ticks listed as "Demo feed" passes; the gallery uses fixtures (FR-15, §3 States, AC-A11).
- **Q-7 Shortcut.** **Resolved:** `⌘⇧A` toggles the drawer; `⌃Tab` / `⌃⇧Tab` switch tabs inside it (FR-3, AC-S1). `⌘⇧R` belongs to 0015's Sync this work item (accepted the same day).
- **Q-8 Pinned items row.** **Resolved:** its own row, labeled "refreshed with All active PRs, Work items and Pipeline runs" (FR-7, AC-A6).
- **Q-9 Export format.** **Resolved:** JSON Lines for Export, `⌘C` and `ADOKEN_SYNC_LOG`, replacing today's space-separated line (FR-25, FR-26, AC-B10; §4.5 `ADOKEN_SYNC_LOG` amendment).
- **Q-10 Expected failures in the background.** **Resolved:** background avatar 401/404 are logged and listed under Failed only, but don't turn the indicator red unless the user caused the request (FR-21, AC-B7).
- **Q-11 Resource meter in the statusbar (added 2026-10-01, owner notes).** FR-5 and FR-28 amendments add spec 0023's optional memory / CPU meter to the right group. **Resolved (2026-10-01):** after the Ken chip and before the demo pill, off by default, dropping its CPU part right after the notice truncates and hiding after the org (FR-5, FR-28; 0023 FR-1, FR-8, FR-11); its own questions are resolved in 0023 §10.

## 11. Out of scope

- Request and response bodies, request headers, a header inspector, HAR export, curl copy: they break FR-19's redaction promise. Cut.
- Replaying, retrying or cancelling a single request: Sync now covers "try again"; per-request controls are a debugger, not less noise. Cut.
- Persisting the log or pass history across restarts (Q-4); a "pop out" window (Q-1). Revisit if dogfood asks.
- Logging onboarding, PAT verification and minting (0001), and `az` CLI calls: they run before the shell. The PAT-create response must never pass through a recorder.
- Requests by other tools (browser, `az`, git) against the same rate limit: adoken can't see them. Azure DevOps' Usage page shows them (§5).
- Toasts or notifications for failed requests or lanes: the statusbar tone and row states are the signal (less noise). Auth stays 0001's banner.
- Charts and sparklines of TSTU over time: Totals and the 5-minute share answer the question. Possibly later.
- Editing cadence in the drawer: Settings › Sync owns it (0009).
- Per-item "Sync this item": belongs to spec 0015 (work item editing), not this one.
- Multi-org: one org per install (0011 deferred).

## 12. References

- Rate and usage limits (TSTU, 200 / 5 min, `TF400733`, `Retry-After`, `X-RateLimit-Resource` "display to a human, don't compute", `-Delay`, `-Limit`, `-Remaining`, `-Reset`, `-Cost`): https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/rate-limits?view=azure-devops
- Monitor usage (Organization settings › Usage; user agent, command, TSTU and delay columns; Project Collection Administrators only): https://learn.microsoft.com/en-us/azure/devops/organizations/accounts/usage-monitoring?view=azure-devops
- Integration best practices (handle failures gracefully, `VS402335` query timeout, prefer reporting APIs): https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/integration-bestpractices?view=azure-devops
- Projects – Get (7.1; `projectId` path segment, which accepts a name: the redaction case in FR-19): https://learn.microsoft.com/en-us/rest/api/azure/devops/core/projects/get?view=azure-devops-rest-7.1
- Service hooks Web Hooks consumer (needs a public endpoint: why freshness is poll-bounded): https://learn.microsoft.com/en-us/azure/devops/service-hooks/services/webhooks?view=azure-devops
- Scopes table (`vso.work`, `vso.build`; for the "Needs a scope" state): https://learn.microsoft.com/en-us/azure/devops/integrate/get-started/authentication/oauth?view=azure-devops#available-scopes
- `x-vss-activityid`: **no Microsoft Learn page found** (searched 2026-10-01). Treated as an undocumented, optional response header (§5, AC-B12).
