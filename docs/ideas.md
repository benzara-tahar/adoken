# adoken ideas backlog (beyond v0.1)

Owner: product-manager · Last updated 2026-10-01

Brainstormed by **pain point → job to be done → smallest delightful slice**, then ranked with RICE. Nothing here is committed; an idea moves to `docs/roadmap.md` only with an accepted spec. Every idea must serve **snappy** or **less noise** — ideas that serve neither are in "Cut".

## Scoring

`RICE = Reach × Impact × Confidence ÷ Effort`

- **Reach:** % of active adoken users who would use it in a quarter (0–100).
- **Impact:** 3 massive · 2 high · 1 medium · 0.5 low · 0.25 minimal (on the snappy / less-noise promise).
- **Confidence:** 100% measured · 80% strong signal (dogfood, pain-point bank, replies) · 50% plausible · 30% speculative.
- **Effort:** builder-weeks, including spec, tests, gallery stories.
- **ADO flag:** new scopes, rate-limit cost, or API gaps. "—" = none beyond v0.1.

## Ranked backlog

| # | Idea | Pain → JTBD → smallest slice | Serves | R | I | C | E | **RICE** | ADO flag |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | **Global summon hotkey** → specced and scheduled as `docs/specs/0021-global-hotkey.md` (Accepted 2026-10-01; v0.1, on the cut line) | "Switching to adoken is itself a context switch" → bring adoken forward from any app → one configurable global shortcut (Hyper+A `⌃⌥⇧⌘A`) brings adoken to the front; an optional second shortcut opens the palette | Snappy | 70 | 1 | 80% | 0.5 | **112** | — (no ADO calls; no macOS permission prompt) |
| 2 | **Iteration compare: "changes since my last review"** | Re-reviews re-read the whole PR → see only what changed since I last voted/viewed → default diff = my last reviewed iteration vs latest, with a toggle | Snappy, Less noise | 80 | 3 | 80% | 2 | **96** | Uses `$compareTo` on iteration changes (`vso.code`); no new scope |
| 3 | **Multi-line comments + suggestion blocks** | Line comments can't express "replace these 5 lines" → propose exact code → range select + `suggestion` fenced block rendering and authoring | Snappy | 70 | 2 | 80% | 2 | **56** | `vso.code_write` (already in Review set) |
| 4 | **Azure CLI session auth (no PAT)** | PAT policies block or annoy; Microsoft recommends Entra tokens → connect with zero PATs if `az` is signed in → `AzCliToken` provider refreshing a 1 h token via `az account get-access-token` in the background | Less noise (no token chores) | 50 | 2 | 50% | 1 | **50** | Entra token for resource `499b84ac-…`; tenant Conditional Access may force re-login; `az` must stay installed |
| 5 | **Work item quick actions** → specced and scheduled as `docs/specs/0015-work-item-editing.md` (Accepted 2026-10-01; scope grew to full detail editing: slices A + B in v0.1, slice C move to project in v0.2, see below). Remainder kept here: quick actions on Today nudges and inbox rows (0015 §11) | Updating state/assignee means opening the slow WI form → change state, assign, comment from Cmd+K and nudges → 3 actions, keyboard only | Snappy | 60 | 2 | 80% | 2 | **48** | `vso.work_write`, already in the Review set (0001 §5.2) |
| 6 | **Saved searches as inbox tabs** | Same filters retyped daily → pin a query as a tab → "Save as tab" from Cmd+P with live count | Less noise | 50 | 1 | 80% | 1 | **40** | — |
| 7 | **Menu bar mini-inbox** | Want a glance without switching apps → P0/P1 count + list in the menu bar → read-only popover with open/snooze | Less noise | 60 | 1 | 80% | 1.5 | **32** | — |
| 8 | **Pipeline approvals from the inbox** | Deployments wait on me and I find out late → approve/reject from the notification → approvals list + one-key approve with comment | Snappy | 30 | 2 | 80% | 1.5 | **32** | Pipelines approvals API; approving needs a high-privilege scope (`vso.build_execute` / pipeline-resources scope — verify); extra polling tier for pending approvals |
| 9 | **Deep links + "Open in adoken" from the browser** | Links in Teams/Slack/email open the slow web UI → `adoken://` URL scheme + tiny browser extension/bookmarklet redirecting `dev.azure.com` PR/WI links | Snappy | 40 | 1 | 80% | 1 | **32** | — |
| 10 | **"What I did" standup digest** | Standup archaeology across PRs/WIs/builds → one keystroke copies yesterday's activity as Markdown → generated from local events only | Less noise | 50 | 1 | 50% | 1 | **25** | — (local events) |
| 11 | **VS Code theme import** (dropped 2026-10-02, spec 0025 FR-11) | "I want my editor theme" → import a VS Code theme file → reuse zeron's `crates/theme` importer with attribution | Snappy (feel) | 30 | 0.5 | 80% | 0.5 | **24** | — |
| 12 | **Create work item from Cmd+P** | Filing a bug is a 10-field form → `>new bug` with title + template defaults → title, type, area, assign to me | Snappy | 40 | 1 | 80% | 1.5 | **21** | `vso.work_write`, already in the Review set (0001 §5.2) |
| 13 | **Pipeline failure triage** | "Build failed" tells me nothing → see the failing task and last 50 log lines inline → failed step + log tail; "Re-run failed jobs" later | Snappy, Less noise | 60 | 2 | 50% | 3 | **20** | Timeline + logs APIs (`vso.build`), logs can be large (range requests); re-run needs `vso.build_execute` |
| 14 | **Entra ID OAuth sign-in** | Tenants that block PATs can't use adoken; Enterprise needs SSO → native browser sign-in → `EntraOAuth` provider behind `AuthProvider` | Less noise (no tokens) | 30 | 3 | 80% | 4 | **18** | App registration + admin consent; Azure DevOps OAuth is deprecated, Entra is the path |
| 15 | **ADO email noise tuner** | adoken filters well, but ADO still emails → guided "turn off the ADO emails adoken now covers" → list user's ADO notification subscriptions with one-click opt-outs | Less noise | 50 | 1 | 50% | 1.5 | **17** | `vso.notification_write` (new scope); per-user subscription APIs |
| 16 | **Failed tests on the PR** | Build failed because of tests; finding which means 4 clicks → failed test names + messages on the PR checks panel | Snappy | 40 | 1 | 50% | 2 | **10** | Test results API needs `vso.test` (new scope) |
| 17 | **Code search in Cmd+P** | Finding a symbol across repos → `code:` scope in the palette → network-backed results in a separate section | Snappy | 40 | 1 | 50% | 2 | **10** | Needs the Code Search extension installed in the org; network-bound (can't meet 50 ms, shown async); `vso.code` |
| 18 | **Wiki search in Cmd+P** | Docs live in ADO wiki nobody can find → `wiki:` scope → titles indexed locally, content on open | Snappy | 30 | 1 | 50% | 1.5 | **10** | `vso.wiki` (new scope) |
| 19 | **Multi-org** → **deferred to Later** (user decision 2026-09-30: too ambitious, too early, nobody asked; was scheduled for v0.1). Draft kept in `docs/specs/0011-organizations-and-local-data.md` | Consultants juggle orgs → switch org with one key → one store per org (ADR 0001 already), org switcher | Snappy | 15 | 2 | 80% | 3 | **8** | Sync budget per org; one PAT per org |
| 20 | **Video / animated backgrounds** | Personalization showcase → looped video background, paused when unfocused | — (delight) | 20 | 0.5 | 80% | 1 | **8** | — |
| 21 | **Per-project accent colors** | Many projects look identical → project accent on rows and PR header | Less noise (scan speed) | 20 | 0.25 | 80% | 0.5 | **8** | — |
| 22 | **PR checkout in a worktree** | Want to run the PR locally → "Checkout" creates a git worktree and opens the editor | Snappy | 30 | 1 | 50% | 2 | **7.5** | Git auth reuses the user's credential manager, not adoken's PAT |
| 23 | **User sound packs** | Share custom sounds → drop-in sound pack folder, same loader as themes | — (delight) | 15 | 0.25 | 80% | 0.5 | **6** | — |
| 24 | **AI PR summary (bring your own key)** | Big PRs lack context → one-paragraph summary on demand → BYO key, local prompt, never automatic | Snappy | 40 | 0.5 | 50% | 2 | **5** | Sends diff content to a third-party model: explicit opt-in only; conflicts with "no data leaves the machine" unless local model |
| 25 | **Offline write queue** | Commenting on a plane → queued writes sent on reconnect with conflict checks | Snappy | 20 | 1 | 50% | 2 | **5** | Stale-iteration conflicts on comments/votes |
| 26 | **Classic Release pipelines** | Some orgs still deploy with classic releases → show release status alongside builds | Less noise | 20 | 1 | 50% | 2 | **5** | `vsrm.dev.azure.com`, `vso.release` (new scope) |
| 27 | **Team view: review load** | Leads can't see who's overloaded → per-teammate open review count from already-synced PRs | Less noise | 15 | 2 | 50% | 5 | **3** | Needs team membership (`vso.project` teams); no server allowed, so per-user computation only |
| 28 | **Semantic search** | Can't remember any exact word → natural-language query over titles/descriptions → local embeddings (sqlite-vec) | Snappy | 40 | 1 | 30% | 5 | **2.4** | Bodies need `$expand` fetches (TSTU cost) |
| 29 | **Reviewer suggestions** | Picking reviewers is guesswork → suggest from file history | Less noise | 20 | 1 | 30% | 3 | **2** | Commit history per path is expensive to fetch |
| 30 | **Linux official builds** | Linux devs in ADO shops → supported packages | Snappy | 5 | 2 | 50% | 4 | **1.25** | — |
| 31 | **Community theme gallery** on adoken.dev | Share themes → browse + one-click install | — (delight) | 15 | 0.5 | 50% | 3 | **1.25** | Needs hosting (static site is fine) |

### Accepted from the owner's dogfood notes (2026-10-01)

These specs were accepted with all recommendations on 2026-10-01 (overrides noted in each spec's §10) and now live on `docs/roadmap.md` (v0.1 features and cut line); they are no longer backlog items.

- `docs/specs/0015-work-item-editing.md` (#5): redesigned work item detail, editing, comments with @mentions, freshness and Sync this item. Slices A + B in v0.1 (A not on the cut line, B on it); slice C (move to project, RICE 5) in v0.2.
- `docs/specs/0016-statusbar-activity.md`: Activity drawer with sync activity (RICE 32) and network log (RICE 19), v0.1, on the cut line after the Ken layer and calm celebrations (network log cut before sync activity).
- `docs/specs/0017-today-redesign.md`: Today as a day summary with a mood illustration, readable PR rows and a side column (RICE 80), v0.1; only its crossfade and narrow-layout polish are on the cut line.
- `docs/specs/0018-navigation-history.md`: back / forward across views (RICE 96), v0.1, on the cut line after sync activity, before pins.
- `docs/specs/0019-pr-review-polish.md`: group reviewers (RICE 270), copy title and branches (200), file filter (144), status actions (90) in v0.1; the Threads view (47) on the cut line.
- `docs/specs/0020-guided-tours.md`: first-use tours (RICE 24), v0.1, cut first.
- `docs/specs/0021-global-hotkey.md` (#1): v0.1, on the cut line before 0015 slice B.
- `docs/specs/0022-celebrations.md`: calm celebrations (RICE 56) on the cut line after the Ken layer; arcade style (24) rides with the Ken layer.
- `docs/specs/0023-resource-meter.md`: memory and CPU in the statusbar (RICE 27), off by default, cut second.
- Amendments: `docs/specs/0001-onboarding.md` (one token, Replace token, silent `az` replacement, Welcome screen), `docs/specs/0008-work-items-pipelines.md` FR-8 (area and iteration dropdown, RICE 112, rides with 0015 slice B), `docs/specs/0009-settings.md` (Account page, new settings tables).

### Deferred from the 2026-10-01 specs

Cut from the specs above (their §10 and §11) and parked here. Scored where the spec gave a RICE; the rest wait for a real signal (dogfood or beta replies) before they are scored.

| Idea | Pain → JTBD → smallest slice | Serves | R | I | C | E | **RICE** | ADO flag | Source |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **"Open adoken at login"** (scheduled in `docs/roadmap.md` v0.2) | The global shortcut does nothing when adoken isn't running → start with the Mac → Settings › General switch, `SMAppService.mainApp` (macOS 13+), default off, opens without a window until summoned | Snappy | 40 | 0.5 | 80% | 0.6 | **27** | — | 0021 Q-3 |
| **History menu on the back button** | Going back five steps is five presses → right-click or long-press on ‹ lists the last 10 entries | Snappy | 20 | 1 | 80% | 1 | **16** | — | 0018 Q-4 |
| **Celebrate auto-complete merges** | A PR I set to auto-complete merges later with no moment → celebrate it when the merge is confirmed | — (delight) | 20 | 0.5 | 60% | 1 | **6** | — | 0022 Q-4 |
| **Per-type celebration state editor** (scheduled in `docs/roadmap.md` v0.2) | Editing `step_states` / `milestone_states` in `settings.toml` → a per-type editor in Settings › General | Less noise | 10 | 0.5 | 50% | 0.5 | **5** | — (type metadata already cached, 0015 FR-33) | 0022 Q-7 |
| **Two-finger trackpad swipe for back / forward** | "Swipe between pages" like Safari → needs `trackSwipeEvent` in the GPUI fork, competing with horizontal diff scrolling | Snappy | | | | | unscored | — | 0018 Q-6 |
| **Fuzzy path matching in the PR file filter** | Substring terms miss abbreviations → fuzzy subsequence like `Cmd+P`, only if asked (fuzzy matches nearly everything on 300 similar paths) | Snappy | | | | | unscored | — | 0019 Q-1 |
| **Threads view "recent first"** | Most recently active threads first, beside file order; "changes since my last review" (v0.2) covers most of it | Less noise | | | | | unscored | — | 0019 Q-5 |
| **Search comment text and per-thread unread state in the PR review** | Finding the thread that mentioned X → filter threads by text; mark threads read | Less noise | | | | | unscored | — | 0019 §11 |
| **`[today] illustration` switch** | Hide Today's mood picture, only if dogfood finds it noisy | Less noise | | | | | unscored | — | 0017 Q-9 |
| **Mood history and per-user mood weighting on Today** | "How did my week go?" → mood per day; tune which counts drive the mood | — (delight) | | | | | unscored | — | 0017 §11 |
| **Tour "what's new"** | A toured view changed a lot → show the new steps once to people who saw the tour | Less noise | | | | | unscored | — | 0020 Q-7 |
| **Tours for the PR list, Work items list, Pipelines, Settings, Activity drawer** | Only if beta questions cluster on one of them | Snappy | | | | | unscored | — | 0020 §11 |
| **Resource meter history** | A sparkline or per-subsystem breakdown, only if the plain number doesn't reveal regressions in dogfood | Snappy | | | | | unscored | — | 0023 §11 |

### Priority notes

- **v0.2 order deviates slightly from pure RICE on purpose:** iteration compare (#2) goes first because it is the flagship review win. The global hotkey (#1) moved into v0.1 with spec 0021 (owner decision 2026-10-01). See `docs/roadmap.md` v0.2.
- **Scope additions:** the Review access level already requests `vso.work_write` (0001 §5.2, user decision 2026-09-30, for 0004 FR-17 links), so #5 (spec 0015) and #12 need no re-authorization; only the "adoken edits no other work item field" wording in 0001 changes.
- **Entra ID (#14)** ranks mid-table on RICE but is a *gate* for locked-down enterprises and the Enterprise tier (`docs/pricing.md`); it is scheduled by business need, not score.

## Cut (and why)

| Idea | Why cut |
| --- | --- |
| Service-hook relay server for real-time push | Requires hosting user ADO data on an adoken server; breaks local-first and the "no adoken server in the data path" promise. Polling within budget is good enough (`docs/specs/0002-sync.md`). |
| Parsing ADO notification emails to fill mention gaps | Needs mailbox access; privacy and security-review nightmare. |
| Mobile companion app | Needs a server or cloud sync to be useful; desktop focus. |
| Kanban board / backlog editing | Rebuilding Azure Boards is a product in itself; slow-to-build, not snappy-first. Work item editing in the detail (#5, spec 0015) covers the daily need. |
| Git client features (commit, push, rebase) | Tower/Fork territory; dilutes positioning (`docs/pricing.md` risk table). |
| Gamified leaderboards across a team | Noise and social pressure; the Ken layer stays personal and opt-in. |
| Telemetry-driven "insights" | adoken ships no telemetry. |
| Navigation history persisted across restarts (0018 Q-1) | Restoring a stale PR review after a restart helps rarely and costs a store table and migration; launch opens the first section. |
| A sort control on Today's Your PRs (0017 Q-7) | The owner fixed the order (most recently active first); a control nobody needs is noise. |
| Celebrating others' actions or anything sync observes, Ken scoring of work item moves (0022 FR-1, Q-5) | Noise, and it would reward board shuffling; celebrations are for my own confirmed work. |
| Dock bounce, OS banners or haptics for celebrations (0022 §11) | Interrupts other apps; the in-window effect is enough. |
| Resource meter warnings or thresholds (0023 Q-6) | An alarm would mostly fire on legitimate cache growth; a leak shows as a number that keeps climbing. |
| Azure DevOps Server (on-prem) | Different auth, versions, and network realities; small overlap with macOS-first early adopters. |

## References

- Pull Request Iteration Changes (`$compareTo`): https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-iteration-changes/get?view=azure-devops-rest-7.1
- Scopes table (`vso.work_write`, `vso.build_execute`, `vso.notification_write`, `vso.test`, `vso.wiki`, `vso.release`): https://learn.microsoft.com/en-us/azure/devops/integrate/get-started/authentication/oauth?view=azure-devops#available-scopes
- Issue Entra tokens with Azure CLI: https://learn.microsoft.com/en-us/azure/devops/cli/entra-tokens?view=azure-devops
- Rate limits: https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/rate-limits?view=azure-devops
