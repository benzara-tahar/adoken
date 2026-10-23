# Searching with Cmd+P

`⌘P` (Ctrl+P on Windows/Linux) opens one search box over everything adoken has synced: pull requests, work items, pipeline runs, people, repositories, projects, your pins, and every app command. Searches run on your machine against the local store; nothing is sent to Azure DevOps while you type.

This manual describes what the app does today (code in `crates/app/src/palette/`). Where the design doc (`docs/design/command-palette.md`) describes something that isn't built yet, §9 says so.

## 1. Quick start

| You want | Type |
|---|---|
| Your review queue, open PRs, and active work items | nothing (empty box) |
| A PR by number | `!4112` |
| A work item by number | `#20499` |
| Either one, you're not sure which | `4112` |
| Something you pasted from the browser | the `https://dev.azure.com/…` link |
| Open PRs by a teammate | `! author:grace is:open` |
| Work items assigned to a teammate that are still open | `# assigned:sofia is:open` |
| A command | `>sync` (or `⌘⇧P`) |

Then press `↩` to open the selected row, `⌘↩` to open it in the browser, or `⌘K` for the other actions.

## 2. How a query is built

A query has up to four parts, in this order:

```
[scope] [free text and filters, in any order]
  !      retry author:@me -is:draft
```

- **Scope** (optional, must come first): which kind of result to show. See §3.
- **Free text**: fuzzy, typo-tolerant match on titles and a few other fields (§2.2).
- **Filters**: `key:value` tokens that narrow results by a field (§4).
- **Jump**: a number or a pasted link, which goes straight to one item (§6).

### 2.1 Filter rules

- **Several values of one key are OR:** `type:bug,task` is bugs or tasks. No spaces around the commas.
- **Different keys are AND:** `type:bug assigned:sofia` is bugs that are assigned to Sofia.
- **The same key twice is also AND:** `state:active state:new` matches nothing. Use `state:active,new`.
- **Negation:** put `-` before the key. `-is:draft` excludes drafts, `-state:closed,removed` excludes both states. A `-` before a word that is not a filter (`-draft`) is searched as text.
- **Quotes** keep spaces in a value: `iteration:"sprint 44"`, `assigned:"Camila Reyes"`.
- **Keys are case-insensitive** (`STATE:ACTIVE` works), and so are values.
- **A filter hides kinds it can't apply to.** `type:bug` only applies to work items, so PRs and pipelines disappear from the results even without a scope (the table in §4 lists which kinds each key applies to).
- **Unknown keys** are searched as text, and the footer offers a fix when the key is within two typos of a real one: `auther:grace` → "did you mean `author:`?" Press `Tab` to apply the fix.
- A key with no value yet (`author:`) does nothing until you type a value.

### 2.2 Free text

Free text is matched fuzzily (word prefixes, subsequences, and small typos: one typo for 4–7 letter words, two for 8+, none under 4 letters). It searches these fields:

| Kind | Fields searched |
|---|---|
| Pull request | title; repo, source branch, target branch, author name |
| Work item | title; type, state, assignee name, tags |
| Pipeline run | pipeline name; run number, branch |
| Person | display name; email |
| Repository | name; project name |
| Project | name |
| Command | title |

Descriptions and comments are not searched.

## 3. Scopes

Type a scope at the very start of the query. A **sigil** (one character) takes effect immediately, with or without a space. A **keyword** takes effect once it is followed by a space; `Tab` adds the space for you.

| Scope | Sigil | Keywords | Shows |
|---|---|---|---|
| Pull requests | `!` | `pr:` | PRs |
| Work items | `#` | `wi:` | Work items |
| People | `@` | `people:` | People |
| Commands | `>` | `cmd:` | App commands (`⌘⇧P` opens the palette in this scope) |
| Pipeline runs | — | `p:` `pipe:` | Pipeline runs |
| Repositories | — | `r:` `repo:` | Repos |
| Projects | — | `proj:` | Projects |
| Pins | — | `pin:` | Your pins, in pin order (`⌘J` opens the palette here) |

- Keywords are case-insensitive (`PR: `, `Pin:`).
- `repo:` is both a scope keyword and a filter key. **With a space** (`repo: pay`) it's the repository scope. **Without one** (`repo:payments`) it's the PR filter.
- A sigil on its own lists everything of that kind, newest first: `!` lists all PRs, `#` all work items.
- A keyword on its own (`p: `) does **not** do this yet; see §9. Add a filter or a word after it: `p: status:failed`, `p: deploy`.
- With no scope, every kind is searched in one list (filters may still hide some kinds, §2.1).

## 4. Filter reference

People values (`author`, `reviewer`, `assigned`, `by`):
- `@me` is you, the signed-in account.
- Anything else is a **case-insensitive substring** of a person's display name or email, with a leading `@` ignored. So `grace`, `@grace`, `Kim`, and `grace.kim@` all find Grace Kim. A short value can match more than one person: `ma` matches every name or email that contains "ma".
- `me` without the `@` is a substring search for the letters "me" (e.g. Ravi **Me**non), **not** you. Always write `@me`.

Date values (`updated`, `created`):
- `<7d`: within the last 7 days. `>2w`: older than 2 weeks. Units: `h` hours, `d` days, `w` weeks, `m` months (30 days), `y` years (365 days).
- `2026-09-01..2026-09-15`: between two dates, inclusive, in UTC.

| Key | Aliases | Applies to | Values | Matching | Examples |
|---|---|---|---|---|---|
| `is:` | | PRs | `open` (= `active`), `draft`, `merged` (= `completed`), `abandoned`, `mine` (you're the author), `review-requested` (you're a reviewer, whatever your vote) | fixed words | `is:open`, `-is:draft` |
| `is:` | | Work items | `open`, `closed`, `mine` (assigned to you) | `open` = the state's category is Proposed or In Progress; `closed` = anything else (Resolved, Completed, Removed) | `is:open` |
| `author:` | | PRs | person | creator | `author:@me`, `author:grace` |
| `reviewer:` | | PRs | person | anyone on the reviewer list, any vote | `reviewer:@me` |
| `vote:` | | PRs | `approved`, `suggestions`, `waiting`, `rejected`, `none` | your vote; with a `reviewer:` filter, the vote of those reviewers | `vote:none`, `reviewer:grace vote:approved` |
| `repo:` | | PRs | repo name | substring | `repo:payments` |
| `branch:` | | PRs (source branch), pipeline runs | branch name, without `refs/heads/` | substring | `branch:main`, `branch:feature/sso` |
| `target:` | | PRs | target branch name | substring | `target:main` |
| `type:` | | Work items | `bug`, `task`, `story` (also `userstory`, `user-story`, `"user story"`), `feature`, `epic`, `issue` | exact | `type:bug,task` |
| `state:` | | Work items | the state's name as your process names it (`New`, `Active`, `Doing`, `Resolved`, `Closed`, …) | whole name, any case; not a substring | `state:active`, `state:"in review"` |
| `assigned:` | `assignee:` | Work items | person | assignee | `assigned:@me`, `assignee:sofia` |
| `priority:` | | Work items | `1`–`4`, or `urgent` (1), `high` (2), `normal` (3), `low` (4) | exact | `priority:1,2`, `priority:high` |
| `area:` | | Work items | area path | substring of the full path; `\` separates levels | `area:payments`, `area:payments\ledger` |
| `iteration:` | | Work items | iteration path | substring of the full path | `iteration:"sprint 44"` |
| `tag:` | | Work items | tag | whole tag, any case; not a substring | `tag:security` |
| `status:` | | Pipeline runs | `failed`, `succeeded`, `partial`, `canceled` (or `cancelled`), `running`, `queued` | fixed words | `status:failed` |
| `by:` | | Pipeline runs | person | who the run was requested for | `by:@me` |
| `project:` | | PRs, work items, pipeline runs, repos, projects | project name | substring | `project:payments` |
| `updated:` | | PRs, work items | date | PR last update / work item changed date | `updated:<7d` |
| `created:` | | PRs, work items | date | creation date | `created:2026-09-01..2026-09-30` |

## 5. Cookbook

Names below come from the demo dataset (`adoken --demo`), where you are Ada Lindqvist. Replace them with your colleagues' names. Any part of a name or email works (§4).

### Pull requests

| Question | Query |
|---|---|
| Open PRs authored by a developer | `! author:grace is:open` |
| … including their drafts? `is:open` already includes drafts. Without drafts: | `! author:grace is:open -is:draft` |
| My open PRs | `! author:@me is:open` (same as `! is:mine is:open`) |
| My open PRs that aren't drafts | `! is:open -is:draft author:@me` |
| My merged PRs | `! is:merged author:@me` |
| PRs where I'm a reviewer and **haven't voted yet** | `! is:review-requested vote:none is:open` |
| Open PRs where I'm a reviewer that someone else wrote | `! reviewer:@me -author:@me is:open` |
| Open PRs where a developer is a reviewer | `! reviewer:grace is:open` |
| … and that developer hasn't voted yet | `! reviewer:grace vote:none is:open` |
| PRs a developer approved | `! reviewer:grace vote:approved` |
| All drafts | `! is:draft` |
| Open PRs into `main` in one repo | `! repo:payments target:main is:open` |
| PRs updated in the last 2 days | `! updated:<2d` |
| Open PRs nobody has touched for 2 weeks | `! is:open updated:>2w` |
| My PRs about a topic | `! fraud author:@me` (text plus a filter) |

The **empty query** already shows your review queue: active PRs where you're a reviewer and haven't voted, then your own active PRs, then your open work items (at most 30 rows).

### Work items

| Question | Query |
|---|---|
| Open work items assigned to a developer | `# assigned:sofia is:open` |
| Everything assigned to me, any state | `# is:mine` |
| My open work items | `# assigned:@me is:open` |
| My closed work items | `# assigned:@me -is:open` (or `is:closed`) |
| Work items in one state | `# state:active` |
| Several states | `# state:new,active` |
| A developer's items that aren't finished | `# assigned:marcus -state:closed,removed` |
| P1 bugs that a developer has in Active | `# type:bug assigned:sofia state:active priority:1` |
| Bugs and tasks in one sprint and area | `# type:bug,task iteration:"sprint 44" area:payments` |
| Open P1/P2 bugs in a sprint that aren't mine | `# is:open -assigned:@me iteration:"sprint 44" type:bug priority:1,2` |
| My open user stories | `# type:story is:open assigned:@me` |
| High-priority items for someone whose name has a space | `# assigned:"Camila Reyes" priority:high` |
| Open items with a tag | `# tag:security is:open` |
| Work items changed today | `# updated:<1d` |
| Work items created in September | `# created:2026-09-01..2026-09-30` |
| Work items in a sub-area | `# area:payments\ledger is:open` |
| Bugs whose title mentions login | `type:bug login` (no scope needed: `type:` hides other kinds) |

Tip: `state:` needs the full state name, and names differ between processes (Agile uses Active, Scrum uses Committed, a custom process might use Doing). To match "anything still open" whatever the process, use `is:open`.

### Pipelines

| Question | Query |
|---|---|
| Failed runs | `p: status:failed` |
| Failed runs on `main` | `p: status:failed branch:main` |
| Runs for a PR's merge build | `p: branch:4146` (matches `refs/pull/4146/merge`) |
| My runs | `p: by:@me` |
| Failed runs someone started | `p: by:sanjay status:failed` |
| Runs of one pipeline | `p: deploy` (text matches the pipeline name) |

### People, repos, projects

| Question | Query |
|---|---|
| Find a person | `@grace` |
| Find a repo | `r: pay` |
| Everything in a project that's still open | `project:payments is:open` (PRs, then work items) |

Pressing `↩` on a person runs `! author:@<handle> is:open`, their open PRs. On a repo it runs `! repo:<name> is:open`, and on a project `! project:<name> is:open`.

### Pins

| Question | Query |
|---|---|
| All my pins, in pin order | `pin:` (or `⌘J`) |
| Pins whose title, label, or id contains text | `pin: sso` |
| Pinned bugs | `pin: type:bug` |
| Pins in one project | `pin: project:payments` |

The `pin:` scope searches your pins only. Filters apply to the pinned item's current data, and the kind rules from §2.1 still hold (`pin: type:bug` hides pinned PRs). On a pinned pipeline definition only `project:` can match; any other filter hides it. With an empty query, up to 9 pins appear above Recent.

## 6. Jumps and links

| Input | Result |
|---|---|
| `!4112` or `pr:4112` | That PR |
| `#20499`, `wi:20499` or `wi: 20499` | That work item |
| `4112` (digits only, nothing else) | The PR and/or work item with that id |
| `https://dev.azure.com/<org>/<project>/_git/<repo>/pullrequest/<id>` | That PR |
| `…/_workitems/edit/<id>` | That work item |
| `…/_build/results?buildId=<id>` | That pipeline run |
| `https://<org>.visualstudio.com/…` (same paths) | Same as above |

- If the id isn't in your local data, you get a row saying it isn't synced yet. `↩` asks the app to open it.
- A link to another organization shows "This link is for another organization".
- Digits with text or a filter (`4112 retry`, `4112 author:@me`) are a normal search, not a jump.

## 7. Keyboard

| Key | Action |
|---|---|
| `⌘P` | Open / close the palette |
| `⌘⇧P` | Open in the commands scope (`>`) |
| `⌘J` | Open in the pins scope |
| `↑` `↓` | Previous / next row (wraps around) |
| `PageUp` `PageDown` | One page up / down |
| `↩` | Open (PRs, work items, runs), run (commands), or show open PRs (people, repos, projects) |
| `⌘↩` | Open in the browser |
| `⌘K` | Action menu for the selected row (Open, Open in browser, Pin/Unpin, Rename pin…). Right-click also opens it |
| `⌘D` | Pin / unpin the selected PR, work item, or pipeline |
| `Tab` | Turn a scope keyword into a scope (`pr:` → `pr: `), or fix a mistyped filter key |
| `Esc` | Clear the query; if it's already empty, close |

## 8. Results, limits, and what's searchable

- **Order.** Free text ranks by match quality. A scope or filters **without** text list matches in store order: PRs first (most recently updated first), then work items (most recently changed), then pipeline runs (most recently queued), then people, repos, projects.
- **Cap.** At most `max_results` rows (default 500; you can set 10–500 under `[palette]` in `settings.toml`). When the cap is hit, the footer says "Showing first 500 — refine your search". Add a filter to see the rest.
- **Empty query.** Shows Pinned (up to 9), then Recent (the last 8 items you opened; `recent_count` changes this), then "My work" (§5).
- **Only synced data is searchable.** The palette reads the local store, which holds whatever sync has fetched (spec 0002 FR-2):
  - PRs: every active PR in your selected projects, plus closed PRs from the last 30 days that you created or reviewed. Closed PRs older than that are only searchable if the cold-tier "PR title index" is on (title, id, repo, author only); otherwise they drop out after 30 days.
  - Work items: those you're assigned to or created are synced in full. Other work items in your selected projects come from the title index (id, title, type, state, assignee, changed date, project), so `area:`, `iteration:`, `priority:`, and `tag:` can't match them.
  - Pipeline runs: runs you requested in the last 7 days, runs for your active PRs' branches, and the latest runs of pinned definitions. Older runs are pruned after 14 days.
  - People: identities that appeared in synced data.
- New data shows up after the next sync pass (`⌘R` syncs now).

## 9. Known gaps

These are in the design doc but **not built yet**. They're listed so you don't hunt for them:

- A keyword scope with nothing after it (`p: `, `r: `, `pr: `, `proj: `, `people: `, `cmd: `) is searched as the text `p:` instead of listing that kind. Sigils (`!` `#` `@` `>`) and `pin:` work on their own.
- No autocomplete suggestions for filter keys or values, no `?` help scope, no `n:` inbox scope, and no `label:` filter.
- No `@current` for `iteration:` or `area:`. Name the sprint instead (`iteration:"sprint 44"`).
- `repo:` filters PRs only, not pipeline runs.
- No grouped results ("Top hit", per-kind groups, "Show all…") and no preview pane (`⌘.`).
- No `⌥↑`/`⌥↓` group navigation, `⌘⇧C`/`⌘⌥C` copy link/reference, or `⌘⌫` clear in the palette. To open in the browser, use `⌘↩`.
- No "assigned to nobody" filter.
- No `@me` for "created by" on work items. `is:mine` and `assigned:` look at the assignee only.
