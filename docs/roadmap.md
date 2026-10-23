# adoken roadmap

Owner: product-manager · Last updated 2026-10-01 · Status: proposed. All open questions in specs 0001–0007 resolved by the user on 2026-09-25 (recommendations accepted). 2026-09-30 user decisions: PR review writes accepted (`docs/specs/0004-pr-review.md`), real avatars added (`docs/specs/0012-avatars.md`), multi-org deferred to Later (`docs/specs/0011-organizations-and-local-data.md`). 2026-10-01: pins accepted for v0.1 (`docs/specs/0014-pins.md`, all recommendations); work item detail and editing accepted (`docs/specs/0015-work-item-editing.md`, all recommendations: slices A + B in v0.1, slice C move to project in v0.2); statusbar activity accepted for v0.1 (`docs/specs/0016-statusbar-activity.md`, all recommendations: sync activity + network log in one Activity drawer). Later on 2026-10-01 (owner, all PM recommendations, "build now"): Today redesign (`docs/specs/0017-today-redesign.md`; loose-end pill overridden to "Open Nd", days in the current state), navigation history (`docs/specs/0018-navigation-history.md`), PR review polish (`docs/specs/0019-pr-review-polish.md`), guided tours (`docs/specs/0020-guided-tours.md`), global hotkey (`docs/specs/0021-global-hotkey.md`, pulled from v0.2 into v0.1), celebrations (`docs/specs/0022-celebrations.md`; a PR celebrates on the confirmed merge) and resource meter (`docs/specs/0023-resource-meter.md`) accepted for v0.1; amendments accepted to 0001 (one token with every permission, no renew prompts, Replace token, Welcome screen, Read-only kept as a link, 1-year lifetime; `az` tokens replaced silently before expiry with one quiet inbox item, owner override of Q-7), 0008 (area and iteration dropdown), 0009 (Account page, new settings tables) and 0016 (resource meter slot); cut line reordered below.

Every milestone serves **snappy** or **less noise**. No code without an accepted spec (`docs/specs/`). Ideas that are not on this page live in `docs/ideas.md`, ranked with RICE.

## Product principles (confirmed for public claims)

- **No telemetry.** adoken sends no usage analytics. Success signals are local-only stats the user can see, dogfood, and public replies (each spec, §8).
- **No adoken server in the ADO data path.** The app talks only to `dev.azure.com` / `vssps.dev.azure.com` / `app.vssps.visualstudio.com` from the user's machine. (Licensing after 1.0 may refresh a license file from a Cloudflare Worker; it carries no ADO data, see `docs/pricing.md`.)
- **Least privilege.** One org-scoped PAT, minted once with everything adoken calls and nothing it doesn't: 5 scopes for Review, 5 read-only scopes for Read-only; never full access, never global, never `vso.build_execute`, `vso.graph`, `vso.tokens` or `vso.threads_full`. No feature asks for a scope later (`docs/specs/0001-onboarding.md` §5.2, FR-12).
- **Budgeted polling.** ADO cannot push to a desktop app; adoken polls within ≤ 10 TSTU / 5 min steady state so it never gets the user throttled (`docs/specs/0002-sync.md` §5.2).
- **Performance is a feature.** Frame < 16 ms, Cmd+P < 50 ms on 100k items, cold start < 500 ms (AGENTS.md).

## Timeline at a glance

| Week | Version | Theme | Specs |
| --- | --- | --- | --- |
| W1 | v0.0.1 | Skeleton | 0007 (tokens, built-in themes), perf harness |
| W2 | v0.0.2 | Connect | 0001 |
| W3 | v0.0.3 | Sync | 0002 |
| W4 | v0.0.4 | Dogfood: search | 0003 |
| W5 | v0.0.4.x | Dogfood: inbox | 0005 (inbox, filters, snooze, quiet hours) |
| W6–W7 | v0.0.5 | Review + nudges | 0004, 0006, 0012 (avatars) |
| W8 | **v0.1.0** | Public beta | 0007 complete (backgrounds, sounds, Ken layer), 0014 (pins), 0015 slices A + B (work item detail and editing), 0016 (sync activity, network log, resource meter slot), 0017 (Today redesign), 0018 (navigation history), 0019 (PR review polish), 0020 (guided tours), 0021 (global hotkey), 0022 (celebrations), 0023 (resource meter); amendments to 0001, 0008, 0009; polish, signed build |
| +6 weeks | v0.2 | Depth | see below |
| +8 weeks | v0.3 | Windows | see below |

Weeks are targets for a solo builder, not promises. If a milestone slips, the **cut line** (below) moves; the scope does not silently shrink.

## Is v0.1 too big? The split decision

All seven v0.1 scope items (onboarding, sync, Cmd+P, PR review, notification center, smart nudges, themes/backgrounds/sounds) **stay in v0.1**, each at its "smallest delightful slice". Depth moves to **v0.2**. Justification:

- Every item is needed for the v0.1 story "the daily ADO loop without the web UI": without the inbox, nudges have nowhere to land; without PR review, the inbox's main action opens a browser; without themes/backgrounds, the launch loses its most shareable visual.
- The heavy risk is concentrated in **PR review** (local diffs, virtualization, thread line mapping) and **sync** (rate budget). Those get the most calendar (W3, W6–W7) and the most-deferred depth.
- Depth items were scored with RICE (`docs/ideas.md`) and deferred when effort was high relative to the v0.1 story:

| Deferred to v0.2 | Why deferred (RICE / risk) |
| --- | --- |
| Iteration compare ("changes since my last review") | RICE 96 (2nd overall), but needs iteration-aware line mapping from v0.1 to be solid first; ships first in v0.2 |
| Multi-line range comments + `suggestion` blocks | 2 weeks of composer + anchoring work; single-line comments cover the core loop |
| "Open adoken at login" (`docs/specs/0021-global-hotkey.md` Q-3) | RICE ≈ 27; the global hotkey itself moved into v0.1 (owner decision 2026-10-01), and launching at login is a separate, smaller convenience |
| Per-type celebration state editor in Settings (`docs/specs/0022-celebrations.md` Q-7) | RICE 5; v0.1 edits the two state lists in `settings.toml` and shows a read-only summary |
| `AzCliToken` auth (no PAT at all) | Second auth path doubles onboarding test matrix |
| Video backgrounds, user sound packs (VS Code theme import dropped by 0025 FR-11) | Delight, not loop; zeron importer reuse makes it cheap later |
| Move work item to another project (0015 slice C) | RICE 5: needs the "Move work items out of this project" permission Contributors lack by default, and Azure DevOps resets the state on a move. The rest of work item editing (0015 slices A + B) is in v0.1 |
| Pipeline re-run / approvals (`vso.build_execute`) | High-privilege scope; notifications cover "know it failed" |

**Cut line if v0.1 slips** (in this order, each moved to v0.2; re-ordered 2026-10-01 with each new spec's slot answer): (1) guided tours (`docs/specs/0020-guided-tours.md` Q-1, RICE 24; the Today tour goes with them), (2) resource meter (`docs/specs/0023-resource-meter.md` Q-1, RICE 27), (3) PR review Threads view (`docs/specs/0019-pr-review-polish.md` group B, Q-8, RICE 47; groups A, C, D, E are not on the cut line), (4) Today's mood crossfade and narrow-layout polish (`docs/specs/0017-today-redesign.md` Q-8; the rest of 0017, including every mood rule, is not on the cut line), (5) global hotkey (`docs/specs/0021-global-hotkey.md` Q-5, RICE 56; back to v0.2), (6) work item editing slice B: area, iteration, description editing, links (`docs/specs/0015-work-item-editing.md` Q-1), with the area and iteration dropdown that rides on its tree picker (`docs/specs/0008-work-items-pipelines.md` FR-8, Q-5), (7) Ken layer, with the arcade celebration style (`docs/specs/0022-celebrations.md` group B), (8) calm celebrations (0022 group A, Q-9, RICE 56), (9) network log (`docs/specs/0016-statusbar-activity.md` group B), (10) sync activity (0016 group A, with the Activity drawer), (11) navigation history (`docs/specs/0018-navigation-history.md` Q-5, RICE 96), (12) pins (`docs/specs/0014-pins.md`), (13) delivery windows per source (quiet hours stay), (14) nudge types N6–N7 (Today's Loose ends go with them). 0015 slice A (redesigned detail, freshness, Sync this item, state, assignee, title, comments) is **not** on the cut line: it is the owner's daily dogfood loop (user decision 2026-10-01). Neither are the 0001 amendment (one token, Replace token, silent `az` replacement, Welcome screen), 0019 groups A, C, D, E or 0009's Account page. Never cut: keychain-only secrets, rate governor, reduce-motion, perf budgets, and Complete / auto-complete PR with its unseen-commit protection (user decision, `docs/specs/0004-pr-review.md` Q-1). Multi-org is not on the cut line: it left v0.1 entirely (Later, user decision 2026-09-30).

---

## v0.0.1 — Skeleton (W1)

- **Goal:** a native window that launches under budget, with the theme system and perf harness that every later feature is measured against.
- **Features:**
  - Workspace, app shell, UI gallery with per-theme snapshots — `docs/specs/0007-theming-backgrounds-sounds.md` (FR-1, FR-2, FR-10).
  - Perf harness: synthetic 100k-item corpus, frame recorder, cold-start timer — used by `docs/specs/0003-command-palette.md` AC-1.
  - Cmd+P shell over synthetic data (no ADO yet) — `docs/specs/0003-command-palette.md`.
- **Exit criteria:**
  - Cold start < 500 ms to first frame on the reference Mac, measured in CI-reproducible harness.
  - 4 built-in themes load from `.adoken-theme.toml`; AA contrast checks pass (0007 AC-2).
  - Zero color literals outside `crates/theme` (0007 AC-1).
  - Cmd+P on 100k synthetic items: p99 < 50 ms (0003 AC-1) — or a written gap analysis if not yet met.
- **Build-in-public beat:** intro thread "Why I'm building a native Azure DevOps client" + 15 s video of cold launch with the timer overlay and the Cmd+P speed test on 100k fake items.

## v0.0.2 — Connect (W2)

- **Goal:** a real org connected in under a minute with least privilege.
- **Features:** `docs/specs/0001-onboarding.md` — az detection, az-assisted org-scoped PAT, paste fallback with scope probes, Keychain, Replace token, sign-out; `AuthProvider` trait.
- **Tasks:**
  - **Create a free personal Azure DevOps org** (personal Microsoft account, so it lives in a different tenant from the work org) seeded with a couple of repos/PRs/work items. It is the second tenant for the PAT Lifecycle API spike, a safe target for destructive tests (Complete PR, revoke), and doubles as the demo org for screenshots and videos (`marketing/strategy.md` §5.4).
  - Run the spike on the work org (the builder's day-job org, a large enterprise tenant; it may require admin consent for the Azure CLI app to call the PAT API) and on the personal org.
- **Exit criteria:**
  - 0001 AC-1…AC-16 pass on the fake-`az` + mock-server harness.
  - Spike result recorded for **both** tenants (the work org + the personal org): PAT Lifecycle API accepted `az` tokens, or failed with a recorded error (e.g. admin consent required) and Path B is documented as the route for that tenant.
  - Fresh macOS user account → org connected in < 60 s wall clock (video).
- **Build-in-public beat:** "Least-privilege PAT in one click" demo + thread on the Dec 1, 2026 global PAT retirement and why adoken only mints org-scoped tokens.

## v0.0.3 — Sync (W3)

- **Goal:** my ADO world in the local store, fresh within a minute, without throttling the user.
- **Features:** `docs/specs/0002-sync.md` — tiers, rate governor, title index, event stream; `docs/adr/0001-local-store.md` accepted.
- **Exit criteria:**
  - 0002 AC-1…AC-15 pass.
  - Real-org measurement: `X-RateLimit-Cost` ledger shows ≤ 10 TSTU / 5 min foreground over a working day; zero "requests delayed" emails.
  - Spikes answered: `@RecentMentions` over REST (works / fallback), reporting-revisions backfill cost on a large project.
- **Status (2026-09-25):** sync runs live, read-only, against the personal demo org (1 project, 14 work items, 2 closed PRs, 1 build). Baseline pass 13 requests / 0.07 TSTU; steady state ≈ 0.07 TSTU per 5 min (budget 10). Six real-payload mismatches fixed (spec 0002 §13). `@RecentMentions`/`@Follows` accepted over REST but untested with data. **Open:** a working-day ledger and backfill cost on a large, busy project (needs the work org, read-only); mention markup, build timeline/stages, hot-tier thread cost, throttling behavior.
- **Build-in-public beat:** "Polling Azure DevOps without getting throttled: TSTUs explained" thread with real ledger numbers.

## v0.0.4 — Dogfood: search + inbox (W4–W5)

- **Goal:** the builder uses adoken all day for finding things and triaging, instead of ADO web hubs and email.
- **Features:**
  - `docs/specs/0003-command-palette.md` on real data (scopes, filters, URL paste, direct ids, row customization).
  - `docs/specs/0005-notification-center.md`: inbox, coalescing, self-suppression, filters, snooze, quiet hours, OS banners.
- **Exit criteria:**
  - 0003 AC-1…AC-12 and 0005 AC-1…AC-14 pass.
  - Dogfood: 5 consecutive working days with ADO email notifications archived by a mail rule, no missed mention discovered afterwards.
- **Build-in-public beat:** side-by-side video "find a PR by fragment: adoken vs web" (W4); "212 events filtered, 9 interruptions" weekly card screenshot (W5).

## v0.0.5 — Review + nudges (W6–W7)

- **Goal:** reviews happen in adoken; nothing I own or owe rots silently.
- **Features:** `docs/specs/0004-pr-review.md` (files, local diffs, threads, line comments, votes, complete; write path accepted 2026-09-30); `docs/specs/0006-smart-nudges.md` (Today card, N1–N7, caps); `docs/specs/0012-avatars.md` (real profile pictures for the signed-in user and every person shown, initials fallback).
- **Exit criteria:**
  - 0004 AC-1…AC-14, 0006 AC-1…AC-10 and every 0012 AC pass.
  - Dogfood: ≥ 80% of the builder's review requests completed in adoken for a week; median time-to-first-vote trending down.
  - 300-file PR fixture scrolls at 60 fps (0004 AC-3).
- **Build-in-public beat:** "Rendering a 300-file PR at 60 fps in GPUI" craft post (r/rust/Zed crowd) + first review "combo" clip if the Ken layer is ready.

## v0.1.0 — Public beta (W8)

- **Goal:** anyone on macOS in a single ADO org can download adoken and run their daily ADO loop in it (one org per install, many projects).
- **Features:** all of 0001–0007 at the v0.1 slice; `docs/specs/0007-theming-backgrounds-sounds.md` complete (custom backgrounds with readability guard, user theme files, sounds opt-in, Ken layer opt-in); `docs/specs/0014-pins.md` (pinned PRs, work items and pipelines in the sidebar; user decision 2026-10-01; number jumps removed by 0025); `docs/specs/0015-work-item-editing.md` slices A + B (redesigned work item detail with rich description, "Synced 2m ago" freshness and Sync this item `⌘⇧R`; edit state, assignee, title, area, iteration, description and work item links; comments with @mentions; user decision 2026-10-01); `docs/specs/0016-statusbar-activity.md` (Activity drawer `⌘⇧A` with the Sync tab and the Network log behind a statusbar indicator; user decision 2026-10-01); the 2026-10-01 owner decisions (all recommendations; cut-line order above): `docs/specs/0017-today-redesign.md` (Today as a day summary with a mood illustration, full-title PR rows with stage bars and today's commits and comments, Up next, Waiting on you, Loose ends with "Open Nd"); `docs/specs/0018-navigation-history.md` (`⌘[` / `⌘]`, mouse buttons and titlebar ‹ › back and forward across views, restoring filters, selection and scroll); `docs/specs/0019-pr-review-polish.md` (file filter and "only files with threads", Threads view, group reviewers hidden by default, findable status actions, copy title and branches); `docs/specs/0020-guided-tours.md` (one short first-use tour each for Today, Inbox, PR review, work item detail and the palette); `docs/specs/0021-global-hotkey.md` (Hyper+A `⌃⌥⇧⌘A` brings adoken to the front from any app; one shortcut, per 0025); `docs/specs/0022-celebrations.md` (calm confetti on my own confirmed merges and forward work item moves, on by default and silent; arcade style with Ken); `docs/specs/0023-resource-meter.md` (optional memory and CPU in the statusbar, off by default); amendments: `docs/specs/0001-onboarding.md` (one token with every permission, no renew prompts, Replace token, silent `az` replacement with one quiet inbox item, Welcome screen, Read-only as a link, 1-year lifetime), `docs/specs/0008-work-items-pipelines.md` FR-8 (area and iteration dropdown), `docs/specs/0009-settings.md` (Account page; `[review]`, `[celebrations]`, `[tours]`, `[hotkey]`, `[activity] resource_meter`), `docs/specs/0016-statusbar-activity.md` (resource meter slot); `docs/specs/0025-feature-diet.md` (owner decision 2026-10-02: fewer shortcuts, section keys in sidebar order, a cheat sheet grouped by screen, one Quiet mode, toggles instead of command pairs, Sounds as on/off and volume, no VS Code import, one global shortcut).
- **Exit criteria:**
  - Every AC in 0001–0009 (0008 AC-9–AC-14 and 0009 AC-14–AC-19 included), 0014, 0015 (except AC-21, slice C), 0016, 0017–0023 and 0025 passes, minus any group the cut line moved to v0.2; perf budgets green in the harness; no open P0/P1 bugs.
  - Signed and notarized macOS build (required for Keychain access groups and OS notifications); update mechanism in place (release-manager).
  - `CHANGELOG.md` entry, privacy/data-flow page on adoken.dev (supports the "my company won't allow it" objection).
  - Beta licensing per `docs/pricing.md` (everything unlocked, 90-day build expiry, Founder license on sale).
- **Build-in-public beat:** themes/backgrounds reveal Monday; launch Tuesday (Show HN, X thread, r/azuredevops) per `marketing/strategy.md` §5.3.

## v0.2 — Depth (≈ 6 weeks after v0.1)

- **Goal:** turn "useful" into "can't go back" for reviewers; remove the last reasons to open the browser.
- **Features (priority order; RICE scores and rationale in `docs/ideas.md`):**
  1. Iteration compare / "changes since my last review" (0004 follow-up spec).
  2. Multi-line comments + suggestions (0004 follow-up).
  3. `AzCliToken` auth provider — no PAT at all when `az` is signed in (0001 follow-up).
  4. Move work item to project (0015 slice C).
  5. "Open adoken at login": Settings › General switch, `SMAppService.mainApp`, default off, opens without a window until summoned (0021 Q-3, RICE ≈ 27).
  6. Saved searches as inbox tabs.
  7. Per-type celebration state editor in Settings (0022 Q-7, RICE 5; v0.1 edits the lists in `settings.toml`).
  8. Video backgrounds, user sound packs. (VS Code theme import was dropped by 0025 FR-11, 2026-10-02.)

  The global summon hotkey left this list: it is in v0.1 (`docs/specs/0021-global-hotkey.md`, owner decision 2026-10-01), on the cut line.
- **Exit criteria:** each feature has an accepted spec and passing ACs; no regression in v0.1 perf budgets; top 3 beta-requested features addressed or explicitly declined in public.
- **Build-in-public beat:** "What you asked for" thread mapping each shipped item to the reply that requested it; Van Westendorp pricing survey (`docs/pricing.md` E1).

## v0.3 — Windows (≈ 8 weeks after v0.2)

- **Goal:** the same app, same budgets, on Windows 11 — most ADO shops are Windows-heavy.
- **Features:** GPUI Windows target; Windows Credential Manager for the PAT (`AuthProvider` unchanged); `az` detection for Windows install paths (`%ProgramFiles%\Microsoft SDKs\Azure\CLI2\wbin\az.cmd`, winget/MSI); Windows toast notifications; data dir `%APPDATA%\adoken` (ADR 0001); signed MSIX/MSI.
- **Exit criteria:** every v0.1/v0.2 AC re-run on Windows 11 (x64 + ARM64); perf budgets met on a mid-range Windows laptop (reference machine to be named); installer signed.
- **Build-in-public beat:** "Same binary budgets on Windows" side-by-side video; Founder-license validation gate (`docs/pricing.md` §7: < 100 by v0.3 → revisit).

## Later (unscheduled, ordered by current RICE)

| Theme | Why later | Spec needed |
| --- | --- | --- |
| **Entra ID OAuth** (native sign-in, no az, no PAT) | Needed for tenants that block PATs and for Enterprise tier; requires an app registration and consent flows. `AuthProvider` seam already in place. | 0008+ |
| **Multi-org** (up to 3 orgs, org switcher, notify-only sync for inactive orgs, remove organization, reset) | Deferred by the user on 2026-09-30: too ambitious, too early, nobody asked for it. Multiplies onboarding, sync budget and UI surface. Store is already one file per org (ADR 0001). Revisit when beta users ask for a second org. | draft kept: `docs/specs/0011-organizations-and-local-data.md` (Deferred) |
| **Semantic search** | Natural-language queries over titles/bodies; local embeddings only (sqlite-vec per ADR 0001 revisit triggers). Unproven value vs. fuzzy + filters. | yes |
| **Team features** | Shared nudge rules, team review-load view, stale-PR dashboard for leads. Needs a sharing mechanism that doesn't break "no adoken server". | yes |
| **Linux** | Best-effort; community builds once Windows parity lands. | no (release task) |

## Explicitly not planned

- Azure DevOps Server / on-prem.
- A relay server for service hooks (breaks local-first; see `docs/specs/0002-sync.md` §5.2).
- Git operations (clone/commit/push) — Tower/Fork territory; adoken stays the ADO workflow client.
- Telemetry of any kind.
