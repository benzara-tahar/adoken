# 0001 — Onboarding: connect Azure DevOps in under a minute

| Field | Value |
| --- | --- |
| Status | Accepted (2026-10-01, owner: amendment with all recommendations; Q-7 amended by the owner). Earlier questions resolved 2026-09-25 |
| Milestone | v0.0.2 (Connect), polished for v0.1.0 |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/app` (flow UI), `crates/ado` (client + `AuthProvider`), `crates/store` (non-secret metadata) |
| Serves | Snappy (time-to-value), Less noise (no scope over-grant, no ADO settings spelunking) |
| Depends on | `docs/adr/0001-local-store.md`, `docs/specs/0002-sync.md` (first sync hand-off) |
| Design | `docs/design/onboarding.md`, `docs/design/principles.md` |

> Amended 2026-10-01 from the owner's dogfood notes (one token with every permission adoken uses, no Renew prompts, scopes and dates in Settings › Account, a new Welcome screen, silent replacement of `az` tokens). Each changed requirement is marked "(amended 2026-10-01, owner notes; accepted 2026-10-01)". The owner accepted the amendment the same day with every recommendation, except Q-7, which the owner amended (FR-20).

## 1. Problem

Getting any third-party client talking to Azure DevOps means: open user settings, find Personal access tokens, guess which of ~40 scopes are needed, pick an expiry, copy a token you can never see again, paste it somewhere, and hope. Most people over-grant ("Full access") because the scope list is opaque, and then forget the token until it silently expires and the tool breaks. On top of that, Microsoft stops accepting **global** PATs on **December 1, 2026**, so any token that says "All accessible organizations" is a time bomb.

(amended 2026-10-01, owner notes; accepted 2026-10-01) Dogfood showed adoken making the same mistake from the other side: a token minted before a feature's scope was added left that feature behind a **Renew token** button, so the owner saw a renew prompt in the review actions row, the Work items row, the work item detail and the Activity drawer, and onboarding promised a reminder 7 days before expiry. "From the beginning, the token should be generated with the required permissions, and the user should be made aware of what the scopes of that token are."

## 2. Users and job to be done

- **Persona:** dev in one ADO org, several projects. Often already has the Azure CLI installed and signed in (it ships with most Azure dev setups). Sometimes in a locked-down tenant where PAT creation is restricted.
- **JTBD:** "When I install adoken, I want it connected to my org with the least access it needs, without visiting ADO settings, so I can see my PRs within a minute."
- **Smallest delightful slice:** detect `az`, mint an org-scoped, least-privilege PAT with one click, store it in the Keychain, and show the inbox filling in.

## 3. UX flow

Visuals, animation timings, and copy live in `docs/design/onboarding.md`. Every animation has a reduce-motion variant (AGENTS.md).

### Path A — Azure CLI assisted (primary)

1. **Welcome.** (amended 2026-10-01, owner notes; accepted 2026-10-01) The `onboarding-welcome` illustration centered at the top, then the title "Welcome to adoken", the description "Review pull requests, track work items, and stay on top of your day." and the primary button **Get started**, which continues to the Connect step (FR-19). `Enter` advances. Reduce motion: everything shows at once, no delay.
2. **Detect Azure CLI** (runs in background from step 1, so it is usually done before the user clicks):
   - Resolve `az`: (a) login-shell lookup `"$SHELL" -lc 'command -v az'` with 2 s timeout, because Finder-launched macOS apps do not inherit the user's shell `PATH`; (b) known locations `/opt/homebrew/bin/az`, `/usr/local/bin/az`, `~/.local/bin/az`; (c) user-picked path.
   - Run `az version -o json`; parse `"azure-cli"` (e.g. `2.83.0`). The `azure-devops` extension is **not** required (we only use core `az account` commands and direct REST).
   - Run `az account show -o json`; non-zero exit = not signed in.
   - Result card: "Azure CLI 2.83.0 · signed in as ada@contoso.com" / "not signed in" / "too old" / "not found".
3. **Sign in if needed.** "Sign in with Azure CLI" spawns `az login` (browser opens). adoken shows a waiting state with Cancel. On success, continue automatically.
4. **Get an Entra token (memory only).** `az account get-access-token --resource 499b84ac-1321-427f-aa17-267ca6975798 -o json`. The token is never persisted and never logged; it lives only for the rest of onboarding (tokens last ~1 h).
5. **Pick organization.** Call Profiles (`me`) then Accounts (`memberId=me`) to list orgs. Single-select list, type-to-filter. "Don't see it?" → text field for the org name or `https://dev.azure.com/{org}` URL (org may live in another tenant; show a hint to run `az login --tenant <tenant>`).
6. **Pick projects.** List projects for the org; all selected by default; type-to-filter; `Space` toggles. At least one required.
7. **Choose access level.** (amended 2026-10-01, owner notes; accepted 2026-10-01) Two options. Each lists every scope of its set (§5.2) with the ADO label, the `vso.*` id and one line on what it allows (FR-13). This is the whole set adoken will ever use; no feature asks for more later (FR-12).
   - **Review** (default): read everything; vote, comment, resolve, complete, abandon and draft PRs; edit, link and comment on work items.
   - **Read-only**: no writes; write controls are replaced by one passive hint (FR-9). Offered as a link under the scope list, "Need a read-only token instead?", not as a choice next to Review (Q-5); switching later is Settings › Account › **Change access…** (FR-16).
   - Expiry picker: 30 / 90 (default) / 180 / 365 days (Q-8). No reminder is promised (FR-18); `az` tokens are replaced silently before they expire (FR-20).
8. **Mint PAT.** `POST https://vssps.dev.azure.com/{org}/_apis/tokens/pats?api-version=7.1-preview.1` with the Entra bearer token and body `{displayName: "adoken · {host-label}", scope: "<space-separated scopes>", validTo: <now+expiry>, allOrgs: false}`. `{host-label}` is the Mac's hostname, unless it looks like an email address, in which case it is `mac-{first 4 hex chars of SHA-256(hostname)}` (Q-4).
   - `patLifespanPolicyViolation` → retry once with 30 days, tell the user the org caps lifetime.
   - `fullScopePatPolicyViolation` / `globalPatPolicyViolation` should never happen (we never request full or global); if they do, show the error verbatim.
   - `accessDenied`, admin-consent error, or tenant policy blocking PAT creation → switch to Path B with the reason shown.
9. **Store and verify.** Write the token to the macOS Keychain (§4 FR-7). Persist non-secret metadata (org, access level, scopes as Azure DevOps returned them, `authorizationId`, `validTo`, creation time, source `az`) in `account.json` (FR-8; amended 2026-10-01, owner notes; accepted 2026-10-01). Verify with `GET https://dev.azure.com/{org}/_apis/connectionData` using the PAT; record `authenticatedUser.id` as "me".
10. **Preferences.** Final screen: sounds on/off (default **off**), Ken layer on/off (default **off**) with a preview, per `docs/design/sound.md` and `docs/design/ken.md`, and quiet hours on/off (default **off**, pre-filled weekdays 19:00–08:00 + all weekend, per 0005 Q-1). `Enter` finishes.
11. **Hand-off.** Start the hot sync tier (0002). Transition to the inbox, which fills progressively; no blocking spinner.

### Path B — paste a PAT (fallback)

Shown when `az` is missing/too old/declined, PAT creation is blocked, or the user picks "I'll paste a token".

1. Org field (name or URL).
2. "Open token page" button → `https://dev.azure.com/{org}/_usersSettings/tokens` in the default browser, plus a checklist of the exact scopes to tick for the chosen access level, each with what it allows (FR-13; amended 2026-10-01, owner notes; accepted 2026-10-01), and a reminder to choose **this organization only**, not "All accessible organizations".
3. Paste field (masked, reveal toggle). Accept the current 84-character format and legacy 52-character tokens; trim whitespace.
4. Verify via `connectionData`, then **scope probes** (read-only calls, one per read scope, §5.3). Missing scopes are listed by human name with a "Fix in browser" link. **Required to finish:** `vso.project` and `vso.code` (or `vso.code_write`) — without them there is no inbox. `vso.work`, `vso.build`, `vso.identity` are optional; each missing one names its consequence (no work items / no pipelines / mentions shown as raw ids) and the user can continue. Write scopes (`vso.code_write`, `vso.work_write`) show as "Write untested" until the first write. (amended 2026-10-01, owner notes; accepted 2026-10-01) A write refused for a missing scope records that scope as missing (FR-17) and only that feature turns read-only, with the passive hint of FR-9; nothing prompts.
5. (amended 2026-10-01, owner notes; accepted 2026-10-01) Optional expiry date ("Expires on"), because adoken can't read a pasted PAT's expiry without `vso.tokens`, which it doesn't request (§5.2). It is shown in Settings › Account and nowhere else; there is no reminder (FR-18).
6. Continue at Path A step 10.

### Token lifecycle: replace, rejection, sign-out

(amended 2026-10-01, owner notes; accepted 2026-10-01; replaces "Renewal, re-auth, sign-out")

- **No expiry reminder** (FR-18). The expiry date lives in Settings › Account.
- **Silent replacement** (FR-20), `az` tokens only: from 7 days before `validTo`, at most once a day, adoken replaces the token in the background when `az` can mint without interaction, then posts one quiet Inbox item ("adoken replaced its Azure DevOps token · the new one expires {date}") and Settings › Account shows "Replaced automatically {date}". Still failing 2 days before expiry: one quiet Inbox item "adoken couldn't replace its token automatically · it expires {date}" with **Replace token…**. No banner, modal, sound or repeat.
- **Replace token** (FR-16), in Settings › Account and as the `⌘P` command **Replace token…**:
  - Created via `az` and `az` is signed in: one click. Get an Entra token → create a new PAT with the **current** §5.2 set for the stored level and the stored lifetime → swap the Keychain item → verify → revoke the old one via `DELETE .../tokens/pats?authorizationId=…`. No browser unless `az login` is needed.
  - Otherwise: the onboarding sheet over the app, from screen 1, with the org fixed.
- **Token rejected or expired** (FR-15): sync stops (no retry loop against a 401); the statusbar says "Token rejected" or "Token expired"; the Activity Sync tab offers **Replace token…**; one Inbox item per rejection episode (Q-6). No banner, modal, toast or sound.
- **Sign out:** confirm sheet; deletes the Keychain item; optional "also revoke the token in Azure DevOps" (checked by default when created via `az`); optional "delete local data" (checked by default). adoken connects one org per install, so switching organization = sign out + onboarding (§11).

### States

| State | Behavior |
| --- | --- |
| `az` not found | Card explains Path B is fine; link to install docs; "Detect again" button. |
| `az` too old (< 2.30.0) | Show found version and minimum; offer Path B or "Detect again" after upgrading. |
| Not signed in | "Sign in with Azure CLI" button; `az login` progress; Cancel returns to choice. |
| Offline | Detection still runs; network steps show "Offline" with Retry; nothing is half-written. |
| Throttled (`Retry-After`) | Countdown on the step's button; never auto-hammer. |
| Tenant consent / policy block | Error verbatim + "Paste a token instead". |
| Token rejected or expired (after onboarding) | FR-15: sync stopped, statusbar "Token rejected" / "Token expired", Activity Sync tab's **Replace token…**, local data readable. (amended 2026-10-01, owner notes; accepted 2026-10-01) |

## 4. Functional requirements

- **FR-1** `AuthProvider` trait in `crates/ado`: `authorize(&mut Request)`, `kind()` (`Pat` now; `EntraOAuth`, `AzCliToken` later), `expires_at()`, `on_unauthorized()`. No caller outside `crates/ado` knows which provider is active.
- **FR-2** `az` detection never blocks the UI thread; each subprocess has a hard timeout (2 s shell lookup, 10 s `az version`, 10 s `az account show`, 20 s `get-access-token`, `az login` unbounded with Cancel).
- **FR-3** Minimum Azure CLI version: **2.30.0** (the MSAL-based credential cache; older ADAL-era CLIs are out of support). `[INFERENCE]` validate during dogfood that `get-access-token --resource 499b84ac-…` behaves identically on 2.30.x and current.
- **FR-4** PATs are always created with `allOrgs: false`. adoken never requests `app_token` / full access.
- **FR-5** (amended 2026-10-01, owner notes; accepted 2026-10-01) Scope sets are data, defined once in `crates/ado/src/onboarding/scopes.rs` from §5.2 (ids, levels, required) and `docs/design/onboarding.md` §4 (labels and copy), and shown verbatim in screens 2b, 2p, 2q and Settings › Account. §5.2 is the single source of truth: a spec that calls an endpoint needing a scope outside it can't be accepted until §5.2 is amended.
- **FR-6** Entra tokens obtained from `az` are held in memory only, zeroized on drop, never written to disk, logs, crash reports, or the store.
- **FR-7** PAT storage: macOS Keychain generic password, service `dev.adoken.pat`, account `{org}`. Not in the DB, not in settings files, not in logs. The HTTP layer redacts `Authorization` headers in any debug log.
- **FR-8** (amended 2026-10-01, owner notes; accepted 2026-10-01) Non-secret PAT metadata lives in `account.json` (`crates/ado/src/onboarding/state.rs` `PatMeta`): org, access level, scopes, `authorizationId`, `validTo`, creation time, source `az`|`pasted`, lifetime, and for FR-20 the day of the last silent attempt (`auto_replace_attempted_on`), whether its failure item was posted, and `replaced_automatically_at`. Scopes are, for `az` tokens, Azure DevOps' own list from the create response (`patToken.scope`, already captured as `CreatedPat.scope`; the requested set only when the response has none) and, for pasted tokens, each scope's probe result: granted, write untested, or missing (FR-17). A replacement stores the new token's scopes, never the old token's (today `Connector::renew` copies `old.scopes`).
- **FR-9** (amended 2026-10-01, owner notes; accepted 2026-10-01) Scope-aware UI: a feature whose scope is missing is disabled with one passive explanation naming the permission (for example "Editing work items needs Work Items (Read & write)"), never with a button, toast action, keyboard stop or notification, and never in an error loop. With the full Review set granted, no such explanation renders anywhere.
- **FR-10** Onboarding is resumable: quitting mid-flow resumes at the last completed step; a PAT minted but not yet verified is retried, not re-minted.
- **FR-11** Project selection is editable later in Settings without re-auth.
- **FR-12 One token, every permission** (amended 2026-10-01, owner notes; accepted 2026-10-01). Onboarding mints (Path A step 8) or asks for (Path B step 2) the complete §5.2 set of the chosen level in one go. That set covers every endpoint the shipped and accepted features call (AC-17), so no feature asks for a scope later and a Review token never meets a missing-scope state.
- **FR-13 Scopes on screen** (amended 2026-10-01, owner notes; accepted 2026-10-01). Screens 2b (create), 2p (create in browser) and 2q (paste and verify) list every scope of the chosen level: ADO label and access as Azure DevOps' PAT dialog shows them, the `vso.*` id, and one line on what it allows (`docs/design/onboarding.md` §4). 2q adds each scope's verification result. Settings › Account shows the same rows for the stored token (0009 FR-8).
- **FR-14 No renew prompts** (amended 2026-10-01, owner notes; accepted 2026-10-01). Outside Settings › Account, the `⌘P` command **Replace token…**, the rejected-token state of FR-15 with its one Inbox item, and FR-20's one "couldn't replace" Inbox item, nothing offers to renew, update or replace the token: no button, toast action, keyboard stop, notification or sound, and no passive text says "renew". Removed from the current code: the **Renew token** buttons in the PR actions row (`crates/app/src/review/pr/compose.rs:1176`) and the PR Work items row (`crates/app/src/review/pr/status_links.rs:752`); the work item detail hint button and its keyboard stop (`crates/app/src/workitems/detail/header.rs:611`, `Stop::RenewToken` in `detail/model.rs`); the read-only toast action (`crates/app/src/workitems/detail/mod.rs:762`); and the renew wording in `crates/app/src/activity/passes.rs:240`, `crates/app/src/review/writes.rs` (`LINK_SCOPE_HINT`), `crates/sync/src/writes.rs` (`MissingWorkScope`, `Unauthorized`) and `crates/sync/src/wi/mod.rs` (`READ_ONLY_HINT`, `MissingScope`, `Unauthorized`).
- **FR-15 Rejected or expired token** (amended 2026-10-01, owner notes; accepted 2026-10-01). A 401 on the main transport latches the token as rejected (`PatAuth::on_unauthorized`, AC-13). Then, and only then:
  - sync sends nothing more (AC-13); the statusbar's sync item reads "Token rejected", or "Token expired" when `validTo` is known and past (spec 0016, danger tone);
  - the Activity drawer's Sync tab shows the rejected empty state with one action, **Replace token…**, which opens Settings › Account and starts FR-16;
  - Settings › Account's token row shows the same state;
  - a write attempted meanwhile sends nothing and fails inline with "Azure DevOps rejected adoken's token. Replace it in Settings › Account." (expired: "adoken's token expired on {date}. Replace it in Settings › Account.");
  - synced data stays readable; no window banner, modal, toast or sound;
  - one notification-center item per rejection episode (Q-6): event `TokenRejected` (0002 FR-11), P1, Inbox only by default (no OS banner, no sound; 0005 FR-3), "Azure DevOps rejected adoken's token · sync stopped" (expired: "adoken's token expired on {date} · sync stopped"), opening Settings › Account. It is posted once per episode, never repeated, and marked done automatically when a replacement verifies.

  Likewise, when a newer adoken adds a scope to §5.2 that the stored token lacks, one item of the same kind and delivery says so once ("adoken's token is missing {permission} · replace it in Settings › Account"); the feature itself shows the passive FR-9 hint.

  A 401 or 403 that means a missing scope (work item transport, 0015 FR-7; avatar route, 0012 FR-9) is never a rejection. The state clears as soon as a replacement verifies; sync resumes on the next pass without a restart.
- **FR-16 Replace token** (amended 2026-10-01, owner notes; accepted 2026-10-01; replaces "Renew"). Settings › Account's **Replace token…** and the `⌘P` command **Replace token…** do the same thing:
  - Source `az`, `az` found and signed in: one click. Create a PAT with the **current** §5.2 set for the stored level and the stored lifetime → Keychain swap → `connectionData` verify → revoke the old token (AC-12 order and rollback). This is how a token minted before a scope joined §5.2 gets it.
  - Otherwise (pasted, `az` missing or signed out, or the user chooses **Change access…** in Account): the onboarding sheet over the app, starting at screen 1 with the org fixed and the access level preset to the stored one. No Welcome, org picker, Projects, First sync or Make it yours. Success closes the sheet. adoken can't revoke a pasted token (no Entra token); the success note says so and offers **Open token page**.
  - Projects, preferences and synced data are kept. A feature gated on a newly granted scope turns on without a restart.
- **FR-17 Refused writes record the scope** (amended 2026-10-01, owner notes; accepted 2026-10-01). When Azure DevOps refuses a write for a missing scope (`vso.code_write`, 0004 AC-9; `vso.work_write`, 0015 FR-7), `account.json` marks that scope missing, so every FR-9 gate and Settings › Account agree, also after a restart. A replacement clears it.
- **FR-18 No expiry reminder** (amended 2026-10-01, owner notes; accepted 2026-10-01). adoken never reminds about expiry: no reminder notification, sync event, sound or timer counts down to it. The expiry date is shown in Settings › Account only. The only expiry-driven behavior is FR-20's silent replacement of `az` tokens and its two quiet Inbox items. Removed: `PatMeta::remind_at` (`crates/ado/src/onboarding/state.rs:76`, never called by the app), `SyncEventKind::TokenExpiring` and its sound mapping (`crates/app/src/sound.rs:212`), and the onboarding line "adoken will remind you 7 days before it expires." (`crates/app/src/onboarding/view.rs:885`).
- **FR-19 Welcome screen** (amended 2026-10-01, owner notes; accepted 2026-10-01). Screen 0 shows the `onboarding-welcome` illustration (original art, shared asset contract C2) centered at the top, then the title "Welcome to adoken", the description "Review pull requests, track work items, and stay on top of your day." and the primary button **Get started** (`↩`), which continues to screen 1 (Connect). Title, description and button are UI text, not part of the image. The 拳 text emblem is gone from this screen (the illustration already carries the fist and the name). The image is decoded off the UI thread; the first frame never waits for it, and its box is reserved so nothing moves when it appears (`docs/design/onboarding.md` §3, §6).
- **FR-20 Silent replacement of `az` tokens** (Q-7, amended by the owner 2026-10-01; accepted 2026-10-01). For tokens whose source is `az` only:
  - **When.** From 7 days before `validTo`, at most once per calendar day, while adoken runs and is online and the token is not in the FR-15 rejected state, adoken attempts one replacement off the UI thread. The day's attempt is recorded in `account.json` (`auto_replace_attempted_on`), so a relaunch doesn't retry the same day.
  - **Non-interactive only.** The attempt runs `az account get-access-token --resource 499b84ac-…` with its 20 s timeout (FR-2) and nothing else: never `az login`, never a browser, never a prompt. If `az` is missing, signed out, or the call fails or times out, the attempt fails silently.
  - **How.** The FR-16 `az` path unchanged: create a PAT with the same access level and the same lifetime as the stored token (the current §5.2 set of that level) → swap the Keychain item → `connectionData` verify → revoke the old token, with AC-12's order and rollback. Sync continues on the new token on its next pass; no data, projects or preferences change.
  - **After success, tell once, quietly.** One notification-center item, event `TokenReplaced` (0002 FR-11): priority P3, no OS banner and no sound whatever the delivery settings (0005 FR-3), text "adoken replaced its Azure DevOps token · the new one expires {date}", opening Settings › Account. `account.json` records `replaced_automatically_at`, and Settings › Account's token row shows "Replaced automatically {date}" (0009 FR-8) until the next replacement of any kind.
  - **Still failing 2 days before expiry.** On the first failed attempt at or after `validTo` − 2 days, one item, event `TokenReplaceFailed`: P3, no OS banner, no sound, text "adoken couldn't replace its token automatically · it expires {date}", with one action **Replace token…** (FR-16). It is posted once per token (never repeated, even as later daily attempts keep failing), and marked done when any replacement verifies. Attempts continue daily until success or expiry; at expiry FR-15 applies.
  - Never a banner, modal, toast, sound or repeat. Pasted tokens are never replaced silently (source `pasted`; FR-16's sheet replaces them).

### 4.1 Consequential amendments to other documents

(amended 2026-10-01, owner notes; accepted 2026-10-01) Applied to these documents on 2026-10-01 with the amendment:

- **0002** FR-11 event table: the `TokenExpiring` row is replaced by `TokenRejected` (FR-15, Q-6), `TokenReplaced` and `TokenReplaceFailed` (FR-20).
- **0005** FR-3 table: `TokenExpiring` → P1 is replaced by `TokenRejected` → P1, Inbox only (no banner, no sound), and `TokenReplaced` / `TokenReplaceFailed` → P3, never a banner or sound.
- **0004** §3 item 8: "a single … hint linking to 0001 renewal" → "a single passive hint (0001 FR-9)". AC-22: "shows the reason and Renew token and no Link button" → "shows the reason, no Link button and no token action".
- **0012** FR-9: "until the token changes (renew, replace, sign in again)" → "until the token changes (Replace token, sign in again)". AC-12: "until renew or paste" → "until a replacement verifies".
- **0015** §3 "Read-only token" and FR-28: "one hint with **Renew token**" → "one passive hint, no button"; "keeps my value as a Retry for after renewal" → "keeps my value as a Retry". AC-8: "the hint with Renew token is present" → "the hint is present with no button and no keyboard stop". Q-8's resolution is superseded by 0001 FR-14.
- **0016** §3 states table, "Token rejected / no token" row: "with **Renew token** (0001)" → "the Sync tab's empty state with **Replace token…** (0001 FR-15)"; add the "Token expired" label. FR-8: row 1 "→ Renew token" → "→ the empty state's Replace token…"; row 2 "→ names the scope, Renew token" → "→ names the scope". AC-A10: "with Renew token" → "with no button".
- **docs/design/principles.md** §3.4: delete the window-level auth banner; §5 copy example "Update token" → "Replace token".
- **docs/design/activity.md**: the token-rejected rows (§ states table, §7.1 empty states, gallery `activity-sync-scope`) drop the auth banner and `⌘K › Renew token…`; the rejected empty state's action is **Replace token…**.
- **docs/design/work-item-detail.md** §9.4 and the state, focus and accessibility tables: drop the **Renew token** button, its keyboard stop and the toast action; copy "Editing work items needs Work Items (Read & write)."; the Retry line reads "Your change is kept."
- **docs/design/sound.md** §3: remove `notify.token_expiring`.
- **docs/design/ken.md** §9: `emblem-strokes.svg` is no longer needed (the onboarding reveal is gone); `emblem.svg` stays for the fight card. **docs/design/tokens.md** motion notes: drop "onboarding emblem" from the SVG scale exceptions.
- **docs/roadmap.md** v0.0.2 features: "renewal" → "Replace token".

## 5. ADO API surface and limits

### 5.1 Endpoints

| Purpose | Endpoint | Auth | Notes |
| --- | --- | --- | --- |
| Entra token | `az account get-access-token --resource 499b84ac-1321-427f-aa17-267ca6975798` | az session | ~1 h lifetime. Community scripts also use `--resource https://management.core.windows.net/`; keep as fallback if the ADO resource is rejected by the PAT API. |
| Who am I (Entra) | `GET https://app.vssps.visualstudio.com/_apis/profile/profiles/me?api-version=7.1` | Entra | Profiles API requires Entra, not PAT. |
| List my orgs | `GET https://app.vssps.visualstudio.com/_apis/accounts?memberId={id}&api-version=7.1` | Entra (`vso.profile`) | |
| List projects | `GET https://dev.azure.com/{org}/_apis/projects?api-version=7.1` | Entra or PAT (`vso.project`) | Paged via `continuationToken`. |
| Create PAT | `POST https://vssps.dev.azure.com/{org}/_apis/tokens/pats?api-version=7.1-preview.1` | Entra only (`vso.pats` / user-delegated) | Returns secret once in `patToken.token`. Errors in `patTokenError` (e.g. `patLifespanPolicyViolation`). |
| Revoke PAT | `DELETE https://vssps.dev.azure.com/{org}/_apis/tokens/pats?authorizationId={id}&api-version=7.1-preview.1` | Entra only | Used on Replace token, silent replacement (FR-20) and sign-out (amended 2026-10-01, owner notes; accepted 2026-10-01). |
| Verify PAT, get "me" | `GET https://dev.azure.com/{org}/_apis/connectionData` | PAT | Returns `authenticatedUser`. Not in the REST reference index but used by Microsoft's client libraries (`ConnectionData` interface). |

### 5.2 Scope sets (single source of truth)

(amended 2026-10-01, owner notes; accepted 2026-10-01) One row per scope adoken can request. "Calls" lists every route in `crates/sync/src/routes.rs` plus the onboarding probes (`crates/ado/src/onboarding/api.rs`); AC-17 keeps this table, `scopes.rs` and the code in step. User-facing copy for each row is `docs/design/onboarding.md` §4.

| Scope (ADO label, access) | Level | Required | Calls | Needed by | High-privilege? |
| --- | --- | --- | --- | --- | --- |
| `vso.project` Project and Team (Read) | both | yes | `GET _apis/projects`, `GET _apis/projects/{project}` | 0001, 0002, 0014 | No |
| `vso.code` Code (Read) | Read-only (Review gets it inside `vso.code_write`) | yes | `GET` repositories, pull requests (list, by id, pinned), iterations, iteration changes, threads, PR work items, blobs. 0004 §5 also names policy evaluations and statuses (`vso.code`); no route calls them yet. | 0002, 0003, 0004, 0006, 0014 | No |
| `vso.code_write` Code (Read & write) | Review | yes | everything under `vso.code`, plus `POST` threads, `POST` thread comments, `PATCH` thread status, `PUT` reviewers (vote), `PATCH` pull request (complete, auto-complete, draft / publish, abandon / reactivate) | 0004 FR-6–FR-8, FR-16 | **Yes** |
| `vso.work` Work Items (Read) | Read-only (Review gets it inside `vso.work_write`) | no | `POST` WIQL; `GET` work items, work item by id; `POST` workitemsbatch; `GET` workitemrevisions, comments, work item types and their fields, field, classification nodes | 0002, 0003, 0006, 0008, 0014, 0015 (reads, per-item sync) | No |
| `vso.work_write` Work Items (Read & write) | Review | no | everything under `vso.work`, plus `PATCH` work item (fields, relations including PR links), `POST` work item comment; never `bypassRules` or `suppressNotifications` | 0004 FR-17, 0015 | No |
| `vso.build` Build (Read) | both | no | `GET` builds, build by id, build timeline, pipeline definition | 0002, 0005, 0008, 0014 | No |
| `vso.identity` Identity (Read) | both | no | `GET vssps …/_apis/identities` (by id, people search); gates avatars (`_api/_common/identityImage`, 0012 FR-9) | 0002, 0004, 0012, 0015 FR-13 | No |

No scope: `connectionData` (any valid PAT). Entra token, not the PAT: profile, accounts, PAT create and revoke (§5.1).

- **Review** = `vso.project vso.code_write vso.work_write vso.build vso.identity` (user decision 2026-09-30). This is the "all required permissions" token of the owner's notes: it is exactly what the shipped and accepted features call.
- **Read-only** = `vso.project vso.code vso.work vso.build vso.identity` (Q-5).
- **Least privilege, reconciled** (`docs/roadmap.md` principle): adoken asks for everything it uses, once, and nothing it doesn't: org-scoped (`allOrgs: false`), never full access or global, never `vso.build_execute`, `vso.graph`, `vso.tokens` or `vso.threads_full`. Least privilege now means "nothing adoken doesn't call", not "the fewest scopes per feature, topped up later". Read-only remains the opt-down. The roadmap sentence ("5 scopes for Review, 5 read-only scopes for Read-only") stays true.
- Considered and rejected: `vso.threads_full` (lets us comment without `vso.code_write`, which is high-privilege) — rejected because voting and completing a PR still require `vso.code_write`, so adding it would grow the Review set without shrinking it. Revisit if users ask for "comment-only".
- Not requested in v0.1: `vso.build_execute` (no re-run / approvals), `vso.notification*` (ADO notification subscriptions cannot deliver to a local app anyway), `vso.tokens` (would let adoken read a pasted token's expiry and scopes, but grants management of the user's tokens; Path B step 5 asks for the date instead), `vso.graph` (0012).
- A 401 on a work item write means the token lacks `vso.work_write` (Azure DevOps answers 401, not 403, there; spec 0015 FR-7), never that the token is rejected: it switches the work item detail to read-only (0015 FR-28) and records the scope as missing (FR-17).

### 5.3 Scope probes (paste path)

| Scope | Probe (read-only, `$top=1`) |
| --- | --- |
| `vso.project` | `GET /_apis/projects?$top=1` |
| `vso.code` | `GET /{project}/_apis/git/repositories` |
| `vso.work` | `POST /{project}/_apis/wit/wiql?$top=1` with `SELECT [System.Id] FROM WorkItems WHERE [System.Id] = 0` |
| `vso.build` | `GET /{project}/_apis/build/builds?$top=1` |
| `vso.identity` | `GET https://vssps.dev.azure.com/{org}/_apis/identities?identityIds={me}` |
| `vso.code_write`, `vso.work_write` | Cannot be probed without side effects → "Write untested". (amended 2026-10-01, owner notes; accepted 2026-10-01) A refusal on the first write records the scope as missing (FR-17); no fix-it prompt. |

### 5.4 Limits and platform flags

- **Rate limits:** onboarding makes < 20 requests; negligible against the 200 TSTU / 5 min global limit. Honor `Retry-After` anyway.
- **No push:** not applicable here.
- **PAT policies:** org/tenant admins can restrict PAT creation, cap lifetime, or block full-scoped/global PATs. Path B must always be available.
- **Admin consent:** the PAT Lifecycle API may return an admin-approval error in tenants that restrict first-party app consent; treat as "blocked, use Path B".
- **Global PAT retirement:** global PATs stop working Dec 1, 2026 — we never create them; the paste checklist tells users to pick one org.
- **Inactive PATs:** in Entra-backed orgs a PAT becomes inactive if the user does not sign in within 90 days. (amended 2026-10-01, owner notes; accepted 2026-10-01) Azure DevOps answers 401, so it is the FR-15 state; Settings › Account's rejected row adds "Signed out of Azure DevOps for a while? Sign in in your browser, then replace the token."

## 6. Acceptance criteria

Test harness: a fake `az` script placed on a controlled `PATH` that returns fixture JSON per subcommand, and a mock HTTP server for ADO hosts. Keychain tests use a test-only service name.

- **AC-1** Given a fake `az` that reports `"azure-cli": "2.83.0"` and a signed-in account, when onboarding starts, the detection card shows the version and signed-in user within 3 s of launch and before the user clicks "Connect".
- **AC-2** Given `az` exists only at `/opt/homebrew/bin/az` and the process `PATH` is `/usr/bin:/bin`, detection still finds it.
- **AC-3** Given `az version` reports `2.29.1`, the flow shows "too old" with minimum 2.30.0 and offers Path B; no `get-access-token` call is made.
- **AC-4** Given `az account show` exits non-zero, the "Sign in with Azure CLI" action runs `az login`; on success the flow proceeds to org selection without further clicks.
- **AC-5** Given Accounts returns 3 orgs, the picker lists all 3; typing filters case-insensitively; entering a URL `https://dev.azure.com/fabrikam/` selects org `fabrikam`.
- **AC-6** (amended 2026-10-01, owner notes; accepted 2026-10-01) Given Review level and 90 days, the PAT create request body has `allOrgs: false`, `scope` exactly `vso.project vso.code_write vso.work_write vso.build vso.identity` (order-insensitive; the §5.2 Review set), and `validTo` within ±1 min of now+90 d.
- **AC-7** Given the create call returns `patTokenError: patLifespanPolicyViolation`, adoken retries exactly once with 30 days and shows the lifetime-cap notice.
- **AC-8** Given the create call returns `accessDenied`, the flow switches to Path B with the reason visible; no further create calls are made.
- **AC-9** After success, the Keychain contains service `dev.adoken.pat` account `{org}` with the token; the store contains `authorizationId`, `validTo`, scopes, source `az`; a byte search of the store files, settings files, and logs for the token value and the Entra token returns zero hits.
- **AC-10** Given a pasted token and probes where `vso.build` returns 401/403 and the rest succeed, the result screen lists "Build (Read)" as missing, the user can continue, and pipeline features are disabled with an explanation.
- **AC-11** (amended 2026-10-01, owner notes; accepted 2026-10-01; replaces the expiry notification) Given a pasted token with a typed expiry date of now + 7 d and, separately, a pasted token without a date, when an injected clock runs 6 days with sync on, then the notification center gains zero items, no sound plays, no sync event about expiry is emitted, and screen 2b's expiry control has no reminder text. (`az` tokens: FR-20, AC-28, AC-29.) (unit: `adoken-notify` classifier over the sync event stream + view snapshot `onboarding-review-access--*`)
- **AC-12** (amended 2026-10-01, owner notes; accepted 2026-10-01) Given source `az`, Replace token performs create → Keychain swap → `connectionData` verify → revoke old (`DELETE` with old `authorizationId`), in that order; if verify fails, the old token stays in the Keychain and the new one is revoked. After success `account.json` holds the new `authorizationId`, `validTo`, creation time and the scopes from the create response.
- **AC-13** (amended 2026-10-01, owner notes; accepted 2026-10-01) Given any ADO call on the main transport returns 401, sync stops issuing requests to that org until a replacement token verifies (verified: zero requests in the next 5 min on the mock server; then one Replace token and the next pass sends requests again without a restart).
- **AC-14** Given the app is quit after step 8 but before step 9, relaunch resumes verification and does not mint a second PAT (mock server sees one create call total).
- **AC-15** Sign-out with both boxes checked removes the Keychain item, calls revoke once, and deletes the local store; relaunch shows Welcome.
- **AC-16** With macOS Reduce Motion on, no onboarding transition animates position/scale (checked via the animation-token override being "none/crossfade").
- **AC-17 Scope table covers the code** (FR-5, FR-12; amended 2026-10-01, owner notes; accepted 2026-10-01). Given the route table `crates/sync/src/routes.rs` `ROUTES` and the onboarding probes, when a test maps each route to its §5.2 scope, then every route has exactly one mapping (a §5.2 scope, or "none" for `connectionData` only; an unmapped route fails the test), every mapped scope is in `scopes(Review)` (read scopes counting as covered by their `_write` scope), and `scope_string(Review)` and `scope_string(ReadOnly)` equal the §5.2 sets. (unit: `adoken-sync`, with the mapping table beside `ROUTES`)
- **AC-18 No renew prompt with a full token** (FR-14; amended 2026-10-01, owner notes; accepted 2026-10-01). Given a connected Review token whose stored scopes are the full Review set, when a scripted UI test opens an active PR (actions row, Work items row, status menu, Link work item…), a work item detail (state, assign, comment, link), the Activity drawer's Sync and Network tabs, the pins list and the `⌘P` results for "token", then the accessibility tree contains no element whose label or text matches `/renew|update token/i`, no missing-scope hint renders, and the only token action is the `⌘P` command **Replace token…**. (UI test against the mock org; same walk over the gallery stories `review-*`, `work-item-detail-*`, `activity-*`)
- **AC-19 Missing scope stays passive** (FR-9, FR-14; amended 2026-10-01, owner notes; accepted 2026-10-01). Given a pasted Review token whose `vso.work_write` is missing, then the work item detail shows the one hint "Editing work items needs Work Items (Read & write)" with no button and no keyboard stop (`Tab` walk), pressing `s` shows a toast with no action, and the PR Work items row shows its reason with no button. Given a Read-only token, the PR actions row shows "Review actions need Code (Read & write)" with no button. (unit: detail stop walk; stories `work-item-detail-read-only`, `review-link-scope-missing`)
- **AC-20 Rejected and expired** (FR-15; amended 2026-10-01, owner notes; accepted 2026-10-01). Given the mock answers 401 to a sync request, then the statusbar sync item reads "Token rejected", the Activity Sync tab shows the rejected empty state whose only action is **Replace token…**, activating it opens Settings on Account and starts Replace token, no toast or banner appears, and a vote attempted afterwards shows "Azure DevOps rejected adoken's token. Replace it in Settings › Account." with zero `PUT` requests on the mock. Given `validTo` = yesterday and a 401, the labels read "Token expired" and "adoken's token expired on {date}. Replace it in Settings › Account.". Given a 401 on the work item transport only, none of this happens (0015 AC-8). (unit: `sync::label`, sync tab; mock server)
- **AC-21 Replace upgrades an old token** (FR-16; amended 2026-10-01, owner notes; accepted 2026-10-01). Given a stored `az` Review token whose scopes are `vso.project vso.code_write vso.work vso.build vso.identity` (minted before 2026-09-30), when Replace token runs, then the create request's `scope` is the current Review set, `account.json` lists `vso.work_write`, the open work item detail becomes editable on the next frame without a restart, and the store's row counts are unchanged. (integration: `crates/ado/tests/onboarding.rs` + app smoke run)
- **AC-22 Replace a pasted token** (FR-16; amended 2026-10-01, owner notes; accepted 2026-10-01). Given a pasted token, when Replace token is chosen, then the onboarding sheet opens at screen 1 with the org fixed and the stored access level preset; Welcome, the org picker, Projects, First sync and Make it yours never show; after a successful paste the sheet closes, the success note offers **Open token page**, and the next sync pass runs with the new token. (UI test + mock)
- **AC-23 Refusal is remembered** (FR-17; amended 2026-10-01, owner notes; accepted 2026-10-01). Given a pasted Review token whose `vso.code_write` is "Write untested", when the first vote gets a scope 403, then `account.json` marks `vso.code_write` missing, Settings › Account shows it as missing, and after a relaunch the PR actions row shows the passive hint without sending a write. (integration + mock)
- **AC-24 Scopes on the onboarding screens** (FR-13; amended 2026-10-01, owner notes; accepted 2026-10-01). For each access level, screens 2b, 2p and 2q render exactly the §5.2 rows of that level in Azure DevOps' dialog order, each with label and access, `vso.*` id and the design §4 "What it allows" line; 2q adds each row's result. (unit over `scopes()`; snapshots `onboarding-review-access--*`, `onboarding-create-in-browser--*`, `onboarding-paste-verify--*` in four themes)
- **AC-25 Welcome screen** (FR-19; amended 2026-10-01, owner notes; accepted 2026-10-01). Given a first launch, then screen 0 shows the `onboarding-welcome` image centered horizontally above the title "Welcome to adoken", the description "Review pull requests, track work items, and stay on top of your day." and the primary button "Get started", in that order top to bottom, and no "拳" text node; `↩` or a click on Get started shows screen 1 (Connect). With Reduce Motion on, the first frame shows every element at full opacity. (UI test + snapshots `onboarding-welcome--*` in four themes, reduce-motion variant)
- **AC-26 Welcome never waits for the image** (FR-19; amended 2026-10-01, owner notes; accepted 2026-10-01). Given the image decode artificially delayed by 300 ms, then the first frame shows the title, description and button, the image's bounds are identical before and after the image paints (layout test), and no decode runs on the UI thread (executor assertion in debug builds).
- **AC-27 One quiet item when sync stops** (FR-15, Q-6; amended 2026-10-01, owner notes; accepted 2026-10-01). Given the mock answers 401 to a sync request, then exactly one `TokenRejected` item "Azure DevOps rejected adoken's token · sync stopped" appears at P1, the fake OS sink receives zero banners and the sound sink zero plays (sounds enabled, P1 delivery left at its default), and further 401s in the same episode add nothing. After a replacement verifies, the item is done. Given instead a stored Review token that lacks a scope added to the test's §5.2 table, one item names the missing permission, once across two relaunches. (unit: `adoken-notify` with the fake sinks + `crates/ado` mock)
- **AC-28 Silent replacement succeeds** (FR-20; amended 2026-10-01, owner notes; accepted 2026-10-01). Given an `az` Review token with a 90-day lifetime and `validTo` = now + 8 d, and a fake `az` that mints without interaction, when an injected clock advances day by day with the app running and online, then no create request is sent on day 0, exactly one create → Keychain swap → verify → revoke sequence runs on day 1 (`validTo` − 7 d) with `scope` equal to the current Review set and `validTo` = now + 90 d, `az login` is never spawned, and no second attempt runs that day after a relaunch. Afterwards one `TokenReplaced` item "adoken replaced its Azure DevOps token · the new one expires {date}" exists at P3 with zero banners and zero sounds, Settings › Account's token row reads "Replaced automatically {date}", and the next sync pass uses the new token. (integration: fake `az` + mock server + fake sinks, injected clock)
- **AC-29 Silent replacement fails quietly** (FR-20; amended 2026-10-01, owner notes; accepted 2026-10-01). Given an `az` token with `validTo` = now + 8 d and a fake `az` whose `get-access-token` exits non-zero ("signed out"), when the clock advances day by day, then one attempt runs per day, zero create requests are sent, `az login` is never spawned, and nothing is shown until the first failed attempt at or after `validTo` − 2 d, which posts exactly one `TokenReplaceFailed` item "adoken couldn't replace its token automatically · it expires {date}" at P3 with one action **Replace token…**, zero banners and zero sounds; the following day's failure posts nothing more. Activating the action starts FR-16. A pasted token with a typed `validTo` = now + 8 d gets no attempt at all. (integration: fake `az` + fake sinks, injected clock)
- **AC-30 Lifetimes and the Read-only link** (Q-5, Q-8; amended 2026-10-01, owner notes; accepted 2026-10-01). Screen 2b's expiry control offers exactly 30, 90 (selected), 180 and 365 days; choosing 365 sends `validTo` within ±1 min of now + 365 d (and AC-7's 30-day retry still applies to a capped org). Screens 2b and 2p show the Review scope rows with no access-level segmented control and a link "Need a read-only token instead?" that switches the rows to the Read-only set and back. (unit + snapshots `onboarding-review-access--*`, `onboarding-create-in-browser--*`)

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Welcome screen first frame after launch | < 500 ms cold start | Launch timer to first presented frame |
| Step transition | 60 fps, no frame > 16 ms | Frame-time recorder during scripted run |
| `az` detection complete (installed, signed in) | p95 < 3 s | Timer around detection task, fake and real `az` |
| PAT mint + Keychain write + verify | p95 < 3 s on 100 ms RTT | Mock server with latency injection |
| Path A, launch → first inbox row painted (excluding `az login` browser time and user think time) | < 20 s | Scripted run against mock ADO |
| Path B, paste → probes finished | < 3 s on 100 ms RTT (probes run in parallel, max 4 in flight) | Mock server |
| Welcome image painted after the first frame (amended 2026-10-01, owner notes; accepted 2026-10-01) | p95 ≤ 150 ms; the first frame never waits for it | Timestamps around decode and first image paint, debug build, 20 cold launches |
| Binary size added by the welcome image (amended 2026-10-01, owner notes; accepted 2026-10-01) | ≤ 1 MB | Release binary size before/after |
| Replace token, `az` path (amended 2026-10-01, owner notes; accepted 2026-10-01) | p95 < 3 s on 100 ms RTT, UI thread never blocked | Mock server with latency injection |
| Silent replacement check (FR-20; amended 2026-10-01, owner notes; accepted 2026-10-01) | Runs off the UI thread at most once a day; 0 requests and no `az` process outside the 7-day window; no frame > 16 ms while it runs | Frame recorder + process spawn counter during AC-28 |

## 8. Success signal (telemetry-free)

- Dogfood: fresh macOS user account to first inbox in under 60 s wall clock, recorded on video for the v0.0.2 build-in-public post.
- Local-only: Settings › Account shows "Connected via Azure CLI" vs "Pasted token"; in public beta we ask users to share which path worked. Target: > 70% of replies say `az` path.
- Zero support questions of the form "which scopes do I need?".
- (amended 2026-10-01, owner notes; accepted 2026-10-01) Dogfood: after one Replace token on the owner's pre-2026-09-30 token, a full work week of PR review and work item editing without seeing any token prompt; Settings › Account answers "what can this token do and when does it expire?" without opening Azure DevOps.
- (amended 2026-10-01, owner notes; accepted 2026-10-01) Dogfood: the owner's `az` token crosses its expiry date at least once without any action from the owner; the only trace is one "adoken replaced its Azure DevOps token" item and "Replaced automatically {date}" in Settings › Account.

## 9. Risks

- **PAT Lifecycle API acceptance of az tokens.** Microsoft docs say the API needs a user-delegated Entra token with `vso.pats`; community practice with `az rest` works today, but tenant consent policies can block it. **The builder's day-job org (a large enterprise tenant) may require admin consent** for the Azure CLI's first-party app to call the PAT API; if so, dogfooding there uses Path B. Mitigation: Path B always available; the week-2 spike runs against that work org **and** a free personal ADO org in a personal Microsoft account/tenant created for this purpose (`docs/roadmap.md` v0.0.2).
- **Microsoft's direction is away from PATs.** Docs recommend Entra tokens over PATs. The `AuthProvider` seam keeps a zero-PAT `AzCliToken` provider cheap to add (see Q-1, resolved for v0.2).
- **macOS GUI `PATH`.** Login-shell lookups can be slow or print noise (oh-my-zsh). Mitigation: timeout + known locations + manual path.
- (amended 2026-10-01, owner notes; accepted 2026-10-01) **No reminder means a hard stop.** Without a reminder, a pasted token stops sync on the day it expires. Mitigations: `az` tokens are replaced silently from 7 days before expiry (FR-20), with one quiet item if that still fails 2 days before; the expiry date in Settings › Account; the quiet FR-15 state with one Inbox item (Q-6); one-click Replace on the `az` path; a 365-day lifetime for pasted tokens (Q-8).
- (amended 2026-10-01, owner notes; accepted 2026-10-01) **Silent replacement writes without asking.** FR-20 creates and revokes a PAT in the user's name while they work. Mitigations: `az` tokens only, never interactive, same level and lifetime, the AC-12 rollback order (the old token stays until the new one verifies), and the user is told once with the new expiry date. Tenants whose PAT policy changed since onboarding fail the attempt quietly and fall back to the one "couldn't replace" item.
- (amended 2026-10-01, owner notes; accepted 2026-10-01) **Pasted tokens stay partly opaque.** Without `vso.tokens`, adoken can't read a pasted token's expiry or exact scopes; it shows probe results and the user-entered date. Write scopes stay "Write untested" until first use.
- (amended 2026-10-01, owner notes; accepted 2026-10-01) **Scope growth forces a replace.** If a future spec adds a scope to §5.2, every existing token lacks it. FR-5 makes that a deliberate amendment; FR-16 is the one-click upgrade; FR-15 tells users once (Q-6); FR-20 picks the new scope up at the next silent replacement.

## 10. Open questions — resolved 2026-09-25 and 2026-10-01

All recommendations accepted by the user on 2026-09-25 (Q-1–Q-4); the 2026-10-01 amendment's questions are resolved below.

- **Q-1** Offer an "Azure CLI session, no PAT" mode (`AzCliToken` provider) in v0.1? **Resolved:** no. It ships in v0.2 (item 3 in `docs/roadmap.md` since 2026-10-01; "Azure CLI session auth" in `docs/ideas.md`, RICE 50). v0.1 keeps one auth path.
- **Q-2** Default PAT lifetime? **Resolved:** 90 days (choices 30 / 90 / 180), with one-click az renewal. (Choices extended to 365 by Q-8; renewal became Replace token, FR-16, and silent replacement, FR-20, on 2026-10-01.)
- **Q-3** Keep a Read-only access level in v0.1? **Resolved:** yes, as a visible choice next to Review (default). (Demoted to a link by Q-5 on 2026-10-01.)
- **Q-4** PAT display name when the hostname looks like an email address? **Resolved:** use `adoken · mac-{4-char hash}` in that case, otherwise `adoken · {hostname}` (step 8).

### 2026-10-01 amendment — resolved 2026-10-01

The owner accepted Q-5, Q-6 and Q-8 as recommended and amended Q-7.

- **Q-5 Keep the Read-only level?** **Resolved:** keep it, demoted from a segmented control next to Review to a link under the scope list, "Need a read-only token instead?", on 2b and 2p; switching later is Settings › Account › **Change access…** (Path A step 7, FR-16, AC-30; `docs/design/onboarding.md` §3.2b, §3.2p). Revisits Q-3. v0.1.
- **Q-6 Tell the user once when sync stops for the token?** **Resolved:** yes, one P1 item per rejection episode, Inbox only (no banner, no sound), cleared when a replacement verifies; the same single item once when an existing token lacks a scope a newer adoken added to §5.2. `TokenRejected` replaces `TokenExpiring` in 0002 FR-11 and 0005 FR-3 (FR-15, AC-27). v0.1.
- **Q-7 Replace `az` tokens silently before they expire?** **Resolved (amended by the owner):** yes, for `az` tokens only, from 7 days before `validTo`, at most once a day, when `az` mints without interaction: create → Keychain swap → verify → revoke, same level and lifetime. After success, tell the user once and quietly: one P3 Inbox item (no banner, no sound) "adoken replaced its Azure DevOps token · the new one expires {date}", and Settings › Account shows "Replaced automatically {date}". Still failing 2 days before expiry: one quiet item "adoken couldn't replace its token automatically · it expires {date}" with **Replace token…**; never a banner, modal or repeat (FR-20, AC-28, AC-29; 0005 FR-3 `TokenReplaced` / `TokenReplaceFailed`; 0009 FR-8). v0.1.
- **Q-8 Offer a 1-year lifetime?** **Resolved:** yes, 30 / 90 / 180 / 365 days, default 90 (Path A step 7, AC-30; `docs/design/onboarding.md` §3.2b). v0.1.

## 11. Out of scope

- Entra ID OAuth app registration and browser sign-in (Later milestone; `AuthProvider` seam only).
- Multi-org: deferred to Later (user decision 2026-09-30: too ambitious, too early, nobody asked). One org per install, many projects. The draft stays in `docs/specs/0011-organizations-and-local-data.md`.
- Azure DevOps Server / on-prem (not planned).
- Windows Credential Manager (v0.3 with Windows).
- Editing an existing PAT's scopes in place (we mint a new one instead).
- (amended 2026-10-01, owner notes; accepted 2026-10-01) Expiry reminders and per-feature renew prompts (FR-14, FR-18).
- (amended 2026-10-01, owner notes; accepted 2026-10-01) Reading a pasted token's expiry or scopes from Azure DevOps: needs `vso.tokens`, not requested (§5.2).
- (amended 2026-10-01, owner notes; accepted 2026-10-01) Editing a pasted token's expiry date after onboarding: replace the token instead.

## 12. References

- Use personal access tokens (creation, lifecycle API, global PAT retirement, policies, 84-char format): https://learn.microsoft.com/en-us/azure/devops/organizations/accounts/use-personal-access-tokens-to-authenticate?view=azure-devops
- PAT Lifecycle API — Create: https://learn.microsoft.com/en-us/rest/api/azure/devops/tokens/pats/create?view=azure-devops-rest-7.1
- Issue Entra tokens with Azure CLI (resource `499b84ac-1321-427f-aa17-267ca6975798`): https://learn.microsoft.com/en-us/azure/devops/cli/entra-tokens?view=azure-devops
- Scopes table: https://learn.microsoft.com/en-us/azure/devops/integrate/get-started/authentication/oauth?view=azure-devops#available-scopes
- Accounts — List: https://learn.microsoft.com/en-us/rest/api/azure/devops/account/accounts/list?view=azure-devops-rest-7.1
- Identities — Read: https://learn.microsoft.com/en-us/rest/api/azure/devops/ims/identities/read-identities?view=azure-devops-rest-7.1
- `ConnectionData` interface: https://learn.microsoft.com/en-us/javascript/api/azure-devops-extension-api/connectiondata
- Community `az rest` PAT creation example: https://github.com/jongio/azure-cli-awesome/blob/main/create-devops-pat.azcli
