# 0011 — Organizations, switching, and local data

| Field | Value |
| --- | --- |
| Status | Deferred (user decision 2026-09-30: too ambitious, too early, nobody asked). Was: Draft, decisions from the 2026-09-26 review in §10 |
| Milestone | Later (unscheduled, `docs/roadmap.md`); was v0.1.0 (2026-09-26) |
| Owner | product-manager (spec) · gpui-engineer (build) · ux-designer (`docs/design/organizations.md`, to write) |
| Crates | `crates/ado` (connections, Keychain, profile), `crates/store` (paths), `crates/sync` (one engine per org, shared governor, notify-only tier), `crates/search` (index per org), `crates/notify` (per-org level, banner prefix), `crates/settings` (`[orgs."<id>"]`), `crates/demo` (demo org in profile), `crates/app` (switcher, onboarding, Settings › Organizations, Danger zone, palette) |
| Serves | Less noise (only one org on screen, per-org mute) · Snappy (switch in < 100 ms, bulk add) |
| Depends on | `docs/specs/0001-onboarding.md`, `docs/specs/0002-sync.md`, `docs/specs/0003-command-palette.md`, `docs/specs/0005-notification-center.md`, `docs/specs/0009-settings.md`, `docs/specs/0010-zeron-reuse-batch-2.md` (Z-13 `ADOKEN_PROFILE`, Q-1 hue tokens), `docs/adr/0001-local-store.md` |
| Design | `docs/design/organizations.md` (switcher, org color stripe, remove/reset sheets, copy) — owned by ux-designer, to be written before build |

> **Deferred 2026-09-30.** The user dropped multi-org from v0.1: too ambitious, too early, and nobody asked for it. adoken supports **one org per install, many projects**. No code exists for this spec. The content below is kept unchanged for a later revisit; it supersedes nothing today: sign-out (0001 §3, AC-15), the Settings › Account tab (0009) and the single-org statements in 0001 §11, 0002 §11 and `docs/design/onboarding.md` are the current behavior. Revisit trigger: repeated requests from beta users for a second org (§8).

> Rule: no code without an **Accepted** spec (AGENTS.md). If revived, this spec would supersede the sign-out parts of 0001 (§3 "Renewal, re-auth, sign-out", AC-15) and 0009 (Account tab, FR-7), and the "single org" statements in 0001 §11, 0002 §11 and `docs/design/onboarding.md`.

## 1. Problem

Many developers belong to more than one Azure DevOps organization: the employer's org plus a personal or side-project org, or one org per client for consultants. In the web UI each org is a separate tab with its own notification emails, and nothing tells you that a review is waiting in the other one. adoken v0.0.x connects exactly one org; connecting another means signing out and losing local triage state. There is also no way to wipe adoken and start from scratch, and the current "delete local data" deletes `orgs/`, not the store the app actually opens (`<data_dir>/adoken.sqlite`, `crates/app/src/shell/mod.rs`), so data survives a delete.

## 2. Users and job to be done

- **Persona:** a developer with 2–3 orgs (work + personal, or a small consultancy). Single-org users must see no extra UI.
- **Job to be done:** "When I work in one org, I want only that org on screen but still hear about reviews and mentions in my other orgs, so I switch only when something needs me and never leak one client's data into another client's screen share."
- **Also:** "When something is off, or I want to test the first-run flow, I want to remove an org or reset adoken completely, so I get a clean start I can trust."
- **Smallest delightful slice:** add two orgs in one az flow; a mention in the inactive org shows a banner "perso · Ada mentioned you on !45"; clicking it switches and opens the PR.

## 3. UX flow

Visuals, motion and copy: `docs/design/organizations.md` (ux-designer). Every interaction is keyboard-reachable (0009 FR-6).

### 3.1 Concepts

- **Connection** = one org + one identity + one credential. At most **one connection per org**, at most **3 connections** per profile. Different orgs may use different identities (work Entra account, personal Microsoft account).
- **Active org**: exactly one org is on screen. Every section (Today, Inbox, Pull requests, Work items, Pipelines, Search), Cmd+P, and the sync status pill show the active org only.
- **Inactive orgs** keep a **notify-only** sync running (§4 FR-12) and reach you through OS banners, sounds and the switcher badge.
- **Demo org**: fake data, read-only, not a connection (does not count toward the cap).
- **Repos** have no global selection: sync scope is per-org projects (with per-repo hide); navigation uses filters (`r:` / `repo:` scope, 0003) and pinned repos.

### 3.2 First run

1. **Welcome**: primary "Connect Azure DevOps" (0001 Path A/B), secondary "Try the demo org".
2. **Path A, pick organizations** (replaces 0001 step 5): the Accounts list becomes multi-select (`Space` toggles, type-to-filter). Up to 3 can be selected; further rows are disabled with "adoken supports up to 3 organizations". "Don't see it?" keeps the org name / URL field (one extra org).
3. **Projects** (replaces 0001 step 6): one screen lists each selected org as "All projects (12) · Change". "Change" opens that org's project picker (0001 step 6 behavior). Default: all projects.
4. **Access level + expiry** (0001 step 7): chosen once, applied to every selected org.
5. **Mint** (0001 step 8), per org, up to 3 in parallel. Each org shows its own result. If one org is blocked by policy (`accessDenied`, consent), the others finish and that org offers "Paste a token instead" (Path B) for that org only.
6. **Preferences** (0001 step 10), once.
7. **Hand-off**: the first selected org becomes active; all connected orgs start syncing (inactive ones in notify-only).

Path B (paste) connects one org; its result screen offers "Add another organization".

### 3.3 Everyday switching

1. The **org switcher** is the existing org row at the bottom of the sidebar (`crates/app/src/shell/sidebar.rs`, `docs/design/principles.md` §2: avatar + org name, avatar-only with a tooltip in the collapsed rail). It becomes a button: monogram tinted with the org color + org name + ▾. It shows the same way with one org; its list then holds that org, "+ Add organizations…" and "Manage organizations…".
2. Clicking it (or `Enter` when focused) opens a list: each org with color dot, name, identity (`ada@contoso.com`), unread badge, and a status icon (syncing, token problem). Then "Demo org" (if visible), a divider, "+ Add organizations…" (disabled with the reason at 3), "Manage organizations…" (Settings › Organizations). `↑`/`↓`/`↩`/`Esc`.
3. **Cmd+P › "Switch organization…"** lists the same entries as palette rows (keyboard path).
4. **Implied switches**: opening an item that belongs to another org switches first, then opens it, and shows a toast "Switched to perso" with **[Back to contoso]**. Triggers: clicking an OS banner from an inactive org, choosing a result from "Search other orgs" (§3.5), pasting a URL of a connected org.
5. On switch: the same section stays selected; the target org's last selection in that section is restored (session memory); an open PR/work item detail closes to its list. Drafts are per org and survive switching (0004 FR-8).
6. **Org color**: each connection gets one of 8 `org.*` color tokens (first unused, editable). With 2+ entries in the switcher, a 2 px stripe in the org color runs along the top of the window and the switcher dot shows it. Confirm sheets for writes (vote, complete, abandon, resolve) name the org: "Complete !912 in contoso?".

### 3.4 Notifications across orgs

- The Inbox (0005) lists the active org only.
- Inactive orgs raise OS banners and sounds under the same 0005 rules, gated by their **notification level** (§3.6). Banner title starts with the org name: "perso · Review requested: !45 Add auth".
- The switcher shows an unread count per inactive org; the Dock badge is the sum across all orgs that are not muted.
- Nudges (0006) run for every org. Inactive orgs' nudges arrive as notifications; the Today card shows the active org plus one line "2 nudges in perso" (activating it switches).
- 401 / token-expiry notices for any org always show, regardless of level.

### 3.5 Cmd+P across orgs

- Results, recents and direct ids (`!912`, `#48213`) come from the active org only.
- When the query is non-empty and another connection exists, the last row is **"Search 2 other orgs…"** (`⌥↩`, or select + `↩`). Only then are other orgs queried; their results appear grouped per org with an org chip and color. Choosing one switches and opens it. Other orgs' titles are never rendered without this explicit step (screen-share safety).
- Pasting a `dev.azure.com/{org}/…` or `{org}.visualstudio.com/…` URL: connected org → "Open PR !45 in perso"; not connected → "fabrikam isn't connected · Add organization" (or "Open in browser" at the cap).
- Commands: "Switch organization…", "Add organizations…", "Manage organizations…", "Open demo org", "Clear cache and resync…", "Reset adoken…" (opens the Danger zone; never acts from the palette).

### 3.6 Settings › Organizations (replaces the Account tab of 0009)

- One row per connection, in switcher order (reorder with `⌥↑` / `⌥↓`). Selecting a row shows:
  - Identity, access level (Review / Read-only; changing it mints a replacement token, 0001), token source and expiry, [Renew] / [Replace token].
  - **Projects** (0001 FR-11) and **Repositories**: per repo, [Hide] (excluded from sync and every view) and [Pin] (listed under "Pinned" in the sidebar; activating filters Pull requests and Pipelines to that repo).
  - **Notification level**: All (0005 rules) · Reviews & mentions · Muted (no banners, sounds or badge; 401 / token notices still show).
  - **Color** (8 `org.*` swatches).
  - [Clear cache and resync…] and [Remove organization…].
- Demo org row (when visible): "Demo org · read-only", [Hide demo org].
- **Danger zone** section at the bottom: [Reset adoken…].
- Everything else in Settings stays global (theme, backgrounds, sounds, motion, quiet hours, nudges, palette).

### 3.7 Remove organization

Sheet "Remove contoso from adoken?":
- Always: the token is deleted from the Keychain and the org stops syncing.
- ☑ Delete this organization's data on this Mac (412 MB) — cache and your triage state (read, snooze, drafts, Ken stats).
- ☑ Revoke the token in Azure DevOps — shown only for tokens adoken minted via `az`.
- [Remove] / [Cancel]. Removing the active org activates the next org in order, else the demo org if visible, else Welcome. If revocation fails (no `az`, not signed in, network), removal still completes and the result names the token with a link to the ADO token page.

### 3.8 Clear cache and resync (per org)

Confirm "Clear contoso cache and resync? Your read, snooze and draft state stays." Deletes that org's cache tables and cursors (ADR 0001 "cache" tables), keeps "local" tables and the token, then syncs as a first sync (no notification flood: `baseline_done = 0`).

### 3.9 Reset adoken (Danger zone)

Sheet listing what will be deleted, with sizes: "3 organizations (1.1 GB), 3 tokens in Keychain, demo org, settings, 2 imported backgrounds".
- ☑ Revoke tokens adoken created in Azure DevOps (az-minted only).
- ☐ Keep my settings (theme, sounds, backgrounds).
- Text field: type `RESET` to enable [Reset adoken] (danger solid). `Esc` cancels.
- On confirm: progress in the sheet, then the app relaunches at Welcome.
- Scope: the current profile only (`ADOKEN_PROFILE`, 0010 Z-13). Not touched: the app bundle, macOS notification permission, other profiles.

### States

| State | Behavior |
| --- | --- |
| Empty | Welcome with "Connect Azure DevOps" and "Try the demo org". |
| Demo only | Switcher shows Demo org; sidebar footer "Connect Azure DevOps". Write actions disabled with "Demo org is read-only". |
| Loading / first sync of an added org | Switcher row shows a spinner; switching to it shows progressive fill (0002 §3). |
| Error | Inactive org with 401: switcher row warning icon + P1 notification "perso · Azure DevOps rejected adoken's token" [Renew]. Active org: 0001 banner. |
| Offline / stale | Switching works from local data; status pill "Offline" (0002); stale markers per 0003. |
| Throttled by ADO | One governor for all connections (FR-13); the pill names the org whose request was throttled. |
| At cap (3) | "+ Add organizations…" disabled with "adoken supports up to 3 organizations". |
| Migration pending | First launch after upgrade while offline: legacy single-org layout keeps working; migration retries on the next launch (FR-4). |
| Removal / reset interrupted | Next launch finishes the pending deletion before opening any store (FR-17). |

## 4. Functional requirements

### Data and identity

- **FR-1 Profile root.** Every path below is relative to the profile root: `data_dir()` (`~/Library/Application Support/dev.adoken`), or `<data_dir>/profiles/<name>/` when `ADOKEN_PROFILE=<name>` is set (0010 Z-13). The Keychain service is `dev.adoken.pat`, or `dev.adoken.pat.<name>` with a profile.
- **FR-2 Layout.**

  ```
  <root>/
    connections.json            non-secret connection list, order, active org, demo visibility
    onboarding.json             in-progress add flow (resumable, per org)
    settings.toml               global prefs (0009) + [orgs."<org-id>"] tables
    backgrounds/                imports + cache (0007)
    orgs/<org-id>/adoken.sqlite one store per org (ADR 0001), + -wal / -shm
    orgs/demo/adoken.sqlite     demo org store (seeded by crates/demo)
  ```

  No store at `<root>/adoken.sqlite` after migration. Directories `0700`, files `0600` (ADR 0001).
- **FR-3 Org id.** `<org-id>` is the organization GUID from `connectionData.instanceId` (both paths; `7.1-preview`, per 0002 real-org findings). The org name is stored for URLs and display only. Keychain items are keyed by org id, so an org rename never orphans a token.
- **FR-4 Migration from v0.0.x.** On launch, if `account.json` exists: call `connectionData` once; on success move `adoken.sqlite` (+ `-wal`, `-shm`) to `orgs/<org-id>/`, re-key the Keychain item from org name to org id (write new, verify, delete old), write `connections.json`, delete `account.json`. Offline or failing: leave everything untouched and run in legacy single-org mode; retry next launch. Local tables (snoozes, read state, Ken stats) survive the move.
- **FR-5 connections.json** (written only by `Connector`, atomic temp + rename, serialized with token operations):

  ```json
  {
    "version": 1,
    "active": "<org-id | demo>",
    "demo_visible": false,
    "connections": [
      {
        "org_id": "…", "org_name": "contoso",
        "identity": { "id": "…", "display_name": "Ada", "unique_name": "ada@contoso.com" },
        "access": "review | read_only",
        "token": { "source": "az | pasted", "authorization_id": "…", "valid_to": "…", "scopes": ["vso.project", "…"], "verified": true },
        "projects": "all | [\"<project-id>\", …]",
        "hidden_repos": ["<repo-id>"],
        "added_ms": 0
      }
    ]
  }
  ```
- **FR-6 Per-org presentation prefs** live in `settings.toml` under `[orgs."<org-id>"]`: `color` (`org.1`…`org.8`), `notifications` (`all` | `reviews_and_mentions` | `muted`), `pinned_repos` (repo ids). Written through 0009's safe writer; removing an org deletes its table.
- **FR-7 Cap.** At most 3 connections per profile; the demo org does not count. Enforced in `Connector` (not only in UI).
- **FR-8 One connection per org.** Adding an org that is already connected is refused with "already connected as ada@contoso.com"; to change identity, remove and re-add.

### Onboarding

- **FR-9 Bulk add (az).** Multi-select up to the remaining slots; one access level and expiry for all; one PAT per org with `allOrgs: false` (0001 FR-4); mints run in parallel (max 3); each org succeeds or fails independently.
- **FR-10 Resumable per org.** `onboarding.json` records mint/verify state per org; relaunch never mints a second PAT for an org (extends 0001 FR-10 / AC-14).
- **FR-11 Add later.** "Add organizations…" runs the same flow as a sheet, skipping Welcome and Preferences, listing already-connected orgs as checked and disabled.

### Sync

- **FR-12 Engines.** One sync engine and one writer thread per connection (ADR 0001 connection model per store). The active org runs all tiers (0002). Inactive orgs run **notify-only**: the 0002 hot tier restricted to what can raise a 0005 notification for me (review requested, mention, comments/votes/pushes/completion on my PRs, my builds finished, work item assigned to me) at 2× the hot interval; warm and cold tiers paused with cursors kept.
- **FR-13 Shared governor.** One budget ledger across all connections, because Microsoft documents the limit per user and does not say whether orgs share it (§9). Steady-state target stays **≤ 10 TSTU / 5 min in total** (0002 FR-9): inactive orgs get at most 2 TSTU / 5 min each, the active org the rest. Hard self-limit 50 TSTU / 5 min in total. `Retry-After` and 429 backoff apply per host and org.
- **FR-14 Switch-in.** When an org becomes active: hot pass immediately, warm next, cold resumes from saved cursors. The previously active org drops to notify-only after its in-flight pass.
- **FR-15 Cold start.** Only the active org's store is opened before the first frame. Inactive engines start ≥ 2 s after the first frame (cold-start budget unchanged).

### Removal and reset

- **FR-16 Remove organization** (§3.7): stop the engine and wait for the writer's in-flight transaction (≤ 2 s), delete the Keychain item, delete `orgs/<org-id>/` if checked, delete `[orgs."<org-id>"]`, remove the connection, revoke if checked (best effort, after local removal so a slow network never blocks it).
- **FR-17 Crash-safe deletion.** Removal writes `orgs/<org-id>.removing` and reset writes `reset.pending` before deleting anything. On launch, a marker means "finish that deletion first", before any store opens. The marker is deleted last.
- **FR-18 Reset adoken** (§3.9): stop all engines → revoke (if checked, best effort, results listed) → delete every Keychain item under the profile's service (enumerated by service, so orphans from older builds go too) → delete everything under the profile root except `settings.toml` and `backgrounds/` when "Keep my settings" is checked → relaunch (bundle: LaunchServices `open -n`; bare binary: re-exec). Reset is never triggerable from the palette, a URL, or an env var.
- **FR-19 Clear cache and resync** (§3.8): one transaction deletes cache-table rows and cursors of that org; local tables untouched; next pass is a first sync with `baseline_done = 0`.

### UI

- **FR-20 Isolation.** No view renders another org's titles, people or ids except the switcher counts, notifications (subject to level), and the explicit "Search other orgs" results.
- **FR-21 Switcher** per §3.3, built on the existing sidebar org row (no second org indicator). Unread counts come from each org's store (inactive orgs' notify-only events).
- **FR-22 Implied switch** per §3.3 step 4, with a toast whose [Back] restores the previous org and section.
- **FR-23 Org color.** 8 `org.*` tokens in `docs/design/tokens.md` (may alias the hue tokens from 0010 Q-1); the switcher monogram always uses the org color; the window stripe and the named write confirmations appear only with 2+ entries in the switcher (connections plus a visible demo org).
- **FR-24 Palette.** §3.5; the other-org indexes load lazily on first "Search other orgs" and stay loaded for the session. Recents are per org.
- **FR-25 Notification level** per §3.4 / §3.6, applied in `crates/notify` before banners, sounds and badges.
- **FR-26 Repos.** Hidden repos are excluded from sync requests and every view of that org; pinned repos appear in the sidebar under "Pinned" for the active org.
- **FR-27 Demo org.** Seeded read-only store at `orgs/demo/`; appears in the switcher when `demo_visible` (set by "Try the demo org" or "Open demo org"; cleared when the first real org connects or by "Hide demo org"). Raises no OS banners. `ADOKEN_DEMO=1` remains a developer flag for isolated demo runs (tests, marketing captures).

### Suggested build order

1. FR-1–FR-5 + FR-17: profile root, layout, org id, migration, markers (fixes today's delete bug).
2. FR-16, FR-18, FR-19: remove, reset, clear cache (Danger zone usable with one org).
3. FR-6–FR-11: connections model, cap, bulk add.
4. FR-12–FR-15: engines per org, shared governor, notify-only.
5. FR-20–FR-26: switcher, implied switches, color, palette, levels, repos.
6. FR-27: demo org in the switcher.

## 5. ADO API surface and limits

| Purpose | Endpoint | Scope | Cost / limit notes |
| --- | --- | --- | --- |
| Org id + me | `GET https://dev.azure.com/{org}/_apis/connectionData?api-version=7.1-preview` | PAT | Once per connect and once for migration. `instanceId` = org id (verify, AC-21). |
| List my orgs (bulk add) | `GET https://app.vssps.visualstudio.com/_apis/accounts?memberId={id}&api-version=7.1` | Entra via `az` | Unchanged from 0001; lists orgs of the signed-in az account only. |
| Mint / revoke PAT | 0001 §5.1 | Entra via `az` | One create per org; one revoke per org on remove / reset. |
| Notify-only sync | subset of 0002 hot tier | 0001 scope sets | ≤ 2 TSTU / 5 min per inactive org (FR-13). |

- **Rate limits:** total steady state unchanged at ≤ 10 TSTU / 5 min (FR-13). Onboarding 3 orgs < 60 requests.
- **No push:** inactive-org notification latency = 2× hot interval (≈ 2 min foreground).
- **PAT scopes:** unchanged (0001 §5.2), per org. Access level is one choice per bulk add, changeable per org later.

## 6. Acceptance criteria

Harness: mock ADO serving 3 orgs (distinct `instanceId`s), fake `az` (0001), test Keychain service per test, temp profile root, injected clock.

- **AC-1** Given fake `az` lists 4 orgs, when the user selects 3, then exactly 3 PAT create calls are made, each with `allOrgs: false` and the same scope set and `validTo`; `connections.json` has 3 entries; the Keychain has 3 items keyed by org id; the 4th row is disabled with the cap message.
- **AC-2** Given org B's create returns `accessDenied`, then A and C connect, B offers "Paste a token instead", and no further create call is made for B.
- **AC-3** Given the app quits after A's mint and before its verify, when relaunched, then the mock sees one create call per org in total.
- **AC-4** After connecting A and B, `orgs/<A-id>/adoken.sqlite` and `orgs/<B-id>/adoken.sqlite` exist and `<root>/adoken.sqlite` does not.
- **AC-5** Given a v0.0.x profile (`account.json`, `<root>/adoken.sqlite` holding a snooze row, Keychain item keyed by org name) and the mock online, after launch the store sits at `orgs/<id>/` with the snooze row, the Keychain item is keyed by id and the old item is gone, `account.json` is gone. With the mock offline, every legacy file and the Keychain item are unchanged and the app works with that org.
- **AC-6** With A active and B connected, the Inbox, Pull requests list and Cmd+P render zero B items (assert on rendered rows); switching to B via the switcher renders B's items and zero A items.
- **AC-7** With A active, a new mention of me in B produces exactly one OS banner whose title starts with B's name within 2 hot intervals + 5 s; over 30 simulated minutes, B's warm and cold endpoints receive zero requests.
- **AC-8** Over a simulated steady hour with 3 orgs (A active), summed mocked `X-RateLimit-Cost` ≤ 10 per any 5-minute window across all orgs, and ≤ 2 per window for each of B and C.
- **AC-9** Clicking B's banner switches to B, opens the PR, and shows the toast; [Back to A] restores A and the previously selected section.
- **AC-10** With A active and "retry" matching only in B, Cmd+P shows no result rows from B and a last row "Search 1 other org…"; `⌥↩` shows B's match with B's chip; `↩` switches to B and opens it.
- **AC-11** Pasting a B PR URL while A is active shows "Open PR !45 in B"; `↩` switches and opens. Pasting a URL of an unconnected org shows "… isn't connected · Add organization" (or "Open in browser" at the cap).
- **AC-12** With B muted, a mention in B produces no banner, no sound and no Dock or switcher count; a B token-expiry notice still appears.
- **AC-13** The org stripe is absent with one connection and no demo, and present with two; changing B's color in Settings updates the stripe on the next frame and persists in `[orgs."<B-id>"] color`.
- **AC-14** With 2 connections, the Complete PR sheet title contains the active org's name.
- **AC-15** Remove B with defaults: Keychain item gone, `orgs/<B-id>/` gone, `[orgs."<B-id>"]` gone, one revoke call (az-minted); if B was active, A becomes active. Killing the process after the marker is written and relaunching completes the same end state before any store opens.
- **AC-16** Clear cache and resync on A keeps snoozes, read state and drafts; cache rows refill; the first pass emits zero notifications.
- **AC-17** Reset with 3 orgs, demo, an imported background and settings: the button stays disabled until the field equals `RESET`; afterwards the profile root is empty, zero Keychain items remain under the profile's service, one revoke call per az-minted token, and relaunch shows Welcome. With "Keep my settings", `settings.toml` and `backgrounds/` remain. A second profile's root and Keychain items are unchanged.
- **AC-18** "Try the demo org" makes the demo active and read-only (write actions disabled with the reason); connecting a real org makes it active and hides the demo; "Open demo org" shows it again; with 3 connections the demo still opens; the demo raises no OS banners.
- **AC-19** Hiding repo X in B: no request for X's PRs in B's following passes and no X items in B's views. Pinning repo Y: "Pinned" lists Y; activating it filters Pull requests and Pipelines to Y.
- **AC-20** With 3 connections, "+ Add organizations…" and Cmd+P "Add organizations…" are disabled with the cap reason; `Connector` refuses a 4th connection programmatically.
- **AC-21** Spike, recorded in this spec: on the work org and the personal org, `connectionData.instanceId` equals the Accounts API `accountId`. If not, FR-3 switches to `accountId` for the az path and records the mapping.
- **AC-22** With 3 connections, cold start to first frame < 500 ms, and the first request of an inactive org is ≥ 2 s after the first frame (launch timer + mock request log).

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Switch org → first frame of target section | < 100 ms (local data only) | Perf harness, 100k items per org |
| Cmd+P usable after switch | < 150 ms (tier 2 FTS serves while tier 1 loads, ADR 0001) | Perf harness |
| "Search other orgs" first results | < 100 ms after `⌥↩` at 100k items per org (lazy index load ≈ 7.5 ms per 100k titles, ADR 0001) | Perf harness |
| Cold start with 3 orgs | < 500 ms, unchanged | Launch timer (AC-22) |
| Memory per extra org | < 30 MB RSS (engine + read pool, index loaded lazily) | Perf harness |
| Idle CPU with 3 orgs | < 0.5% average (0002 budget, total) | Activity Monitor, 30 min dogfood |
| Remove org (UI) | sheet closes < 100 ms; deletion off the UI thread | Frame recorder |
| Reset, 1 GB profile, excluding revoke network time | < 3 s to relaunch | Timer |

Global rules (AGENTS.md) apply.

## 8. Success signal (telemetry-free)

- Dogfood: the builder runs the work org + the personal demo org for a week, records build-in-public videos on the personal org with zero work-org titles on screen, and misses no work-org review request while the personal org is active.
- The "delete my data" path is verified by `AC-17` in CI, and no bug report says data survived a reset.
- Build-in-public reply pattern: consultants asking for more than 3 orgs (revisit trigger for the cap).

## 9. Risks

- **Rate limit scope across orgs is unverified.** Microsoft documents 200 TSTU / 5 min "per user"; whether two orgs share it is not stated. FR-13 budgets conservatively as if they do.
- **`instanceId` vs `accountId`.** Assumed equal (`[INFERENCE]`); AC-21 verifies on two real orgs before build.
- **Org rename.** Keying by id keeps tokens and stores; the base URL still changes. On a 404 from `connectionData`, show "contoso not found — renamed?" with an [Update organization URL] action (Q-2).
- **Banner content during screen share.** Inactive-org banners show titles. Per-org Muted covers clients you share screens with; macOS also has a system setting to hide notifications while sharing (`[INFERENCE]`: verify its default on macOS 26).
- **Multiple az accounts.** Bulk add lists orgs of the currently signed-in az account only; orgs in another tenant need `az login --tenant` (0001 step 5 hint) or the paste path.
- **v0.1 scope growth.** Multi-org adds onboarding, sync and UI surface to the public beta (Q-1).
- **Cap of 3** may block consultancies; revisit on repeated requests (§8).

## 10. Open questions

Decisions from the 2026-09-26 review (user):

- **D-1** Org model: **one active org at a time** (Slack-style), not unified, not window-per-org. → §3.1, FR-20.
- **D-2** Storage: **one DB per org** (ADR 0001 kept). → FR-2.
- **D-3** Identity: **one connection per org, bulk add via az**. → FR-8, FR-9.
- **D-4** Repos: **filters + pins**, no global repo. → §3.1, FR-26.
- **D-5** Milestone: **all in v0.1**. → header, `docs/roadmap.md`.
- **D-6** Inactive orgs: **notify-only**. → FR-12.
- **D-7** Switching: **sidebar switcher + Cmd+P command + implied switch on open**; no `⌃1…9`. → §3.3, FR-21, FR-22.
- **D-8** Where am I: **name + org color** (stripe, dot, named confirmations). → FR-23.
- **D-9** Settings: **global + few per-org** (projects, hidden/pinned repos, notification level, color, access). → §3.6, FR-6.
- **D-10** Cmd+P: **active org + explicit "Search other orgs" row**. → §3.5, FR-24.
- **D-11** Remove organization: **delete everything by default** (token, data, revoke if az-minted). → §3.7, FR-16.
- **D-12** Reset: **everything, type `RESET`**, "Keep my settings" unchecked, plus per-org Clear cache. → §3.8, §3.9, FR-18, FR-19.
- **D-13** Demo org: **in the switcher**. → FR-27.
- **D-14** Cap: **3 connections**. → FR-7.
- **D-15** Bulk add: **one access level and expiry for all**. → FR-9.

Still open (PM recommendation in bold):

- **Q-1** If v0.1 slips, where does multi-org sit on the roadmap cut line? **Recommended:** right after the Ken layer (cut #2): v0.1 keeps FR-1–FR-5 and FR-16–FR-19 (new layout, migration, remove, reset), and connections > 1 move to v0.2.
- **Q-2** Org rename: detect and fix how? **Recommended:** on a `connectionData` 404, look the org id up in the Accounts list (az) and offer the new name in one click; paste users type the new URL.
- **Q-3** Inactive-org cadence: **2× the hot interval** (≈ 2 min foreground, background cadence per 0002 FR-5 when hidden)?
- **Q-4** Keep `ADOKEN_DEMO=1` as a developer flag next to the in-app demo org? **Recommended:** yes, tests and marketing captures rely on an isolated demo profile.

## 11. Out of scope

- A unified "all orgs" view and one window per org (D-1).
- Several identities in one org (D-3).
- `⌃1…9` org shortcuts (D-7).
- Detecting the repo from the frontmost terminal/editor (`docs/ideas.md` candidate).
- More than 3 connections (D-14; revisit trigger in §8).
- Per-org themes, backgrounds, quiet hours or nudge settings (D-9).
- Syncing settings or connections between machines.
- Entra ID OAuth and `AzCliToken` (0001 follow-ups; the connection model is provider-agnostic).

## 12. References

- Rate and usage limits: https://learn.microsoft.com/en-us/azure/devops/integrate/concepts/rate-limits?view=azure-devops
- `ConnectionData` (`instanceId`, `authenticatedUser`): https://learn.microsoft.com/en-us/javascript/api/azure-devops-extension-api/connectiondata
- Accounts — List: https://learn.microsoft.com/en-us/rest/api/azure/devops/account/accounts/list?view=azure-devops-rest-7.1
- PAT lifecycle (create / revoke): https://learn.microsoft.com/en-us/rest/api/azure/devops/tokens/pats?view=azure-devops-rest-7.1
- Keychain item queries (`kSecMatchLimitAll`): https://developer.apple.com/documentation/security/secitemcopymatching(_:_:)
