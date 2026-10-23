# 0010 — Zeron reuse, batch 2

| Field | Value |
| --- | --- |
| Status | Draft |
| Milestone | Per package (§3 table): v0.0.5 → v0.3. Nothing here blocks the current milestone. |
| Owner | product-manager (spec) · gpui-engineer (build) · release-manager (Z-11, Z-12) · ux-designer (Q-1, Z-16) |
| Crates | `crates/ui`, `crates/app` (`review/`, `settings/`, `shell/`), `crates/ado` (`data_dir`, Keychain), `scripts/`, `public/`, new `edge/` |
| Serves | Snappy (review speed, perf) · Less noise (consistent UI, isolated dev builds) |
| Depends on | `docs/zeron-reuse.md` (batch 1 audit), `docs/specs/0004-pr-review.md`, `docs/specs/0009-settings.md`, `docs/specs/0007-theming-backgrounds-sounds.md`, `docs/design/tokens.md`, `docs/release.md` |
| Design | `docs/design/tokens.md` (new `file_icon.*` tokens, Q-1), `docs/design/principles.md` (Windows click/drag rule, Z-14) |

> Rule: no code without an **Accepted** spec (AGENTS.md). Each package below lands together with the feature spec that owns its behavior. This spec owns *what to take from zeron and how to adapt it*; the owning spec keeps owning behavior, except for the two amendments called out in FR-8.

## 1. Problem

`docs/zeron-reuse.md` covered zeron's shell, theme, motion, palette, markdown, diff, updater and packaging. A second pass on 2026-09-25 found 16 smaller pieces that adoken will otherwise rebuild from scratch: the inline comment widgets PR review needs, an image viewer, file-type icons, a highlight cache, repo artwork, two popover behaviors, the settings page scaffold, the release-serving Worker, the site download button, isolated dev bundles, and three Windows notes. Rebuilding them costs weeks and re-learns bugs zeron already fixed (drag-eats-click on Windows, SVG raster sizing, atlas memory never freed).

## 2. Users and job to be done

- **Persona:** the builder (and later any contributor agent) implementing 0004, 0009 and the v0.1 release.
- **Job to be done:** "When I start a feature that zeron already solved, I want the exact source, the adaptation rules and a done-check, so I port it in hours without breaking adoken's token, attribution and perf rules."
- **Smallest delightful slice:** Z-1 (comment widgets) ported with stories, because it unblocks 0004's write path.

## 3. Work packages

Source for every package: [zeron](https://github.com/zeronsh/zeron) at commit `d1010657d69c9730f8020edc36cc2649b136b6fa` (MIT, Copyright (c) 2026 Wing). Line counts measured at that commit. Effort: **S** < 1 day, **M** 1–3 days, **L** > 3 days.

| ID | Zeron source (lines) | adoken target | Lands with | Effort |
| --- | --- | --- | --- | --- |
| Z-1 | `crates/ui/src/comment_ui.rs` (316) + `comments.rs` (422, model only) | `crates/ui/src/comment.rs`; draft model in `crates/app/src/review/` | 0004 new comment (v0.0.5) | M |
| Z-2 | `crates/ui/src/image_viewer.rs` (572) | `crates/ui/src/image_viewer.rs`; used by `crates/app/src/review/` | 0004 follow-up (v0.2, Q-2) | M |
| Z-3 | `crates/ui/src/image_media.rs` (426) | `crates/ui/src/image_media.rs`; used by Z-2, Z-6, `crates/app/src/background/` | with Z-2 or Z-6, whichever is first | S |
| Z-4 | `crates/ui/src/file_icons.rs` (513) + `file-icons.json` + `crates/ui/assets/file-icons/` | `crates/ui/src/file_icon.rs`, `assets/icons/symbols/` | v0.1 polish (0004 file tree, Cmd+P rows) | M |
| Z-5 | `crates/ui/src/syntax_cache.rs` (199) | `crates/diff` (next to highlighting) | 0004 syntax highlighting (v0.0.5) | S |
| Z-6 | `crates/ui/src/shell/project_icon.rs` (408) | `crates/ui/src/repo_icon.rs`; fetch in `crates/sync` low tier | v0.1 polish | S |
| Z-7 | `crates/ui/src/popover/hover_intent.rs` (258) | `crates/ui/src/popover/hover_intent.rs` | first nested menu (e.g. Snooze ▸) | S |
| Z-8 | `crates/ui/src/popover/contained.rs` (258) | `crates/ui/src/select.rs` placement | v0.1 (0009 selects) | S |
| Z-9 | `crates/ui/src/settings/widgets.rs` (1678, layout parts only) | `crates/ui/src/settings_page.rs`; `crates/app/src/settings/pages/*` refactor | v0.1 (0009) | M |
| Z-10 | `crates/ui/src/surface_chrome.rs` (46) | spacing constants in `crates/theme`, `toolbar()` in `crates/ui` | v0.1 | S |
| Z-11 | `.github/workflows/deploy.yml` (135) + `edge/src/index.ts` releases routes (≈30 of 430) | new `edge/` Worker + `.github/workflows/deploy.yml` | v0.1.0 release (update feed) | M |
| Z-12 | `apps/landing/public/downloads.js` (45) | `public/src/components/DownloadButton.astro` | v0.1.0 launch page | S |
| Z-13 | `scripts/run-macos-dev.sh` (68) | `scripts/run-dev.sh`; `ADOKEN_PROFILE` in `crates/ado` | now (dev tooling) | S |
| Z-14 | `docs/research/windows-click-drag-audit.md` (74) | rule in `docs/design/principles.md`; helper in `crates/ui` | v0.3 (first `on_drag` + click row, any platform) | S |
| Z-15 | `docs/reference/windows-development.md` (91) | `docs/windows-development.md` | v0.3 | S |
| Z-16 | `docs/adr/0002-separate-surface-preference-from-theme-selection.md` (5) | `docs/adr/0002-surface-preference.md` | v0.1 (ADR only) | S |

### Z-1 Inline comment widgets

- **Take:** `render_comment_adder` (gutter "+"), `render_comment_card`, `render_comment_edit`, `render_comment_draft` (fixed height, "so an open draft never fights the fold tween"), `CommentContentColumn`, the accent bar. From `comments.rs` take only `CommentSide` and the anchoring rule behind `cite_path` (an old-side comment on a renamed file cites the pre-rename path).
- **Drop:** `with_comments` / `extract_badge` (zeron folds comments into an agent prompt; adoken posts them to ADO), `uuid` ids, `ComposerInput`.
- **Adapt:** input → `adoken_ui::TextInput`; colors → `diff.comment` and existing tokens; the draft model carries what 0004 FR-6 needs to post (path, side, line, iteration). Posting, mentions and offline drafts stay owned by 0004 (FR-6, FR-8, FR-11).

### Z-2 Image viewer

- **Take:** `ImageView` geometry (fit-to-viewport capped at 1×, scale, pan), wheel zoom, pointer drag pan, pinch, fit reset.
- **Adapt:** image files in a PR (png, jpg, gif, webp, svg) render as old | new, each its own viewer. Keyboard: `0` fit, `+` / `-` zoom, arrow keys pan (keyboard-first rule). Blobs are fetched only when the file is expanded.

### Z-3 Bounded image decoding

- **Take:** pixel cap for previews, max raster side 4096, SVG raster sizing from viewport × DPI (accounting for GPUI rasterizing SVG at 2×), `release_media` → `ImageSource::evict` on close.
- **Adapt:** background pipeline (`crates/app/src/background/pipeline.rs`) also evicts replaced textures.

### Z-4 File-type icons

- **Take:** resolution order: exact file name → longest compound extension (`.d.ts` before `.ts`) → language / MIME → generic; folder icons; the Symbols manifest.
- **Assets:** Symbols by miguelsolorio, MIT, `github.com/miguelsolorio/vscode-symbols` @ `296ef1b62287fb2315cb5651e552e09e8c8e1de8`. The full set at zeron's commit is 354 SVGs, 1.38 MB, 295 distinct colors. Ship only icons the manifest references.
- **Adapt:** colors go through tokens (Q-1). Lucide stays the UI icon set (tokens.md §2.8); Symbols is only for file identity.
- **Attribution:** `LICENSE.symbols` copied into the bundle by `scripts/bundle.sh` (next to `lucide-LICENSE`) and a `THIRD_PARTY_NOTICES.md` entry.

### Z-5 Highlight cache

- **Take:** key = (language, SHA-256 of content, query generation); caps 96 documents / 24 MB with LRU eviction; hit/miss stats; colors kept **outside** the cache so a theme change recolors without re-parsing.
- **Adapt:** lives beside adoken's tree-sitter highlighting when 0004 adds it (no tree-sitter dependency exists yet). `sha2` is already a workspace dependency.

### Z-6 Repo artwork

- **Take:** monogram fallback (first letter, stable tone from FNV-1a of a seed), candidate icon path list, decode via Z-3.
- **Adapt:** adoken has no local checkout, so artwork comes from ADO: list the repo root at the default branch (`recursionLevel=OneLevel`), pick the first candidate present (also look in `public/` if it exists), fetch that one blob. Max 3 requests per repo, refreshed at most every 7 days, 404s cached, on the sync low tier (0002). Monogram tones come from tokens, not zeron's hex palette (Q-1). Seed = repo id, so renames keep the color.

### Z-7 Hover intent

- **Take:** `HoverIntent<K>` (open now vs defer; 300 ms grace; safe corridor from trigger to child on either side; `cancel` on click/keyboard/dismiss) and its unit tests.
- **Implemented 2026-10-01 with accepted spec 0014:** `crates/ui/src/popover/hover_intent.rs`; the pins peek adds a 150 ms dwell, uses theme tokens for grace/slop, and has keyboard entry. Source path and pinned commit are recorded in `THIRD_PARTY_NOTICES.md`.

### Z-8 Contained dropdown placement

- **Take:** the menu opens on the side of the trigger with room instead of sliding over it; max height = (container height − trigger height) / 2 − 6 px, capped at 320 px; scroll inside.
- **Adapt:** apply to `adoken_ui::Select` inside the Settings window and dialogs.

### Z-9 Settings page scaffold

- **Take (layout only):** `page_column` (centered max-width measure), `page_header`, `page_subtitle`, `section_label`, `section` / `section_card`, `card_row` with hairline dividers, `row_title`, `meta_line`, `PageScroll` + `scroll_faded`.
- **Drop:** zeron's `Select`, switch, option card and buttons (adoken has its own in `crates/ui`), tab selection animation, `error_strip` / `warning_strip` (adoken uses `Notice`).
- **Adapt:** zeron's literals (680 px measure, 12 px radius, `wash(0.045)` fill, `border.opacity(0.6)` dividers, 13/12 px type) become theme tokens / constants. Refactor every page under `crates/app/src/settings/pages/` onto it.

### Z-10 Toolbar metrics

- **Take:** header height = titlebar height, control 24 px, radius 6, icon 14, gap 4, edge inset 8; `toolbar()` and `input()` builders.
- **Adapt:** constants live in `crates/theme` with the other spacing tokens; sidebar, inbox and review pane headers use them.

### Z-11 Release-serving Worker

- **Take:** `GET` / `HEAD /releases/<key>` from an R2 bucket, `..` rejected with 400, missing → 404; `*.txt` / `*.json` (`latest.txt`, `manifest.json`) served with short cache because they flip on release; artifacts immutable by name with long cache. The deploy workflow runs only when `edge/` or the workflow changed, runs typecheck + tests, and skips cleanly when `CLOUDFLARE_API_TOKEN` is not set.
- **Drop:** every Durable Object, auth, device, chat and preview route; `install.sh`.
- **Adapt:** new `edge/` at the repo root (Q-4), domain e.g. `releases.adoken.dev`. It carries no ADO data (roadmap principle "no adoken server in the ADO data path"). It is the server half of the updater (`crates/update`, batch 1) and the later license refresh (`docs/pricing.md`).

### Z-12 Download button

- **Take:** platform detection (UA-CH platform first, mobile excluded, Linux arm vs x64), fallback version baked at build so the link works without the fetch, `latest.txt` fetch with `credentials: "omit"`, semver guard (never downgrade below the fallback), aria-label and title per platform.
- **Adapt:** Astro component in `public/`. Until v0.3, non-macOS visitors see the macOS link plus "Windows: coming in v0.3" (no dead link); mobile shows "Download on your Mac".

### Z-13 Isolated dev bundle

- **Take:** separate bundle (`adoken Dev.app`, id `dev.adoken.app.dev`) so macOS permissions (notifications, Keychain prompts) never collide with an installed adoken; stable signing with an Apple Development identity when present, ad hoc otherwise; `open -W --env …` so LaunchServices owns the process.
- **Adapt:** reuse `scripts/bundle.sh` for assembly (icon, licenses) instead of duplicating it. Add `ADOKEN_PROFILE=<name>` to `crates/ado`: data dir becomes `<data_dir>/profiles/<name>/` and the Keychain service `dev.adoken.pat.<name>`. The dev script sets `ADOKEN_PROFILE=dev` by default. Q-5.

### Z-14 Windows click vs drag

- **Take:** GPUI starts a drag after more than 2 px of movement and then drops the pending click, so rows with both `on_click` and `on_drag` on one hitbox lose clicks to pointer jitter (observed on Windows). zeron's fix: `click_activation_drag_enabled()` returns false on Windows for those rows.
- **Adapt:** adoken has no `on_drag` call today. Add the rule to `docs/design/principles.md` now; add the helper and its regression test with the first row that is both clickable and draggable.

### Z-15 Windows build guide

- **Adapt:** keep MSVC + LLVM (ARM64), `GPUI_FXC_PATH`, data dir, and the Acrylic transparency requirement. Drop zeron's engine, IPC, `cmd.exe` shim and Job Object sections.

### Z-16 Surface preference ADR

- **Decision to record:** frost/glass is a user preference independent of the theme: `[appearance] surface = "auto" | "frosted" | "opaque"`. `auto` keeps today's rule (frost overlays only while a custom background shows, tokens.md §10). Forced frost derives tints from the theme's own surface roles; diff code surfaces stay opaque (0007 FR-6). ux-designer owns the ADR text.

## 4. Functional requirements

- **FR-1 Attribution.** Every adapted file starts with: "Adapted from zeron `<path>` at d1010657d69c9730f8020edc36cc2649b136b6fa, MIT, Copyright (c) 2026 Wing. See THIRD_PARTY_NOTICES.md." (same form as `crates/app/src/background/frost.rs`), and gets a `THIRD_PARTY_NOTICES.md` entry with the path.
- **FR-2 Tokens only.** No hex literal or raw spacing number from zeron survives outside `crates/theme`. New tokens are added to `docs/design/tokens.md` by ux-designer before the package merges.
- **FR-3 No zeron coupling.** No dependency on zeron crates; zeron-only types (`ComposerInput`, `WorkspaceFilesClient`, `zeron_proto`, RPC clients, zeron `Theme`) are replaced, not wrapped.
- **FR-4 Stories and snapshots.** Every UI component from this spec gets a gallery story and a snapshot per built-in theme (AGENTS.md).
- **FR-5 zui-only APIs** (`ImageSource::evict`, backdrop blur) are called only from `crates/ui`, per `docs/zeron-reuse.md` risk 1.
- **FR-6 Tests travel with logic.** zeron unit tests for pure logic (geometry, placement, hover intent, icon resolution, cache eviction) are ported when they defend behavior; tests that pin zeron wiring are not.
- **FR-7 Keyboard-first.** Every ported interaction that zeron drives by pointer gets a keyboard path (Z-1 `c`, Z-2 `0` / `+` / `-` / arrows, Z-7 arrow-key submenu).
- **FR-8 Amendments to other specs.** Z-2 amends 0004 §3 step 3 (image files render in the viewer instead of "collapsed binary"). Z-6 adds repo artwork fetching to the 0002 low tier. Every other package changes no existing behavior.
- **FR-9 Profiles isolate secrets.** With `ADOKEN_PROFILE` set, the app neither reads nor writes the default data dir or the default Keychain service.

## 5. ADO API surface and limits

| Purpose | Endpoint (api-version 7.1) | Scope | Cost / limit notes |
| --- | --- | --- | --- |
| Image blob in a PR (Z-2) | `GET {org}/{project}/_apis/git/repositories/{repo}/blobs/{objectId}` | `vso.code` | Only on explicit expand; size cap (Z-3) enforced before decode. |
| Repo root listing (Z-6) | `GET …/repositories/{repo}/items?scopePath=/&recursionLevel=OneLevel&versionDescriptor.version={default branch}` | `vso.code` | ≤ 3 requests per repo per 7 days, low tier. |
| Repo artwork blob (Z-6) | `GET …/repositories/{repo}/items?path={candidate}&$format=octetStream` | `vso.code` | One blob per repo per refresh; 404 cached 7 days. |

- **Rate limits:** Z-6 on a 200-repo org costs ≤ 600 light requests per week, spread by the 0002 governor; zero when the governor is above its low-tier threshold.
- **No push:** artwork refresh is bounded by the 7-day window; nothing here needs freshness.
- **PAT scopes:** `vso.code` only, already in both access levels (0001 §5.2).

## 6. Acceptance criteria

- **AC-1 (Z-1)** Gallery stories `comment_adder`, `comment_card`, `comment_edit`, `comment_draft` exist and pass snapshots in every built-in theme. A unit test: an old-side comment on a file renamed `a.rs` → `b.rs` cites `a.rs`. In a diff story, opening a draft leaves the y-position of every row above it unchanged.
- **AC-2 (Z-2)** Given a mock PR changing `logo.png` (200×200 → 400×400), the mock server sees zero blob requests until the file is expanded, then exactly two. Both images open fitted. `+` zooms, `0` restores fit; unit tests cover fit ≤ 1×, zoom around the pointer, and pan clamping.
- **AC-3 (Z-3)** A 12,000 × 12,000 PNG decodes to ≤ 4096 px on its longest side. SVG raster size follows viewport × DPI with GPUI's 2× factor (unit, ported). Scripted manual check: open and close a PR with 50 two-megapixel images 5 times; process footprint after the 5th close is within 20 MiB of the 1st close.
- **AC-4 (Z-4)** Resolution unit tests: `Cargo.toml` (exact name) beats `.toml`; `x.d.ts` resolves to the `.d.ts` icon, not `.ts`; unknown `x.zzz` gets the generic icon. Overriding one `file_icon.*` token in a user theme changes the rendered icon color (snapshot). `target/bundle/adoken.app/Contents/Resources/licenses/` contains the Symbols license after `scripts/bundle.sh`. The bundle grows by ≤ 1.5 MB.
- **AC-5 (Z-5)** With 10 highlighted files open, switching theme runs zero parses (parse counter). Unit: the 97th distinct document evicts the least recent; a document pushing retained bytes over 24 MB evicts until under. The same content opened under two paths parses once.
- **AC-6 (Z-6)** A repo with no artwork renders a monogram whose tone is identical across restarts and after a repo rename (seed = id). With a mock repo exposing `public/favicon.svg`, the icon appears after one low-tier cycle with ≤ 3 requests; a second cycle within 7 days makes zero requests.
- **AC-7 (Z-7)** Ported unit tests pass: moving from trigger toward the open child through the corridor keeps it open; leaving the corridor dismisses; a click or key press cancels a pending open.
- **AC-8 (Z-8)** In the Settings window, a `Select` whose trigger sits 80 px above the bottom edge opens upward; no menu ever extends past the window bounds (story at three trigger positions).
- **AC-9 (Z-9)** Every page in `crates/app/src/settings/pages/` renders through the scaffold; no page file sets its own max width or row padding. Settings snapshots are re-reviewed and committed in the same change.
- **AC-10 (Z-10)** Sidebar, inbox and review pane headers share one height and control size from `crates/theme` (story shows them side by side).
- **AC-11 (Z-11)** Worker tests (vitest): `GET /releases/latest.txt` → 200 with short cache; `GET /releases/adoken-0.1.0-macos-arm64.dmg` → 200 with immutable cache; `/releases/../x` → 400; missing key → 404; `HEAD` has no body. The workflow skips (success) without `CLOUDFLARE_API_TOKEN` and does not run when no file under `edge/` changed.
- **AC-12 (Z-12)** With `latest.txt` = `0.1.3` and fallback `0.1.2`, macOS visitors get the 0.1.3 dmg link; with `latest.txt` = `0.1.1` they keep 0.1.2; with the fetch blocked the fallback link works; with JavaScript disabled the link still downloads the fallback; on an iPhone UA no platform link is rewritten.
- **AC-13 (Z-13)** With the installed adoken connected, running `scripts/run-dev.sh` opens `adoken Dev` at Welcome. After connecting a different org in the dev app, the installed app's `account.json` bytes and its Keychain item are unchanged. `ADOKEN_PROFILE` with `/` or `..` is rejected at startup with a clear error.
- **AC-14 (Z-14)** When the first click+drag row lands: on Windows, a mouse-down, 4 px move, mouse-up activates the row and leaves no active drag; on macOS the same row can still be dragged.
- **AC-15 (Z-15)** `docs/windows-development.md` exists and roadmap v0.3 links it.
- **AC-16 (Z-16)** `docs/adr/0002-surface-preference.md` exists with status Proposed and names the setting key, its three values and the `auto` rule.

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Open a comment draft | frame < 16 ms | frame recorder in the diff story |
| Image decode (≤ 4 MP) | off the UI thread; first paint < 150 ms after the blob arrives | timer around decode + first frame |
| Theme switch with cached highlights | < 16 ms, zero parses | parse counter + frame recorder |
| File icon lookup | manifest parsed lazily after first frame; zero cold-start cost | cold-start timer unchanged (±5 ms) |
| Settings page switch after Z-9 | < 16 ms | frame recorder |

Global rules (AGENTS.md) apply: frame < 16 ms, Cmd+P < 50 ms on 100k items, cold start < 500 ms.

## 8. Success signal (telemetry-free)

- Each package merges in its estimated effort or less (tracked in the CHANGELOG entry).
- No bug filed for a problem zeron already fixed (Windows lost clicks, SVG blur, image memory growth).
- Build-in-public: the file-icon + repo-artwork inbox screenshot becomes the v0.1 "polish" post.

## 9. Risks

- **Symbols colors vs the token rule.** 295 distinct colors cannot each be a token (Q-1).
- **Bundle size.** 1.38 MB of raw SVG before pruning; AC-4 caps growth at 1.5 MB.
- **zui drift.** Z-3 depends on `ImageSource::evict`, a zui API; a zui bump can break it (batch 1 risk 1).
- **Artwork noise.** Many repos have no favicon; cached 404s keep the cost bounded, but the first low-tier cycle on a large org issues up to 3 requests per repo.
- **Profile misuse.** `ADOKEN_PROFILE` is a developer knob; if a user sets it they see an empty app. Show the active profile name in the titlebar when set.

## 10. Open questions (for the user)

- **Q-1** Symbols icons carry 295 colors; AGENTS.md forbids hard-coded colors. **Recommended:** quantize each SVG color at load time to the nearest of ~9 hue tokens (`file_icon.slate|blue|red|orange|pink|teal|violet|green|amber`, low chroma → slate). Themes can override them, and the same tokens color repo monograms (Z-6). Alternative: a documented exception treating file icons as content, like images.
- **Q-2** When does the image viewer (Z-2) ship? **Recommended:** v0.2, with iteration compare. v0.1 keeps 0004's "collapsed binary + Open in browser".
- **Q-3** Fetch repo artwork from ADO by default (Z-6)? **Recommended:** yes, on the low tier with the 3-request / 7-day cap; a Settings › General switch turns it off (monograms only).
- **Q-4** Where do releases live (Z-11)? **Recommended:** a small Worker in `edge/` on `releases.adoken.dev` backed by R2, because the license refresh (`docs/pricing.md`) will need a Worker anyway. Alternative: an R2 public bucket on a custom domain with Cache Rules and no code.
- **Q-5** Dev isolation knob (Z-13)? **Recommended:** `ADOKEN_PROFILE=<name>` (isolates both data dir and Keychain service with one variable) rather than a raw `ADOKEN_DATA_DIR`, which would leave the Keychain shared. Spec 0011 FR-1 (profile root, Reset scope) builds on this choice; 0011 is deferred to Later (user decision 2026-09-30), so Z-13 no longer gates any v0.1 work.

## 11. Out of scope

Reviewed on 2026-09-25 and not selected. Recorded here so they are not re-audited:

- `crates/mcp` (MCP server exposing local ADO data to coding agents) → candidate for `docs/ideas.md`.
- `appshots` (window screenshot capture to attach to bugs).
- `.github/workflows/release.yml`, `ui-tests.yml`, `windows.yml` (release-manager owns CI in `docs/release.md`).
- `motion/windows_pulse.rs`, the macOS profiler suite (`scripts/resource-profile.mjs`, `macos-resource-stat.c`, `macos-metal-memory-probe.m`, `macos-profile-window.swift`) and the performance docs.
- Sound-synthesis scripts and `docs/sound-design/` (adoken synthesizes in `crates/sound/src/synth.rs`).
- `files/search.rs`, `badges.rs`, `attachments.rs`, `rail.rs`, tabs, sidebar pins/sections, accounts/devices settings (agent-product specific).
- The display-freeze fix after sleep (`docs/macos-frame-recovery.md`): already in the pinned zui `18a89af04cfe079d55275b111ac349d5640d9cda`.

## 12. References

- zeron (MIT): https://github.com/zeronsh/zeron at `d1010657d69c9730f8020edc36cc2649b136b6fa`; batch 1 audit `docs/zeron-reuse.md`.
- Symbols icon theme (MIT): https://github.com/miguelsolorio/vscode-symbols/tree/296ef1b62287fb2315cb5651e552e09e8c8e1de8
- Git Items — Get / List: https://learn.microsoft.com/en-us/rest/api/azure/devops/git/items?view=azure-devops-rest-7.1
- Git Blobs — Get Blob: https://learn.microsoft.com/en-us/rest/api/azure/devops/git/blobs/get-blob?view=azure-devops-rest-7.1
- Cloudflare R2 Workers API: https://developers.cloudflare.com/r2/api/workers/workers-api-usage/
