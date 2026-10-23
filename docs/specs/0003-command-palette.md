# 0003 — Cmd+P: universal search across PRs, work items, pipelines, people, commands

| Field | Value |
| --- | --- |
| Status | Draft |
| Milestone | v0.0.4 (Dogfood); display customization completes in v0.1.0 |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/search` (index, matcher, ranking), `crates/app` (palette UI), `crates/store` (source rows) |
| Serves | Snappy (primary), Less noise (one place instead of five web hubs) |
| Depends on | `docs/specs/0002-sync.md` (entities, title index, events) |
| Design | `docs/design/command-palette.md` (prefixes, filter grammar, row templates, full keymap), `docs/design/principles.md` |

## 1. Problem

Finding "that PR about the login flow" or "work item 48213" in ADO means picking the right hub (Repos → Pull requests, Boards → Work items, Pipelines), the right project, the right filter, then waiting for each page. ADO's search box is per-hub, slow, and intolerant of typos. People keep browser tabs open as a poor man's index.

## 2. Users and job to be done

- **Persona:** every adoken user, many times a day.
- **JTBD:** "When I remember a fragment of something (a word, an id, a person, a branch), I want to jump to it instantly, so I never navigate ADO's hubs."
- **Smallest delightful slice:** one keystroke, results on every keypress under 50 ms at 100k items, typo tolerant, `#1234` jumps straight to a work item, and pasting any ADO URL opens it natively.

## 3. UX flow

Keymap, prefixes, filter grammar, row templates and motion: `docs/design/command-palette.md` (authoritative). Behavior summary:

1. `Cmd+P` anywhere in the app opens the palette with focus in the query field. Empty query shows **Pinned** (the user's pins in pin order, max 9; spec 0014), then **Recents** (last 10 opened items), then **My active** (my PRs needing action, review requests, my active work items).
2. Typing updates results on every keystroke. Results are grouped by type only when the query is empty or a scope is not set; otherwise one ranked list.
3. **Scopes** (become a chip on space): `!` / `pr:` PRs, `#` / `wi:` work items, `@` people, `>` commands, `p:` / `pipe:` pipelines, `r:` / `repo:` repos, `proj:` projects, `n:` notifications, `pin:` pins (spec 0014; `Cmd+J` opens the palette in this scope).
4. **Filters** (v0.1 set): `author:`, `assignee:`, `reviewer:`, `state:`, `type:`, `project:`; value `@me` is supported for identity filters. `Tab` autocompletes the highlighted scope/filter/value suggestion.
5. **Direct jumps:** `#48213` or `48213` ranks work item 48213 first (fetched on demand if not in the store); `!912` ranks PR 912 first. Pasting a `dev.azure.com` (or legacy `*.visualstudio.com`) PR / work item / build URL shows a single "Open in adoken" row.
6. **Actions:** `Enter` open in adoken; `Cmd+Enter` open in browser; `Cmd+Shift+C` copy web link; `Cmd+Opt+C` copy `!123 Title` as Markdown link; `Cmd+1…9` open nth row; `Cmd+D` pin / unpin the selected PR, work item or pipeline (spec 0014); `Cmd+K` action menu for the selected row (type-specific: copy branch name, copy id, snooze/done for notifications, vote for PRs if scope allows, pin / unpin; for work items also Change state…, Assign…, Assign to me, Comment… and Sync this work item, each opening the item in Work items with that editor open, spec 0015 FR-35); `Esc` unwinds (actions → value → chip → query → close). *(Amended 2026-10-02 by 0025 FR-1, FR-4: no `Cmd+1…9` row keys or number badges; `Ctrl+N`/`Ctrl+P` and `Cmd+↑`/`Cmd+↓` are gone too, arrows and ↩ select rows)*
7. **Customizable result display:** Settings → Palette, or `Cmd+K → Customize row` on any result, chooses which fields each result type shows and in what order, plus density (compact / comfortable) and avatars on/off. Stored in the TOML settings file (not the DB), per `docs/adr/0001-local-store.md`; example below.

```toml
[palette.row.pull_request]
fields = ["id", "title", "repo", "author", "votes", "age"]

[palette.row.work_item]
fields = ["type_icon", "id", "title", "state", "assignee"]

[palette]
density = "compact"
avatars = true
max_results = 50
```

### States

| State | Behavior |
| --- | --- |
| Empty store (first sync) | Commands and recents available; content rows appear as the index fills; footer "Indexing 42,310 / 100,000". |
| No results | "No matches" + "Search ADO web for '…'" action (`Cmd+Enter`) + suggestion to widen scope/filter. |
| Offline | Local results work; on-demand id fetch shows "Offline — not in local index". |
| Stale | Row shows a subtle stale marker if its source tier hasn't synced for > 3× interval. |

## 4. Functional requirements

- **FR-1 Sources.** PRs (all synced, incl. last-30-day closed), work items (full + title index), pipeline definitions and my recent runs, repos, projects, people (identities seen in sync), notifications (0005), commands (every app action is a command with a name, keywords, and optional shortcut), settings pages.
- **FR-2 Indexed text per type.** PR: id, title, repo, source/target branch, author name. Work item: id, title, type, state, assignee, tags (full set only). Pipeline: definition name, folder, run number. Person: display name, unique name/email. Command: name + keywords.
- **FR-3 Matching.** Case-insensitive, diacritic-insensitive, token-based fuzzy (subsequence within a token, prefix bonus, word-boundary bonus, contiguous-run bonus). **Typo tolerance:** per query token, allow 1 edit for tokens of length 4–7, 2 edits for length ≥ 8, 0 below 4, where an edit is an insertion, deletion, substitution, **or adjacent transposition — a transposition counts as ONE edit** (optimal-string-alignment distance). Numeric tokens match ids exactly or by prefix only (no typos on ids).
  - **Implementation contract on top of frizbee** (the matcher chosen in `docs/adr/0001-local-store.md`, which scores a transposition as 2 edits): (1) frizbee retrieves candidates with `max_typos` = FR-3 budget + 1 for tokens of length ≥ 4, so transposition matches are not filtered out; (2) a **transposition-aware rescoring pass** computes the OSA distance between each query token and its best-aligned haystack token for the top ≤ 500 candidates, drops candidates over the FR-3 budget, and rescores so one transposition ranks like one substitution; (3) both steps run inside the AC-1 budget. This lives behind the `search::query` seam so the matcher can be swapped.
- **FR-4 Ranking.** `score = match_quality × type_weight + recency_boost + ownership_boost + open_boost`, where recency uses last-opened (strong) and last-changed (weak), ownership boosts items where I'm author/assignee/reviewer, and open (active PR, non-closed WI) beats closed at equal match. Exact id match always ranks first. Ties break by last-changed desc.
- **FR-5 Index lifecycle.** On launch, the index is built from the store off the main thread; the palette is usable immediately with partial results (commands + recents first). Sync events update the index incrementally (no rebuilds).
- **FR-6 Direct-id fetch.** `#n` / `!n` not in the store triggers one on-demand fetch (user-priority in the governor), then caches the item.
- **FR-7 URL parsing.** Recognize `https://dev.azure.com/{org}/{project}/_git/{repo}/pullrequest/{id}`, `/_workitems/edit/{id}`, `/_build/results?buildId={id}`, and legacy `https://{org}.visualstudio.com/...` equivalents. URLs for a different org show "Not connected to {org}".
- **FR-8 Display customization.** Per result type: ordered subset of the available fields; density; avatars; max results (10–100). Changes apply live (next frame) and persist. Invalid settings fall back to defaults per field, with a one-time warning.
- **FR-9 Keyboard only.** Every action reachable without a mouse; mouse hover never steals selection while typing.

## 5. ADO API surface and limits

| Purpose | Endpoint | Scope | Notes |
| --- | --- | --- | --- |
| Search itself | none — local index | — | Zero ADO cost per keystroke. |
| On-demand WI by id | `GET /_apis/wit/workitems/{id}` | `vso.work` | User-priority, 1 call. |
| On-demand PR by id | `GET /_apis/git/pullrequests/{id}` (org-level get by id) | `vso.code` | 1 call. |
| On-demand build by id | `GET /{project}/_apis/build/builds/{id}` | `vso.build` | 1 call. |

- **Rate limits:** negligible (on-demand fetches only). Debounce: an id fetch fires only after 250 ms of no typing and only once per id per minute.
- **No push:** results are as fresh as sync (0002); stale markers make it visible.
- **PAT scopes:** read scopes only; `Cmd+K → Vote` hidden when `vso.code_write` is absent.
- **ADO search APIs** (almsearch code/work item search) are deliberately not used in v0.1: they are network-bound (can't meet 50 ms) and code search requires the Code Search extension.

## 6. Acceptance criteria

Harness: synthetic corpus generator (100k items: 60k work items, 20k PRs, 5k pipelines/runs, 10k people, 5k misc) with realistic title vocabulary; recorded query set; benchmark in `crates/search`; UI tests with a fake clock.

- **AC-1** On the 100k corpus on an M1-class Mac, keystroke-to-results (match + transposition rescoring + rank + top-50 layout) p99 < 50 ms and p50 < 10 ms over the recorded 500-query set, which includes ≥ 50 queries with a transposition typo.
- **AC-2** `Cmd+P` shows the palette on the next frame (< 16 ms from key event to first presented frame containing it).
- **AC-3** Query `revew login` returns "Review login flow" (WI) in the top 3; `rev` (3 chars) does not typo-match "rex"; numeric `4821` never typo-matches id `4812`.
- **AC-3b** Transposition = one edit: in a fixture whose titles include "Fix login redirect", "Fix logic redirect", "Fix logon redirect" and "Lagoon theme", query `lgoin` ranks "Fix login redirect" **top-1**; query `lgoin redirect` ranks it top-1 as well; `lgion` (two edits away from "login" at length 5) returns none of these items via typo match.
- **AC-4** Query `48213` ranks work item 48213 first when it exists; if it's not in the store, exactly one on-demand fetch is issued after 250 ms idle and the row appears on success.
- **AC-5** Pasting `https://dev.azure.com/fabrikam/Web/_git/site/pullrequest/912` shows one row "Open PR !912" and `Enter` opens the PR review view (0004) without a browser.
- **AC-6** With scope `!` and filter `author:@me`, only PRs I created are returned.
- **AC-7** Given two items with equal match quality, the one I opened most recently ranks higher; given equal recency, an active PR ranks above a completed one.
- **AC-8** Changing PR row fields to `["id","title"]` in Settings updates visible rows on the next frame and persists across restart; a malformed field name falls back to defaults with one warning.
- **AC-9** A `WorkItemChanged` event updating a title makes the new title searchable within 100 ms of the store commit, without an index rebuild (asserted via index generation counter).
- **AC-10** Index build from store for 100k items completes < 300 ms off-thread; during the build, the palette opens and returns commands within budget.
- **AC-11** `Cmd+Enter` on any content row opens the correct web URL in the default browser (URL assertion via a stub opener).
- **AC-12** Every command listed in the app's command registry is findable by its name with an exact query (registry-driven test).

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Open palette | next frame (< 16 ms) | Frame recorder |
| Keystroke → results, 100k items | p99 < 50 ms, p50 < 10 ms | `crates/search` bench + UI trace |
| Index build from store, 100k | < 300 ms off main thread | Bench |
| Incremental index update per event | < 1 ms | Bench |
| Index memory, 100k items | < 60 MB | Heap profile |
| Scrolling results | 60 fps | Frame recorder |

## 8. Success signal (telemetry-free)

- Dogfood: builder stops opening ADO web hubs to find things (self-reported in the weekly demo, shown on screen).
- Local-only stat in Settings → About: "Palette opens this week" and "median results time" (computed and shown on-device only). Users screenshot it in replies.
- Side-by-side video: "find PR by fragment" adoken vs ADO web — the v0.0.4 build-in-public beat.

## 9. Risks

- Typo tolerance + 50 ms p99 at 100k: edit-distance matching is expensive. frizbee (ADR 0001) is SIMD and typo-tolerant for substitutions and deletions but counts an adjacent transposition (`lgoin`) as 2 edits. **Decision (user, 2026-09-25): a transposition counts as one edit**, so we add the retrieval widening + transposition-aware rescoring pass in FR-3. Risk: the widened `max_typos` enlarges the candidate set; mitigation: cap rescoring at 500 candidates and prove it with AC-1's transposition queries.
- frizbee does not fold diacritics; the index must normalize haystacks at build time (ADR) to satisfy FR-3.
- Ranking quality is subjective; ship a recorded "golden queries" test set from dogfood so ranking changes are reviewed as diffs.

## 10. Open questions — resolved 2026-09-25

All recommendations accepted by the user.

- **Q-1** Global system-wide summon hotkey in v0.1? **Resolved:** no; ships in v0.2 (item 2 in `docs/roadmap.md`, RICE 112 in `docs/ideas.md`). **Superseded 2026-10-01:** spec 0021 (accepted) brings it into v0.1, on the cut line.
- **Q-2** Closed PRs older than 30 days searchable by title? **Resolved:** yes, titles only, if the week-3 sync spike shows the backfill fits the cold-tier budget (`docs/specs/0002-sync.md` FR-2 "PR title index"); otherwise they drop out after 30 days.
- **Transposition handling** (raised during review): **Resolved:** an adjacent transposition counts as one edit; custom rescoring on top of frizbee (FR-3, AC-3b).

## 11. Out of scope

- Code search and wiki search (`docs/ideas.md`).
- Semantic / natural-language search (Later milestone).
- Creating work items from the palette (`docs/ideas.md` #12, needs its own spec). Editing work items is spec 0015: palette commands and `Cmd+K` actions open the editor in the Work items detail, never inside the palette.
- Saved searches as inbox tabs (`docs/ideas.md`).

## 12. References

- Work Items — Get Work Item: https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-items/get-work-item?view=azure-devops-rest-7.1
- Pull Requests — Get Pull Request By Id: https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/get-pull-request-by-id?view=azure-devops-rest-7.1
- Builds — Get: https://learn.microsoft.com/en-us/rest/api/azure/devops/build/builds/get?view=azure-devops-rest-7.1
- Rate limits: https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/rate-limits?view=azure-devops
