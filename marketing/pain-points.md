# ADO pain-point bank

Raw material for the "pain point" pillar (25% of posts). Every entry is a real, public complaint or a documented limit, with a link. Researched 2026-09-25; vote counts and statuses are as of that date and will drift.

## How to use this file

1. Pick an entry whose **adoken angle** has shipped (or ships this week). If the angle isn't built, post the pain as a question, not a promise.
2. Paraphrase. Don't quote Reddit or forum users by name or handle, and don't link a thread in a way that sends a crowd into it. Linking Microsoft docs, Developer Community tickets, and papers is fine.
3. Punch at the problem, never at people. The ADO team reads these channels ([example reply from an ADO engineer](https://www.reddit.com/r/azuredevops/comments/1rfdr4y/the_ui_is_horrendous_will_this_ever/)). Where ADO has improved, say so (see the **Credit** column).
4. After posting, fill in **Resonance** (replies ÷ impressions, and the date). The top 3 become launch messaging.

Legend. **Area:** N = notifications, M = mentions/search, B = Boards/WIQL, R = PR review, P = pipelines, A = auth/PAT, X = platform. **Angle milestone:** the version where adoken answers it (see `calendar.md`).

---

## N. Notifications and inbox

### P01. There's no notification inbox inside Azure DevOps

- **Evidence:** "There is no way to view notifications within azure devops." ([r/azuredevops, Feb 2026](https://www.reddit.com/r/azuredevops/comments/1rfdr4y/the_ui_is_horrendous_will_this_ever/)). Developer Community: [Notifications directly inside Azure DevOps just like in Azure Portal](https://developercommunity.visualstudio.com/t/notifications-directly-inside-azure-devops-just-li/568972), reported May 2019, 82 votes, status *Under Review*. Also [Notifications inbox in the Azure DevOps - not email](https://developercommunity.visualstudio.com/t/notifications-inbox-in-the-azure-devops-not-email/1181960) (2020, closed): "Why does Azure DevOps default to mail on…". Microsoft's docs confirm delivery is by email or service hook ([About notifications](https://learn.microsoft.com/en-us/azure/devops/organizations/notifications/about-notifications?view=azure-devops)).
- **adoken angle:** Notification center: PRs, mentions, work items, pipelines in one list; filter by source/priority; snooze. **v0.1 (W5).**
- **Hook ideas:** "Azure DevOps has no notification inbox. Your email is it." / "A 2019 feature request, still under review."
- **Resonance:**

### P02. One code review = 10–20 emails

- **Evidence:** A dev who moved from GitLab: "the process floods my inbox with 10-20 notification emails as code reviewer starts writing his comments." A reply: batching review-comment notifications "definitely isn't supported" ([r/azuredevops](https://www.reddit.com/r/azuredevops/comments/136ern1/)).
- **adoken angle:** Notifications grouped per PR; one row per PR with a count, not one email per comment. **v0.1 (W5).**
- **Hook ideas:** "One code review. Twenty emails."
- **Resonance:**

### P03. You can't mute PR-comment emails without losing the in-app notice

- **Evidence:** A team wanted to stop or collate per-comment emails but keep notifications in the GUI; disabling "A comment is left on a pull request" at org level "removes both the email and the notification in the GUI" ([r/azuredevops](https://www.reddit.com/r/azuredevops/comments/1l8ptum/)).
- **adoken angle:** Per-source delivery windows: see it in the inbox, never as an interruption. **v0.1 (W5)**. Note: per-source windows are item 2 on the roadmap's cut line; quiet hours never get cut. Check before promising.
- **Hook ideas:** "Mute the email, lose the notification. Pick one."
- **Resonance:**

### P04. No digest for PRs or work items

- **Evidence:** The only built-in digest is for wiki page follows: "a 30-minute digest email that includes all edits" ([Follow wiki pages](https://learn.microsoft.com/en-us/azure/devops/project/wiki/follow-notifications-wiki-pages?view=azure-devops)). Teams build Power Automate flows to get weekly summaries ([Tech Community blog](https://techcommunity.microsoft.com/blog/healthcareandlifesciencesblog/azure-devops-customize-email-notifications-using-power-automate/3739979)).
- **adoken angle:** Delivery windows and quiet hours: held notifications arrive as one batch when the window opens. **v0.1 (W5).**
- **Credit:** The wiki digest is a good idea. Say so.
- **Hook ideas:** "The wiki gets a digest. Your PRs don't."
- **Resonance:**

### P05. Tagged by mistake? Enjoy the thread forever

- **Evidence:** "someone tagged me by mistake on a PR comment… now I'm getting pinged repeatedly via email… I don't really want, nor should I be expected, to use an email filter here." ([r/azuredevops](https://www.reddit.com/r/azuredevops/comments/se34en/)). Suggested fix in the thread was a REST API call to remove yourself as reviewer.
- **adoken angle:** Mute a PR thread locally in one keystroke. **v0.1 (W5).**
- **Hook ideas:** "Wrong @mention, right inbox, forever."
- **Resonance:**

### P06. Default subscriptions notify everyone of everything

- **Evidence:** "By default, Azure DevOps sets up a series of global notification subscriptions… These basically equate to 'Notify ALL THE THINGS!'" ([Brett McKenzie](https://brettmckenzie.net/posts/how-to-avoid-being-flooded-by-notifications-in-azure-devops/)). Subscriptions at different levels "are independent of each other. There's no interaction. If you turn off something at the project level but not at the user level, then you get the notification" ([Notification FAQs](https://learn.microsoft.com/en-us/azure/devops/organizations/notifications/faq-notifications?view=azure-devops)).
- **adoken angle:** One place to decide what interrupts you, independent of org subscriptions. **v0.1 (W5).**
- **Hook ideas:** "Turned it off at project level? User level says hi."
- **Resonance:**

### P07. Microsoft wrote a doc called "Why did I get this email?"

- **Evidence:** [Why did I get this email?](https://learn.microsoft.com/en-us/azure/devops/organizations/notifications/troubleshoot-unexpected-email?view=azure-devops): "you might be part of a group receiving the email… the subscription administrator maybe configured the email delivery…".
- **adoken angle:** Every notification row shows why it's there (you're a reviewer / mentioned / author / on-call pipeline). **v0.1 (W5)**, confirm with spec.
- **Credit:** It's a genuinely helpful doc. Tone: "the fact this doc is needed says it all", not "lol docs".
- **Hook ideas:** "There's an official doc titled 'Why did I get this email?'"
- **Resonance:**

### P08. Draft PRs get posted to Teams channels

- **Evidence:** "it is also posting draft pull requests which we do not want. Can you please make it an option to filter out draft pull requests" ([MicrosoftDocs/msteams-docs#7762](https://github.com/MicrosoftDocs/msteams-docs/issues/7762)); same request on [Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/4437198/azure-devops-teams-notofication-for-pull-requests).
- **adoken angle:** Drafts are quiet by default in adoken's inbox. **v0.1 (W5)**, confirm with spec.
- **Hook ideas:** "Your draft PR, announced to the whole channel."
- **Resonance:**

### P09. Build-failure email fatigue

- **Evidence:** "Developers can get too many build notification emails and will start ignoring them" ([CatLight](https://catlight.io/a/azure-devops/azure-devops-build-notifications)); standard advice is to hand-build filtered custom subscriptions per pipeline ([example](https://www.tutorialpedia.org/blog/email-notification-for-scheduled-build-that-failed-with-azure-devops-vsts/)).
- **adoken angle:** Pipeline notifications only for runs you triggered or own; the rest are browsable, not pushed. **v0.1 (W5).**
- **Hook ideas:** "The 40th 'build failed' email is the one you ignore."
- **Resonance:**

---

## M. Mentions and search

### P10. "Where was I @mentioned?" has no single answer

- **Evidence:** Developer Community: [Notification feed for user activity (like @mentions) in VSTS](https://developercommunity.visualstudio.com/t/notification-feed-for-user-activity-like/365606), reported Oct 2018, **164 votes**, status *New*. Microsoft Q&A: [How to get @mentioned items using WIQL](https://learn.microsoft.com/en-us/answers/questions/954648/how-to-get-@mentioned-items-using-wiql-devops-quer). Community answers suggest pulling comments via the REST API and parsing them yourself ([Fabric community](https://community.fabric.microsoft.com/t5/Power-Query/Pulling-mentioned-from-Azure/m-p/4399944)).
- **Be precise:** work item queries *do* have `@RecentMentions` ("Items with an @mention for you in the last 30 days"), but it's web-portal only: not in REST APIs, Excel, or notification filters ([Query by title, ID, or rich-text fields](https://learn.microsoft.com/en-us/azure/devops/boards/queries/titles-ids-descriptions?view=azure-devops), [managed queries note](https://learn.microsoft.com/en-us/azure/devops/boards/queries/about-managed-queries?view=azure-devops)). There's no equivalent for PR comments, and no one list covering both. Never say "ADO can't find mentions at all".
- **adoken angle:** Mentions from PR threads and work item discussions synced and indexed locally (`docs/specs/0002-sync.md` FR-2); "mentions of me" is an inbox filter and a Cmd+P scope. **v0.0.3 sync (W3), v0.1 inbox.**
- **Hook ideas:** "Where was I @mentioned this week? Two places, no list." / "Work items: 30 days of mentions. PRs: good luck."
- **Resonance:**

### P11. Queries can't search comments

- **Evidence:** "Currently the only Comment related field that can be used in queries is Comment Count." ([Query comments by author, mentions and content](https://developercommunity.visualstudio.com/t/query-comments-by-author-mentions-and-content/627871), June 2019, 65 votes, status *New*).
- **adoken angle:** Local FTS over comment and description text (ADR 0001: SQLite FTS5). **v0.0.3 (W3).**
- **Hook ideas:** "The only comment field you can query: Comment Count."
- **Resonance:**

### P12. Code search misses results

- **Evidence:** [Code search results incomplete when using asterisk (*) wildcard](https://developercommunity.visualstudio.com/t/azure-devops-code-search-results-incomplete-when-u/1298854); [Code Search returns no results in repository…](https://developercommunity.microsoft.com/t/Azure-DevOps-Code-Search-returns-no-resu/11113374) (June 2026).
- **adoken angle:** Not a code-search tool in v0.1; Cmd+P covers PRs, work items, pipelines, people. Use as a **question** post, not a promise.
- **Hook ideas:** "What do you actually search for in ADO?"
- **Resonance:**

### P13. "Click click click" instead of a query language

- **Evidence:** "If I wanted to query a selection of issues I could just write a quick [piece] of JQL. With ADO its click click click click…" ([r/scrum](https://www.reddit.com/r/scrum/comments/ys6jgu/)).
- **adoken angle:** Cmd+P with typo tolerance; type what you remember. **v0.0.4 (W4).**
- **Hook ideas:** "You remember three words of the title. That should be enough."
- **Resonance:**

---

## B. Boards and WIQL

### P14. WIQL returns IDs, not data

- **Evidence:** "WIQL returns ids only. SELECTed columns come back as a columns[] description, not as values. Titles/states/tags need a second workitemsbatch call" ([freaxnx01/agent-workflow#386](https://github.com/freaxnx01/agent-workflow/issues/386)); see the [Query By Wiql REST reference](https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/wiql/query-by-wiql?view=azure-devops-rest-7.1).
- **adoken angle:** Craft post on sync design: IDs first, then batched hydration, then local queries forever after. **v0.0.3 (W3).**
- **Hook ideas:** "WIQL gives you IDs. The data costs a second call."
- **Resonance:**

### P15. 20,000-item query cap, 32K-character WIQL limit

- **Evidence:** "any work item query (WIQL or saved query) that would return >20,000 items fails with error VS402337… you can't raise that cap" ([Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/5555331/unable-to-retrieve-test-plans-after-20k-records-th)); "Remember the 32-K character limit for WIQL queries" ([WIQL syntax](https://learn.microsoft.com/en-us/azure/devops/boards/queries/wiql-syntax?view=azure-devops)).
- **adoken angle:** Craft: incremental sync by `ChangedDate` windows keeps each query small. **v0.0.3 (W3).**
- **Hook ideas:** "Query returns 20,001 work items? It returns an error."
- **Resonance:**

### P16. Area Path filters fail silently

- **Evidence:** "scoping by Area Path fails silently … returns zero rows" with no error ([freaxnx01/agent-workflow#386](https://github.com/freaxnx01/agent-workflow/issues/386)).
- **adoken angle:** Craft: validate project/area names during onboarding; empty ≠ error. Swap-bank craft post.
- **Hook ideas:** "Zero results, or wrong area path? WIQL won't say."
- **Resonance:**

### P17. Back means close, close means back

- **Evidence:** "Click on a feature, then child item, then browser back button. It closes the work item view, instead of taking you back to the feature… So close means back and back means close" ([r/azuredevops, Feb 2026](https://www.reddit.com/r/azuredevops/comments/1rfdr4y/the_ui_is_horrendous_will_this_ever/)). Related: "going back one step in your history changes nothing except for the route you are on, the UI isn't updated" ([r/azuredevops, Jun 2024](https://www.reddit.com/r/azuredevops/comments/1dpp21k/)).
- **adoken angle:** Real navigation history (Cmd+[ / Cmd+]). Only claim if built.
- **Hook ideas:** "Back closes. Close goes back. Got it."
- **Resonance:**

### P18. Link type "parent": which one is the parent?

- **Evidence:** "'Link type: parent'. Which is the parent?" and "related work is also called linked items" ([r/azuredevops](https://www.reddit.com/r/azuredevops/comments/1rfdr4y/the_ui_is_horrendous_will_this_ever/)).
- **adoken angle:** Plain-language labels ("B is a child of A"). v0.1 is read-only for work items, so use as a relatable pain/question post.
- **Hook ideas:** "Link type: Parent. Parent of what, exactly?"
- **Resonance:**

### P19. Sprint picker opens at the top, every time

- **Evidence:** "The dropdown for your sprints used to open on the sprint you are currently on, now it just opens at the top meaning you have to scroll down each time, which gets old quick when you have 200+ sprints." ([r/azuredevops](https://www.reddit.com/r/azuredevops/comments/1dpp21k/)). An ADO team member replied in-thread that New Boards Hub is a UX-layer rewrite and asked for specifics.
- **adoken angle:** Current iteration is the default filter. Only claim if built.
- **Credit:** The team responded and asked for details; mention the rewrite context.
- **Hook ideas:** "Sprint 214 of 214. Scroll from sprint 1."
- **Resonance:**

### P20. Ellipsis roulette

- **Evidence:** "there must be two dozen 'ellipsis menus' and 'gear-icon-settings' you have to memorize. And they're not even consistently placed." and "permissions, that are located in like 5 or more places" ([r/azuredevops, Jul 2025](https://www.reddit.com/r/azuredevops/comments/1lvo9gs/)). "Is it in the left margin menu? in a settings/gear icon? in an ellipsis? (WHICH ellipsis??)" ([r/azuredevops, 2026](https://www.reddit.com/r/azuredevops/comments/1rfdr4y/the_ui_is_horrendous_will_this_ever/)).
- **adoken angle:** Every action is in Cmd+P / a command palette. **v0.1.**
- **Hook ideas:** "Which ellipsis? There are dozens."
- **Resonance:**

### P21. Markdown in work items took years, and is opt-in per field

- **Evidence:** "Adding Markdown capabilities to the work item is a long-standing request." GA July 2025; HTML stays the default; opt in per field; New Boards only ([Azure DevOps Blog](https://devblogs.microsoft.com/devops/markdown-support-arrives-for-work-items/)). Users still find the wording confusing: "switch to Markdown editor… you can convert this field. (also… convert it to what???)" ([r/azuredevops](https://www.reddit.com/r/azuredevops/comments/1rfdr4y/the_ui_is_horrendous_will_this_ever/)).
- **adoken angle:** Render both HTML and Markdown descriptions cleanly. Mostly a **credit** post.
- **Credit:** This shipped. Celebrate it.
- **Hook ideas:** "Markdown in work items shipped. Credit where due."
- **Resonance:**

---

## R. Pull request review

### P22. Waiting on the PR page

- **Evidence:** "There is a huge latency (many minutes) before the code snippets are displayed. There is a spinning icon in place of all code snippets." ([Pull request overview is extremely slow…](https://developercommunity.visualstudio.com/t/pull-request-overview-is-extremely-slow-showing-co/1118078), 2020). "The final few files are not being diffed efficiently, and the file reader is spinning… while the service status reports all green" ([Poor performance when reviewing a pull request](https://developercommunity.visualstudio.com/t/poor-performance-when-reviewing-a-pull-request/471959), 2019). Also [New pull request page slow](https://developercommunity.visualstudio.com/t/new-pull-request-page-slow/1275962).
- **adoken angle:** PRs in your queue are prefetched to disk; diffs render from local data. **v0.1 (W6–W7).** Post measured time-to-first-diff only.
- **Hook ideas:** "Status page: all green. PR page: spinner."
- **Resonance:**

### P23. Syntax highlighting only in full-file view

- **Evidence:** "can we please have syntax highlighting for the PR view without opening each file in the whole view? The feature is already there, why not activate it for the 'small' view" ([Syntax Highlighting for Pull Requests](https://developercommunity.visualstudio.com/t/Syntax-Highlighting-for-Pull-Requests/10332940), Apr 2023, closed; "Since all of the other feature requests are closed, here we go again"). The community built browser extensions to fill the gap: [ckob/ado-syntax-highlighter](https://github.com/ckob/ado-syntax-highlighter), [alanlecart/ado-pr-highlighter](https://github.com/alanlecart/ado-pr-highlighter).
- **adoken angle:** Highlighted diffs everywhere, themed. **v0.1 (W6).**
- **Hook ideas:** "People write browser extensions to get colored diffs."
- **Resonance:**

### P24. "Depressingly mouse-dependent" reviews

- **Evidence:** "The ADO web experience is depressingly mouse-dependent, particularly when reviewing pull requests… Moving up and down the tree of changed files is painfully clunky" ([ADO Pull Request Keyboard Shortcuts…](https://developercommunity.visualstudio.com/t/ADO-Pull-Request-Keyboard-Shortcuts----N/10725130), Aug 2024).
- **Credit:** Sprint 261 (Jan 2025) added shortcuts to navigate PR discussions and threads ([release notes](https://learn.microsoft.com/en-us/azure/devops/release-notes/2025/repos/sprint-261-update)); F7/Shift+F7 jump between diffs.
- **adoken angle:** Full keyboard review: next/prev file, hunk, thread; mark viewed; approve. **v0.1 (W6).**
- **Hook ideas:** "Review a 40-file PR without touching the mouse."
- **Resonance:**

### P25. Batch comments exist, hidden, and vanish on reload

- **Evidence:** The workaround is "'Save all comments' menu item from the ellipsis… to the right of the Complete button" ("thanks! never even saw that option..."), and the catch: "you will lose all (unsaved) comments when reloading the page, or restarting the browser" ([r/azuredevops](https://www.reddit.com/r/azuredevops/comments/136ern1/)).
- **adoken angle:** "Unsent composer drafts persist across restarts" (`docs/specs/0004-pr-review.md` FR-8). **v0.1 (W6–W7).**
- **Hook ideas:** "Your 12 draft comments, gone with one refresh."
- **Resonance:**

### P26. Big files get truncated diffs

- **Evidence:** "For any single file that's larger than 5 MB, the diff view shows truncated file content. For a diff view of the full content, download and review such files using a local diff tool." ([Review pull requests](https://learn.microsoft.com/en-us/azure/devops/repos/git/review-pull-requests?view=azure-devops)).
- **adoken angle:** Craft post only if our diff view handles large files well (measure first).
- **Hook ideas:** "Over 5 MB? Download it and diff it yourself."
- **Resonance:**

### P27. Merge conflicts: take left, take right, or go local

- **Evidence:** "You can either take left or right but solving a conflict, you have to do it locally." The recommended extension "Freezes alot. As a merge tool its pretty half baked." ([r/azuredevops](https://www.reddit.com/r/azuredevops/comments/1dub0ly/why_is_it_so_difficult_to_solve_merge_conflicts/)).
- **adoken angle:** Not in v0.1. Use as a **question** post ("how does your team handle conflicts?").
- **Hook ideas:** "Left or right. Those are your options."
- **Resonance:**

### P28. Approvals reset on empty commits and main syncs

- **Evidence:** "The requirement is for approval votes to reset exclusively when there are actual changes pushed… excluding empty commits or syncs with the main branch… I haven't discovered a configuration option to address this scenario." ([Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/1688029/azure-devops-prs-reset-approvals-even-when-commits)).
- **adoken angle:** Show "what changed since your approval" (iteration compare is v0.2). Pain post only for now.
- **Hook ideas:** "Merged main. Lost three approvals."
- **Resonance:**

### P29. Stale PRs need third-party nagging

- **Evidence:** Teams install reminder bots: [Pull Reminder for DevOps](https://pullreminder.io/) ("Stale pull requests slow down releases, block other work, and frustrate everyone"), [BroomBot](https://github.com/thomasrayner/BroomBot), [CatLight](https://catlight.io/a/azure-devops/azure-devops-pull-request-notifications).
- **Credit (strong):** Microsoft Research built *Nudge*, deployed on 147 repositories at Microsoft, with "significant reduction in completion time, by over 60%, for pull requests which were nudged" ([arXiv 2011.12468](https://arxiv.org/pdf/2011.12468)). Microsoft proved nudges work; they just aren't in the product.
- **adoken angle:** Smart nudges: stale PRs waiting on you, your active work gathering dust. Once, when it matters. **v0.1 (W7).**
- **Hook ideas:** "Microsoft measured it: nudged PRs finished 60% faster."
- **Resonance:**

### P30. Large PRs get truncated by tooling

- **Evidence:** "Azure DevOps analysis can truncate pull requests after 100 changed files" ([pr-agent#3462](https://github.com/The-PR-Agent/pr-agent/issues/3462)); paging through PR changes is easy to get wrong.
- **adoken angle:** Craft: paging iterations/changes correctly; test with a 300-file demo PR. **W6 craft.**
- **Hook ideas:** "File 101 of 300 exists. Your tool might disagree."
- **Resonance:**

---

## P. Pipelines

### P31. Big logs: download the zip and grep

- **Evidence:** "Verbose logs are 5-10x larger and slow down the log viewer… Always download the raw logs for complex failures… I grep through these locally when the web UI is not giving me enough context." ([Grizzly Peak Software](https://www.grizzlypeaksoftware.com/library/pipeline-debugging-diagnosing-failed-builds-3fpt6iee)). Live logs have line limits ([Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/2341474/how-to-increase-max-lines-of-live-azure-pipeline-l)).
- **adoken angle:** v0.1 syncs pipeline runs and failures, not a log viewer. Question/relatable post; log viewer is a v0.2+ idea to test.
- **Hook ideas:** "Pipeline failed. Step one: download a zip."
- **Resonance:**

### P32. YAML errors you only see at queue time

- **Evidence:** "we now instead have to write … yaml files with absolutely no validation and guidance" ([HN comment, Jan 2025](https://news.ycombinator.com/item?id=42755559)). `az pipelines create` only printed "Could not queue the build because there were validation errors or warnings" and hid details in debug logs ([azure-devops-cli-extension#1032](https://github.com/Azure/azure-devops-cli-extension/issues/1032)).
- **Credit:** The web YAML editor does validate and has IntelliSense ([YAML pipeline editor](https://learn.microsoft.com/en-us/azure/devops/pipelines/get-started/yaml-pipeline-editor?view=azure-devops)); complaints are about templates/parameters that only resolve at queue time.
- **adoken angle:** None in v0.1. Relatable only; don't promise.
- **Hook ideas:** "Valid YAML. Invalid pipeline. Find out at queue time."
- **Resonance:**

### P33. Pipeline notifications are all or nothing

- **Evidence:** Advice for scheduled-build failures is a hand-written custom subscription with a pipeline-name filter per pipeline ([tutorial](https://www.tutorialpedia.org/blog/email-notification-for-scheduled-build-that-failed-with-azure-devops-vsts/), [Tech Community thread](https://techcommunity.microsoft.com/discussions/azure/email-notification-for-failed-scheduled-pipleline-runs-on-azure-devops/3959245)).
- **adoken angle:** Pipelines in the notification center with priority rules. **v0.1 (W5).**
- **Hook ideas:** "One custom subscription per pipeline. Per person."
- **Resonance:**

---

## A. Auth and PATs

### P34. Global PATs stop working December 1, 2026

- **Evidence:** "December 1, 2026 – All existing global PATs will be fully decommissioned. Tokens will stop working after this date." ([Azure DevOps Blog, updated Mar 2026](https://devblogs.microsoft.com/devops/retirement-of-global-personal-access-tokens-in-azure-devops/)). Check yours: User settings → Personal access tokens → Access scope "All accessible organizations".
- **Credit:** Good security call; say so.
- **adoken angle:** adoken only ever creates org-scoped tokens (`allOrgs: false`, spec 0001 FR-4). **v0.0.2 (W2).** Reminder post Nov 30.
- **Hook ideas:** "Global PATs stop working on December 1."
- **Resonance:**

### P35. Expired PATs can't be regenerated

- **Evidence:** "Once it's in the expired state, you can't revive that exact secret you have to create a brand-new PAT, but you can clone all the same settings" ([Microsoft Q&A, Apr 2026](https://learn.microsoft.com/en-us/answers/questions/5857213/in-azure-devops-i-can-no-longer-regenerate-an-expi)).
- **adoken angle:** Expiry reminder 7 days ahead; one-click renew when connected via az (spec 0001, renewal section). **v0.1.**
- **Hook ideas:** "Your token expired. Please recreate it from memory."
- **Resonance:**

### P36. PATs max out at a year, and people want forever

- **Evidence:** [Allow personal access tokens that do not expire](https://developercommunity.visualstudio.com/t/allow-personal-access-tokens-that-do-not-expire/817953) (Dec 2019). Users get one reminder email a week before expiry ([Revoke PATs for users](https://github.com/MicrosoftDocs/azure-devops-docs/blob/main/docs/organizations/accounts/admin-revoke-user-pats.md)); "If a PAT expires, automated builds, deployments, and scheduled jobs can fail" ([Medium](https://tkolber.medium.com/list-all-personal-access-tokens-pats-expiration-dates-in-azure-devops-for-all-users-7c4ed87dacef)).
- **Tone:** Don't side with "never expire". Expiry is right; the renewal UX is the pain.
- **adoken angle:** 90-day default, renew in one click. **v0.1.**
- **Hook ideas:** "Expiring tokens are right. Renewing them shouldn't hurt."
- **Resonance:**

### P37. ~40 scopes, so everyone picks "Full access"

- **Evidence:** The [scopes table](https://learn.microsoft.com/en-us/azure/devops/integrate/get-started/authentication/oauth?view=azure-devops#available-scopes) is long; admins now have a policy to *block* full-scoped PATs ([Manage PATs with policies](https://learn.microsoft.com/en-us/azure/devops/organizations/accounts/manage-pats-with-policies-for-administrators?view=azure-devops)), which tells you how common over-granting is. Our reasoning is in spec 0001 §1.
- **adoken angle:** Exactly 5 scopes for Review, shown in plain words and `vso.*` names; read-only mode. **v0.0.2 (W2).**
- **Hook ideas:** "Which of these scopes do you need? All of them, apparently."
- **Resonance:**

### P38. Tokens go inactive if you don't sign in

- **Evidence:** In Entra-backed orgs a PAT becomes inactive when its user hasn't signed in for 90 days (see [Use personal access tokens](https://learn.microsoft.com/en-us/azure/devops/organizations/accounts/use-personal-access-tokens-to-authenticate?view=azure-devops); summarized in spec 0001 §5.4).
- **adoken angle:** 401 → clear banner "sign in to ADO in your browser, then Renew", no retry loop (spec 0001). **v0.1.**
- **Hook ideas:** "Valid token, inactive user, mystery 401."
- **Resonance:**

### P39. No new OAuth apps since April 2025

- **Evidence:** "As of April 23, 2025, the Azure DevOps OAuth app platform is no longer accepting new app registrations" ([Azure DevOps Blog](https://devblogs.microsoft.com/devops/no-new-azure-devops-oauth-apps/)); Microsoft points to Entra ID instead.
- **adoken angle:** Craft post: why v0.1 is PAT (az-assisted) behind an `AuthProvider` trait, with Entra next. **W2 or swap bank.**
- **Hook ideas:** "Building an ADO app in 2026? There's no OAuth app option."
- **Resonance:**

---

## X. Platform and meta

### P40. "Is Azure DevOps dying?" anxiety

- **Evidence:** [Is Azure DevOps surviving?](https://www.reddit.com/r/azuredevops/comments/1s5khty/is_azure_devops_surviving/) (Mar 2026); "Microsoft is getting ready to replace Azure DevOps with GitHub" ([HN, Jan 2025](https://news.ycombinator.com/item?id=42755559)) with pushback "Microsoft is extensively using Azure DevOps internally". An ADO team member: "no one should be thinking 'switch to GH because ADO is being sunset'" ([r/azuredevops](https://www.reddit.com/r/azuredevops/comments/1rfdr4y/the_ui_is_horrendous_will_this_ever/)). Public projects were retired in 2026 ([Microsoft Learn](https://learn.microsoft.com/en-us/azure/devops/organizations/projects/public-projects-retirement?view=azure-devops)).
- **Tone:** Never speculate about ADO's future. Our line: "Lots of teams live in ADO today. adoken is for them."
- **Use:** FAQ answer only. Not a post.

### P41. Features you'd expect, a few years late

- **Evidence:** "'new features' … stuff that GitHub, Gitlab, Gitea, and more have had for more than half a decade (like showing that commits are signed or not)" ([r/azuredevops](https://www.reddit.com/r/azuredevops/comments/1s5khty/is_azure_devops_surviving/)).
- **adoken angle:** None directly. Avoid; comparisons with GitHub start flame wars.
- **Use:** Background only.

### P42. SSH: RSA only, HTTPS by default

- **Evidence:** "Defaults to https urls for cloning, Only accepts RSA SSH keys" ([Ask HN, Apr 2026](https://news.ycombinator.com/item?id=47689487)). The same poster: "the whole UI is quite clean, no LLM push anywhere as well. Almost refreshing."
- **adoken angle:** None. Use the positive half as a friendly quote in a craft post about calm software.
- **Hook ideas:** "Someone on HN called ADO 'almost refreshing'. Fair."

### P43. Rate limits: 200 TSTUs per 5 minutes

- **Evidence:** "The global limit is 200 TSTUs within any sliding five-minute window." ([Rate and usage limits](https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/rate-limits?view=azure-devops)).
- **adoken angle:** Craft: incremental, polite sync; honor `Retry-After`; never hammer. **W3 craft.**
- **Hook ideas:** "Syncing an ADO org without getting rate-limited."

### P44. Dark theme readability

- **Evidence:** [Dark theme description readability issues](https://developercommunity.visualstudio.com/t/Dark-theme-description-readability-issue/10519515) (Nov 2023); [Dark Reader issue on dev.azure.com](https://github.com/darkreader/darkreader/issues/1474).
- **adoken angle:** Themes built on semantic tokens with AA contrast checks. **v0.1 (W8).**
- **Hook ideas:** "Dark mode where every text color was actually checked."

### P45. The "Welcome to Azure DevOps" email that wouldn't stop

- **Evidence:** "Please stop the Welcome to Azure DevOps emails. Receiving one every 15-30 minutes." ([Developer Community, 2019](https://developercommunity.visualstudio.com/t/welcome-to-azure-devops-email-spam/462579)); similar report "[Automatic email each hour 'Welcome to Azure DevOps'](https://developercommunity.visualstudio.com/t/Automatic-email-each-hour-%E2%80%9CWelcome-to-Az/10977977)" (Oct 2025).
- **Use:** A bug, not a design choice. Light humor only, in replies. Not a post.

---

## Resonance summary

Update after each pain post. Sort by reply rate.

| ID | Posted | Impressions | Replies | Reply rate | Bookmarks | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| | | | | | | |
