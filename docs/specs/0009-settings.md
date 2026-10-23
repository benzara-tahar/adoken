# 0009 — Settings window

| Field | Value |
| --- | --- |
| Status | Accepted |
| Milestone | v0.1 (see `docs/roadmap.md`) |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/app` (`settings/`), `crates/settings` (file writes), `crates/ui` (controls) |
| Serves | Less noise · Snappy |
| Depends on | 0005 (notification settings), 0006 (nudge settings), 0007 (themes, backgrounds, sounds), `docs/adr/0001-local-store.md` (preferences in TOML) |
| Design | `docs/design/principles.md` §2 (titlebar ⌘,), `docs/design/tokens.md` §9.4 / §10, `docs/design/sound.md` |

> Amended 2026-10-01 from the owner's notes and the specs written from them (0001 token amendment, 0019, 0020, 0021, 0022, 0023). Each changed item is marked "(amended 2026-10-01, owner notes; accepted 2026-10-01)". The owner accepted the amendments on 2026-10-01 with every recommendation; spec 0001 Q-7's silent replacement adds the "Replaced automatically {date}" line to Account (FR-8, AC-19).

> Rule: no code without an **Accepted** spec (AGENTS.md).

## 1. Problem

Every preference adoken has lives in one `settings.toml`, but the only way to change most of them is to hand-edit that file and restart; notification settings sat in a panel inside the inbox, and nudges, sounds and backgrounds had no UI at all. Tuning noise is the product's promise, so the knobs must be one keystroke away and apply immediately.

## 2. Users and job to be done

- **Persona:** any adoken user, first day and every day after.
- **Job to be done:** "When something is too loud, too quiet or too ugly, I want to find its switch by typing its name and see the change right away, so I get back to work."
- **Smallest delightful slice:** ⌘, → type "quiet" → toggle quiet hours → it applies before the window closes.

## 3. UX flow

1. `⌘,`, app menu **Settings…**, or `⌘P` › **Settings…** opens one Settings window (a second invocation focuses it). It remembers the last tab for the session.
2. Left rail of tabs: General, Appearance, Backgrounds, Sounds, Notifications, Nudges, Sync, Keyboard, Ken, Account, About. `↑`/`↓` move through the rail when it has focus; `⌘[` / `⌘]` go to the previous/next tab from anywhere in the window; `Tab` walks the controls.
3. A search box on top of the rail (`⌘F` focuses it) filters settings by title and keywords across all tabs: tabs without matches dim and step over, rows without matches hide, and `↩` jumps to the first tab with a match. `Esc` clears the search, a second `Esc` closes the window (`⌘W` always closes).
4. Every control applies live and is persisted to `settings.toml` without a Save button.
5. Tabs:
   - **General:** motion (System / Reduced / Full, spec 0007 §3.6), start section on launch.
     (amended 2026-10-01, owner notes; accepted 2026-10-01) Also:
     - **Pull requests** section (shared contract C1; behavior in spec 0019 FR-14, FR-15): one switch **Show group reviewers** (`[review] show_group_reviewers`, default off), sub-label "Teams and groups added by branch policies, such as "[Project]\React PR reviewers". Hidden groups still count toward approvals.", search keywords: group, team, policy, reviewers, required. `⌘P` toggles it with "Group reviewers: hidden → Show" / "Group reviewers: shown → Hide" (0019 FR-14).
     - **Celebrations** (spec 0022): on/off, **Pull requests I complete**, **Work item state changes** (`[celebrations] enabled`, `pull_requests`, `work_items`). The state lists (`step_states`, `milestone_states`) are edited in `settings.toml` only in v0.1.
     - **Tours** (spec 0020): switch **Show tours the first time you open a view** (`[tours] auto_start`) and button **Show tours again** (clears `[tours] seen`).
     - Switch **Show memory and CPU in the statusbar** (`[activity] resource_meter`, default off; spec 0023).
   - **Appearance:** theme picker (0007 §3.1), embedded from the theme module; **Code font** (below).
   - **Backgrounds:** none / three built-ins / "Choose image…" (system file picker), dim and blur sliders, per-view overrides (show/hide, dim, blur for one view), live preview of the effective result; the real window updates live too.
   - **Sounds:** master on/off (default off), pack (Dojo / Arcade), master volume, per-category on/off + volume with a preview button, play when adoken is in the background. *(Amended 2026-10-02 by 0025 FR-13: Sounds shows only on/off and master volume; no pack, per-category, background or quiet-hours sound options)*
   - **Notifications:** everything the inbox panel had (priority rules, delivery per priority, quiet hours, delivery windows, per-source minimums, own activity, dock badge, user rules on/off, active mutes with Unmute) plus the OS banner permission row. `⌘⇧,` in the inbox opens this tab.
   - **Nudges:** N1–N7 on/off and threshold, daily cap (0–10), working days and hours.
   - **Sync** (user decision 2026-09-30, spec 0002 FR-4a): **Sync now** (`⌘R`, also inside the Settings window) with the last sync time or the pause state; **Pause sync** (Off / For 1 hour / Until resumed); **How often** per kind — My pull requests, All active pull requests, Work items, Pipeline runs — each a select of 30 s, 1 min, 2 min, 5 min, 15 min, 30 min, Manual only (only choices at or above the kind's minimum; the row says why the minimum is what it is) with "Synced 12s ago" beside it, plus the fixed Search index and projects row (30 min, 60 min in the background); **In the background** (Same / Slower (×5) / Paused); **Rate limit**: "~3% of your Azure DevOps rate limit" estimated for the chosen cadence from measured per-pass costs, the measured last-5-minutes TSTU, and a Within budget / Above target / Paused chip. Demo or not connected: a notice says sync isn't running; the settings still save. `⌘P` has "Sync: Settings…", "Sync: Pause until resumed", "Sync: Pause for 1 hour" and "Sync: Resume".
     Added by spec 0016 (accepted 2026-10-01): the link **Open sync activity ⌘⇧A** (opens the Activity drawer's Sync tab) and the switch **Show network activity in the statusbar** (`[activity] network_indicator`, default on; 0016 FR-21, FR-28).
   - **Keyboard:** read-only list of every command with a shortcut, grouped by area.
     (amended 2026-10-01, owner notes; accepted 2026-10-01) At the top, a **Global shortcut** section (spec 0021 FR-8, FR-11, FR-12, Q-2): two recorders, **Show adoken** (`[hotkey] summon`) and **Show adoken with search open** (`[hotkey] summon_palette`, 0021 FR-9), each with Record, Clear and Reset, plus the registration status line. *(Amended 2026-10-02 by 0025 FR-12: one recorder, **Show adoken** (`[hotkey] summon`); "Show adoken with search open" and `summon_palette` are removed)*
   - **Ken:** the Ken layer's own section (ken.md), embedded.
   - **Account:** (amended 2026-10-01, owner notes; accepted 2026-10-01) demo → "Demo org · read-only", nothing else. Connected → organization and signed-in identity; **Token** (how it was made, created or "Replaced automatically" date, expiry date or "No expiry date recorded", its state) with **Replace token…**; **Access** (Review / Read-only) with **Change access…**; **Permissions** (one row per scope of the token's level: what it allows and whether it is granted); **Sign out…** (confirm; deletes the Keychain item and saved setup, never synced data). Details in FR-8, FR-9; layout and copy in `docs/design/onboarding.md` §10. One org per install; an Organizations tab and a Danger zone (reset) are deferred to Later with multi-org (`docs/specs/0011-organizations-and-local-data.md`, user decision 2026-09-30).
   - **About:** version, third-party licenses (in-app, offline).

### Code font

User decision 2026-09-30: code surfaces (diffs, code blocks, inline code) render in a code font the user chooses; keycaps stay in the platform's monospace face.

- **Default:** JetBrains Mono v2.304 (OFL-1.1), bundled in the binary as static Regular, Italic and Bold faces and registered with the text system before the first frame (no file IO, borrowed bytes).
- **Appearance › Code font** (search words: code font, editor, monospace, zoom, ligatures): one searchable list — **JetBrains Mono (bundled)**, **System monospace (SF Mono)**, then every installed monospace family, each previewed in its own face. `↑`/`↓` from the search box move the highlight, `↩` or a click applies. Installed families are found by a one-time scan on the background executor when the list first shows (hidden system faces and proportional families are left out). Below the list: **Code font size** (stepper, 11–16 px, default 12), **Ligatures** (switch, default off so a review shows every character as typed), and a three-line diff preview painted live.
- **Commands:** `⌘P` › **Code font…** (opens Settings on Appearance searched for "code font", focus in the font search), **Increase code font size** `⌘=`, **Decrease code font size** `⌘-`, **Reset code font size** `⌘0`, **Toggle code ligatures**. The size keys work anywhere in the app, including the diff. *(Amended 2026-10-02 by 0025 FR-6: the three size commands have no default keys; ⌘=, ⌘- and ⌘0 are unbound)*
- **Storage:** `settings.toml [appearance]` keys `code_font` (`"JetBrains Mono"`, `"system"`, or a family name), `code_font_size` (integer 11–16; out of range clamps with a note, fractional sizes round), `code_ligatures` (bool). They share the table with `theme`: both are written through one in-memory copy of `[appearance]` so neither drops the other's keys.
- **Live apply:** size and ligatures repaint on the next frame. A system or installed family is resolved off the UI thread first (loading a family can take milliseconds) and then repaints. The size applies to code blocks and diffs; inline code keeps `type.code.inline`'s size so it stays in proportion with the text around it. Diff rows take their height from the size (`type.code` line height scaled with the size, rounded to whole pixels: 12 → 18, 16 → 24); the open diff resets its virtualized list at the new height and keeps its scroll position.
- **Fallback:** a configured family the text system doesn't have renders as JetBrains Mono (then the system monospace face if the bundled faces failed to register). The statusbar shows "Code font “…” isn't available · showing …" once per family per session (8 s), and the Code font section shows a warning notice while the fallback lasts. The setting is kept, so installing the font later takes effect on the next launch or edit.

### States

| State | Behavior |
| --- | --- |
| Empty | No `settings.toml` yet: every control shows its default; the first change creates the file. |
| Loading | The window paints at once with defaults greyed out for ≤ one background read; no spinner under 150 ms. |
| Error | `settings.toml` has a TOML syntax error: a notice names the file and error; controls show defaults and **nothing is written** (the user's text is never overwritten). Write failures show the IO error inline. |
| Offline / stale | Not applicable (local only). |
| Throttled by ADO | Not applicable. |
| Demo | Account shows "Demo org · read-only"; sign-out is not offered; settings go to the demo directory's `settings.toml`. |

## 4. Functional requirements

- **FR-1 One file.** All preferences live in `<data>/settings.toml` (demo: `<demo dir>/settings.toml`), one top-level table per module: `[general]`, `[appearance]`, `[background]`, `[sound]`, `[notify]`, `[nudges]`, `[sync]`, `[ken]`, `[sidebar]` (incl. `pinned_collapsed`, spec 0014 FR-4), `[palette]`, `[pins]` (`auto_unpin`, spec 0014 FR-12), `[activity]` (`panel_height` in px, clamped to the drawer's range; `tab` = `"sync"` / `"network"`; `network_indicator` bool, default `true`; spec 0016 FR-1, FR-2, FR-28).
  (amended 2026-10-01, owner notes; accepted 2026-10-01) Also:
  - `[review]`: `show_group_reviewers` bool, default `false` (shared contract C1; spec 0019 FR-14, FR-15). Hiding is display only.
  - `[celebrations]` (spec 0022): `enabled` bool, default `true` (0022 Q-1, resolved 2026-10-01: on by default); `pull_requests` bool, default `true`; `work_items` bool, default `true`; `step_states` list of state names, default `["Code Review", "In Review", "Ready for Review", "Review"]`; `milestone_states` list, default `["Committed", "Done"]`. Names match case-insensitively; a name in both lists counts as a milestone.
  - `[tours]` (spec 0020): `auto_start` bool, default `true`; `seen` array of tour ids.
  - `[activity] resource_meter` bool, default `false` (spec 0023).
  - `[hotkey]` (spec 0021 FR-11; `summon_palette` FR-9): `summon` keystroke string, default `"cmd-ctrl-alt-shift-a"`, `""` = off; `summon_palette` keystroke string, default `""` (off). *(Amended 2026-10-02 by 0025 FR-12, FR-14: `summon_palette` is removed; an old file that has it still loads and the key is never written back)*
- **FR-2 Safe writes.** Writes are key-wise merges that keep comments, formatting and other tables; serialized in-process (no lost updates between modules); atomic (temp file + rename); off the UI thread; coalesced while a slider drags. A file with a syntax error is never overwritten.
- **FR-3 Live apply.** A change is visible in the running app on the next frame (sounds, background, motion, notification and nudge engines, the sync scheduler) without waiting for the disk write.
- **FR-4 Hot reload.** A write by any module, or an external edit of the file (checked every 2 s), re-reads the file in every open view within one poll.
- **FR-5 Validation.** Out-of-range values clamp to the documented ranges on load (daily cap 0–10, thresholds 0–720 h / 1–30 days, volumes 0–1, dim 0–0.9, blur 0–64, sync intervals up to each kind's minimum and to the nearest offered choice, `[activity] panel_height` to the drawer's range from spec 0016 FR-1 and AC-S2); unknown values fall back to defaults per key, never per file.
  (amended 2026-10-01, owner notes; accepted 2026-10-01) A non-list `step_states` / `milestone_states` falls back to that key's default and is reported (0022). Unknown tour ids in `[tours] seen` are kept but ignored (0020). An unparsable or modifier-less `[hotkey]` value falls back to that key's default and is reported (0021 FR-5).
- **FR-6 Keyboard-first.** Every tab and every control is reachable by keyboard; Settings is a `⌘P` command.
- **FR-7 No ADO writes.** Nothing in Settings writes to Azure DevOps (Sync now only asks the scheduler for a read pass), (amended 2026-10-01, owner notes; accepted 2026-10-01) except **Replace token…** and **Change access…**, which create a token and revoke the old one through the PAT Lifecycle API exactly as spec 0001 FR-16 describes. Sign-out only deletes local secrets and setup state (0001 AC-15) and is unavailable in demo mode.
- **FR-8 Account shows the token** (amended 2026-10-01, owner notes; accepted 2026-10-01). Connected, the Account tab shows, from `account.json` only (spec 0001 FR-8; no request is made to open it):
  - **Token:** how it was made ("Created with Azure CLI" / "Pasted"), the created date, and the expiry date, or "No expiry date recorded" for a pasted token without one. After adoken replaced an `az` token silently (spec 0001 FR-20), the row reads "Replaced automatically {date}" with the new expiry date, until the next replacement of any kind. While spec 0001 FR-15 holds, the row says "Rejected by Azure DevOps" or "Expired on {date}" in the danger tone.
  - **Access:** Review or Read-only, with one line on what that level does.
  - **Permissions:** every §5.2 scope of the token's level in Azure DevOps' dialog order, each with ADO label and access, `vso.*` id, what it allows (`docs/design/onboarding.md` §4) and its status: Granted, Not checked yet (a pasted write scope before its first write) or Missing (with its consequence). When any is missing, one line above the rows says which features are off; elsewhere only the passive FR-9 hints and, once, spec 0001 FR-15's inbox item say so (0001 FR-9, FR-14).
  - Search keywords: account, token, pat, scope, permission, access, expiry, expires, created, replace, replaced, automatically, sign out.
- **FR-9 Replace token and Change access** (amended 2026-10-01, owner notes; accepted 2026-10-01). **Replace token…** runs spec 0001 FR-16: for an `az` token with `az` signed in, the button shows "Replacing…" and the rows update in place on success (inline note "New token created. The old one was revoked.", or "…couldn't revoke the old one; it expires on {date}."); otherwise it opens the onboarding sheet over the app at screen 1. **Change access…** opens the same sheet with the other level preselected. A failure shows the cause inline and leaves the old token in place. The `⌘P` command **Replace token…** opens Settings on Account and starts the same action. Neither control exists in demo mode.

## 5. ADO API surface and limits

(amended 2026-10-01, owner notes; accepted 2026-10-01) Purely local, except **Replace token…** and **Change access…** (FR-9), which use spec 0001 §5.1: Entra token from `az`, PAT create, `connectionData` verify, PAT revoke. Opening the Account tab sends nothing.

- **Rate limits:** none locally; a replacement sends ≤ 4 requests (0001 §5.4).
- **No push to local apps:** not applicable.
- **PAT scopes:** none for Settings itself. The Account tab displays the scopes of 0001 §5.2; the PAT Lifecycle calls use the Entra token, not the PAT.

## 6. Acceptance criteria

- **AC-1** Given a `settings.toml` with comments and a `[palette]` table, when the user changes a sound setting, then the file keeps the comments and `[palette]` byte for byte and gains the new `[sound]` values. (unit: `adoken-settings`)
- **AC-2** Given two modules writing different tables concurrently 25 times each, then no write is lost. (unit)
- **AC-3** Given a `settings.toml` with a syntax error, when any setting changes, then the file is unchanged and the window shows the error. (unit + story)
- **AC-4** Given `[nudges]` with `daily_cap = 25` and a threshold of `"900h"`, when loaded, then the cap is 10, the threshold clamps to 720 h, and each clamp is reported. (unit)
- **AC-5** Given the search "quiet", then only Notifications (quiet hours) and Sounds (quiet-hours sound options) stay selectable. (unit)
- **AC-6** Given demo mode, then Account shows "Demo org · read-only" and no sign-out control. (story)
- **AC-7** Every tab has a gallery story with snapshots in at least four built-in themes.
- **AC-8** Given no code font keys, then diffs render in the bundled JetBrains Mono at 12 px with 18 px rows and ligatures off; given `code_font_size = 40`, then the size is 16 and the clamp is reported; given `code_font = 3`, then the bundled font is used and the value is reported. (unit: `adoken-theme`; story `review-*`)
- **AC-9** Given `code_font = "Fira Code"` on a machine without it, then code renders in JetBrains Mono, the statusbar notice shows once, and the Code font section shows the warning. (unit: fallback order; story `settings-code-font-missing`)
- **AC-10** Given an open diff, when the code font size goes from 12 to 16 (Settings, `⌘=`, or an outside edit of `settings.toml`), then the next frame shows 24 px rows and the scroll position stays on the same line. (unit: row height; smoke run) *(Amended 2026-10-02 by 0025 FR-6: via Settings or an outside edit; `⌘=` no longer changes the size)*
- **AC-11** Given the theme picker and the code font writing `[appearance]` in turn, then both `theme` and the code font keys stay in the file. (shared-table writer)
- **AC-12** Given no `[sync]` table, then every kind syncs at spec 0002 FR-4's cadence (my PRs and pipelines 1 min, all PRs and work items 2 min, background ×5); given `all_prs = "10s"`, then it is 1 min and the clamp is reported; given `work_items = "4min"`, then it is 5 min and reported; given `my_prs = 30` or `background = "sometimes"`, then those keys fall back alone and are reported. (unit: `settings::sync_prefs`)
- **AC-13** Given a Sync tab edit, then the running scheduler uses the new cadence without a restart (`Control::Prefs`, replanned from each lane's last pass; unit: `adoken-sync` scheduler tests). (stories `settings-sync`, `settings-sync-paused`, `settings-sync-manual`)
- **AC-14 Account, az token** (FR-8; amended 2026-10-01, owner notes; accepted 2026-10-01). Given `account.json` for an `az` Review token created 2026-09-25 with `valid_to` 2026-12-24 and the five Review scopes, when the Account tab opens, then it shows "Created with Azure CLI", "Sep 25, 2026", "Expires Dec 24, 2026", access "Review", five permission rows in dialog order (Build, Code, Identity, Project and Team, Work Items) each "Granted" with its `vso.*` id and what it allows, a **Replace token…** button, and the mock server records zero requests. (unit + story `settings-account`)
- **AC-15 Account, pasted token** (FR-8; amended 2026-10-01, owner notes; accepted 2026-10-01). Given a pasted Review token without an expiry date, whose `vso.build` probe was missing and whose write scopes are untested, then the Account tab shows "Pasted", the created date, "No expiry date recorded", Build "Missing" with "Pipelines won't sync.", Code and Work Items "Not checked yet", and one summary line naming pipelines as off. Given 0001 FR-15 holds with `valid_to` yesterday, the token row reads "Expired on {date}". (unit + stories `settings-account-pasted`, `settings-account-rejected`)
- **AC-16 Replace from Settings** (FR-9; amended 2026-10-01, owner notes; accepted 2026-10-01). Given an `az` token and a signed-in fake `az`, when **Replace token…** is pressed (or `⌘P` › Replace token…), then the token row shows the new created and expiry dates with the success note and the old `authorizationId` is revoked (0001 AC-12). Given a pasted token, the onboarding sheet opens at screen 1 instead. Given demo mode, neither control exists. (integration + story)
- **AC-17 Group reviewers switch** (amended 2026-10-01, owner notes; accepted 2026-10-01). Given no `[review]` table, then **Show group reviewers** is off; toggling it writes `show_group_reviewers = true` to `[review]` and the PR list shows group reviewers on the next frame (0019 FR-15); searching "policy" keeps General selectable and shows the row; `show_group_reviewers = "yes"` loads as `false` and is reported. (unit + story `settings-general`)
- **AC-18 New tables load safely** (amended 2026-10-01, owner notes; accepted 2026-10-01). Given `[celebrations] step_states = "Review"`, `[tours] seen = ["pr-diff", "nope"]` and `[hotkey] summon = "a"`, then `step_states` is the default list and reported, `seen` keeps both ids and ignores "nope", `summon` is `"cmd-ctrl-alt-shift-a"` and reported, and every other key of those tables loads as written. (unit: `adoken-settings`)
- **AC-19 Account after a silent replacement** (FR-8; spec 0001 FR-20, Q-7; amended 2026-10-01, owner notes; accepted 2026-10-01). Given `account.json` for an `az` token with `replaced_automatically_at` 2026-12-17 and `valid_to` 2027-03-17, when the Account tab opens, then the token row reads "Replaced automatically on Dec 17, 2026 · Expires Mar 17, 2027" and the mock server records zero requests; after a manual **Replace token…** the row reads "Created with Azure CLI on {today}" again. (unit + story `settings-account-replaced`)

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| ⌘, to first painted frame | < 50 ms | debug build smoke run; the file is read after the first frame |
| Control change to next frame | < 16 ms | live apply is in-memory; disk write on the background executor |
| Cold start impact | 0 | `settings::init` registers globals only; the first stat runs 2 s after launch |
| Cold start impact of the bundled code font | < 1 ms | `code_font::init` hands three borrowed TTFs to the text system (no IO, no parse); `perf-cold-start` before/after |
| Installed-family scan | off the UI thread | once per session when the Code font list first shows; ≈ 1.3 s for ~200 families on a 2024 Mac |
| Opening the Account tab (amended 2026-10-01, owner notes; accepted 2026-10-01) | first frame < 50 ms, 0 requests | `account.json` is already loaded at launch; mock server request count |
