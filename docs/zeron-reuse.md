# Zeron reuse audit

Reference: [zeron](https://github.com/zeronsh/zeron) at commit `d1010657d69c9730f8020edc36cc2649b136b6fa`. Zeron is MIT, Copyright (c) 2026 Wing. It is a GPUI desktop app for coding agents and shares a lot of desktop plumbing with adoken. This doc records what adoken takes from it, how, and at what cost.

Verdicts:
- **depend**: use the same upstream crate or pin.
- **copy**: take the code nearly verbatim.
- **adapt**: take the structure or code and reshape it to adoken's contracts.
- **skip**: don't use it.

Every copy or adapt goes into `THIRD_PARTY_NOTICES.md` with the source path and commit (AGENTS.md).

Effort: **S** < 1 day, **M** 1–3 days, **L** > 3 days, counted from when the owning spec is accepted.

## Decision: GPUI source

adoken pins the same GPUI as zeron: `zeronsh/zui` at rev `18a89af04cfe079d55275b111ac349d5640d9cda`, crates `gpui` and `gpui_platform`, features `font-kit`, `runtime_shaders`, `wayland`, `x11` (root `Cargo.toml`). zui is Zed's GPUI (upstream `f14fea9`) pulled out into its own repo, plus zeron's patches.

Why zui and not upstream Zed or crates.io `gpui`:
1. **Reuse needs the same API.** Most of what is worth taking from zeron (frost, edge fade, window vibrancy, `ImageSource::evict`, headless snapshot rendering) calls APIs that only exist in zui or match its revision exactly. On a different GPUI, each port turns into a rewrite.
2. **Custom backgrounds and glass.** zui adds `BackdropBlur` (Metal, Direct3D and wgpu paths), per-primitive `EdgeFade`, a fix for macOS 26 `UnderWindowBackground` vibrancy, and correct destination alpha on transparent windows. Upstream has none of these. They matter for the "themes + custom backgrounds" v0.1 scope.
3. **Small checkout.** zui is a standalone workspace, not the whole Zed monorepo.

`runtime_shaders` compiles Metal shaders at startup, so a build needs only the Command Line Tools and not a full Xcode `metal` compiler. This machine has CLT only. See the cold-start risk below.

Verified: `cargo build`, `cargo test`, and a launch of `cargo run -p adoken` all work on macOS 26.7 (arm64) with rustc 1.97.1. zui's own toolchain file says 1.95. adoken tracks `stable` and declares `rust-version = "1.95"`.

## Component table

| Component | Zeron path | Verdict | Effort | Notes |
| --- | --- | --- | --- | --- |
| GPUI fork | `Cargo.toml` (`gpui`, `gpui_platform`) | **depend** (done) | S | Same rev and features. The dev profile that optimizes dependencies (`opt-level = 2`) is copied as well. |
| `gpui_tokio` bridge | `Cargo.toml`, `crates/ui/src/lib.rs` `run_app` | **depend** (declared) | S | Declared in the root `Cargo.toml` with the same zui rev. `crates/ado` (onboarding, spec 0001) runs on Tokio (reqwest, `tokio::process`); the app spawns its futures with `gpui_tokio::Tokio::spawn`. Zeron owns a multi-thread Tokio runtime and passes its handle to GPUI. |
| Theme domain model | `crates/theme/src/lib.rs` | **adapt** (done) | S | `Color` (hex, blend, WCAG contrast), the variant/family/registry split, fallback-per-appearance `resolve`, and contrast issues that warn rather than block. adoken swaps zeron's struct-of-colors for the macro-generated `ColorTokens` from `docs/design/tokens.md`, with TOML files, aliases and §9.3 rules. |
| Built-in palettes | `crates/theme/src/builtins.rs` | **skip** | – | 30 variants adapted from VS Code themes, each needing its own attribution. adoken ships Dojo Night and Dojo Day (`crates/theme/themes/adoken.adoken-theme.toml`). |
| VS Code theme importer | `crates/theme/src/vscode.rs`, `src/bin/zeron-theme-import.rs` | **adapt** (v0.2+) | M | JSONC, `include` chains and TextMate scope mapping, plus a hardening pass. Could compile VS Code themes into `.adoken-theme.toml`. Not in v0.1 scope. |
| Theme library | `crates/theme/src/library.rs`, `crates/ui/src/theme_library.rs` | **skip** | – | Zeron keeps imported and linked themes in one JSON library. adoken's contract is one file per theme in `<config>/themes/` with hot reload (tokens.md §9.1). |
| GPUI theme bridge | `crates/ui/src/theme.rs` | **adapt** (partly done) | M | adoken's `adoken_ui::Theme` is a much smaller global that converts tokens to `Hsla` once per install. Still to take: `oklch_to_srgb`/`rgb_to_hsl` (~100 lines) for accent-preset derivation and contrast enforcement (tokens.md §3, §9.3). Skip the `ink`/`hairline`/`wash` alpha helpers: adoken expresses those as tokens. |
| Light/dark following | `crates/ui/src/appearance.rs` | **adapt** | S | `AppearanceMode` (system, light or dark), the `observe_window_appearance` subscription, and `refresh_windows` rather than `notify`. `crates/app` already reads the system appearance at launch; live following arrives with settings. |
| Frosted overlays | `crates/ui/src/frost.rs` | **adapt** (with the palette) | S | Wraps a popover in a single scene layer with `BackdropBlur` painted first. Note: tokens.md §10.2 says "GPUI has no backdrop blur". That is true upstream, but zui has it, and UX should revisit the ≥ 0.9 overlay opacity rule. |
| Edge fade | `crates/ui/src/edge_fade.rs` | **adapt** | S | Scroll-edge fades for lists over a custom background. Depends on zui. |
| Window chrome and lifecycle | `crates/ui/src/lib.rs` (`open_main_window`, `restored_main_window_bounds`, `on_reopen`) | **adapt** (partly done) | S | Done: transparent titlebar with traffic-light inset. Still to take: geometry persistence, dock reopen after ⌘W, calling `set_menus` after the keymap, and Linux client-side decorations. |
| Custom background pipeline | `crates/ui/src/new_thread_background_{image,mask,effects}.rs` | **adapt** | M | Decoding contract, masking, effects. Map onto the tokens.md §10.3 pipeline (bucketed downscale, blur, disk cache). Processing must stay on background executors. |
| Command palette | `crates/ui/src/shell/command_palette.rs` | **adapt** | M | Card, overlay, header, footer and key hints, the results fade, and an Enter-latch fix for X11 repeats. Swap its substring `matches_query` for `adoken-search` (frizbee, see ADR 0001) and its entries for the 0003 result model. |
| Popover and list keyboard reducers | `crates/ui/src/popover.rs`, `crates/ui/src/pickers.rs` | **adapt** | M | Pure, unit-tested wrap-around navigation, ranked filtering, key classification, hover intent. Good fit for keyboard-first lists. |
| App menus and fixed bindings | `crates/ui/src/app_menus.rs` | **adapt** (pattern used) | S | `crates/app` already uses `set_menus` plus `bind_keys` plus `on_action`. Next: an Edit menu routed to the focused input, and a Window menu. |
| Rebindable shortcuts | `crates/ui/src/settings/shortcuts.rs`, `settings.rs` (`KeymapConfig`) | **adapt** | M | Recording, conflict detection, reset. Every action must also be reachable from Cmd+P, so shortcut ids should come from one action registry. |
| Settings persistence | `crates/ui/src/settings.rs` | **adapt** | M | Keep the pattern: one writer, debounced geometry writes, clamping of hand-edited values. Switch the format from JSON to `settings.toml` (tokens.md §9.4, §10.1). |
| Motion kit | `crates/ui/src/motion.rs` | **adapt** | S | Exact `CubicBezier`, a shared throttled pulse clock, reduce-motion via GPUI's flag, and translate implemented as a relative inset. tokens.md §7 uses the same `ease.out` curve (0.16, 1, 0.3, 1). Still needed: the spring easing from tokens.md §7.2. |
| Sound playback | `crates/ui/src/sound.rs` | **adapt** code, **skip** assets | S | Platform players (`afplay`, PowerShell, `paplay`) on a background thread, no audio crates, failures swallowed. Zeron's WAVs are not reused; Ken assets must be original (AGENTS.md). |
| OS notifications | `crates/ui/src/notify.rs` | **adapt** | M | macOS `NSUserNotification` with a click callback, and `notify-send` on Linux. Risk: the API has been deprecated since 10.14, so plan `UNUserNotificationCenter` for signed builds. Windows is a no-op in zeron as well. |
| Typography and fonts | `crates/ui/src/typography.rs` | **adapt** | S | Registers bundled fonts. Needed when Inter and JetBrains Mono are bundled (tokens.md §4). |
| Icons and asset source | `crates/ui/src/icons.rs` | **adapt** pattern, **skip** assets | S | `rust-embed` `AssetSource` plus `icon()`. Zeron's icons are Solar (CC BY 4.0) and Symbols; adoken uses Lucide (ISC) per tokens.md §2.8. |
| Markdown renderer | `crates/ui/src/markdown/*` | **adapt** | L | pulldown-cmark rendering, text selection, link handling, a cross-frame layout cache. Needed for PR descriptions and comments in 0004. The mermaid part is optional. |
| Diff view and syntax | `crates/ui/src/changes.rs`, `crates/syntax` | **adapt** | L | Diffing via the `similar` crate, tree-sitter highlighting, a virtualized diff. Core of the 0004 PR review. Theme keys map onto adoken's `syntax.*` and `diff.*` tokens. |
| Updater | `crates/update` | **adapt** (release time) | L | `manifest.json` with per-artifact sha256, bounded metadata fetch, macOS bundle swap and relaunch, Windows path. Currently tied to zeron's R2 bucket and edge URLs; adoken would point it at its own Cloudflare R2 feed. |
| macOS packaging and signing | `scripts/package-macos.sh`, `dist/macos/Info.plist`, `scripts/dmg-background.py` | **adapt** | S | Bundle layout, `iconutil`, hardened-runtime `codesign`, `notarytool` via API key, stapling the app before tarring it for the updater, dmg. Owned by release-manager. |
| Windows and Linux packaging | `scripts/package-windows.ps1`, `scripts/package-linux.sh` | **adapt** (v0.3 / best-effort) | S | PE machine-type detection for artifact naming; a Linux tarball with an XDG install script. |
| Snapshot and fixture tooling | `crates/ui/examples/*-fixture.rs`, `examples/macos-resource-profile.rs` | **adapt** technique (done) | S | Zeron has no story gallery. Its fixtures use `/usr/sbin/screencapture` on a live window, or the bench platform with a headless Metal renderer plus `render_to_image`. adoken uses the latter through GPUI's `HeadlessAppContext` (below). |
| `gpui-base` (zeronsh/gpui-component) | `Cargo.toml` `gpui-base` rev `103f13da` | **depend** (evaluate) | S | Apache-2.0. Provides the text input and editor base built on Ropey. The PAT paste field (0001) and the Cmd+P query (0003) need a text input. Evaluate it against porting GPUI's `examples/input.rs` when 0003 starts. |
| Agent product crates | `crates/{engine,harness,rpc,proto,doc,sync,mcp,preview}`, `apps/zeron`, `edge/`, `crates/ui/src/{terminal,browser,appshots,composer*,transcript,queue,history}` | **skip** | – | Agent orchestration, the CRDT (loro) sync, the terminal and the browser. Nothing adoken's scope needs. |

**Batch 2 (2026-09-25):** 16 more components (comment widgets, image viewer and decoding, file-type icons, highlight cache, repo artwork, hover intent, contained dropdowns, settings scaffold, toolbar metrics, release Worker, download button, isolated dev bundle, three Windows notes) are specified in `docs/specs/0010-zeron-reuse-batch-2.md`, including what was reviewed and declined.

## Snapshot tests (implemented)

`crates/gallery/tests/snapshots.rs` is an integration test with `harness = false`, so it runs on the process main thread the way GPUI expects.

1. It builds `gpui::HeadlessAppContext::with_platform(text_system, (), gpui_platform::current_headless_renderer)`. This combines GPUI's deterministic `TestDispatcher`, the native macOS CoreText text system (taken from a headless `MacPlatform`), and the real Metal renderer drawing offscreen. It needs no window server or display.
2. For every variant in `ThemeRegistry::builtin()` and every story in `adoken_gallery::STORIES`, it installs the theme, opens a window at the story's logical size, and runs until parked. It then calls `window.draw(cx)` and `window.render_to_image()`.
3. Each image is compared against `crates/gallery/snapshots/<story>--<variant>.png`. A pixel counts as different when any channel is off by more than 8 (anti-aliasing noise). The test fails when more than 0.2 % of pixels differ. On failure the actual image goes to `target/snapshot-failures/`.
4. `UPDATE_SNAPSHOTS=1 cargo test -p adoken-gallery --test snapshots` rewrites the goldens. Review the PNG diff before committing.

New stories and new built-in variants are covered automatically. The current matrix is 3 stories × 2 themes = 6 goldens.

Limits and plan:
- **macOS only.** `current_headless_renderer()` returns `None` elsewhere in the pinned GPUI, so the test prints a skip on Windows and Linux. For Windows v0.3: zui has Direct3D and wgpu renderers, so either implement `PlatformHeadlessRenderer` for wgpu (one patch to zui) or run snapshots on macOS CI only.
- **Font drift.** Goldens depend on the macOS system font (`.SystemUIFont`) and Menlo. Pin the CI macOS image. Once Inter and JetBrains Mono are bundled and registered in the test, goldens stop depending on the OS font.
- **Story sizes are fixed** in `STORIES`. Text overflowing a story shows up as a visual diff, not an error.

## Risks

1. **Fork maintenance (high).** zui is maintained by one person and has no git history in common with Zed. Upstream fixes need manual `format-patch` ports (see zui `README.md`). If zeron stops maintaining it, adoken inherits the fork. Mitigations:
   - Pin an exact rev and bump it deliberately.
   - Mirror zui under adoken's GitHub org before v0.1 ships (Apache-2.0 allows it).
   - Keep zui-only APIs (`BackdropBlur`, `EdgeFade`) inside `adoken-ui` so a move to upstream GPUI touches one crate.
2. **API churn.** GPUI is Zed's internal API. Expect breaking changes on every bump (for example, `KeyDownEvent` fields and `Window::draw` visibility). Budget a day per bump; snapshot tests catch visual regressions.
3. **Cold start with `runtime_shaders` (medium).** Compiling Metal shaders at launch costs time against the < 500 ms cold-start budget. Measure time to first frame. If it's over budget, build releases on CI with full Xcode and drop the feature for release builds, which precompiles the metallib.
4. **License attribution.**
   - Zeron code: MIT notice kept in `THIRD_PARTY_NOTICES.md` and in file headers.
   - GPUI and zui: Apache-2.0 plus zui's `NOTICE` (egoist/zed attribution). Both must ship with binaries.
   - zui contains a GPL-3.0 `path` crate. adoken does not link it today (`cargo tree --workspace -i path` finds no match). Re-check on every bump, because it arrives through `util` if `http_client`'s optional `util` feature is enabled.
   - Zeron's third-party assets have their own licenses: Solar icons (CC BY 4.0), Symbols (MIT), palettes adapted from VS Code themes, and zeron's own sounds. None are copied.
   - Before the first public binary, replace the hand-written notices with a generated license bundle (e.g. `cargo about`).
5. **Deprecated platform APIs.** Zeron's banners use `NSUserNotification` (deprecated). Copying it means planning a move to `UNUserNotificationCenter`, which needs a signed bundle.
6. **Future-incompat warnings.** Cargo warns that `block v0.1.6` and `proc-macro-error2 v2.0.1`, both pulled in through GPUI, contain code a future Rust release will reject. It's harmless now but will break builds on some future toolchain until zui updates them.
