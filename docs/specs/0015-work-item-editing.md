# 0015 — Work item detail and editing

| Field | Value |
| --- | --- |
| Status | Accepted (2026-10-01, owner: all recommendations) |
| Milestone | v0.1: slices A + B. Slice A is **not** on the cut line; slice B is on the cut line ahead of the Ken layer (first until 2026-10-01; since then sixth, after the specs accepted that day: tours, resource meter, the 0019 Threads view, Today polish and the global hotkey). v0.2: slice C, move to project (FR-24, AC-21). Q-1, `docs/roadmap.md`. Replaces the roadmap's v0.2 "Work item quick actions" (`docs/ideas.md` #5); the owner asked for this now (2026-10-01 dogfood notes). |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/store` (new cache columns and tables, local drafts), `crates/sync` (work item writes, per-item refresh, metadata fetches), `crates/app` (`workitems` detail, editors, pickers, palette commands), `crates/ui` (rich text view, tree picker, people picker), `crates/gallery` |
| Serves | **Snappy:** a state change, an assignment or a comment takes one key and lands on the next frame, instead of a multi-second work item form load. **Less noise:** no browser tab per work item; my own edits never come back as notifications; the detail says how fresh it is, so I don't reload to check. |
| Depends on | `docs/specs/0001-onboarding.md` §5.2 (scopes), `docs/specs/0002-sync.md` (FR-7 governor, FR-11/12 events, FR-14/15 writes), `docs/specs/0003-command-palette.md`, `docs/specs/0004-pr-review.md` (FR-8 drafts, FR-14 link keys, FR-17 work item picker and write pattern), `docs/specs/0008-work-items-pipelines.md`, `docs/specs/0012-avatars.md`, `docs/specs/0014-pins.md`, `docs/adr/0001-local-store.md`. Related, not required: `docs/specs/0016-statusbar-activity.md` (its network log shows the requests this spec sends). |
| Design | `docs/design/work-item-detail.md` (ux-designer): §2 Layout, §3 Header (§3.4 Freshness and Sync this item), §4 Fields, §5 Description, §6 Links, §7 Discussion, §8 History, §9 Edit states, §10 Keyboard and focus, §11 Motion and sound, §12 States, §13 Accessibility, §14 Gallery, §15 New tokens and icons. Layout context: `docs/design/principles.md` §2, §3. |

Slices A and B are implemented in the working tree; slice C waits for v0.2. Status remains **Accepted** until AC-28's real-org probe is recorded. The probe is `ADOKEN_PROBE_ORGS=<personal-org> cargo run -p adoken-sync --example wi_probe -- --org <personal-org>`: it only touches items it creates, and it refuses any org not listed in `ADOKEN_PROBE_ORGS`. Its first run (2026-10-01) stopped at the read-only preflight before writing anything: "token valid for reads, lacks Work Items (Read & write)". That personal-org token predates the 2026-09-30 Review change that added `vso.work_write`, so it must be renewed before the probe can run.

> Rule: no code without an **Accepted** spec (AGENTS.md). The spec owns behavior, acceptance, and budgets; design docs own visuals, motion, sound, and copy.

## 1. Problem

"The work item details view is so simple." Today adoken's detail (0008) is read-only: a key/value list (Assigned to, Project, Area, Iteration, Created, Changed), the description as plain text, links, discussion and history, plus Pin and Open in Azure DevOps. Every change, even moving my task to Resolved or answering a question in the discussion, means opening the browser. The web work item form takes seconds to load, and every save, comment and reassignment there is a separate round of clicks. I also can't tell how old what I'm looking at is: the statusbar says when sync last ran, but not when *this* item was last fetched, and I have no way to refresh just this one.

## 2. Users and job to be done

- **Persona:** a developer who owns a handful of work items, moves them through states during the day, reassigns or re-sprints them during standup, and answers questions in their discussions.
- **Job to be done:** "When I'm looking at a work item, I want to change its state, owner, sprint, links or text, and reply in its discussion, without leaving adoken or waiting for a page, so I can keep the board true in seconds and get back to code."
- **Smallest delightful slice:** in the work item detail, `s` → type "res" → `↩` moves the item to Resolved on the next frame (pending until Azure DevOps confirms); `c` → type → `⌘↩` posts a comment; the header says "Synced 2m ago" and `⌘⇧R` refreshes just this item.
- **Priority (RICE, `docs/ideas.md` scoring), split into three slices that can each ship or be cut alone:**

  | Slice | Contents | Reach | Impact | Confidence | Effort | **RICE** |
  | --- | --- | --- | --- | --- | --- | --- |
  | **A** | Redesigned detail and rich description view, freshness label, Sync this item, State, Assigned to, Title, comments with @mentions | 70 | 2 | 80% | 1.5 wk | **75** |
  | **B** | Area, Iteration, Description editing, work item links (add, change parent, remove) | 40 | 1 | 80% | 1.5 wk | **21** |
  | **C** | Move to another project | 10 | 1 | 50% | 1 wk | **5** |

  Slice C scores low because moving a work item needs the **Move work items out of this project** permission, which Contributors don't have by default (§5), and because Azure DevOps resets the state on a move. It stays in this spec because the owner asked for it; it ships in v0.2 (Q-1).
- **Already assumed elsewhere:** 0001 §5.2 already requests `vso.work_write` at the Review access level (for 0004 FR-17 links), so **no new scope** is needed. That also removes the reason `docs/ideas.md` gave for batching #5 with #12 ("re-authorize once").

## 3. UX flow

### 3.1 Layout

The detail keeps 0008's place (Work items view, list + detail; `⌘⇧↩` full width) and becomes, top to bottom (visuals: design §2–§8):

1. **Header:** type icon, `#id`, Title, State chip (the only place State is shown and changed), Priority (read-only), Freshness ("Synced 2m ago"), then actions: Sync this item, Pin (`⌘D`, 0014), Open in Azure DevOps.
2. **Fields:** Assigned to, Project, Area, Iteration, each a button showing its value. Below them, a quiet line: "Created Apr 22 by Kwame Mensah · Changed 4d ago by Mei Tanaka".
3. **Description:** rendered rich text (FR-3). Editable (FR-17–FR-20).
4. **Links:** Parent, Children, Related, Predecessors, Successors, Duplicates, plus read-only pull requests, commits, hyperlinks and other artifacts. Followed by "Add link".
5. **Discussion:** comments, oldest first, with the composer last.
6. **History:** unchanged from 0008 FR-3.

### 3.2 Happy path

1. `⌘5`, `↓` to "Timezone bug in daily settlement job". The detail shows at once from the store, with "Synced 1m ago" in the header.
2. `s` opens the state menu, listing only the states this item can move to (FR-11). Type "res", `↩`. The chip shows "Resolved" with a pending clock and "Sending…" on the next frame. About 300 ms later Azure DevOps confirms and the clock disappears. The list row moves to the Resolved group.
3. `a` opens the people picker. Type "dan", `↩` assigns Dana Ruiz. `i` would have assigned it to me in one key.
4. `c` focuses the composer. Type "@da", pick Dana, then "can you verify on staging?", `⌘↩`. The comment shows at once with "Sending…", then as posted. Dana gets Azure DevOps' usual mention email.
5. `⌘⇧R` (or the header's Sync button) refreshes just this item. The freshness label reads "Synced just now".

### 3.3 Every action, keyboard and pointer

Keys work in the Work items view when a work item is selected in the list or the detail has focus, **never while a text field has focus** (the same rule as the PR review's single-letter keys, 0004). Every action is also a `⌘P` command named "Work item: …" and appears in the `?` cheat sheet. Pointer: every field, chip and link row is a button; nothing appears only on hover (principles §1.2).

| Action | Key | Pointer | Editor | Slice |
| --- | --- | --- | --- | --- |
| Edit title | `t` | Click the title | Inline single line: `↩` saves, `Esc` discards | A |
| Change state | `s` (alias `⇧S`, as in the PR review) | Click the State chip | Menu, type to filter, `↩` applies | A |
| Assign | `a` | Click Assigned to | People picker (FR-13) | A |
| Assign to me | `i` | First row of the people picker | None, applied at once | A |
| Comment | `c` (alias `⇧C`, as in the PR review) | Click the composer | Composer: `⌘↩` posts, `Esc` leaves and keeps the draft | A |
| Sync this work item | `⌘⇧R` | Header Sync button | None | A |
| Open in Azure DevOps / Copy link / Copy id | `o` / `y` / `⇧Y` (as in 0004 FR-14) | Header buttons | None | A |
| Change area | `⇧A` | Click Area | Tree picker (FR-14) | B |
| Change iteration | `⇧I` | Click Iteration | Tree picker with Current / Next first (FR-14) | B |
| Edit description | `d` | Click the description, or its Edit button | Text editor: `⌘↩` saves, `⌘E` toggles Write / Preview, `Esc` leaves and keeps the draft (FR-17) | B |
| Add link | `l` | "Add link" | Link-type menu, then the 0004 FR-17 work item picker (FR-21) | B |
| Remove link | `⌫` on a focused work item link row | The row's Remove button | Confirmation, `↩` removes | B |
| Move to project | `⇧P` | Click Project | Sheet: project → area → iteration, with what will change (FR-24) | C (v0.2) |

In the detail, `↑`/`↓` (`j`/`k`) walk every stop in order (title, state, fields, description, each link row, Add link, each comment, composer). `↩` edits or opens the focused stop. `Esc` unwinds one level: editor → detail → list. This replaces 0008's links-only walk.

### 3.4 Alternate paths

- **Someone else changed it meanwhile (conflict):** if Azure DevOps refuses my write because the item has a newer revision and the other person **didn't** touch the field I edited, adoken resends once on top of their revision; nothing to see. If they **did** change that field, the field shows their value and an inline notice: "Changed in Azure DevOps by Dana Ruiz 1m ago. Your edit wasn't saved." with **Apply mine** and **Keep theirs**. My value is kept until I choose (FR-8).
- **A process rule refuses the change:** e.g. a custom rule makes a field required on Resolved. The chip reverts to the server's state and shows Azure DevOps' reason with the field name ("Azure DevOps needs Root cause to move to Resolved."). If the field is one adoken can edit inline (a value list, text, or a person), an inline input appears and `↩` sends the state together with that field. Otherwise: **Open in Azure DevOps** (FR-9).
- **Read-only token:** the fields read like plain values (no buttons). The header has one passive hint with no button, following 0001 FR-9 (copy: design §9; amended 2026-10-01, owner notes; accepted 2026-10-01). Edit keys show the same hint as a toast with no action; `⌘P` commands are listed but disabled, with the reason (FR-28).
- **Offline:** edit controls stay visible, disabled, with the tooltip "Offline — reconnect to send" (principles §3.5). Description and comment drafts keep saving locally. Sync this item is disabled.
- **Only partly synced** (an item from the search index: not mine, not pinned): the header reads "Partly synced". When the selection rests on it for 400 ms, adoken fetches it once (FR-31). The description and links show skeleton lines until the fetch lands.
- **Description too rich to edit here:** a description holding images, tables or colored or styled text opens no editor. It shows "This description uses formatting adoken can't edit yet" with **Open in Azure DevOps** (`o`) (FR-18).
- **Move to project (slice C, v0.2):** `⇧P` opens a sheet. Pick the target project (every project in the org, synced or not). Then pick the area and iteration in it (defaulting to the project roots). The sheet states what Azure DevOps will do: "State resets to New. Child items stay in Storefront." `↩` moves. Types that can't move (test cases and other hidden types) show the reason instead of the sheet (FR-24).

### States

| State | Behavior |
| --- | --- |
| Empty | Per-section empty lines as in 0008 ("No description", "No links", "No comments yet"). An empty description in an editable item shows "Add a description · d". |
| Loading / first sync | Unchanged from 0008: the detail paints from the store with no wait. A partly synced item shows skeleton lines in Description and Links until its fetch lands (FR-31). Pickers paint from cache at once. When metadata isn't cached yet (states, area/iteration trees, directory search), the picker shows nothing for 150 ms, then a "Loading…" row (principles §3.2). |
| Error | A failed write: the field shows the server's value and an inline error naming the cause, with **Retry** holding my value (FR-7). A failed per-item sync: "Couldn't sync · Retry" in the freshness slot; the data shown is kept. A 404 on sync or write: "This work item was deleted or you lost access to it", last data kept, editing disabled. |
| Offline / stale | Offline: §3.4. Stale: more than 15 min since the item was last confirmed (FR-29) shows "Updated 18m ago" in the stale style (principles §3.3); `⌘⇧R` refreshes it. |
| Throttled by ADO | Writes and per-item syncs wait in the governor's user queue (0002 FR-7, FR-8). After 3 s pending, the line reads "Waiting for Azure DevOps…" (the 0004 FR-15 wording). Nothing is dropped. |
| Pending write | Principles §3.3: new value plus a `clock` glyph and "Sending…". Never a done state before Azure DevOps answers. |
| Conflict | §3.4 and FR-8. |
| Rule violation | §3.4 and FR-9. |
| Read-only token | §3.4 and FR-28. |
| Demo org | Edits apply to the demo store locally, through the same pending → done states (a fake delay ≤ 300 ms). Sync this item and Open in Azure DevOps are disabled with the existing demo reason. |

## 4. Functional requirements

### Detail and rendering

- **FR-1 Sections and order.** As §3.1. State lives only in the header chip; Priority is read-only header meta; Created/Changed is one meta line below Fields. The Pin toggle and Open in Azure DevOps stay in the header (0014 FR-2, 0008).
- **FR-2 Opens from the store.** Selecting an item paints its detail from the store on the next frame, like 0008 FR-1. Rich text is parsed off the UI thread and cached per `(id, rev)`. The detail never waits on the network; fetches (FR-30, FR-31) update it in place.
- **FR-3 Rich description.** Description and work item comments are rendered from their **source** (HTML or Markdown, per Azure DevOps' format for that field or comment). Supported: paragraphs, line breaks, headings, bold, italic, underline, strikethrough, inline code, code blocks (monospace, no syntax highlighting), ordered/unordered/nested lists, Markdown task lists (read-only checkboxes), blockquotes, horizontal rules, links, and tables (a plain grid; wide tables scroll horizontally inside the section). Mentions (`@<id>` in Markdown, `data-vss-mention` anchors in HTML) show the person's display name. `#123` work item references and Azure DevOps work item URLs open in adoken; other links open in the browser. **Images are not fetched:** each shows a placeholder chip "Image" that opens the image in the browser (Q-3). Unknown tags render their text. Inline styles (colors, fonts) are not reproduced. Rendering never makes a network request. **Repro Steps and Acceptance Criteria** (`Microsoft.VSTS.TCM.ReproSteps`, `Microsoft.VSTS.Common.AcceptanceCriteria`), when the item has them, render read-only under Description with the same renderer, each in its own section; they are never edited here (Q-10, §11).
- **FR-4 Discussion.** Comments show author avatar (0012), name, relative time and rendered body, oldest first. Comments I post appear at once as pending (FR-6). Comments by me are marked "You". Discussion count in the section header.

### Editing: common rules

- **FR-5 Write path.** Every field edit is one `PATCH …/wit/workitems/{id}?$expand=relations&api-version=7.1` (JSON Patch) starting with `{"op": "test", "path": "/rev", "value": <base rev>}`. Base rev is the revision the user was looking at when the edit started. Writes are user-initiated, so they bypass tier scheduling but not the governor: they run at the governor's user priority, which has its own slot (0002 FR-7, FR-14). adoken never sets `bypassRules` or `suppressNotifications` (Q-13).
- **FR-6 Optimistic, never fake.** The edited element shows the new value with the pending clock and "Sending…" on the next frame (principles §3.3). No success toast for routine writes: success is the pending clock clearing. The only success sound is the existing `ui.action_done`, which plays only when the user has turned sounds on, as for votes (design §11, `docs/design/sound.md` §2.1). Exception: the celebrations of spec 0022 (a Milestone's toast in the static fallback, `ui.celebrate_*` in place of `ui.action_done`; amended 2026-10-01, owner notes; accepted 2026-10-01). The store is changed only by Azure DevOps' answer (FR-7), never by the optimistic value.
- **FR-7 Server wins.** A 200 response is the re-read (0002 FR-15): its full work item (fields, relations, new rev) is applied to the store in one transaction, together with the self-caused events a sync pass would derive (0002 FR-11/12, e.g. `WorkItemAssigned` with `self_caused = true`). No follow-up GET is sent. Values Azure DevOps set by rule (Reason, Changed by, State-dependent defaults) show as returned. On failure the element reverts to the stored value and shows the cause inline, with **Retry** holding my value. The cause names:
  - 401 or 403 from the work item transport where the token lacks `vso.work_write`: the read-only hint (FR-28). Azure DevOps answers **401** when the PAT lacks `vso.work_write` (observed, `crates/app/src/sync.rs` `OptionalAdoTransport`). That must never mark the token as rejected.
  - 403 otherwise: "Azure DevOps didn't let you edit work items in this area" plus Azure DevOps' own message.
  - 404: §States Error.
  - 5xx or network error: "Couldn't reach Azure DevOps. Nothing changed. [Retry]".
- **FR-8 Conflicts.** On any other 4xx, adoken re-reads the item (one GET at user priority) instead of trusting the status code. The status Azure DevOps returns for a failed `test /rev` is not documented (§9); AC-28 records it.
  - **Server rev = base rev:** a rule refused the change (FR-9).
  - **Server rev > base rev, and every field (or relation) I edited still has its base value on the server:** resend once with `test /rev` = the server rev. This is field-level auto-merge; no UI beyond the pending state.
  - **Server rev > base rev, and someone changed a field I edited:** show the server value with the conflict notice: who changed it and when (`System.ChangedBy`, `System.ChangedDate`), "Your edit wasn't saved", **Apply mine** (resend on the new rev) and **Keep theirs**. Never overwrite silently. Never resend more than once without a user action.
- **FR-9 Rule violations.** When Azure DevOps refuses a change with a field rule (observed as `TF401320: Rule Error for field <name>. Error code: Required, …`; not documented on Learn), the inline error names the field. If the field is a value list, a text field or a person (from `workitemtypes/{type}/fields?$expand=allowedValues`, fetched once when the error names a field), an inline input lets me fill it and `↩` resends the original change with that field in the same PATCH. For anything else: Azure DevOps' message and **Open in Azure DevOps**. adoken does not pre-validate rules (no `validateOnly` round trip before each write: it would double the cost and latency of every edit).
- **FR-10 One write in flight per item.** Edits to one item are sent in order, one at a time; each uses the rev from the previous answer. Edits made while one is pending show as pending too. Different items write in parallel within the governor. A queued edit whose base field changed in the previous answer goes through FR-8's rules. Nothing is sent offline and nothing is queued across restarts (0002 §11: no offline write queue). Unsent description and comment text survives as a draft (FR-20).

### Editing: fields

- **FR-11 State.** The menu lists the states reachable from the current one (`transitions[current]` of the work item type; when Azure DevOps returns no transitions, all states of the type). Order: Proposed, In Progress, Resolved, Completed, Removed categories, each state with its category tint. The current state is checked. Typing filters. `↩` sends `add /fields/System.State`. Reason is left to Azure DevOps' defaults. Type metadata (states with categories, transitions) is fetched once per `(project, type)` on first use and cached for 24 h (FR-33).
- **FR-12 Title.** Inline single-line editor prefilled with the title. `↩` sends `add /fields/System.Title` when it changed. An empty title is refused locally ("Title can't be empty") and nothing is sent. Other limits are Azure DevOps' (its message is shown).
- **FR-13 Assigned to.** The people picker shows, in order: **Assign to me** (`i`), **Unassigned**, people I assigned in adoken recently (local, up to 5), then people already in the store, matched fuzzily on name and sign-in name. Groups are excluded, and so is anyone without a known sign-in name. When I've typed 3+ characters, local matches number fewer than 5 and typing pauses for 300 ms, adoken sends one directory search (`GET vssps …/_apis/identities?searchFilter=General&filterValue=…`, `vso.identity`, user priority; a newer keystroke cancels it). Its results join under "From your directory". The value sent is Azure DevOps' distinct display name `"Display Name <sign-in name>"`, so two people with the same name can't be confused; Unassigned sends `""` (Learn: "Reset an identity field"). If Azure DevOps refuses the person (e.g. not a member of the project), its message is shown (FR-7).
- **FR-14 Area and Iteration.** Tree pickers over the item's project classification nodes. Typing filters by any path segment and keeps ancestors visible. `→`/`←` expand and collapse; `↩` sends `add /fields/System.AreaPath` or `/fields/System.IterationPath`. The iteration picker lists **Current** (dates include today), **Next** (the next start date) and **Recent** (the last two finished) above the tree, using node start/finish dates. Trees are fetched once per project on first use with one `classificationnodes?$depth=14` call (areas and iterations together; 14 is the maximum depth) and cached for 24 h (FR-33). Until the tree arrives, the picker offers the paths seen in the loaded work items (0008 FR-2).
- **FR-15 Project (slice C, v0.2).** See FR-24.
- **FR-16 Unedited fields.** Priority, Tags, Created and Changed are shown, not edited (§11).

### Editing: description

- **FR-17 Editor.** `d` opens a monospace text editor in place of the rendered description, with a Write / Preview toggle (`⌘E`) using FR-3's renderer. `⌘↩` saves; `Esc` leaves and keeps a draft. A format hint under the editor reads "Markdown" or "HTML (saved as HTML)".
  - **Markdown fields** (Azure DevOps `multilineFieldsFormat` = Markdown) are edited as their Markdown source, losslessly. Saving sends `add /fields/System.Description` with the Markdown and `add /multilineFieldsFormat/System.Description: "Markdown"`.
  - **HTML fields** are edited as Markdown converted from the HTML. Saving converts the Markdown back to HTML and sends only `add /fields/System.Description`. The field stays HTML. adoken never converts a field to Markdown: Azure DevOps makes that irreversible (Q-2).
- **FR-18 No silent loss.** The HTML editor opens only when converting the stored HTML to Markdown and back yields the same document (same text, structure, links and mentions). The allowed constructs are FR-3's minus tables and images. Presentational attributes Azure DevOps' own editor adds without visible effect (`box-sizing`, `display: inline`) are ignored. Anything else (images, tables, colors, fonts, highlights, `<span>`s with visible styles, attachments) keeps the editor closed and shows §3.4's message with Open in Azure DevOps. Markdown fields always open: their source is kept as written.
- **FR-19 Description conflicts.** Text is never auto-merged. If the server's description changed since the base rev, FR-8's conflict notice offers **Compare** (theirs above, mine below, read-only), **Overwrite with mine** and **Discard mine**.
- **FR-20 Drafts.** Description and comment drafts are user-authored **local** state: saved as I type (debounced, off the UI thread), restored when the item is reopened and marked "Draft" on the section, deleted when the save is confirmed or I discard. Like 0004 FR-8, drafts persist across restarts and are never sent anywhere.

### Links

- **FR-21 Add link (slice B).** `l` opens a link-type menu: **Parent**, **Child**, **Related**, **Predecessor**, **Successor**, **Duplicate**, **Duplicate of**. "Parent" reads **Change parent…** when the item has one. Then comes the 0004 FR-17 work item picker (the `⌘P` engine scoped to work items: `#id`, bare ids, filters, pasted work item URLs; an unsynced id links all the same). `↩` sends `add /relations/-` with `rel` (`System.LinkTypes.Hierarchy-Reverse`, `Hierarchy-Forward`, `Related`, `Dependency-Reverse`, `Dependency-Forward`, `Duplicate-Forward`, `Duplicate-Reverse`) and the target's work item URL. Change parent removes the old parent relation and adds the new one in **one** PATCH. Refused locally, with the reason and no request: linking an item to itself, a Parent or Child that would make a cycle among stored items, and a link that already exists. Everything else is Azure DevOps' to refuse (its message is shown), e.g. the 1,000 links per item limit.
- **FR-22 Remove link (slice B).** Work item links only. `⌫` on a focused link row, or the row's Remove button, asks "Remove link to #4821?"; `⌘P` › "Work item: Remove link…" first opens a picker over the item's work item links (as 0004's Unlink), then the same confirmation. `↩` sends `remove /relations/{index}` guarded by `test /rev`. The index comes from the stored relation order at the stored rev; FR-8 covers a shifted index. Pull request, commit, branch, build, hyperlink and attachment links are shown read-only with what they point to; PR rows open the PR (tooltip: "Link or unlink pull requests from the pull request", 0004 FR-17).
- **FR-23 Link display.** Grouped by type in FR-21's order, then pull requests, then other artifacts. Each work item row shows type icon, `#id`, title, state chip and assignee avatar when the target is stored, else `#id` and "Not synced". `↩` opens a work item in Work items, or a PR in its review.

### Move to project (slice C, v0.2: not built in v0.1)

- **FR-24 Move.** `⇧P` (or clicking Project) opens a sheet, never a one-key write. Steps:
  1. Pick the target project from the org's projects (synced or not).
  2. adoken checks the item's type exists there (one `workitemtypes` GET for the target, cached per FR-33). If not: "Project Payments has no Bug type", and no move.
  3. Pick area and iteration in the target (FR-14 pickers on the target's trees; default the project roots).
  4. The sheet states the consequences Azure DevOps documents: the state resets to the type's initial state, and child items stay in the current project with their links kept.
  5. `↩` sends one PATCH with `test /rev` and `add` of `System.TeamProject`, `System.AreaPath` and `System.IterationPath`.

  Hidden types and test-management types (Test Case, Shared Steps, Shared Parameter, Code Review Request/Response, Feedback Request/Response) can't be moved; the action is disabled with that reason. A 403 says "Moving needs the Move work items out of this project permission in Azure DevOps (Contributors don't have it by default)". After the move the item stays selected. If its new project isn't synced, it stays reachable as a referenced project (0014 §5, 0002 FR-2 pinned metadata), and the list shows it until the next reload.

### Discussion

- **FR-25 Post a comment (slice A).** The composer is last in Discussion. `c` focuses it and `⌘↩` posts with `POST {project}/_apis/wit/workItems/{id}/comments?format=markdown&api-version=7.1-preview.4`, body `{"text": "<markdown>"}`. The comment shows at once as pending and is replaced by Azure DevOps' answer. Then adoken sends one GET of the item (`$expand=relations`), because posting a comment creates a new revision, and a stale rev would make the next edit a needless conflict. The posted comment is stored and emits only self-caused events. Empty or whitespace-only comments aren't sent.
- **FR-26 @mentions.** Typing `@` in the composer opens FR-13's people picker (groups included, because Azure DevOps mentions groups too). `↩` inserts a mention chip. Mentions are sent as `@<identity id>` in the Markdown text, the form Learn documents for Markdown comments (Q-4). Azure DevOps sends its own mention email to that person; adoken adds nothing.
- **FR-27 Not in scope for comments:** editing or deleting comments and reactions (§11).

### Access

- **FR-28 Access level.** Edit controls exist only when the token has `vso.work_write` (Review level, 0001 §5.2), or in the demo org, whose writes apply locally (FR-37). Otherwise:
  - values render as plain text;
  - the header shows one passive hint, no button and no keyboard stop (0001 FR-9; amended 2026-10-01, owner notes; accepted 2026-10-01);
  - edit keys show the hint as a toast with no action;
  - `⌘P` commands are listed but disabled, with the reason (command-palette §4.4);
  - Sync this item still works (it only needs `vso.work`).

  A write answered with the FR-7 missing-scope 401/403 switches the item to this mode and keeps my value as a Retry (amended 2026-10-01, owner notes; accepted 2026-10-01).

### Freshness and per-item sync

- **FR-29 Freshness label.** The header shows when this item was last **confirmed** from Azure DevOps. That is the later of:
  - (a) the last time its full fields were fetched or returned by a write, and
  - (b) the last successful pass of the sync lane that covers it.

  A lane covers an item when it would have fetched the item had it changed: the "mine" WIQL lane (0002 FR-2) for items assigned to or created by me, the pinned batch for pinned items (0014 FR-11). "Synced 2m ago" (relative time, principles §5) under 15 min; "Updated 18m ago" in the stale style at 15 min or more; "Partly synced" when the item's description and links were never fetched (title-index rows, 0002 FR-2). The tooltip and accessible label give the full timestamp and the lane: "Last confirmed 14:02:11 · refreshes every 2 min with Work items", or "· only when you sync it" for items no lane covers.
- **FR-30 Sync this work item.** `⌘⇧R`, the header button, or `⌘P` › "Work item: Sync this work item" sends, at governor user priority:
  - one `GET …/wit/workitems/{id}?$expand=relations` (all fields and relations);
  - the comment pages (0002's comment fetch, `7.1-preview.4`, newest first, at most 10 pages).

  It runs while sync is paused (like `⌘R`, 0002 FR-4a) and for items outside synced projects. The button shows an inline spinner (principles §3.2) and then "Synced just now". A press within 10 s of a successful item sync sends nothing and shows "Just synced". Changes it finds update the detail and the list in place and emit events like any sync pass (others' changes notify as usual). A 404 shows §States Error.
- **FR-31 Partly synced items fetch themselves.** When the selection rests 400 ms on a partly synced item, adoken runs FR-30 for it once. This happens at most once per item per 10 min, and never while offline. Arrowing through the list sends nothing for items the selection only passes over.
- **FR-32 Freshness storage.** Per item, the store records:
  - `synced_ms`: set whenever the row is written from an Azure DevOps response (any lane, a write answer, FR-30);
  - `full_synced_ms`: set when full fields and relations were applied.

  Lane coverage uses the existing `sync_cursor.last_ok_ms`. Both columns are **cache** columns (ADR 0001): a cache rebuild clears them with the row.

### Metadata, storage, keyboard, accessibility

- **FR-33 Metadata cache.** Fetched on first use and cached in the store for 24 h; a cache older than 24 h still paints at once and refreshes in the background on use. Never fetched by scheduled sync:
  - work item type metadata (states with category and color, transitions) per `(project, type)`;
  - field metadata only when FR-9 needs it;
  - classification trees per project;
  - the target project's types for FR-24 (slice C, v0.2).
- **FR-34 Store changes** (`docs/adr/0001-local-store.md` amendment, §Amendments):
  - **Cache tables** (dropped and refetched by a rebuild):
    - `work_item` gains `description_source`, `description_format` (`html` | `markdown`), `synced_ms` and `full_synced_ms`. `description` stays as the plain-text projection for search and previews. The raw sources of Repro Steps and Acceptance Criteria (FR-3, Q-10) are stored the same way, with their format.
    - Work item comments keep their raw `body_source` and `format`.
    - A `wi_link` table holds every relation in server order with `rel`, target id or URL, and the rev it was read at.
    - Metadata tables back FR-33.
  - **Local tables:** drafts (FR-20) and recent assignees (FR-13).
  - **Migration:** a store migration fills the new columns by refetching my items and their discussions once, as migration 0002 did.
- **FR-35 Keyboard and palette.** §3.3's keys, contexts and aliases; all overridable in `keymap.toml`. `⌘P` commands: "Work item: Edit title", "Change state…", "Assign…", "Assign to me", "Unassign", "Change area…", "Change iteration…", "Move to project…" (slice C, v0.2), "Edit description", "Add link…", "Remove link…", "Comment", "Sync this work item", "Open in Azure DevOps", "Copy link", "Copy id". The palette's `⌘K` menu on a work item result gains "Change state…", "Assign…", "Assign to me", "Comment…" and "Sync this work item". Each opens the item in Work items with that editor open (principles §1.5: one place for each thing).
- **FR-36 Accessibility.**
  - Each field is a `Role::Button` labeled with field, value and shortcut ("Assigned to, Ada Lindqvist. Change, A").
  - The state menu is a `Role::Menu`; the people picker a `Role::ListBox`; area/iteration a `Role::Tree` with `aria_expanded`.
  - "Sending…", "Saved" (accessible only), conflicts and errors are announced politely; a missing-scope error assertively (principles §6).
  - Every pointer action has a key (§3.3). Reduce motion: pending/confirm transitions have no animation (design §11).
- **FR-37 Demo and gallery.** The demo store's writes apply locally through the 0004 `apply_local` path with the same store effects and events. Gallery stories (design §14):
  - the redesigned detail with a rich description (headings, list, table, image chip, mention);
  - state menu, people picker with directory results, area tree, iteration picker, link-type menu and picker;
  - description editor in Write and Preview, blocked description, comment composer with a mention chip;
  - pending, conflict, rule violation, read-only token, partly synced, offline, stale;
  - the move sheet (slice C, v0.2);
  - the stacked layout under 1,100 px.

  One snapshot per built-in theme (AGENTS.md).

### Amendments to other specs (applied 2026-10-01)

- **0001 §5.2:** the Work items row's Review note becomes "`vso.work_write`, to link work items to pull requests (0004 FR-17) and to edit work items and post comments (0015)". The last bullet's "adoken edits no other work item field" becomes "adoken edits the fields, links and discussion listed in 0015 and never uses `bypassRules` or `suppressNotifications`". Add: a 401 on a work item write means the token lacks `vso.work_write`, never that the token is rejected.
- **0002:**
  - FR-14: "User actions (vote, comment, resolve, complete)" adds "work item field, link and comment writes (0015)".
  - FR-15: "for work items, the PATCH response (with `$expand=relations`) is the re-read; a posted comment is followed by one item GET".
  - FR-1: WorkItem stores description source and format, all relations (`wi_link`), `synced_ms` and `full_synced_ms`; WorkItemComment stores its source and format.
  - New FR: per-item refresh at user priority (0015 FR-30/31), allowed while paused.
  - FR-10 ledger, and 0016 (accepted): these requests count as user-initiated.
- **0008:**
  - Title and FR-1: the views read only the local store; the work item **detail** also sends user-initiated writes and per-item syncs (0015).
  - §3 detail: description and comments are rich text (0015 FR-3), not plain text; `↑↓` walk every detail stop (0015 §3.3).
  - Q-1: superseded by 0015.
  - §11: drop "Editing work items" (boards drag and drop stays out).
- **0003 / `docs/design/command-palette.md` §4.4:** the work item `⌘K` row adds FR-35's actions. §7.2 adds `⌘⇧R` "Sync this work item" (Work items view) and points to 0015 §3.3 for the view's single-letter keys.
- **0004 FR-17:** its work item picker becomes the shared picker reused by 0015 FR-21 (behavior unchanged).
- **`docs/adr/0001-local-store.md`:** `work_item.description` "plain text; hot set only" adds the raw source and format columns and `synced_ms`/`full_synced_ms`. `comment.body` for work items adds raw source and format. New cache tables `wi_link` and metadata; new local tables for drafts and recent assignees.
- **For the orchestrator (roadmap, ideas):**
  - `docs/roadmap.md`: the "Deferred to v0.2" row "Work item quick-edit" is removed, and v0.2 item 5 "Work item quick actions" becomes "Move work item to project (0015 slice C)" (Q-1).
  - `docs/ideas.md`: #5 is specced in 0015 and scheduled by it. #12's "batch the scope" note no longer applies, because Review already holds `vso.work_write`. "Quick actions from nudges" stays an idea.

## 5. ADO API surface and limits

| Purpose | Endpoint (api-version) | Scope | Cost / limit notes |
| --- | --- | --- | --- |
| Edit fields, add/remove links, move project | `PATCH {org}/_apis/wit/workitems/{id}?$expand=relations&api-version=7.1`, `application/json-patch+json`, first op `test /rev` | `vso.work_write` | 1 request per edit (FR-5); the response is the re-read (FR-7). A conflict costs 1 GET plus at most 1 resend (FR-8). Moving needs the project permission (below) and is "Team Services only (not TFS)". |
| Per-item sync; conflict re-read; after a comment | `GET {org}/{project}/_apis/wit/workitems/{id}?$expand=relations&api-version=7.1` | `vso.work` | 1 request. `$expand` and `fields` can't be combined (0014 §5). |
| Comments: read | `GET {org}/{project}/_apis/wit/workItems/{id}/comments?order=desc&api-version=7.1-preview.4` | `vso.work` | **Preview** API, same as 0002. Paged by `continuationToken`; `$expand=renderedText` adds the HTML of Markdown comments. |
| Comments: post | `POST {org}/{project}/_apis/wit/workItems/{id}/comments?format=markdown&api-version=7.1-preview.4` | `vso.work_write` | **Preview** API. The "Add Work Item Comment" reference marks `format` (`markdown` / `html`) as required; the older "Add Comment" page of the same version has no `format`. 1 request + 1 item GET (FR-25). |
| States and transitions of a type | `GET {org}/{project}/_apis/wit/workitemtypes/{type}?api-version=7.1` | `vso.work` | `states` (name, category, color) and `transitions` in one call; cached 24 h per (project, type). |
| Field rules after a rule error | `GET {org}/{project}/_apis/wit/workitemtypes/{type}/fields?$expand=allowedValues&api-version=7.1` | `vso.work` | Only when FR-9 needs it. `alwaysRequired` covers always-required fields only; conditional rules ("required on Resolved") aren't exposed here, hence FR-9's reactive design. |
| Area and iteration trees | `GET {org}/{project}/_apis/wit/classificationnodes?$depth=14&api-version=7.1` | `vso.work` | Areas and iterations in one call; iteration nodes carry start/finish dates in `attributes`. Limits: 10,000 paths per project each, depth 14. Cached 24 h. |
| Types in a target project (move, slice C, v0.2) | `GET {org}/{project}/_apis/wit/workitemtypes?api-version=7.1` | `vso.work` | Only in the move sheet; cached 24 h. |
| Directory people search | `GET https://vssps.dev.azure.com/{org}/_apis/identities?searchFilter=General&filterValue={text}&queryMembership=None&api-version=7.1` | `vso.identity` | Debounced, ≥ 3 chars, only when local matches < 5 (FR-13); cancelled by the next keystroke. Results' ids are assumed to equal work item identity ids [INFERENCE; AC-28 checks]. |
| Resolve a sign-in name by id | `GET https://vssps.dev.azure.com/{org}/_apis/identities?identityIds={id}&api-version=7.1` | `vso.identity` | Only when a stored person lacks a sign-in name (FR-13). |

- **Rate limits:** writes and per-item syncs are user-initiated, so they use the governor's user slot (0002 FR-7) and still count toward the FR-9 budget. No write's TSTU cost has been measured yet; measured GETs cost 0.004–0.04 TSTU (0002 §13, 0014 §5). A heavy editing burst of 30 edits plus 10 comments in 5 minutes is about 60 requests, against the 300-requests-per-5-min cap and the 10 TSTU target. AC-28 records real costs. Metadata adds at most a few requests per project per day. Nothing here adds scheduled traffic: FR-31's auto-fetch is triggered by the user's selection and capped once per item per 10 min.
- **No push to local apps:** other people's edits reach the detail at the cadence of the lane that covers the item (Work items default 2 min, 0002 FR-4a) or immediately with Sync this item. Conflicts therefore surface on write (FR-8), not before. The freshness label (FR-29) makes the window visible.
- **PAT scopes:** `vso.work_write` (writes and comments; Review level, already requested, not high-privilege in 0001's table), `vso.work` (reads and metadata), `vso.identity` (directory search; both levels). No new scope. Without `vso.work_write` (the Read-only level), FR-28 applies.
- **Permissions beyond scopes:** "Edit work items in this node" governs edits per area (403 → FR-7). Moving needs **Move work items out of this project**, which Contributors lack by default. Stakeholders can't move work items. Test-management and hidden types can't move. Child items don't move with their parent. Azure DevOps resets State and Reason on a move.
- **Markdown description format:** `multilineFieldsFormat` became generally available in July 2025 (Azure DevOps blog). It is set in a PATCH with `/multilineFieldsFormat/System.Description`. It is irreversible once Markdown, and it isn't in the 7.1 Learn reference for Work Items – Update. adoken already parses it from work item responses (`crates/sync/src/wire.rs`). Whether the GET with `$expand=relations` and `workitemsbatch` include it is unverified (AC-28).
- **Mentions:** Learn documents `@<userID>` for Markdown comments and `<a href="#" data-vss-mention="version:2.0,{userID}">` for HTML. Neither has been checked on real mention data (0002 §9); AC-28 posts one on the personal org.

## 6. Acceptance criteria

Harness: mock ADO server (records method, path, query and JSON body), fixture org with Agile-style Bug/Task/User Story types and transitions, injected clock, frame recorder, gallery.

- **AC-1 Rich description.** Given a stored HTML description with a heading, a nested list, a link, a mention anchor, a 3×3 table and an image, the detail model renders a heading, list items at two levels, a link that opens the browser, the mentioned person's display name, a 3×3 grid and an "Image" chip. The mock server records zero requests. Given the same content as Markdown with `format = markdown`, the rendered model is equal. (unit: rich text model; story)
- **AC-2 State write.** Given #42 at rev 7 in Active, `s`, "res", `↩`:
  - the chip shows Resolved with the pending clock on the next frame;
  - the mock receives exactly `PATCH /_apis/wit/workitems/42?$expand=relations&api-version=7.1` with `[{"op":"test","path":"/rev","value":7},{"op":"add","path":"/fields/System.State","value":"Resolved"}]`;
  - after the 200 (rev 8, Reason "Fixed" set by the server), the pending clears, the store holds rev 8 and Reason "Fixed", `synced_ms` equals the clock, the list row is in the Resolved group, and no GET follows.

  (mock + unit)
- **AC-3 Transitions and metadata cache.** Given type metadata where Active may go to Active, Closed, Resolved and New, the state menu lists New, Active (checked), Resolved, Closed in category order. A second open within 24 h sends no request; after 24 h the menu paints from cache and one refresh GET follows. (mock + unit, injected clock)
- **AC-4 Assign.**
  - `a`, "da" lists local matches at once and sends no directory request.
  - "dan" with fewer than 5 local matches sends one identities search 300 ms after the last keystroke; typing again before then sends none.
  - `↩` on Dana Ruiz sends `"Dana Ruiz <dana@contoso.com>"`; `i` sends my distinct name; Unassigned sends `""`.
  - No group appears in the picker.

  (mock + unit, injected clock)
- **AC-5 Auto-merge.** Given base rev 7 and a state edit, the mock answers 400 to the PATCH, and the re-read GET returns rev 8 where only `System.Tags` differs. adoken resends once with `test /rev` 8 and succeeds. No conflict notice is ever shown, and the mock saw exactly PATCH, GET, PATCH. (mock)
- **AC-6 Real conflict.** As AC-5, but the GET shows `System.State` changed to Closed by Dana at rev 8. The chip shows Closed with the conflict notice naming Dana. Nothing is resent. **Apply mine** sends a PATCH with `test /rev` 8 and Resolved; **Keep theirs** sends nothing and clears the notice. (mock + story)
- **AC-7 Rule violation.**
  - Given a 400 whose message is `TF401320: Rule Error for field Root cause. Error code: Required, …` and a re-read at the same rev, the chip reverts and the error names "Root cause".
  - When the fields metadata lists allowed values for it, a value menu appears, and choosing one sends a single PATCH with State and Root cause.
  - For a field type adoken doesn't edit, only Open in Azure DevOps is offered.

  (mock + unit: error parser over recorded messages)
- **AC-8 Read-only token.**
  - With Read-only scopes, no edit control in the detail is focusable, the hint is present with no button and no keyboard stop (amended 2026-10-01, owner notes; accepted 2026-10-01), `s` shows the hint toast, and "Work item: Change state…" is listed disabled with the reason.
  - With Review scopes and a mock answering 401 on the work item PATCH, the item switches to read-only mode, the main transport's next sync pass still runs, and the token isn't marked rejected.

  (unit + mock)
- **AC-9 Offline and throttled.**
  - Offline: edit controls are disabled with the tooltip and Sync this item is disabled; typing in the composer saves a draft.
  - Given `Retry-After: 30` from the host: a state edit stays pending, shows "Waiting for Azure DevOps…" after 3 s, and reaches the mock only after the 30 s window.

  (unit + mock, injected clock)
- **AC-10 One write in flight.** State, then assignee, within 50 ms: the second PATCH reaches the mock only after the first's 200, with `test /rev` equal to that response's rev. At no time are two PATCHes for #42 in flight. (mock)
- **AC-11 Title.** `t`, clear, `↩` shows "Title can't be empty" and sends nothing. Changing the text sends `add /fields/System.Title`. `Esc` sends nothing and restores the title. (unit + mock)
- **AC-12 Area and iteration.**
  - The first `⇧A` for a project sends one `classificationnodes?$depth=14` GET; a second picker that day sends none.
  - On a 10,000-node fixture, filtering by "infra" keeps matches with their ancestors in ≤ 5 ms.
  - On the iteration fixture with today inside Sprint 44, the picker shows Current = Sprint 44 and Next = Sprint 45 above the tree.
  - `↩` sends the full path.

  (mock + bench + unit, injected clock)
- **AC-13 Markdown description.** For an item with `multilineFieldsFormat` Markdown, `d` shows the stored Markdown byte for byte, `⌘E` toggles Preview, and `⌘↩` sends `/fields/System.Description` with the edited Markdown plus `/multilineFieldsFormat/System.Description: "Markdown"`. (unit + mock)
- **AC-14 HTML description round trip.** Given the live sample from `crates/sync/src/wire.rs` tests (box-sizing styled `<div>`s, a list), `d` opens the editor with Markdown text. Saving an added line sends HTML that renders to the same document plus the new line, and sends **no** `multilineFieldsFormat` op. (unit: converter round-trip over a fixture set; mock)
- **AC-15 Blocked description.** For HTML containing `<img>`, a `<table>`, or `style="color:red"`, `d` opens no editor and shows the message with Open in Azure DevOps; the mock records nothing. (unit)
- **AC-16 Drafts.** A description draft and a comment draft survive an app restart, are restored and marked "Draft", and are deleted after a confirmed save. `Esc` keeps them; Discard deletes them. A store cache rebuild keeps them. (unit + migration test)
- **AC-17 Comment with mention.** `c`, "@da", choose Dana (id `d291…`), type " please verify", `⌘↩`:
  - the mock receives `POST /{project}/_apis/wit/workItems/42/comments?format=markdown&api-version=7.1-preview.4` with `{"text":"@<d291…> please verify"}`, then one item GET;
  - the comment shows pending, then posted, and renders "@Dana Ruiz please verify";
  - the store's comment row has `format = markdown`;
  - the emitted events are all `self_caused = true`, and no notification is created.

  (mock + unit)
- **AC-18 Add and change links.**
  - `l`, Child, "#123", `↩` sends `test /rev` plus `add /relations/-` with `rel: System.LinkTypes.Hierarchy-Forward` and the URL of work item 123.
  - On an item with parent #7 at relation index 2, Change parent to #9 sends one PATCH: `test /rev`, `remove /relations/2`, then `add /relations/-` with `Hierarchy-Reverse` and the URL of #9.

  (mock)
- **AC-19 Remove link.**
  - `⌫` on the Related row (stored index 3 at rev 7), then `↩`, sends `test /rev 7` and `remove /relations/3`.
  - When the mock refuses it and the re-read shows the link at index 2 at rev 8, adoken resends with index 2 and rev 8 once.
  - On a PR link row, `⌫` does nothing and the tooltip names the PR review.

  (mock)
- **AC-20 Local link guards.** Linking #42 to itself, adding a child of #42 as its parent, or re-adding an existing Related link shows the reason. The mock records zero requests. (unit)
- **AC-21 Move to project (slice C, v0.2).**
  - `⇧P` opens the sheet. A target without the item's type shows the reason and offers no move.
  - Otherwise, after choosing area and iteration, `↩` sends one PATCH with `test /rev` and the three `add` ops (`System.TeamProject`, `System.AreaPath`, `System.IterationPath`). The sheet says the state will reset.
  - A 403 shows the move-permission message.
  - On a Test Case the action is disabled with the reason.

  (mock + unit)
- **AC-22 Freshness.**
  - Given my item fetched at T and a later successful "mine" lane pass at T+5m with no change, the label at T+6m reads "Synced 1m ago".
  - A pinned-elsewhere item no lane covers reads "Updated 15m ago" in the stale style at T+15m.
  - A title-index-only item reads "Partly synced".
  - Each label's accessible text has the full timestamp and the lane.

  (unit, injected clock)
- **AC-23 Sync this item.**
  - With the governor's 3 sync slots busy, `⌘⇧R` sends the item GET immediately (4th slot), then the comments GET.
  - A second press within 10 s sends nothing and shows "Just synced".
  - While sync is paused it still runs.
  - A 404 shows the deleted/lost-access state and keeps the last data.

  (mock + unit, injected clock)
- **AC-24 Partly synced auto-fetch.**
  - Arrowing past five partly synced items at 100 ms per step sends nothing.
  - Resting 400 ms on one sends one FR-30 sync; reselecting it within 10 min sends nothing.

  (mock + unit, injected clock)
- **AC-25 Keyboard and palette.**
  - Every §3.3 key triggers its action from the list selection and from the detail.
  - None fires while the title editor, the description editor or the composer has focus (typing "s" inserts "s").
  - Every FR-35 command exists in `⌘P` and runs the same action.
  - The cheat sheet lists every key.

  (unit: keymap + dispatch; scripted smoke run `ADOKEN_DEMO=1 ADOKEN_NAV=workitems`)
- **AC-26 Accessibility.** The FR-36 labels exist on the fixture (role and label assertions). With reduce motion on, a confirmed write runs no animation frames. (unit + story)
- **AC-27 Gallery.** Every FR-37 story exists with one snapshot per built-in theme. (snapshot test)
- **AC-28 Real-org probe.** On the personal org (`docs/roadmap.md`: a safe target for destructive tests), a scripted probe:
  - changes a test item's state, assignee, area and iteration;
  - adds and removes a Related link;
  - edits a Markdown and an HTML description;
  - posts a Markdown comment with a mention;
  - provokes one stale `test /rev`;
  - runs one directory search.

  It records in §5: the status and message of the failed `test /rev`, whether `multilineFieldsFormat` comes back on the item GET and on `workitemsbatch`, whether the Markdown comment renders as Markdown in the web UI, whether the directory id equals the work item identity id, and the TSTU per request type. No PAT or payload text is recorded. (recorded real-org ledger)

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Selection → detail painted (synced item) | Next frame (< 16 ms); rich text parsed off the UI thread, cached per `(id, rev)` | Frame recorder on a 20 KB description + 200 comments fixture |
| Rich text parse of a 20 KB description | ≤ 5 ms on a background executor | Bench in `crates/ui` |
| Key → editor or menu open | Next frame | Frame recorder |
| Optimistic value shown after `↩` | Next frame; the store write and network run off the UI thread | Frame recorder + unit |
| People picker filter over 5,000 stored identities | ≤ 2 ms per keystroke | Bench |
| Area/iteration filter over 10,000 nodes | ≤ 5 ms per keystroke | Bench (AC-12) |
| Edit → confirmed (network) | Network-bound; no frame > 16 ms while waiting; "Waiting for Azure DevOps…" after 3 s | Mock server at 80 ms RTT + frame recorder |
| Cold start | Unchanged (< 500 ms); no new work on the startup path | Launch timer |

Global rules (AGENTS.md) always apply: frame < 16 ms, Cmd+P < 50 ms on 100k items, cold start < 500 ms. Regressions are bugs.

## 8. Success signal (telemetry-free)

- **Dogfood:** for a working week, the owner changes state, assignee and iteration, and comments, only in adoken. "Open in Azure DevOps" on work items drops to cases FR-18 or FR-9 send there. A local, never-sent counter of work item writes vs Open in Azure DevOps presses, visible in the 0016 network log (Network › Totals), shows the ratio.
- **No conflict noise:** across the week, FR-8 conflict notices appear only when someone really edited the same field (the auto-merge path absorbs the rest), and no write was lost (every failure kept a Retry or a draft).
- **Public replies:** build-in-public posts about "move a work item to Resolved with one key" get "finally, no work item form" replies rather than "does it sync back?" questions.

## 9. Risks

- **HTML round-trip fidelity.** Converting HTML to Markdown and back can corrupt formatting. Mitigations: FR-18's equivalence gate, a fixture corpus from real descriptions (sanitized), Markdown fields edited losslessly, Open in Azure DevOps as the always-available fallback.
- **Undocumented failure shapes.** The status of a failed `test /rev` and the `TF401320` rule message aren't on Learn. Mitigation: FR-8 decides by re-reading the item, not by status code; FR-9 falls back to Azure DevOps' raw message; AC-28 records the real shapes.
- **Preview comments API.** `7.1-preview.4` can change. Mitigation: sync already depends on it (0002), and one constant pins the version.
- **Markdown comments in HTML-editor orgs.** Learn's wording ("if your organization uses the new Markdown editor") suggests per-org behavior. Mitigation: AC-28 checks the web rendering. If Markdown comments don't render, FR-25 switches to `format=html` with `data-vss-mention` anchors (the encoder already exists for parsing, `crates/sync/src/wire.rs`).
- **Directory search scope.** In Entra-backed orgs the directory includes people who aren't project members; assigning them fails. Mitigation: local people first, directory results labeled, Azure DevOps' refusal shown as is.
- **Move surprises.** State reset and children left behind can upset a board. Mitigation: a sheet that states both before `↩`, slice C deferred to v0.2 (Q-1).
- **Store growth.** Raw description and comment sources roughly double those columns. Accepted: they are bounded by the full set (mine + pinned), not the 100k title index.
- **Stale rev after comments.** Comments create revisions. Mitigation: FR-25's one GET after a comment; FR-8 auto-merge covers the rest.

## 10. Open questions — resolved 2026-10-01

All recommendations accepted by the owner.

- **Q-1 Milestone and slicing.** **Resolved:** slice A (redesign, freshness, Sync this item, state, assign, title, comments) in **v0.1**, built right after pins and **not** on the cut line, since it is the owner's daily dogfood loop. Slice B (area, iteration, description editing, links) in **v0.1** as the **first** item on the cut line, ahead of the Ken layer. Slice C (move to project, FR-24, AC-21) in **v0.2**: RICE 5, needs a permission most Contributors lack. Encoded in the Milestone row and `docs/roadmap.md`.
- **Q-2 Editing HTML descriptions.** **Resolved:** edit them as Markdown and save back as HTML when FR-18's round trip is exact; otherwise block with Open in Azure DevOps. Never convert a field to Markdown (FR-17, FR-18, AC-14, AC-15). A WYSIWYG HTML editor stays rejected.
- **Q-3 Images in descriptions and comments.** **Resolved:** an "Image" chip that opens the browser; no fetch (FR-3, AC-1). Fetching attachments is a later spec if dogfood asks.
- **Q-4 Comment format.** **Resolved:** post Markdown (`format=markdown`) with `@<id>` mentions, switching to HTML only if AC-28 shows Markdown comments don't render in the org (FR-25, FR-26, AC-17, AC-28; §9).
- **Q-5 Who can be assigned.** **Resolved:** Assign to me, Unassigned, recent, people already synced, plus a debounced directory search (FR-13, AC-4). Team member lists stay rejected.
- **Q-6 Conflict policy.** **Resolved:** silent resend only when the other change didn't touch my field; otherwise show both and let me choose; never last-writer-wins (FR-8, FR-19, AC-5, AC-6).
- **Q-7 Which links.** **Resolved:** add and remove Parent, Child, Related, Predecessor, Successor, Duplicate and Duplicate of; every other link read-only; PR links stay managed from the PR (FR-21, FR-22, FR-23, AC-18, AC-19, AC-20). Adding hyperlinks: later if asked (§11).
- **Q-8 Read-only token.** **Resolved:** values look like plain text plus one hint with Renew token, following 0004 FR-17 (FR-28, AC-8). Superseded on 2026-10-01 by 0001 FR-14: the hint is passive, with no button (FR-28, AC-8).
- **Q-9 Auto-fetch partly synced items.** **Resolved:** yes, after the selection rests 400 ms, at most once per item per 10 min (FR-31, AC-24).
- **Q-10 Repro Steps and Acceptance Criteria.** **Resolved:** render them read-only under Description when present, with the same storage and renderer; editing them later (FR-3, FR-34, §11).
- **Q-11 Removing a link: confirm or Undo?** **Resolved:** confirm (`↩`), matching 0004's Unlink (FR-22, AC-19).
- **Q-12 Shortcuts.** **Resolved:** §3.3's single letters (`t s a i c d l`, `⇧A ⇧I`, aliases `⇧S ⇧C`) plus `⌘⇧R`, active only in Work items and never in text fields (§3.3, FR-35, AC-25). `⇧P` (move to project) arrives with slice C in v0.2. `⇧A` keeps meaning Abandon in the PR review, where it asks first.
- **Q-13 Silence Azure DevOps emails for my edits?** **Resolved:** never use `suppressNotifications` (FR-5, §11).

## 11. Out of scope

- **Creating work items** (`docs/ideas.md` #12): its own spec; shares this spec's pickers.
- **Bulk edit** of several items at once: later, if dogfood asks.
- **Other fields:** Priority, Tags, Effort/Story Points, Remaining Work, Reason, custom fields; editing Repro Steps and Acceptance Criteria (Q-10).
- **Changing the work item type** ("Team Services only" in the Update reference): later, if ever.
- **Attachments:** viewing, uploading or removing; images stay chips (Q-3).
- **Comments:** editing or deleting comments, reactions.
- **History:** revision history beyond 0008 FR-3 (the `updates` API): later spec.
- **Following** a work item: ADO follow means ADO emails, the opposite of less noise (0014 §11).
- **Other write channels:** an offline write queue (0002 §11); `bypassRules` (admin-only and dangerous); `suppressNotifications` (Q-13).
- **Other links:** adding hyperlink, commit, branch or build links; linking PRs from the work item side (0004 FR-17 owns it).
- **Quick actions on Today nudges and inbox rows:** `docs/ideas.md` #5 remainder, after this ships.
- **Boards / backlog drag and drop** (0008 §11).

## 12. References

- Work Items – Update (JSON Patch, `test /rev`, `/relations/-`, remove `/relations/{index}`, `validateOnly`, `bypassRules`, `suppressNotifications`, `$expand`, identity values by distinct display name, reset identity, move work items; `vso.work_write`): https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-items/update?view=azure-devops-rest-7.1
- Work Items – Get Work Item (`$expand`, `fields`, `asOf`): https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-items/get-work-item?view=azure-devops-rest-7.1
- Work Items – Get Work Items Batch: https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-items/get-work-items-batch?view=azure-devops-rest-7.1
- Comments – Add Work Item Comment (`format` = markdown | html, 7.1-preview.4, `vso.work_write`): https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/comments/add-work-item-comment?view=azure-devops-rest-7.1
- Comments – Add Comment (same version, no `format`): https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/comments/add-comment?view=azure-devops-rest-7.1
- Comments – Get Comments (paging, `order`, `$expand=renderedText`, 7.1-preview.4): https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/comments/get-comments?view=azure-devops-rest-7.1
- Work Item Types – Get (`states`, `transitions`): https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-item-types/get?view=azure-devops-rest-7.1
- Work Item Types – List: https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-item-types/list?view=azure-devops-rest-7.1
- Work Item Type States – List: https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-item-type-states/list?view=azure-devops-rest-7.1
- Work Item Types Field – List (`$expand=allowedValues`, `alwaysRequired`): https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-item-types-field/list?view=azure-devops-rest-7.1
- Classification Nodes – Get Root Nodes (`$depth`, iteration `attributes`): https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/classification-nodes/get-root-nodes?view=azure-devops-rest-7.1
- Work Item Relation Types – List: https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-item-relation-types/list?view=azure-devops-rest-7.1
- Link types reference (reference names, one parent per item, tree topology prevents cycles): https://learn.microsoft.com/en-us/azure/devops/boards/queries/link-type-reference?view=azure-devops
- Identities – Read Identities (`searchFilter=General`, `identityIds`; `vso.identity`): https://learn.microsoft.com/en-us/rest/api/azure/devops/ims/identities/read-identities?view=azure-devops-rest-7.1
- Teams – Get Team Members With Extended Properties (rejected source, Q-5): https://learn.microsoft.com/en-us/rest/api/azure/devops/core/teams/get-team-members-with-extended-properties?view=azure-devops-rest-7.1
- Use @mentions (Markdown `@<userID>`, HTML `data-vss-mention`): https://learn.microsoft.com/en-us/azure/devops/organizations/notifications/at-mentions?view=azure-devops
- Move work items and change the work item type (permissions, hidden/test types, state reset, children stay): https://learn.microsoft.com/en-us/azure/devops/boards/backlogs/move-change-type?view=azure-devops
- Set permissions for work tracking ("Move work items out of this project" not granted to Contributors by default): https://learn.microsoft.com/en-us/azure/devops/organizations/security/set-permissions-access-work-tracking?view=azure-devops
- Apply rules to workflow states (fields required on state change): https://learn.microsoft.com/en-us/azure/devops/organizations/settings/work/apply-rules-to-workflow-states?view=azure-devops
- Custom rules: https://learn.microsoft.com/en-us/azure/devops/organizations/settings/work/custom-rules?view=azure-devops
- Work tracking object limits (1,000 links per item; 10,000 area/iteration paths per project; depth 14): https://learn.microsoft.com/en-us/azure/devops/organizations/settings/work/object-limits?view=azure-devops
- Markdown support in work items (roadmap): https://learn.microsoft.com/en-us/azure/devops/release-notes/roadmap/2024/boards-markdown-in-work-items
- Markdown Support Arrives for Work Items (GA July 2025; `/multilineFieldsFormat/{field}` in REST, irreversible), Azure DevOps blog: https://devblogs.microsoft.com/devops/markdown-support-arrives-for-work-items/
- Rate and usage limits: https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/rate-limits?view=azure-devops
- Not on Learn, community-observed (verify via AC-28): `TF401320` rule error shape, https://github.com/MicrosoftPremier/VstsExtensions/issues/149; `multilineFieldsFormat` in GET responses, https://github.com/microsoft/azure-devops-mcp/issues/1445
