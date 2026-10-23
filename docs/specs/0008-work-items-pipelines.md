# 0008 — Work items and pipelines: views over the local store

| Field | Value |
| --- | --- |
| Status | Accepted (2026-09-25, feature wave brief) |
| Milestone | v0.1.0 (views); the work item detail's editing is spec 0015 (slices A + B in v0.1, accepted 2026-10-01); pipeline re-run stays in v0.2 (`docs/roadmap.md`) |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/app` (`workitems`, `pipelines`), `crates/store` (reads), `crates/ui` (icons; tree picker per-node counts, FR-8) |
| Serves | Snappy (every read is local), Less noise (my items and my failed runs first) |
| Depends on | `docs/specs/0002-sync.md` (work item full set, builds FR-2/FR-3, events FR-11), `docs/specs/0003-command-palette.md` (targets), `docs/specs/0004-pr-review.md` (linked PRs open there) |
| Design | `docs/design/principles.md` §2 (list + detail), §3 (states); `docs/design/tokens.md` §2.8 (`wi.*`, `pipeline.*`, `priority.*`) |

## 1. Problem

"What am I on, and did my build break?" takes three web hubs (Boards, Pipelines, Repos), each a multi-second page load with its own filters. The palette (0003) can jump to one item, but there is no place to *browse* my work items or recent runs, and palette/inbox/Today results for work items and runs land in a generic Search detail pane.

## 2. Users and job to be done

- **Persona:** developer who lives in PRs but owns a handful of work items and breaks a build now and then.
- **JTBD:** "When I sit down or get pinged, I want to see my open work and my latest runs instantly, so I can pick the next thing without opening the browser."
- **Smallest delightful slice:** `⌘5` shows my open work grouped by state in under a frame; `⌘6` shows every pipeline with a last-runs sparkline, and a failed run's stages, duration and linked PR are one keystroke away.

## 3. UX flow

Both views follow principles §2: list pane (380) + detail pane; `⌘⇧↩` toggles the list (same action as Pull requests).

**Work items (`⌘5`).** Tabs (`←`/`→` or palette): **My work** (open items assigned to, created by, or last changed by me), **Assigned** (assigned to me, any state), **Recently changed** (any item changed in the last 7 days), **By area**, **By iteration** (open items in one area / iteration path or below it; the path label is a dropdown listing every area or iteration of the synced projects, type to jump, FR-8; `[` / `]` still step through the paths that have open items, most-used first). Every tab is grouped by state (in-progress states, then proposed, resolved, done, removed), each group sorted by priority then last change (Recently changed: last change only). Rows: type icon (`wi.*`), title, `#id · state · assignee · iteration`, priority glyph, linked-PR count, age. (amended 2026-10-01, owner notes; accepted 2026-10-01)
Selecting a row shows its detail at once: type + id, title, state/priority/tag chips, fields (assigned to, area, iteration, parent, created, changed), description, linked PRs, discussion, history. Description and comments render as rich text (spec 0015 FR-3), not plain text; the detail's layout, editing and freshness are spec 0015's (amended 2026-10-01). `↩` moves focus into the detail, where `↑↓` walk every detail stop, not only links (spec 0015 §3.3); `↩` opens or edits the focused stop (PR → Pull requests view, parent → its detail), `⎋` returns to the list. "Open in Azure DevOps" is disabled in the demo org.

**Pipelines (`⌘6`).** Tabs: **Pipelines** (definitions), **Mine** (runs requested for me), **Failed** (failed or partially succeeded), **All runs**. Definition rows: status of the latest run, name, folder, a sparkline of the last 12 runs (bar height = duration, color = result), age. `↩` on a definition filters the runs list to it (chip; `⌘⌫` clears). On run tabs `[` / `]` filter by branch. Run rows: status icon, `pipeline #number`, branch · requested for · reason, duration, age. Run detail: result, duration (live "running for" while in progress), queued/started/finished, requested for, reason, branch, commit, linked PR (opens the PR view), and stages with per-stage result and duration. Definition detail: pass rate, median duration, large sparkline, recent runs. *(Amended 2026-10-02 by 0025 FR-5: Pipelines is `⌘5`, Work items `⌘4`, Search `⌘6`)*

Palette, Today and Inbox targets for work items (`#id`) and runs open these views with the item selected.

### States

| State | Behavior |
| --- | --- |
| Empty | Per tab, principles §3.1: "Nothing assigned to you" (good) vs "No work items synced yet"; filtered to nothing shows [Clear filters `⌘⌫`]. |
| Loading / first sync | Nothing for 150 ms, then skeleton rows until the first read lands (kept ≥ 300 ms once shown). Later reloads (after store commits) update in place. |
| Error | Pane banner: "Couldn't read work items from the local store. Your synced data is untouched. [Retry]". |
| Offline / stale | Reads are local; header shows "Updated 18m ago" when the last sync is older than 15 min. |
| Throttled by ADO | Not applicable to the lists: they make no network calls. The path dropdown's tree fetch (FR-8) waits like any user-priority read and shows the loaded paths meanwhile. The work item detail's writes and per-item syncs: spec 0015 §States. |

## 4. Functional requirements

- **FR-1** Views read only the local store on background executors; the lists issue no network request, except the path dropdown's classification tree read (FR-8), which is 0015 FR-33's cached metadata read, sent at most once per project per 24 h when the user opens the dropdown. The work item **detail** also sends user-initiated writes and per-item syncs (spec 0015, amended 2026-10-01); nothing else does. (amended 2026-10-01, owner notes; accepted 2026-10-01)
- **FR-2** Work item tabs, grouping and ordering as in §3; counts in tab labels. The `[` / `]` path cycle uses the paths of loaded open items; the path dropdown lists the full classification trees (FR-8). (amended 2026-10-01, owner notes; accepted 2026-10-01)
- **FR-3** Work item detail shows linked PRs with their status when the PR is in the store (`!id` otherwise), the parent when loaded, comments with `@<guid>` resolved to names, and history = the item's sync events (changed, assigned, mentioned) plus created/last-changed. Earlier revisions are not synced; the section says so.
- **FR-4** Pipelines tabs, sparkline and filters as in §3. Run duration = finish − start; running runs show elapsed time.
- **FR-5** Stages are read from the `pipelines_stage` store table (core schema since store migration 0002; filled by sync from the build timeline, spec 0002 FR-2). A one-stage YAML run shows its jobs. Runs without stage rows say "Stage details aren't synced yet". The demo org seeds stage rows deterministically from each run's result (`adoken_demo::demo_stages`). Real orgs sync a 7-day window; "Pipelines: load older runs" (palette, or the empty state's button) pages further back.
- **FR-6** Every view action is a palette command; list navigation is `↑↓`/`jk`, `↩`, `⎋`, `←→` for tabs, `[`/`]` for paths/branches, `/` opens the path dropdown on By area / By iteration (FR-8), `⌘⌫` clears filters. (amended 2026-10-01, owner notes; accepted 2026-10-01)
- **FR-7** Store commits (sync, demo feed) reload the lists, debounced, keeping the selection by id.
- **FR-8 Area and iteration dropdown** (amended 2026-10-01, owner notes; accepted 2026-10-01). On By area and By iteration, the path label is a dropdown button. A click on it, `/` in the list (`WorkItemList && !TextInput`), or `⌘P` › "Work items: Filter by area…" / "Work items: Filter by iteration…" (from any tab or view: switches to that tab first) opens the tree picker of spec 0015 FR-14 (`crates/ui/src/picker/tree.rs`), anchored to the label, with the keyboard in its search field.
  - **Contents.** One root per project that has loaded work items (one project: one root, expanded). Each project's nodes are its stored classification tree (`wi_class_tree`, 0015 FR-33), merged with every path seen on a loaded open item, so no loaded item is unreachable and nodes added since the tree was fetched still show. A project without a stored tree shows the loaded paths only until its tree arrives. The current filter path is expanded to and selected.
  - **Counts.** Each node shows the number of loaded open items at or below it; nodes with none show "0" and stay selectable.
  - **Type to jump.** Typing filters by any path segment and keeps ancestors visible (0015 FR-14, AC-12's ≤ 5 ms on 10,000 nodes). `↑↓` move, `→`/`←` expand / collapse, `↩` or a click chooses and closes, `Esc` closes unchanged; the keyboard returns to the list.
  - **Iterations.** Current / Next / Recent shortcuts above the tree (0015 FR-14), computed per project from node dates; with more than one project, each shortcut row names its project.
  - **Choosing.** A chosen node filters the tab to open items whose path is that node or lies below it (Q-3). The label shows the full path, with "and below" when the node has children. The list selects its first item. The chosen path persists for the session, like today's `[` / `]` choice.
  - **Fetching.** On open, each listed project whose stored tree is missing or older than 24 h is fetched once (0015 FR-33: `GET {project}/_apis/wit/classificationnodes?$depth=14`, `vso.work`), at user priority, attributed "You: filter by area" / "You: filter by iteration" in the network log (0016). The arriving tree merges in place without moving the selection or the query. Demo mode and offline send nothing; then the footer reads "Showing paths from synced items". A failed fetch keeps the loaded paths and retries on the next open (0015 FR-33).
  - **Building.** Nodes and counts are built on a background executor after each list load and each tree arrival, never on open, so opening paints in one frame.
  - **`[` / `]`** keep stepping through the paths that have open items, most items first (unchanged); from a chosen node that isn't one of those paths, `]` starts at the first.

## 5. ADO API surface and limits

No new calls from the lists except the path dropdown's tree read (FR-8). Data comes from 0002's work item full set, discussions and build sync; stage rows from `GET {project}/_apis/build/builds/{id}/timeline?api-version=7.1` (`vso.build`), see 0002 §5.1. The work item detail's writes, per-item syncs and metadata reads are listed in spec 0015 §5.

| Purpose | Endpoint (api-version) | Scope | Cost / limit notes |
| --- | --- | --- | --- |
| Area and iteration trees for the path dropdown (FR-8, amended 2026-10-01) | `GET {org}/{project}/_apis/wit/classificationnodes?$depth=14&api-version=7.1` | `vso.work` (already required for Work items) | Shared with 0015 FR-14/FR-33: one call per project per 24 h, whichever opens first. Only on a user's open of the dropdown, at user priority under the governor; never by scheduled sync. With many projects the first open sends one call per project that has loaded items. Limits: 10,000 paths per project each, depth 14 (0015 §5). |

## 6. Acceptance criteria

- **AC-1** Given the demo store, My work lists exactly the open items assigned to, created by or last changed by me, grouped by state with in-progress groups first (unit test on the view model).
- **AC-2** Given items with priorities 1–4 and none, each state group orders by priority ascending (none last), then last change descending (unit test).
- **AC-3** By area / By iteration list only open items whose path equals the selected path or lies below it; `]` past the last path wraps (unit test). (amended 2026-10-01, owner notes; accepted 2026-10-01)
- **AC-4** Mine / Failed / All runs filters and the pipeline and branch filters compose; clearing restores the tab (unit test).
- **AC-5** The sparkline of a definition holds its last 12 runs oldest → newest with heights proportional to duration (unit test).
- **AC-6** Demo stage seeding: a failed run has exactly one failed stage and every later stage skipped; a succeeded run has only succeeded stages (unit test).
- **AC-7** Gallery stories for both views render in every built-in theme (snapshot test).
- **AC-8** `ADOKEN_DEMO=1 ADOKEN_NAV=workitems|pipelines` opens each view on the demo org (scripted `screencapture -l` check).
- **AC-9 Dropdown contents** (FR-8, amended 2026-10-01, accepted 2026-10-01). Given a temp store with projects P1 (stored tree with 40 nodes, 6 of them on loaded open items) and P2 (no stored tree, open items on 3 paths) and no connection: opening the By area dropdown shows two roots; P1 lists all 40 nodes with counts (nodes without items show 0), P2 lists its 3 loaded paths; an item whose path isn't in P1's stored tree appears under P1 too. (unit on the node builder)
- **AC-10 Type to jump and choose** (FR-8). On By area, `/` opens the dropdown with the keyboard in its search field; typing "infra" lists matching nodes with their ancestors; `↩` on `P1\Infra` closes it, the label reads "P1\Infra and below", the list shows exactly the open items under `P1\Infra` with the first selected, and the keyboard is on the list. `Esc` instead leaves the filter unchanged. "Work items: Filter by iteration…" from Pull requests opens Work items on By iteration with the dropdown open. (unit + smoke)
- **AC-11 Fetch once** (FR-8). Against the mock server, with P1's stored tree 2 h old and P2's missing: the first open sends exactly one `classificationnodes` GET, for P2, at user priority with the origin "You: filter by area"; the P2 tree merges while open without moving the selection or the query; a second open the same day sends none; a 500 keeps the loaded paths and the next open retries; demo mode sends none and shows "Showing paths from synced items". (mock server)
- **AC-12 Iteration shortcuts** (FR-8). With today inside P1's Sprint 44 and P2's Sprint 7, Current lists both, each naming its project; Next lists each project's next sprint. (unit)
- **AC-13 Cycling kept** (FR-8). `[` / `]` step through the paths with open items, most items first; after choosing a parent node, `]` selects the first such path. (unit)
- **AC-14 Dropdown stories** (FR-8). Area (one project, two projects), iteration with shortcuts, filtered, P2 still loading; one snapshot per built-in theme. (snapshot test)

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Tab switch / selection move | < 16 ms frame | In-memory filter over the loaded set (≤ 5,000 items), unit-timed in tests |
| List load after store open | off the UI thread | Background executor; first paint never waits |
| Path dropdown open (FR-8, amended 2026-10-01) | Painted within 1 frame with 5 projects × 10,000 nodes and 5,000 loaded items | Frame recorder; nodes and counts prebuilt |
| Path dropdown node build (FR-8) | < 20 ms on a background executor for 5 × 10,000 nodes and 5,000 items | Criterion bench |
| Path dropdown keystroke (FR-8) | ≤ 5 ms filter on 10,000 nodes (0015 AC-12) | Bench |

## 8. Success signal (telemetry-free)

Dogfood: the builder stops opening Boards/Pipelines in the browser for "what's mine" and "why did it fail".

## 9. Risks

- Work item volume: only the newest 5,000 changed items plus all of mine are loaded; By area/iteration on larger orgs shows that window (said in the empty state). The dropdown's counts count that window too, so "0" can mean "none loaded"; the footer says "Counts cover the newest 5,000 synced items" when the window is full (amended 2026-10-01).
- Many projects (FR-8): an org with 30 selected projects sends up to 30 tree reads on the first open of the day. They run at user priority under the governor (0002 FR-7); the loaded paths show at once, so nothing waits on them.

## 10. Open questions — resolved 2026-09-25 and 2026-10-01

- **Q-1** Quick-edit (state, assignee) from the detail? **Resolved:** no, v0.2 (roadmap, `vso.work_write`). **Superseded 2026-10-01:** spec 0015 (accepted) edits the work item detail in v0.1 (slices A + B); move to project in v0.2.
- **Q-2** Re-run / approve from run detail? **Resolved:** no, v0.2 (`vso.build_execute`).

### 2026-10-01 amendment — resolved 2026-10-01

All recommendations accepted by the owner.

- **Q-3 Choosing a parent area or iteration: include everything below it?** **Resolved:** yes, "the node and below", as Azure DevOps' "Under" queries do; exact matching stays reachable by choosing a leaf (FR-8, AC-3, AC-10).
- **Q-4 Fetch missing or stale trees when the dropdown opens?** **Resolved:** yes, once per project per 24 h, user-initiated, `vso.work` already granted (FR-1, FR-8).
- **Q-5 Milestone of the amendment.** **Resolved:** v0.1, RICE 112; it rides with 0015 slice B on the cut line (`docs/roadmap.md`).

## 11. Out of scope

Queuing or cancelling runs, logs and test results, boards/backlog drag and drop. Editing work items moved to spec 0015. Path dropdown (FR-8): selecting several paths at once, saved path filters, and a team's "current iteration" from team settings (`work/teamsettings/iterations?$timeframe=current` needs a team context adoken doesn't have; node dates are used instead).

## 12. References

- Builds — Get Build Timeline: https://learn.microsoft.com/en-us/rest/api/azure/devops/build/timeline/get?view=azure-devops-rest-7.1
- Work Items — Get Work Items Batch: https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-items/get-work-items-batch?view=azure-devops-rest-7.1
- Classification Nodes — Get Classification Nodes: https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/classification-nodes/get-classification-nodes?view=azure-devops-rest-7.1
