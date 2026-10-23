# 0025 — Feature diet: fewer shortcuts, one quiet switch, smaller settings

| Field | Value |
| --- | --- |
| Status | Accepted (2026-10-02, owner interview; every cut below was picked by the owner) |
| Milestone | v0.1 |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/app` (palette, pins, shell, help, review, inbox, activity, code_font, ken, celebrate, sound, tours, resource, sync, theme_picker, hotkey, settings), `crates/ui` (picker, action_menu), `crates/sound`, `crates/gallery` (stories, snapshots) |
| Serves | **Less noise** (fewer keys to learn, a cheat sheet you can read, a shorter ⌘P, fewer settings) · **Snappy** (arrows are as fast as the number keys they replace) |
| Depends on | `0003-command-palette.md`, `0005-notification-center.md`, `0007-theming-backgrounds-sounds.md`, `0009-settings.md`, `0014-pins.md`, `0020-guided-tours.md`, `0021-global-hotkey.md`, `0022-celebrations.md`, `0023-resource-meter.md`; each is amended by this spec where they disagree |
| Design | No new visuals. Removed hints and rows leave no gap; the cheat sheet keeps its current look with new group names |

> Rule: no code without an **Accepted** spec (AGENTS.md). The spec owns behavior, acceptance, and budgets; design docs own visuals, motion, sound, and copy.

## 1. Problem

"When I press ⌘P I see ⌘1, ⌘2, ⌘3 on every row. I don't need them, the arrows are just as fast. The shortcuts sheet isn't well organized either: most keys are useful, some aren't. There are too many ways to do the same thing."

Audit (2026-10-02): about 150 key bindings in 26 modules, about 170 ⌘P commands, and a cheat sheet built from about 35 raw key contexts ("Text input", "Picker", "Review sheet", "Activity splitter") listed in the order they were registered. Seven commands quiet the app in overlapping ways. Several features have a second path nobody asked for.

## 2. Users and job to be done

- **Persona:** every adoken user, first week and every day after.
- **Job to be done:** "When I learn adoken, I want one obvious key per action and a cheat sheet grouped by the screen I'm on, so I remember the keys that matter."
- **Smallest delightful slice:** no number badges in ⌘P, and a cheat sheet grouped by screen.

## 3. UX flow

Nothing new to learn. Arrow keys and ↩ select rows in ⌘P, menus and pickers. Sections keep ⌘1–⌘6, now in sidebar order. `⌘/` opens a cheat sheet grouped by screen. ⌘P › "Quiet mode: turn on or off" silences sounds, celebrations and the arcade layer in one step.

## 4. Functional requirements

### A. Keys

- **FR-1 Palette rows have no number keys.** ⌘1–⌘9 (`palette::JumpToResult`) are unbound and removed, along with the ⌘1…⌘9 badges on result, pin and command rows. A command row still shows its own shortcut (e.g. "Toggle sidebar ⌘B"). Also removed from the palette: ⌘↑/⌘↓ (first/last) and ⌃N/⌃P. These stay: ↑/↓ (wrap), PageUp/PageDown, ↩, ⌘↩, Tab, Esc, ⌘K.
- **FR-2 Pins have no number jumps.** ⌃1–⌃9 (Alt+1–9 on Windows) are unbound and removed, along with the nine "Jump to pin N" ⌘P commands and the numbers on rail icons, sidebar rows and tooltips. These stay: ⌘D, ⌘J (pins in ⌘P), `pin:`, ⌘Z undo unpin, ⌥⌘↑/⌥⌘↓ reorder, drag, rename. Amends 0014 FR-8 and AC-4.
- **FR-3 Menus have no digit picks.** Digits no longer pick in the vote menu (1–5), the link-type menu (1–7) or the Inbox menus (1–5), and any digit hint in those menus is gone. ↑/↓, ↩, Esc and clicks still work. The Inbox chip filters (1–5 sources, `!` `@` `#` `$` priorities) and onboarding's 1/2 stay (owner kept them).
- **FR-4 One key per action where an alias added nothing.** Removed: ⌘\ (sidebar, ⌘B stays); ⌃N/⌃P in the palette, pickers and action menus; ⌘↑/⌘↓ as first/last in lists (Home/End stay wherever they are bound). The owner kept `j`/`k` and `h`/`l` next to the arrows.
- **FR-5 Section keys follow the sidebar.** ⌘1 Today, ⌘2 Inbox, ⌘3 Pull requests, ⌘4 Work items, ⌘5 Pipelines, ⌘6 Search (previously ⌘4 Search, ⌘5 Work items, ⌘6 Pipelines). The sidebar tooltips, the "Go to …" commands, tours and the cheat sheet read the new keys from the keymap.
- **FR-6 No global code-size keys.** ⌘=, ⌘- and ⌘0 are unbound. ⌘P "Increase / Decrease / Reset code font size" stay as unbound commands, and Settings › Appearance › Code font is unchanged.

### B. Cheat sheet

- **FR-7 Grouped by screen, still generated.** `⌘/` builds its rows from the live keymap as before (rebinds in `keymap.toml` show up), but groups follow a fixed map from key context to screen, in this order: **Everywhere** (no context, `Workspace`), **Today**, **Inbox**, **Pull requests** (list), **Pull request review** (review, review sheet, file tree filter, threads), **Work items** (list, detail, editors), **Pipelines**, **Search** (⌘P), **Pins** (sidebar), **Activity**, **Settings**, then **Command palette** (commands with no key, unchanged). Bindings that only drive a widget's internals are hidden: text input and text area editing, pickers, action menus, inbox/vote/link menus, the activity splitter, onboarding, tours, the fight-card dialog. Each key context must be either mapped to a group or listed as hidden. An unlisted context fails a unit test, so new views can't silently add a raw-context group.

### C. ⌘P and quieting

- **FR-8 One Quiet mode.** ⌘P has two commands: "Quiet mode: turn on or off" and "Quiet mode for 1 hour". While Quiet mode is on, adoken plays no sounds and shows no celebrations, Ken HUD, K.O. or Ken toasts. In short, today's calm mode plus sound mute. Inbox rows, OS banners and badges follow the notification rules as before. Quiet mode lasts until turned off, or until the hour runs out, and survives a restart the way calm mode does today. These commands are removed: "Calm mode for 1 hour", "Calm mode until tomorrow", "End calm mode", "Toggle sounds", "Mute sounds for 1 hour", "Turn celebrations on", "Turn celebrations off". Settings › Sounds (on/off) and Settings › Celebrations (on/off) stay as the persistent switches. Wherever calm mode had an indicator, it now shows Quiet mode.
- **FR-9 On/off pairs become one toggle.** Two ⌘P commands that set opposite states of the same thing become one command, titled like the existing toggles ("Group reviewers: show or hide", "Only files with threads: turn on or off"): first-use tours on/off, memory and CPU show/hide, Ken streak pause/resume, sync "Pause until resumed"/"Resume" (→ "Sync: Pause or resume"; "Sync: Pause for 1 hour" stays), and Show threads/Show files (one command on the existing key). Commands with three or more states (Activity: sync / network / hide, Threads: active / resolved / all) stay.
- **FR-10 No per-theme rows.** The 33 "Theme: <name>" ⌘P rows are removed. "Theme: Choose…" opens the picker, as before.

### D. Settings and extras

- **FR-11 No VS Code theme import.** Removed: the "Theme: Import VS Code theme…" command, any Settings button for it, and the converter. User themes in the themes folder still load and hot-reload (0007).
- **FR-12 One global shortcut.** The second chord ("Show adoken with search open", `[hotkey] summon_palette`) is removed from Settings › Keyboard, from the registration code and from the settings model. The summon chord, its recorder, clear and reset stay. Amends 0021.
- **FR-13 Sounds: on/off and volume.** Settings › Sounds keeps only on/off and master volume. Removed: the pack picker (Dojo is the one pack), per-category and per-event switches and volumes, "Play sounds while adoken is in the background", and the quiet-hours sound options. The Arcade sounds play only for Ken events while Ken's arcade mode is on. Events that were off by default can't be turned on any more, so they are removed. The removed options' former defaults become fixed rules. No sounds play while adoken is in the background. During notification quiet hours, your own actions keep their feedback sounds and Ken is silent. A notification sound plays only for an urgent (P0) notification that your quiet-hours delivery rule lets through, so that rule ("allow P0 through", 0005) is now the one switch for urgent sounds.
- **FR-14 Old settings files still load.** A `settings.toml` or `keymap.toml` that still has removed keys (`[sound]` pack, category and per-event keys, `[hotkey] summon_palette`, mute-until, calm state) loads without error and without losing the rest of the file. Removed keys are ignored, and adoken never writes them back.

## 5. ADO API surface and limits

None. No requests are added or removed.

## 6. Acceptance criteria

- **AC-1** Given ⌘P is open with results, when ⌘1, ⌘↑ or ⌃N is pressed, then the selection does not jump, and no row shows a ⌘1…⌘9 badge. ↑/↓ still wrap. (palette unit test + gallery snapshot)
- **AC-2** Given three pins, when ⌃1 is pressed, then nothing opens. The rail and sidebar show no pin numbers, and ⌘P `>jump to pin` finds nothing. (pins test + snapshot)
- **AC-3** Given the vote menu is open, when `2` is pressed, then no vote is cast. The same holds for the link-type and Inbox menus. (menu tests)
- **AC-4** ⌘4 opens Work items, ⌘5 Pipelines and ⌘6 Search. The sidebar tooltip for Search reads ⌘6. ⌘\ does nothing, and ⌘B still toggles the sidebar. (shell test)
- **AC-5** ⌘= changes nothing. ⌘P "Increase code font size" still raises it. (code_font test)
- **AC-6** The cheat sheet's groups appear in FR-7 order. No group is named after a raw context ("Text input", "Picker", "Workspace"). Every context in the live keymap is mapped or hidden, and an unknown context fails the test. A key rebound in `keymap.toml` still shows its new key. (help::sheet unit tests)
- **AC-7** Given Quiet mode is turned on, when a sound event, a celebration trigger and a Ken K.O. fire, then none play. After "Quiet mode for 1 hour" and an injected clock advanced 61 minutes, sounds play again. ⌘P contains none of the seven removed commands. (ken/celebrate/sound tests with injected clock)
- **AC-8** ⌘P lists exactly one row each for tours, memory and CPU, the Ken streak, sync pause/resume and threads/files, and no "Theme: <name>" rows. (palette catalog test over `palette_commands()`)
- **AC-9** Settings › Sounds shows only on/off and volume, and Settings › Keyboard shows one global shortcut row. Settings search finds no removed option. (settings model test + snapshots)
- **AC-10** A `settings.toml` with every removed key loads. All other values are kept, and the next save writes none of the removed keys. (settings round-trip test)

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| ⌘P open, cheat sheet open | No regression from today | existing palette bench, gallery smoke |

Removing keys and rows only shortens keymap and catalog work. Global rules (AGENTS.md) apply.

## 8. Success signal (telemetry-free)

The cheat sheet fits on about one screen per view, and ⌘P `>` lists about 50 fewer commands. Nobody asks for the number keys back during dogfooding.

## 9. Risks

- Muscle memory: ⌘4 changes from Search to Work items. Mitigation: the cheat sheet, tooltips and changelog say so.
- Users with removed sound options lose their per-category tweaks. Mitigation: before this spec they were off by default; FR-14 keeps the file loading.

## 10. Open questions — resolved 2026-10-02

- **Q-1 Which digit shortcuts go?** **Resolved:** palette ⌘1–⌘9, pin ⌃1–⌃9, and digits inside menus. Inbox chip digits and section ⌘1–⌘6 stay; section keys move to sidebar order. → FR-1, FR-2, FR-3, FR-5.
- **Q-2 Which aliases go?** **Resolved:** ⌘↑/⌘↓, ⌃N/⌃P and ⌘\. `j`/`k`/`h`/`l` stay. → FR-4.
- **Q-3 Cheat sheet?** **Resolved:** grouped by screen, still generated. → FR-7.
- **Q-4 Cut whole features?** **Resolved:** none. Tours, resource meter, Ken, network log and backgrounds stay.
- **Q-5 Overlaps?** **Resolved:** one Quiet mode, toggles instead of pairs, no per-theme rows, no global code-size keys. Dev/demo commands stay. → FR-6, FR-8, FR-9, FR-10.
- **Q-6 Smaller extras?** **Resolved:** VS Code import and the second global shortcut go. Sounds shrink to on/off and volume with one pack. The Threads view stays. → FR-11, FR-12, FR-13.

## 11. Out of scope

- Removing whole features: the owner kept them all (Q-4).
- User theme folder and hot reload: they stay. Only the VS Code import goes (FR-11).
- `j`/`k`, Inbox chip keys, onboarding 1/2, text-field editing keys (including macOS ⌘←/⌘→): unchanged.

## 12. References

- Audit inventories, 2026-10-02 (shortcut and feature scouts); `docs/roadmap.md` cut line.
