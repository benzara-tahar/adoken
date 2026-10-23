# 0021 — Global hotkey: summon adoken from any app

| Field | Value |
| --- | --- |
| Status | Accepted (2026-10-01, owner: all recommendations; overrides noted) |
| Milestone | v0.1, on the cut line, cut before 0015 slice B (Q-5; pulled forward from v0.2). Windows at v0.3 with the platform port (FR-13) |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/app` (hotkey registration on macOS, summon behavior, Settings › Keyboard section, palette command, cheat sheet row), `crates/gallery` (stories) |
| Serves | **Snappy** (one chord from any app instead of hunting adoken with `⌘Tab` or the Dock) |
| Depends on | `docs/specs/0003-command-palette.md` (Q-1 deferred this; the palette variant, Q-2), `docs/specs/0009-settings.md` (`settings.toml`, Keyboard tab, FR-3 live apply, FR-5 validation; `[hotkey]` listed there, amended in this batch), `docs/specs/0002-sync.md` (foreground cadence on activation), `docs/specs/0010-zeron-reuse-batch-2.md` (zeron reuse and attribution rules) |
| Design | Settings rows, buttons and toasts use the existing Settings and toast components and tokens (`docs/design/tokens.md`); copy is in §3.3. `docs/design/command-palette.md` §7.2 gains one row (§4.4) |

> Rule: no code without an **Accepted** spec (AGENTS.md). The spec owns behavior, acceptance, and budgets; design docs own visuals, motion, sound, and copy.

## 1. Problem

Owner dogfood note (2026-10-01): "Is it possible to have the possibility to use a shortcut to focus the app? Like, Hyper+A should open the app or, if it is open, bring it into focus. … Raycast's new version, v2, has the possibility to map the Caps Lock key to the Hyper key."

Getting to adoken is itself a context switch: `⌘Tab` through a dozen apps, or the Dock, then finding where you were. GPUI key bindings work only while an adoken window has focus, and adoken registers nothing system-wide (no `RegisterEventHotKey` or equivalent anywhere in the workspace). `docs/ideas.md` ranks a global hotkey first (RICE 112), phrased as "opens the palette over the current app"; the owner asks for "open the app or bring it into focus". This spec follows the owner and keeps the palette as an optional second shortcut (Q-2).

## 2. Users and job to be done

- **Persona:** developer who lives in an editor and a terminal and checks Azure DevOps dozens of times a day; often a keyboard power user with a Hyper key (Caps Lock → ⌃⌥⇧⌘ via Raycast's Hyper Key setting or Karabiner-Elements).
- **Job to be done:** "When a ping or a thought sends me to Azure DevOps, I want one chord to bring adoken to the front exactly where I left it, so the detour costs no searching."
- **Smallest delightful slice:** Hyper+A from any app brings adoken's main window to the front with the keyboard where it was, whether adoken was behind, hidden, minimized or had its window closed.
- **Priority (RICE):** `docs/ideas.md` scores 112 for a 0.5-week palette shortcut. This scope (summon behavior across window states, a shortcut recorder, conflict and failure feedback) is ≈ 1 week: Reach 70 · Impact 1 · Confidence 80% · Effort 1 = **56**, still in the top five.
- **Cut:** launching adoken when it isn't running (needs a login item, Q-3), a second hotkey per view, per-hotkey actions beyond summon and summon-with-search.

## 3. UX flow

### 3.1 Happy path

1. adoken runs behind VS Code. The user presses Hyper+A (Caps Lock + A with a Hyper mapping; physically ⌃⌥⇧⌘A).
2. adoken becomes the active app, its main window comes to the front and becomes key, and the keyboard is on the element that had it when adoken was last active (e.g. the fourth Inbox row). `↓` works at once.
3. `⌘Tab` (or a click) returns to VS Code as usual.

### 3.2 Configure

1. `⌘,` › Keyboard (or `⌘P` › "Global shortcut settings…"). At the top, **Global shortcut**:
   - "Show adoken" with the recorder showing `⌃⌥⇧⌘A`, buttons Record, Clear, Reset; a status line.
   - "Show adoken with search open" (Q-2), unset by default. *(Amended 2026-10-02 by 0025 FR-12: removed; Settings › Keyboard shows one global shortcut)*
   - Tip line: "Tip: map Caps Lock to Hyper (⌃⌥⇧⌘) with Raycast's Hyper Key or Karabiner-Elements, then Hyper+A is one finger and a letter."
2. Record (or `↩` on the recorder): "Press the new shortcut…". `Esc` cancels, `⌫` clears. Pressing ⌃⌥⌘K saves `cmd-ctrl-alt-k`, applies at once, and the status line reads "Press ⌃⌥⌘K to test".
3. Pressing it while that Settings tab is in front updates the status line to "Works · last used just now" without switching windows (FR-8).

### 3.3 Copy (sentence case, no emoji)

| Situation | Text |
| --- | --- |
| Status: registered, never pressed since | "Press ⌃⌥⇧⌘A to test" |
| Status: registered and pressed | "Works · last used 2 min ago" |
| Status: off | "Off" |
| Status: registration failed | "Couldn't register ⌃⌥⇧⌘A. Another app may use it; pick another shortcut." |
| Refused: no ⌘ or ⌃ | "Add ⌘ or ⌃ to the shortcut." |
| Refused: adoken binding | "⌘P opens search in adoken. Pick another shortcut." (names the command) |
| Refused: macOS binding | "⌘Tab is used by macOS. Pick another shortcut." |
| Refused: modifiers only | "Add a key, like A." |
| Launch toast after a failure (once per value) | "Global shortcut ⌃⌥⇧⌘A isn't available." Button: "Settings" |
| Cheat sheet row | "Show adoken from any app · ⌃⌥⇧⌘A (global)" |

### States

| State | Behavior |
| --- | --- |
| Empty (shortcut cleared) | Nothing registered; status "Off"; cheat sheet row hidden. |
| Loading / first sync | Summon works during onboarding and first sync: it brings whichever window shows (onboarding or the shell). |
| Error (registration failed) | Status line and one launch toast per value (FR-7); adoken otherwise unaffected. |
| Offline / stale | Unaffected: no network involved. |
| Throttled by ADO | Unaffected. |

## 4. Functional requirements

### 4.1 Registration

- **FR-1 System-wide registration (macOS).** adoken registers each configured shortcut with Carbon `RegisterEventHotKey` on the application event target and receives presses through `InstallEventHandler` (`kEventClassKeyboard` / `kEventHotKeyPressed`), forwarding them to the UI thread through a channel. Pattern and code ported from zeron `crates/ui/src/appshots/macos.rs` (`start_global_shortcut`, `refresh_global_shortcut`, `shortcut_keycode`, `layout_keycode`, `appshot_hot_key_handler`) and `crates/ui/src/appshots/shortcut.rs` (preference, recording), commit `d1010657d69c9730f8020edc36cc2649b136b6fa`, MIT; each ported item is listed in `THIRD_PARTY_NOTICES.md` with source path and commit (AGENTS.md, 0010). No Accessibility, Input Monitoring or other permission is requested.
- **FR-2 Lifetime.** Registration happens after the first frame (never on the cold-start path) and lasts while adoken runs. A changed or cleared value unregisters the old shortcut before registering the new one. Quit unregisters.
- **FR-3 Default.** `cmd-ctrl-alt-shift-a` (⌃⌥⇧⌘A, "Hyper+A"), on from the first launch. Four modifiers make an accidental press unlikely for users without a Hyper key.
- **FR-4 Key by printed character.** The key is stored by its printed character and resolved to a key code against the current ASCII-capable keyboard layout (zeron `layout_keycode`), so "A" is A on AZERTY too. adoken re-resolves and re-registers when the keyboard input source changes.
- **FR-5 Accepted shortcuts.** At least one of ⌘ or ⌃ plus one non-modifier key. Refused with the §3.3 reason: no ⌘ or ⌃ (macOS 15 refuses ⌥-only and ⌥⇧-only global shortcuts, and bare keys would swallow typing in every app); modifiers only; a chord bound in adoken's own keymap in any context (it would never reach adoken again, since a global registration takes the key first); a chord reserved by macOS (`⌘Tab`, `⌘⇧Tab`, `⌘Space`, `⌃Space`, `⌘Q`, `⌘H`, `⌘M`, `⌘W`, `⌘⌥Esc`, `⌘⇧3`, `⌘⇧4`, `⌘⇧5`).

### 4.2 Summon

- **FR-6 Summon behavior.** A press of "Show adoken":

  | adoken state when pressed | Result |
  | --- | --- |
  | Running, main window visible, another app frontmost | adoken activates; the main window comes to the front and becomes key |
  | Hidden (`⌘H`) | Unhides, then as above |
  | Main window minimized | Deminiaturizes, then as above |
  | Main window closed, app still running | Opens the main window as at launch (shell, or onboarding when not connected), then as above |
  | adoken frontmost, Settings window key | The main window becomes key; Settings stays open behind it |
  | adoken frontmost, main window key | Nothing changes (Q-1) |
  | Main window on another Space or in full screen | macOS switches to that Space (Q-4) |
  | A sheet, menu or the palette is open in the main window | Brought to the front unchanged |
  | adoken not running | Nothing (a registration exists only while adoken runs; Q-3) |

  In every case the keyboard returns to the element that had it when the main window was last key. Presses within 300 ms of the previous one are ignored (key bounce, double taps).
- **FR-7 Activation must win.** After a press, adoken is the frontmost app and its main window key within the FR-6 budget (§7), including on macOS 14+ where app activation is cooperative. When the standard activation leaves another app in front, adoken falls back to asking LaunchServices to open its own bundle (zeron `foreground_after_capture`, ported with attribution). A bare `cargo run` without an app bundle uses the standard activation only.
- **FR-8 Testing from Settings.** While Settings › Keyboard is the key window, a press of a configured shortcut only updates its status line ("Works · last used just now") and does not switch windows. While recording, every configured global shortcut is unregistered so the recorder can capture the same chord, and re-registered on save or cancel.
- **FR-9 Summon with search open (Q-2).** A second shortcut, `summon_palette`, unset by default. A press summons as FR-6 and then opens `⌘P` with an empty query and the keyboard in its input; if the palette is already open, it stays open with its query. *(Amended 2026-10-02 by 0025 FR-12: removed: no `summon_palette`; an old value in `settings.toml` is ignored and never written back)*
- **FR-10 No side effects.** A summon sends no Azure DevOps request of its own, plays no sound and adds no animation (window ordering is macOS's). Becoming active has the same effect on sync as any activation (`Shell::activation_changed`, 0002 foreground cadence).

### 4.3 Settings, feedback and commands

- **FR-11 Settings.** `[hotkey]` in `settings.toml` (0009 FR-1, amended in this batch): `summon` (keystroke string in adoken's keymap syntax, default `"cmd-ctrl-alt-shift-a"`, `""` = off) and `summon_palette` (default `""`). Settings › Keyboard starts with the Global shortcut section of §3.2. Changes apply on the next frame (0009 FR-3), including hand edits of the file. A value that fails FR-5 or doesn't parse falls back to the default for that key and is reported (0009 FR-5). *(Amended 2026-10-02 by 0025 FR-12: `[hotkey]` has `summon` only)*
- **FR-12 Failure feedback.** A non-zero status from `RegisterEventHotKey` (or no key code for the character) shows the §3.3 failure status line and, once per distinct value, a launch toast with a Settings button that opens Keyboard at the Global shortcut section. The same failing value on later launches shows the status line only. "Last used" (FR-8 copy) is the user's check for conflicts the system doesn't report (§9).
- **FR-13 Palette and cheat sheet.** `⌘P` › "Global shortcut settings…" opens Settings › Keyboard at the Global shortcut section. The cheat sheet (`?`) lists "Show adoken from any app" and, when set, "Show adoken with search open", marked global. Recording a shortcut happens only in Settings (a recorder needs the keyboard). *(Amended 2026-10-02 by 0025 FR-12: only "Show adoken from any app")*
- **FR-14 Windows (v0.3).** The Windows port registers the same preference with `RegisterHotKey` (`MOD_NOREPEAT`); this spec's ACs re-run there. The Windows default is chosen in that port: Ctrl+Alt+Shift+Win is the Office key, whose letter shortcuts open Microsoft apps (§9).

### 4.4 Amendments to other docs (accepted 2026-10-01; applied the same day)

- **0009** (owned by its editor in this batch): FR-1 lists `[hotkey]` (`summon`, `summon_palette`); §3 Keyboard tab starts with the Global shortcut section; FR-5 per-key fallback for `[hotkey]`. *(Amended 2026-10-02 by 0025 FR-12: `summon` only)*
- **docs/design/command-palette.md §7.2:** new row "`⌃⌥⇧⌘A` (configurable) | Show adoken from any app (system-wide, spec 0021)". §4.5 commands: "Global shortcut settings…".
- **docs/roadmap.md / docs/ideas.md:** "Global summon hotkey" moved from v0.2 into v0.1 per Q-5; ideas row 1's wording becomes "brings adoken to the front; optional second shortcut opens the palette".

## 5. ADO API surface and limits

| Purpose | Endpoint (api-version) | Scope | Cost / limit notes |
| --- | --- | --- | --- |
| — | **None.** | none | 0 TSTU (FR-10). |

- **Rate limits:** none added. Activation follows the existing foreground cadence (0002).
- **No push to local apps:** not applicable.
- **PAT scopes:** none. **macOS permissions:** none (Carbon hot keys need no Accessibility or Input Monitoring grant).

## 6. Acceptance criteria

Harness: a `HotkeyRegistrar` seam around Carbon (register, unregister, status) with a fake that records calls and can fail; a fake summon trigger injected into the gallery shell tests (`crates/gallery/tests/pr_list_nav.rs` pattern); a named scripted check for real system behavior.

- **AC-1 Default registration.** With no `[hotkey]` table, after the first frame the fake registrar receives one registration: US layout key code 0 (A) with ⌘⌃⌥⇧; none before the first frame (launch timer unchanged). Status reads "Press ⌃⌥⇧⌘A to test". (unit)
- **AC-2 Summon matrix.** A pure function from (app active, hidden, main window minimized / closed / key, Settings key, Keyboard tab shown, recording) to the summon action returns each FR-6 row's result; a press 200 ms after the previous one returns nothing, one at 350 ms acts. (unit)
- **AC-3 Real summon (scripted check `hotkey_summon`).** On a signed dev build on macOS 14 or later, with Terminal granted Accessibility for `osascript` only: from Terminal frontmost, `tell application "System Events" to key code 0 using {command down, control down, option down, shift down}` makes adoken the frontmost process (`name of first process whose frontmost is true`) with its main window key within 150 ms; after `⌘H`, after minimizing, and from another app in full screen, within 500 ms (macOS's window animation included); after closing the main window, a main window opens and is key. With adoken frontmost and its main window key, the press changes neither the frontmost app nor the key window. Two observations are recorded, not asserted: the result with Raycast already holding ⌃⌥⇧⌘A, and the result with a password field focused in Safari. The run's numbers go into a "§13 Probe results" section of this spec, as in 0016. (scripted manual check)
- **AC-4 Focus restored.** With the fourth Inbox row focused, activating another app and firing the fake trigger shows the main window with the fourth row focused, and `↓` focuses the fifth. With the Settings window key, the trigger makes the main window key. (smoke)
- **AC-5 Recorder.** Record suspends registration (fake: unregister); pressing ⌃⌥⌘K saves `summon = "cmd-ctrl-alt-k"`, then the fake sees register(⌃⌥⌘K) and no register of the old chord; Record then `Esc` re-registers the previous chord unchanged; `⌫` while recording sets `summon = ""` and unregisters. (unit)
- **AC-6 Validation.** ⌥⇧K → "Add ⌘ or ⌃ to the shortcut."; ⌘P → names adoken's search; ⌘Tab → names macOS; ⌘⌥ alone → "Add a key, like A."; each leaves the stored value unchanged. (unit)
- **AC-7 Failure feedback.** The fake fails with status −9878: the status line shows the failure copy and one toast appears; a relaunch with the same value shows the status line and no toast; changing the value retries registration. (unit)
- **AC-8 Settings file.** `summon = ""` registers nothing and hides the cheat sheet row; `summon = "banana"` falls back to the default and is reported; editing the file while running re-registers on the next frame. (unit)
- **AC-9 Layout.** With an injected French AZERTY layout, `a` resolves to key code 12; switching the input source re-registers with the new code. (unit, zeron's `layout_keycode` tests adapted)
- **AC-10 Test from Settings.** With Settings › Keyboard key, the trigger updates the status to "Works · last used just now" and the key window stays Settings. (smoke)
- **AC-11 Summon with search.** With `summon_palette` set, the trigger from the background shows the main window with the palette open, empty query, keyboard in the input; with the palette open and query "fix", it stays open with "fix". (smoke) *(Amended 2026-10-02 by 0025 FR-12: withdrawn with FR-9)*
- **AC-12 No side effects.** In connected mode against the mock server, 10 summons from the background produce exactly the requests of 10 equivalent activations without the hotkey (same foreground cadence), and no sound event. (mock server + unit)
- **AC-13 Attribution.** `THIRD_PARTY_NOTICES.md` lists every ported zeron item with its source path and commit `d1010657…`; CI's notices check fails without it. (scripted check)
- **AC-14 Gallery.** Settings › Keyboard Global shortcut section: active untested, works, recording, refused, failed, off; one snapshot per built-in theme. (snapshot)

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Press → adoken frontmost with its main window key (window visible behind another app) | < 150 ms on the reference Mac; hidden / minimized add only macOS's own window animation | AC-3 scripted check with timestamps |
| Press → first frame with focus restored | 1 frame (< 16 ms) after activation | Frame recorder in the AC-4 smoke |
| Registration | < 1 ms, after the first frame | Launch timer (cold start unchanged, < 500 ms) |
| Idle cost | Zero CPU and no timers: event-driven | Activity Monitor during the dogfood week |

Global rules (AGENTS.md) always apply: frame < 16 ms, Cmd+P < 50 ms on 100k items, cold start < 500 ms. Regressions are bugs.

## 8. Success signal (telemetry-free)

- **Local stat the user can see:** "Works · last used …" in Settings › Keyboard.
- **Dogfood:** for a week the owner reaches adoken with Hyper+A rather than `⌘Tab` or the Dock, and no note mentions a summon that didn't bring the window forward.
- **Public replies:** build-in-public posts with the Hyper+A clip draw "how did you map Caps Lock" questions rather than "it doesn't work".

## 9. Risks

- **Silent conflicts.** Another app (Raycast, Alfred, BetterTouchTool) may hold the same chord. Carbon reports a duplicate registration with `eventHotKeyExistsErr` (−9878) at least within one process; whether it does across apps is unverified `[INFERENCE]`. Mitigation: the status line's "last used" makes a dead shortcut visible; AC-3 records the behavior with Raycast holding Hyper+A.
- **Cooperative activation (macOS 14+).** `activateIgnoringOtherApps:` is deprecated and activation can be declined. Mitigation: FR-7's LaunchServices fallback, already proven in zeron; AC-3 on macOS 14+.
- **Carbon's future.** `RegisterEventHotKey` is a Carbon API still available and widely used `[INFERENCE: no deprecation of the hot key API announced]`. Mitigation: the `HotkeyRegistrar` seam isolates it.
- **Secure Event Input.** While another app has a password field focused, some global key paths stop working `[INFERENCE: Carbon hot keys are expected to keep working]`. AC-3 includes one press with a password field focused in Safari and records the result.
- **Hyper is not standard.** Without a remapper, ⌃⌥⇧⌘A takes four fingers. Mitigation: the tip line, and any chord with ⌘ or ⌃ is accepted.
- **Windows Office key.** Ctrl+Alt+Shift+Win is Windows' Office key; some letters open Microsoft apps. The v0.3 port picks the Windows default (FR-14).

## 10. Open questions — resolved 2026-10-01

All recommendations accepted by the owner.

- **Q-1 Pressing it while adoken is already in front.** **Resolved:** do nothing (FR-6).
- **Q-2 A second shortcut "Show adoken with search open", unset by default?** **Resolved:** yes, in the same slot (FR-9, FR-11 `summon_palette`). *(Amended 2026-10-02 by 0025 FR-12: reversed: the second shortcut is removed)*
- **Q-3 "Open the app" when adoken isn't running.** **Resolved:** not in this spec; Settings › General "Open adoken at login" (`SMAppService.mainApp`, macOS 13+, default off, opens without showing a window until summoned) is a separate v0.2 item, RICE ≈ 27 (Reach 40 · Impact 0.5 · Confidence 80% · Effort 0.6 weeks) (`docs/roadmap.md` v0.2).
- **Q-4 Main window on another Space.** **Resolved:** switch to its Space (FR-6 table).
- **Q-5 Milestone.** **Resolved:** v0.1, on the cut line, cut before 0015 slice B; it leaves v0.2's list (`docs/roadmap.md`).

## 11. Out of scope

- Launching adoken when it isn't running, login item (Q-3: "Open adoken at login", v0.2).
- More global shortcuts (per view, per action) and actions other than summon and summon-with-search.
- Configuring Caps Lock → Hyper inside adoken (needs a key remapper with Input Monitoring; Raycast and Karabiner-Elements do it).
- Windows (v0.3, FR-14) and Linux (best effort, no global shortcut planned).

## 12. References

- zeron (MIT), commit `d1010657d69c9730f8020edc36cc2649b136b6fa`: `crates/ui/src/appshots/macos.rs` (Carbon registration lines 222-302, key codes 304-381, handler 383-393, LaunchServices foreground 642-669), `crates/ui/src/appshots/shortcut.rs` (preference, recording suspension).
- Apple Developer Forums, "[macOS Sequoia] Using RegisterEventHotkey with option and shift modifiers doesn't work anymore": https://developer.apple.com/forums/thread/763878
- Feedback FB15168205 (Option-only and Option+Shift global shortcuts refused on macOS 15): https://github.com/feedback-assistant/reports/issues/552
