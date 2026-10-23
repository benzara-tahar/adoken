# Third-party notices

adoken is MIT-licensed (see `LICENSE`). It includes or adapts the third-party works listed below. Each one keeps its own license. Rule from AGENTS.md: when you copy or adapt code, record the source path and commit here in the same change.

## Adapted source code

### Zeron

- Source: https://github.com/zeronsh/zeron at commit `d1010657d69c9730f8020edc36cc2649b136b6fa`.
- License: MIT. Copyright (c) 2026 Wing. The full text is below.

| adoken file | Zeron source | What was taken |
| --- | --- | --- |
| `crates/theme/src/color.rs` | `crates/theme/src/lib.rs` (`Color`, `ColorParseError`) | The sRGB color type: CSS hex parsing and serialization, `blend_over`, and WCAG luminance and contrast. adoken made the parser `const`. `mix` and `with_alpha` were added back for the theme importer, and `ensure_contrast` became an OKLab-lightness search (tokens.md §9.3) instead of zeron's mix toward black/white; `best_on_color` was not taken. |
| `crates/theme/src/lib.rs` | `crates/theme/src/lib.rs` (`ThemeVariant`, `ThemeFamily`, `ThemeRegistry`, `ThemeSelection`, `validate`) | The shape of the model: resolved variants grouped into families, a registry that falls back to the default variant per appearance, and contrast validation that returns issues rather than rejecting the theme. The token set, file format, alias resolution and rules come from adoken's `docs/design/tokens.md`. |
| `Cargo.toml` (`[profile.dev.package."*"]`, gpui pin comment) | `Cargo.toml` | Dev profile that optimizes dependencies, and the pinned `zeronsh/zui` gpui revision with its features. |
| `crates/gallery/tests/snapshots.rs` | `crates/ui/examples/macos-resource-profile.rs`, `crates/ui/examples/appshots-fixture.rs` | The technique only: offscreen Metal rendering plus `Window::render_to_image` for PNG capture. The test harness itself is new code. |
| `crates/ado/src/onboarding/az.rs` (`AzLocator`) | `crates/harness/src/shell_env.rs`, `crates/harness/src/executable.rs` | The technique only: ask the user's login shell for the tool's path because Finder/launchd launches don't inherit the shell `PATH`, keep the last absolute path line to skip rc-file noise, enforce a hard timeout, then fall back to known install locations. The code is new. |
| `Cargo.toml` (`tokio`, `reqwest`, `gpui_tokio` entries) | `Cargo.toml` | The async/HTTP stack choice: a Tokio runtime bridged into GPUI with `gpui_tokio`, and reqwest 0.12 over rustls. |
| `crates/ui/src/icon.rs` | `crates/ui/src/icons.rs` | The pattern: embed each icon with `include_bytes!` and serve all of them through one gpui `AssetSource`, plus the load/list test. adoken's icon set, `IconName` enum and `Icon` component are new. |
| `crates/ui/src/motion.rs` (`CubicBezier`, `pulse_phase`) | `crates/ui/src/motion.rs` (`CubicBezier`, `PulseClock`, `pulse_delta`) | The CSS `cubic-bezier()` solver (Newton iteration, bisection fallback, clamped output) and the leased ~30 fps shared repeat clock for spinners. adoken dropped the stride and Windows timer logic; springs and tokens are new. |
| `crates/app/src/background/frost.rs` (`Frosted`, `frosted`) | `crates/ui/src/frost.rs` (`Frosted`, `frosted`) | The element: paint an overlay's whole subtree in one scene layer with the zui fork's `Window::paint_backdrop_blur` first, so repaints elsewhere can't reorder its quads under the blur. adoken gates it on a custom background being visible and takes the radius from the `OVERLAY_BLUR` token; `layered` and the theme frost check were dropped. |
| `crates/diff/src/rows.rs` (`split`), `crates/diff/src/file.rs` | `crates/ui/src/changes.rs` (`split_pairs`, `line_anchor`), `crates/ui/Cargo.toml` (`similar`) | The technique only: side-by-side rows pair a change block's deletions and additions by position (the longer side gets one-sided rows), deletions anchor to the old file and everything else to the new one, and the line diff comes from the `similar` crate. adoken diffs blob texts instead of parsing patches; the code is new. |
| `crates/ui/src/diff.rs` (`gutter_digits`) | `crates/ui/src/changes.rs` (`FileDiff::max_line`, `gutter_width`) | The technique only: size line-number gutters analytically from the largest line number instead of a fixed column. |
| `crates/store/src/lib.rs` (`Store::open`, pragmas) | `crates/sync/src/store.rs` | The technique only (as ADR 0001 cites): bundled SQLite via rusqlite, WAL journal, `synchronous=NORMAL` set at open. The writer/read-pool handle, schema, migrations and upserts are new code. |
| `crates/theme/src/draft.rs` (`Seeds`, `DraftVariant::from_seeds`) | `crates/theme/src/builtins.rs` (`Seeds`, `variant`) | The seed shape a theme is derived from (editor background, shell, raised, card, text/muted/faint, accent, danger/warning/success, twelve syntax roles) and the idea of deriving overlays, washes and borders from it. The mapping onto adoken's tokens, the contrast enforcement and the TOML writer are new. |
| `crates/theme/examples/port_zeron_themes.rs`, `crates/theme/themes/*.adoken-theme.toml` except `adoken`, `arcade`, `sakura` | `crates/theme/src/builtins.rs` (every built-in except Shades of Purple) | The seed palettes of zeron's curated built-in themes, with each upstream's repository, audited revision and license (see "Bundled theme palettes" below). adoken derives its tokens from them and lists every contrast repair at the top of each file. |
| `crates/app/src/theme_picker/preview.rs` (`Miniature`) | `crates/ui/src/settings/appearance.rs` (`miniature`) | The layout of a theme card's miniature: a narrow sidebar of bars beside a bordered pane. adoken adds list rows, a diff-washed code block and an accent button, all painted in the card's own theme. |
| `scripts/bundle.sh`, `crates/app/bundle/Info.plist`, `docs/release.md` | `scripts/package-macos.sh`, `scripts/run-macos-dev.sh`, `dist/macos/Info.plist` | The macOS bundling steps: assemble `Contents/{MacOS,Resources}`, fill the Info.plist template's version with `sed`, build the `.icns` from a pre-masked 1024 px PNG with a `sips` loop (16–512 @1x/@2x) plus `iconutil`, ad hoc `codesign --force --sign -`, and the Developer ID (`--options runtime --timestamp`, no `--deep`) → `notarytool submit --wait` → `stapler staple` sequence (documented, not executed). adoken's bundle id, keys, icon art and script are its own. |
| `crates/ui/src/loaders.rs` (`gradient_spinner`, `mini_glyph_spinner`, `mini_mono_spinner`, `progress_ring`, `splash_overlay`, pulse math), `crates/ui/src/status.rs` (`Spinner`) | `crates/ui/src/loaders.rs` (`gradient_spinner`, `mini_glyph_spinner`, `mini_mono_spinner`, `upload_progress_ring`, `splash_overlay`), `crates/proto/src/motion.rs` (`pulse_wave`, `staggered_phase`, `gspin_opacity`, `gspin_cell_phase`, `MINI_RING`) | The 3×3 matrix spinner with its rising arrow-up phase map and keyframe curve, the 2×3 perimeter-chase mini spinner, the progress ring stroked as polylines from 12 o'clock, and the splash overlay with its fade-and-lift exit. adoken tints them with theme tokens instead of fixed colors and drives them from its own shared clock. Zeron's logo loader was not taken: adoken's fist loader and its pixel grid are original (same grid as `public/src/art/fist.ts`). |
| `crates/app/src/netwatch.rs` (macOS path monitor) | `crates/sync/src/net_path.rs` | The `NWPathMonitor` bridge: the Network.framework C declarations, a serial dispatch queue, one `RcBlock` update handler, and treating only `unsatisfied` as offline. adoken turns reports into deduplicated scheduler controls, cancels/releases the monitor on drop, and adds `NSWorkspace` sleep/wake observers (new code). |
| `crates/app/src/settings/pages/mod.rs` (`section`, `row`), `crates/app/src/settings/view.rs` (rail), `crates/ui/src/switch.rs` | `crates/ui/src/settings/widgets.rs` (`section`, `section_card`, `card_row`, `row_title`, `toggle_switch`, `section_tab`) | The settings page layout only: a small section label over a filled, rounded block of hairline-split rows with the title and muted description on the left and the control on the right, a left column of section tabs, and a pill switch whose knob slides across the track. adoken's rows, search filtering, tokens, spring timing and all behavior are new code. |
| `crates/app/src/avatar/mod.rs` (`Avatar`) | `crates/ui/src/history.rs` (commit author avatar) | The technique only: a person's picture drawn as `img(…)` with `rounded_full` + `ObjectFit::Cover` inside a fixed-size circle, with the initial as the fallback in the same box. adoken's loading, disk/decoded caches, sizes, tokens and fetch rules (spec 0012) are new code. |
| `crates/ui/src/text_input.rs` (`scroll_to_reveal`) | `crates/ui/src/composer.rs` (`input_scroll_offset_for_cursor`) | The technique only: move the input's scroll by the smallest amount that brings the caret fully into the viewport, then clamp to the scrollable range. adoken applies it horizontally to a single line, adds an edge margin, reveals the IME composition too, and keeps the caret after the last character inside the clamp. |
| `crates/ui/src/fonts.rs`, `crates/app/src/code_font/mod.rs` (`CodeFont`, `is_monospace`, fallback) | `crates/ui/src/typography.rs` (`register_family`, `face_is_fixed_width`, `FIXED_WIDTH_TOLERANCE`, `resolve_effective_mono`, `UiFontFamily`) | The technique only: embed static font faces with `include_bytes!` and hand them to `TextSystem::add_fonts` as borrowed bytes at launch; a Bundled / System / Installed(name) family choice with a requested vs. effective family and a bundled fallback; fixed-width detection by measuring `i`/`m`/`W`/`0` advances within a 1 % tolerance. adoken measures through gpui's text system on the background executor when the picker first shows (zeron scans every system face with `fontdb` at launch), and its settings, picker and diff row metrics are new code. |
| `crates/ui/src/popover/hover_intent.rs` (`HoverIntent`, `HoverAction`, `corridor`) | `crates/ui/src/popover/hover_intent.rs` | Spec 0010 Z-7: the hover-intent state machine (open now vs defer, grace, cancel on click/keyboard/dismiss, `contains_pointer`), the safe-corridor triangle toward a child on either side, and its three unit tests. adoken takes the grace, dwell and corridor slop from theme tokens (`motion.hover_grace`, `motion.hover_intent`, `space.*`) and adds `dwell` for surfaces opened by hover alone (pins peek, spec 0014). |
| `crates/richtext/Cargo.toml` (`pulldown-cmark`), `crates/richtext/src/markdown.rs` | `Cargo.toml`, `crates/ui/src/markdown/parser.rs` (`parse_full`) | The technique only: parse Markdown with `pulldown-cmark` (tables, task lists, strikethrough enabled) into a block tree, folding a tight list item's bare inline run into a paragraph. adoken's document model, HTML parser, Markdown/HTML writers and FR-18 round-trip gate are new code. |
| `crates/app/src/hotkey/macos.rs` (`CarbonRegistrar`, `hot_key_handler`, `CarbonLayout`, `activate_via_launch_services`), `crates/app/src/hotkey/model.rs` (`key_code`, `Chord::carbon_modifiers`) | `crates/ui/src/appshots/macos.rs` (`start_global_shortcut`, `refresh_global_shortcut`, `shortcut_keycode`, `layout_keycode`, `appshot_hot_key_handler`, `foreground_after_capture`, the Carbon modifier masks) | Spec 0021 FR-1/FR-4/FR-7: Carbon `RegisterEventHotKey` on the application event target with an `InstallEventHandler` (`kEventHotKeyPressed`) callback forwarding presses over a channel; unregister-before-register; named-key codes and printed-key resolution against the current ASCII-capable layout with `UCKeyTranslate` (unshifted, then shifted); the LaunchServices `openApplicationAtURL` fallback that brings the app's own bundle forward when cooperative activation declines. adoken registers one hot key and checks its `EventHotKeyID` (read with `GetEventParameter`), puts registration behind a `Registrar` trait and the layout behind a `Layout` trait, and uses objc2 instead of objc 0.2. |
| `crates/app/src/hotkey/mod.rs` (`Hotkeys::start_recording`, `set_recording`), `crates/app/src/hotkey/model.rs` (`Controller::set_recording`, `Chord::parse`) | `crates/ui/src/appshots/shortcut.rs` (`Shortcut::parse`, `set_recording`, `set_shortcut`), `crates/ui/src/settings/shortcuts.rs` (`start_recording`, `record_keystroke`) | The preference and recorder pattern: parse the stored combo with gpui's `Keystroke`, require a modifier, suspend the global registration while recording and re-register on save or cancel, and record through `intercept_keystrokes` so a chord adoken binds is refused instead of running its action. adoken's validation rules (FR-5), copy and Settings section are new. |

```
MIT License

Copyright (c) 2026 Wing

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Bundled theme palettes

adoken ships community themes ported from zeron's curated palette adaptations (see the Zeron rows above). The palette values are adapted under each upstream project's license. The projects are not affiliated with or endorsed by adoken; their names identify the corresponding palettes. Each theme file in `crates/theme/themes/` names its upstream and lists every contrast adjustment adoken made (docs/design/tokens.md §9.3).

| adoken family | Upstream project | Audited revision | License |
| --- | --- | --- | --- |
| `zeron` (Zeron Light, Zeron Dark) | Zeron / comet, https://github.com/zeronsh/comet | `d138049` | MIT, Copyright (c) 2026 Wing |
| `vscode-default` (Light+, Dark+) | Visual Studio Code, https://github.com/microsoft/vscode | `e33d147d4c0fa65ce17cb73ec9d798f064b4bf1f` | [MIT](https://github.com/microsoft/vscode/blob/e33d147d4c0fa65ce17cb73ec9d798f064b4bf1f/LICENSE.txt) |
| `catppuccin` (Latte, Mocha) | Catppuccin for VS Code, https://github.com/catppuccin/vscode | `befc9e6fc41980f4241408f7049755d47c06ff45` | [MIT](https://github.com/catppuccin/vscode/blob/befc9e6fc41980f4241408f7049755d47c06ff45/LICENSE) |
| `tokyo-night` (Light, Night) | Tokyo Night, https://github.com/tokyo-night/tokyo-night-vscode-theme | `7c0f11eaef322f293621ca7befe462214b7ea468` | [MIT](https://github.com/tokyo-night/tokyo-night-vscode-theme/blob/7c0f11eaef322f293621ca7befe462214b7ea468/LICENSE.txt) |
| `dracula` | Dracula for Visual Studio Code, https://github.com/dracula/visual-studio-code | `1b9ecf4d7e0c8cc2e2e890a7a41ad1db5fff1e6c` | [MIT](https://github.com/dracula/visual-studio-code/blob/1b9ecf4d7e0c8cc2e2e890a7a41ad1db5fff1e6c/LICENSE) |
| `github` (Light, Dark) | GitHub VS Code Theme, https://github.com/primer/github-vscode-theme | `cd78e5e4e7bcf132a6f428ae0f32264bb1b729cf` | [MIT](https://github.com/primer/github-vscode-theme/blob/cd78e5e4e7bcf132a6f428ae0f32264bb1b729cf/LICENSE) |
| `ayu` (Light, Dark, Mirage) | Ayu for VS Code, https://github.com/ayu-theme/vscode-ayu | `444ef92911cb75c3933c8003e3a7c79b6b6c914f` | [MIT](https://github.com/ayu-theme/vscode-ayu/blob/444ef92911cb75c3933c8003e3a7c79b6b6c914f/LICENSE) |
| `gruvbox` (Light, Dark) | Gruvbox Theme, https://github.com/jdinhify/vscode-theme-gruvbox | `ca3b8ad203e84a884ca33fb84b5795cf43032709` | [MIT](https://github.com/jdinhify/vscode-theme-gruvbox/blob/ca3b8ad203e84a884ca33fb84b5795cf43032709/LICENSE) |
| `rose-pine` (Dawn, Moon) | Rosé Pine for VS Code, https://github.com/rose-pine/vscode | `d8f5ebe8e096fa833e997c07eb7685ee1677a4ba` | [MIT](https://github.com/rose-pine/vscode/blob/d8f5ebe8e096fa833e997c07eb7685ee1677a4ba/LICENSE) |
| `nord` | Nord Visual Studio Code, https://github.com/nordtheme/visual-studio-code | `8ead09822c02d0d49d0f764104505e5a34d3689f` | [MIT](https://github.com/nordtheme/visual-studio-code/blob/8ead09822c02d0d49d0f764104505e5a34d3689f/license) |
| `one-dark-pro` | One Dark Pro, https://github.com/Binaryify/OneDark-Pro | `e6ccf638d5b69aa38cd1005edb0ee7ba7ef6fedc` | [MIT](https://github.com/Binaryify/OneDark-Pro/blob/e6ccf638d5b69aa38cd1005edb0ee7ba7ef6fedc/LICENSE.txt) |
| `atom-one-dark` | Atom One Dark Theme, https://github.com/akamud/vscode-theme-onedark | `a8be970644982221f9b61fb1c4b3da74b4beab79` | [MIT](https://github.com/akamud/vscode-theme-onedark/blob/a8be970644982221f9b61fb1c4b3da74b4beab79/LICENSE) |
| `night-owl` (Light, Night Owl) | Night Owl, https://github.com/sdras/night-owl-vscode-theme | `cc291eba7976b20d7c66bde6883c27b902196b07` | [MIT](https://github.com/sdras/night-owl-vscode-theme/blob/cc291eba7976b20d7c66bde6883c27b902196b07/LICENSE.md) |
| `winter-is-coming` (Light, Dark Blue) | Winter is Coming, https://github.com/johnpapa/vscode-winteriscoming | `260547834cb6ac37dd5b8bb5842cc1c8d3164946` | [MIT](https://github.com/johnpapa/vscode-winteriscoming/blob/260547834cb6ac37dd5b8bb5842cc1c8d3164946/LICENSE.md) |
| `palenight` | Palenight Theme, https://github.com/whizkydee/vscode-palenight-theme | `6291efaace90855abe3d79025327ca41b9a3138c` | [MIT](https://github.com/whizkydee/vscode-palenight-theme/blob/6291efaace90855abe3d79025327ca41b9a3138c/license.md) |
| `synthwave-84` | SynthWave '84, https://github.com/robb0wen/synthwave-vscode | `ecfa2fe1279f7233663fa3f98a96e6756000567b` | [MIT](https://github.com/robb0wen/synthwave-vscode/blob/ecfa2fe1279f7233663fa3f98a96e6756000567b/LICENSE) |
| `cobalt2` | Cobalt2, https://github.com/wesbos/cobalt2-vscode | `c4e9574372b85afad1682ed0fdd1ac0411c62512` | [MIT](https://github.com/wesbos/cobalt2-vscode/blob/c4e9574372b85afad1682ed0fdd1ac0411c62512/LICENSE) |
| `andromeda` | Andromeda, https://github.com/EliverLara/Andromeda | `d1abb48c69493000aa0133a32d594eb25e523d4f` | [MIT](https://github.com/EliverLara/Andromeda/blob/d1abb48c69493000aa0133a32d594eb25e523d4f/LICENSE.md) |

Not ported: zeron's "Shades of Purple". Its license (`e8eb49f33e5db05ceba6677367b33ddb27ad821c`) adds a condition that anything built with it be MIT-licensed. adoken was proprietary when the themes were ported; now that it is MIT, the theme can be ported in a later change.

Copyright notices retained from the pinned upstream licenses:

- Copyright (c) 2026 Wing
- Copyright (c) 2015 - present Microsoft Corporation
- Copyright (c) 2021 Catppuccin
- Copyright (c) 2018-present Enkia
- Copyright (c) 2016 Dracula Theme
- Copyright (c) 2020 Primer
- Copyright (c) 2016 Ike Kurghinyan
- Copyright © 2017 JD
- Copyright (c) 2021 Rosé Pine
- Copyright (c) 2016-present Sven Greb <development@svengreb.de> (https://www.svengreb.de)
- Copyright (c) 2013-2022 Binaryify
- Copyright (c) 2015 Mahmoud Ali
- Copyright (c) 2018 Sarah Drasner
- Copyright (c) 2015-2017 JohnPapa.net, LLC
- Copyright (c) 2017-present Olaolu Olawuyi
- Copyright (c) 2019 Robb Owen
- Copyright (c) 2018 Wes Bos, Roberto Achar
- Copyright (c) 2017 <eliverlara@gmail.com>

Each of these is MIT-licensed; the MIT permission notice (as reproduced in full under Zeron above) applies to each palette with its own copyright line. Ship this section with any binary distribution.

## Bundled assets

### Lucide icons

- Source: `assets/icons/lucide` (Lucide 1.48.0, https://github.com/lucide-icons/lucide/releases/tag/1.48.0; manifest entry in `assets/manifest.toml`).
- Embedded in the binary by `crates/ui/src/icon.rs` (only the glyphs listed in `IconName`).
- License: ISC, with MIT for the icons derived from Feather. The full text is `assets/icons/lucide/LICENSE`; ship it with any binary distribution.

### JetBrains Mono

- Source: `assets/fonts/jetbrains-mono` (JetBrains Mono v2.304, https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304; static `JetBrainsMono-Regular.ttf`, `-Italic.ttf`, `-Bold.ttf` from `JetBrainsMono-2.304.zip`; manifest entry in `assets/manifest.toml`).
- Embedded in the binary by `crates/ui/src/fonts.rs` as the default code font (spec 0009 §3 Code font). Font files unmodified.
- License: SIL Open Font License 1.1, Copyright 2020 The JetBrains Mono Project Authors (https://github.com/JetBrains/JetBrainsMono). The full text is `assets/fonts/jetbrains-mono/OFL.txt`; ship it with any binary distribution (`scripts/bundle.sh` copies it to `Contents/Resources/licenses/JetBrainsMono-OFL.txt`).

## Linked dependencies

### GPUI (zui fork)

- Source: https://github.com/zeronsh/zui at rev `18a89af04cfe079d55275b111ac349d5640d9cda`. It is extracted from Zed (https://github.com/zed-industries/zed, upstream `f14fea9`) and carries Zeron patches.
- Crates used: `gpui`, `gpui_platform` and their in-tree dependencies (`gpui_macos`, `gpui_macros`, `collections`, `refineable`, `scheduler`, `sum_tree`, `util`, `http_client`, `media` and others).
- Adapted source: `crates/app/src/os_notify/macos.rs` adapts `crates/gpui_macos/src/system_notifications.rs` at the rev above: the app-bundle guard (`NSBundle.mainBundle.bundleIdentifier` before touching `UNUserNotificationCenter`), the `define_class!` response delegate that forwards clicks through a channel, and the `willPresentNotification` answer. adoken added thread identifiers, fixed action categories, authorization status reporting and the dock badge.
- Adapted source: `crates/ui/src/text_input.rs` adapts `crates/gpui/examples/input.rs` at the rev above: the IME-aware `EntityInputHandler` and the custom text element. adoken split the editing model out as `EditState` so it is unit-tested without a window, and every color comes from the theme.
- Adapted source: `crates/ui/src/text_area.rs` adapts `crates/gpui/examples/input.rs` at the rev above, through `crates/ui/src/text_input.rs`: the same IME handler and text element, extended to many lines. Wrapping uses GPUI's `shape_text` and `WrappedLine` geometry; the editing, scrolling and theming are adoken code.
- License: Apache-2.0. Copyright 2022 - 2025 Zed Industries, Inc. zui's `NOTICE` says its macOS overlay-view pattern was adapted from the Apache-2.0 GPUI crates at https://github.com/egoist/zed at `57bd4fe181639797d395978d5de17bc9e10a6219`. Any binary distribution must include that NOTICE and the Apache-2.0 text.
- zui also contains the `path` crate, which is GPL-3.0-or-later. adoken does not depend on it: `cargo tree --workspace -i path` finds no match. Re-run that check whenever the gpui pin changes.

### Other crates

All other Rust dependencies, direct and transitive, are pinned in `Cargo.lock` under their own licenses. None of the normal dependencies is GPL, AGPL or LGPL (checked with `cargo tree -e normal -f '{p} {l}'`). Before the first public binary ships, a generated license bundle (e.g. `cargo about`) must replace this paragraph.
