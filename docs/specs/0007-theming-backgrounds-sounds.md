# 0007 — Themes, custom backgrounds, and sounds

| Field | Value |
| --- | --- |
| Status | Draft |
| Milestone | v0.0.1 (token plumbing + built-in themes) → v0.1.0 (backgrounds, user themes, sounds, Ken layer) |
| Owner | product-manager (spec) · gpui-engineer (build) · ux-designer (tokens, assets) |
| Crates | `crates/theme` (tokens, loader, validation), `crates/ui` (consumers), `crates/app` (settings, background compositor, audio) |
| Serves | Snappy (the app feels fast and alive, not heavy), Less noise (sounds distinguish what matters without looking) |
| Depends on | `docs/specs/0005-notification-center.md` (sound gating by priority, quiet hours) |
| Design | `docs/design/tokens.md` (token list, `<id>.adoken-theme.toml` format, background config), `docs/design/sound.md` (event → sound map, volumes), `docs/design/ken.md` (arcade layer), `docs/design/principles.md` (a11y) |

## 1. Problem

Developer tools people love (terminals, editors, Raycast) are personal: themes, wallpapers, a satisfying sound when something lands. Azure DevOps is a grey enterprise web page you can't make your own. Personalization is also a build-in-public hook ("my ADO client with my wallpaper"). The risk is that eye candy costs frame time, contrast, or battery — which would break the "snappy" promise.

## 2. Users and job to be done

- **Persona:** dev who customizes their tools; streamers/build-in-public folks who show their setup.
- **JTBD:** "When I spend my day in adoken, I want it to look and sound like mine, without it ever getting slower or harder to read."
- **Smallest delightful slice:** great built-in themes, drop-in image background with blur/dim that stays readable, and an opt-in sound pack tied to the events that matter.

## 3. UX flow

1. **Settings → Appearance:** one theme list: a search field, "Import VS Code theme…", and a grid of every built-in and user theme as a card with a live miniature. Hovering or arrow keys only move the highlight; a click or `Enter` applies the theme at once and saves it as the one `[appearance] theme` (user decision 2026-09-30: no hover/selection preview, no Dark/Light filter, no "Match system" light/dark pairing, no following the macOS appearance, no ⇧⌘L light/dark toggle). Until a theme is chosen, adoken starts in Dojo Night or Dojo Day to match macOS at launch. Settings files with the old `mode` / `light_theme` / `dark_theme` keys load as the theme that was showing and are rewritten on the next pick (`docs/design/tokens.md` §9.4). *(Amended 2026-10-02 by 0025 FR-11: no "Import VS Code theme…"; user theme files still load and hot-reload)*
2. **Background:** "Choose image…" (png, jpg, webp, heic) or drag an image onto the window. Controls: fit (cover / contain / tile), position (focal point pick), blur (0–40), dim (0–90%), and scope (whole app / inbox only / everywhere except diffs). Live preview on the real UI.
3. **Themes folder:** "Open themes folder" reveals `<data_dir>/themes/` (`<data_dir>` per `docs/adr/0001-local-store.md`, i.e. `~/Library/Application Support/dev.adoken` on macOS). Dropping a `<id>.adoken-theme.toml` there makes it appear in the picker within 1 s; edits hot-reload. Invalid files show a precise error in the picker (file, line, token) and never crash or partially apply.
4. **Sounds:** master toggle (default **off**; offered on the last onboarding screen, 0001), master volume, per-event toggle + preview button, "Play sounds only for P0/P1" option. Mapping per `docs/design/sound.md`. *(Amended 2026-10-02 by 0025 FR-13: Sounds are on/off and master volume only; no per-event toggles or P0/P1 option)*
5. **Ken layer** (default **off**): playful arcade touches from `docs/design/ken.md` — review combo counter, merge "K.O." moment, playful empty states. Original assets only (no Capcom / Street Fighter names, sprites, or audio).
   Celebrations for finished work (PR merged, work item moved to review or done), with Ken on or off: `docs/specs/0022-celebrations.md` (amended 2026-10-01, owner notes; accepted 2026-10-01).
6. **Motion:** follows macOS Reduce Motion; app-level override (System / Reduced / Full).

### States

| State | Behavior |
| --- | --- |
| Background file moved/deleted | Falls back to theme background; one-time notice; setting kept for re-pick. |
| Huge image (e.g. 12k × 8k) | Downscaled on import to 2× the largest display's size; original not modified. |
| Invalid theme file | Listed with error; previous theme stays active. |
| Audio device unavailable | Sounds silently skipped; no error spam. |
| Quiet hours (0005) | No sounds, regardless of settings. |

## 4. Functional requirements

- **FR-1 Tokens only.** Every color, spacing, radius, font size, motion duration/easing, and sound id used by UI comes from theme tokens (AGENTS.md). A CI lint forbids color literals outside `crates/theme`.
- **FR-2 Built-in themes (v0.1):** at least 4 — one light, one dark, one high-contrast, one signature adoken theme — all authored in the same `<id>.adoken-theme.toml` format and loaded by the same loader as user themes.
- **FR-3 Validation.** On load: schema check, missing tokens inherit from the base theme named in `extends` (or the default), and **automated contrast checks**: body text ≥ 4.5:1, large text/icons ≥ 3:1 against their resolved surfaces (WCAG AA). Built-ins must pass; user themes that fail get a warning badge but still load.
- **FR-4 Backgrounds.** Image decoded, downscaled, blurred, and composited **once** per (image, window size bucket, blur) and cached as a texture; resizing re-renders debounced (150 ms) with the stale texture scaled meanwhile. No per-frame blur.
- **FR-5 Readability guard.** With a background active, surfaces use the theme's translucent/frosted surface tokens, and adoken computes the background's luminance under each major region; if text contrast would drop below AA, the effective dim is raised automatically (the user's slider shows the minimum enforced value).
- **FR-6 Scope.** Background never renders behind diff code (FR default "everywhere except diffs") unless the user opts in; diffs always keep an opaque code surface.
- **FR-7 Sounds.** Event → sound mapping from `docs/design/sound.md`. Playback: low-latency, preloaded after first enable, mixed at master volume; at most one sound per 2 s (coalesced) so bursts don't chatter; gated by 0005 (priority delivery, quiet hours, windows).
- **FR-8 Reduce motion.** Every animation token has a reduced variant (crossfade ≤ 150 ms or none). The Ken layer's motion also obeys it.
- **FR-9 Persistence.** Selected theme, background config, sound settings in the TOML settings file (not the DB, per `docs/adr/0001-local-store.md`); background image copied into `<data_dir>/backgrounds/` (so the original can move).
- **FR-10 Snapshots.** Every UI component has a gallery story and a snapshot per built-in theme (AGENTS.md workflow).
- **FR-11 Sound packs (v0.1).** Two built-in, original packs: one playful (Ken flavor) and one subtle/minimal, both covering every event in `docs/design/sound.md`. Pack choice is a setting; the playful pack is not tied to the Ken layer toggle. User sound packs are v0.2 (Q-4). *(Amended 2026-10-02 by 0025 FR-13: one pack, Dojo, and no pack setting; the Arcade sounds play only for Ken events while Ken's arcade mode is on)*

## 5. ADO API surface and limits

- None. Purely local.
- Rate limits / push / PAT scopes: not applicable.

## 6. Acceptance criteria

- **AC-1** Grep-based CI check finds zero hex/rgb color literals in `crates/ui` and `crates/app`.
- **AC-2** All 4 built-in themes pass the automated AA contrast checks for every text/surface token pair defined in `docs/design/tokens.md`.
- **AC-3** Dropping a valid theme file into the themes folder makes it selectable within 1 s; editing a token value re-renders the app within 1 s without restart.
- **AC-4** A theme file with a malformed color on line 12 shows an error naming the file, line 12, and the token; the active theme is unchanged.
- **AC-5** With a 6000 × 4000 background, blur 30, dim 40%, scrolling the inbox and a 20k-line diff keeps every frame < 16 ms on an M1-class Mac.
- **AC-6** Background texture memory ≤ 64 MB for a 5K display.
- **AC-7** With a pure-white background image and a dark theme, the readability guard raises effective dim until body text measures ≥ 4.5:1; the dim slider reflects the enforced minimum.
- **AC-8** Diff code surfaces are opaque with default scope; the background is not visible behind code lines (pixel sample in snapshot test).
- **AC-9** Sounds are off after fresh install; enabling in onboarding persists; with sounds on, a P1 mention plays its mapped sound within 50 ms of the banner; during quiet hours nothing plays.
- **AC-10** 5 sound-triggering events within 1 s produce exactly 1 sound.
- **AC-11** With macOS Reduce Motion on, no animation token resolves to a translate/scale transition (token dump test); the Ken merge moment renders its reduced variant.
- **AC-12** Deleting the original background file after import doesn't change the rendered background (imported copy is used).
- **AC-13** Snapshot tests exist and pass for every gallery story × every built-in theme.
- **AC-14** In Settings › Appearance, moving the pointer over any theme card changes nothing; clicking a card (or `Enter` on the highlighted one) applies it immediately, it stays applied after the pointer leaves the grid and after a restart, and `settings.toml` then holds `[appearance] theme = "<id>"` with every other key kept. A file with `mode = "system"` and a light/dark pair opens in the theme matching the current macOS appearance without an error.

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Theme switch | next frame (< 16 ms) after tokens resolved; resolve < 5 ms | UI trace |
| Background import (6000 × 4000 JPEG) | < 400 ms off main thread; UI stays at 60 fps | Bench + frame recorder |
| Resize with background | no frame > 16 ms; re-blur within 300 ms of resize end | Frame recorder |
| Background GPU memory | ≤ 64 MB | Metal capture |
| Sound latency | < 50 ms trigger → audible | Audio loopback test |
| Cold start impact | background + theme add < 30 ms to cold start | Launch timer A/B |
| Idle CPU with static background | no increase vs no background | Activity Monitor |

## 8. Success signal (telemetry-free)

- Build-in-public: users post screenshots of their adoken setup (wallpaper + theme). Count of such posts in the first month of v0.1 is the signal; target ≥ 20.
- Dogfood: builder never turns the background off "because it's slow".
- Zero accessibility complaints about unreadable text with backgrounds.

## 9. Risks

- **IP risk with anime wallpapers:** adoken ships only original assets; user-chosen images stay local and are never uploaded. Marketing must not use third-party copyrighted wallpapers (social-writer / landing-page).
- **Translucency cost on older Intel Macs:** fallback to opaque surfaces if frame budget is missed for 60 consecutive frames (auto "performance mode", with a notice).
- **Theme format churn:** the file format is public once users author files; version it (`format = 1`) from day one.

## 10. Open questions — resolved 2026-09-25

All recommendations accepted by the user.

- **Q-1** VS Code theme import (reusing the zeron importer with attribution) in v0.1? **Resolved:** no, v0.2. v0.1 ships the native `.adoken-theme.toml` format and 4 built-ins. *Update:* the importer landed early (Settings › Appearance › "Import VS Code theme…", `adoken_theme::vscode`); it writes a native theme file into the themes folder. *(Amended 2026-10-02 by 0025 FR-11: the importer and its command are removed)*
- **Q-2** Video / animated backgrounds? **Resolved:** v0.2, behind a "may use more battery" note, paused when the window is unfocused.
- **Q-3** How many sound packs in v0.1? **Resolved:** two original packs — one playful, one subtle (FR-11). *(Amended 2026-10-02 by 0025 FR-13: one pack (Dojo); Arcade sounds are Ken-only)*
- **Q-4** User sound packs (drop-in folder) in v0.1? **Resolved:** no, v0.2, using the same loader pattern as themes.

## 11. Out of scope

- Theme marketplace / community gallery (`docs/ideas.md`).
- Per-project themes (`docs/ideas.md`).
- Custom fonts beyond the theme's font tokens.

## 12. References

- WCAG 2.2 contrast minimum (1.4.3) and non-text contrast (1.4.11): https://www.w3.org/TR/WCAG22/#contrast-minimum
- Reference implementation for theme library and VS Code import: zeron `crates/theme` (https://github.com/zeronsh/zeron, MIT, @winglee) — reuse requires source path + commit in `THIRD_PARTY_NOTICES.md` (AGENTS.md).
