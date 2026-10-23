# 0020 — Guided tours: a short first-use tour per complex view

| Field | Value |
| --- | --- |
| Status | Accepted (2026-10-01, owner: all recommendations; overrides noted) |
| Milestone | v0.1, on the cut line, cut first if v0.1 slips (Q-1). The Today tour ships with spec 0017 and moves with it |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/app` (tour engine, triggers, tour definitions, palette commands, `[tours]` settings), `crates/ui` (coach mark component, anchor element), `crates/gallery` (stories, anchor coverage test, smoke) |
| Serves | **Snappy** (the keyboard paths that make adoken fast are learned in the first session instead of never) · **Less noise** (once per view, 3–5 steps, gone at the first key that isn't a tour key) |
| Depends on | `docs/specs/0003-command-palette.md` (commands, unavailable-command rows), `docs/specs/0004-pr-review.md`, `docs/specs/0005-notification-center.md`, `docs/specs/0009-settings.md` (`settings.toml`, FR-3 live apply, FR-4 hot reload, FR-5 validation), `docs/specs/0015-work-item-editing.md`, `docs/specs/0017-today-redesign.md` (Today anchors), `docs/specs/0018-navigation-history.md` (back/forward; contract C4), `docs/specs/0019-pr-review-polish.md` (file filter, threads-only toggle, Files/Threads switch), `crates/app/src/help` (cheat sheet) |
| Design | `docs/design/tours.md` (coach mark, spotlight, placement, motion, every step's copy, gallery stories). Layout context: `docs/design/principles.md` §2, §6 |

> Rule: no code without an **Accepted** spec (AGENTS.md). The spec owns behavior, acceptance, and budgets; design docs own visuals, motion, sound, and copy.

## 1. Problem

Owner dogfood note (2026-10-01): "Upon first use, it would be nice to have a quick tour about what is possible to do in a given view. Like, for example, if I am in the PR diff editor, we point out what the most relevant sections are and how to use them, shortcuts, etc." adoken is keyboard-first, and its speed lives in single-letter keys that nothing on screen announces: the PR review alone binds 20 keys in its own context (`crates/app/src/review/pr.rs` `bindings`), the inbox 26 (`crates/app/src/inbox/mod.rs`), the palette a query language of sigils and filters (`docs/design/command-palette.md` §3). The cheat sheet (`?` / `⌘/`, `crates/app/src/help/mod.rs`) lists every binding but only answers a question the user already thought to ask, and it shows keys, not where they act. A new user clicks around, finds the app "like the web, but faster", and never learns `n`/`p` threads, `v` viewed, `e` done or `!` scopes. The PR review's Publish / Abandon actions exist but the owner didn't find them (triage `TriagePrReview` item D): discoverability, not capability, is the gap.

## 2. Users and job to be done

- **Persona:** developer on Azure DevOps in their first days with adoken, coming from the web UI, who wants to work at keyboard speed but won't read a manual. Secondary: the returning user who wants a refresher (replay).
- **Job to be done:** "When I open a view for the first time, I want to be shown its 3–5 parts that matter and the key for each, so I can work at full speed from day one without reading a manual."
- **Smallest delightful slice:** open your first pull request; a small card points at the file tree ("j and k move between files; v marks one viewed"), then the diff, the threads, the vote and complete buttons. Four presses of `→` and it's over. Press any other key and it's over too, and that key still does its job.
- **Priority (RICE):** 24 = Reach 80 (every new user hits at least one first use) · Impact 0.75 · Confidence 60% (onboarding tours are often skipped; mitigated by brevity and the no-lost-key rule) · Effort 1.5 weeks (engine, anchors, 5 tours, tests). Slot: Q-1.
- **Cut:** a tour for every view (see §3.1 for the views left out and why), "what's new" tours on updates (§11), interactive tutorials that wait for the user to perform the action (§11), video or animated demos (§11).

## 3. UX flow

Visuals, placement, motion and copy: `docs/design/tours.md`. Shortcuts were checked against every `KeyBinding::new` in `crates/app/src/**` on 2026-10-01: the tour binds keys only in its own `Tour` key context, which exists only while a tour shows, so it shadows nothing at any other time. `⌘K` stays the row action menu (`crates/app/src/pin_actions.rs`); tours get no global shortcut: they are replayed from `⌘P`.

### 3.1 Which views get a tour

Chosen by complexity: how many keys act in the view's own context, how many distinct regions it has, and whether a region's meaning isn't obvious from the web UI.

| View | Own keys (code, 2026-10-01) | Regions | Not obvious | Tour |
| --- | --- | --- | --- | --- |
| **PR review** | 20 in `PrReview` (+ `/`, `⇧T`, `t` with 0019) | header, file tree, diff, threads, actions | viewed state, thread keys, vote/complete/status keys | **Yes, 5 steps** (owner's example) |
| **Command palette** | 22 in `Palette` + a query language (`!` `#` `@` `>`, 20 filter keys) | input, results, footer | sigils, filters, `⌘K` actions, commands live here | **Yes, 4 steps** |
| **Inbox** | 26 in the inbox list (`1`–`5`, `⇧1`–`⇧4`, `e` `u` `s` `m` `⇧M` `z` `r` `c`, `⌘⌫`, `⌘⇧,`) | filters, list, footer, rules | number-key filters, triage keys, quiet hours | **Yes, 4 steps** |
| **Work item detail** | 19 in the detail (`t` `s` `a` `i` `⇧A` `⇧I` `d` `l` `c` `o` `y` `⇧Y` `⌘⇧R` …) | state, fields, description and links, freshness, discussion | single-letter editing, Sync this item, freshness | **Yes, 5 steps** |
| **Today** (0017) | 14 in `Today` | day summary, PRs, Up next, Waiting on you, Loose ends | what the mood picture and stage bars mean, nudge keys | **Yes, 5 steps**, ships with 0017 |
| PR list | 14 in `PrList` | tabs, list | no: the same list pattern as the other lists; `r` repository filter shows its key on its button | No |
| Work items list | 9 in the list | tabs, path filter, list | no (the path filter becomes a picker; 0008 amendment) | No |
| Pipelines | 16 across list and run detail | list, run detail | no: read-mostly | No |
| Settings, Activity drawer | searchable / opened deliberately, every control labeled with its key | — | no | No |

### 3.2 Happy path (first use)

1. The user opens a pull request for the first time (from the list, the inbox, Today or `⌘P`). The review paints from the store as today.
2. Once the review is ready (header painted, file list loaded) and the user has pressed nothing for 1 s, step 1 appears: a coach mark (title, one or two sentences with the keys, "1 of 5", **Back** · **Next** · **Skip tour**) beside the spotlighted region, which keeps working (no scrim, Q-4). Focus moves to **Next**.
3. `→`, `↩` or `Space` (or a click on Next) moves to the next region. `←` goes back. On the last step, Next reads **Done**.
4. The tour ends on Done, `Esc`, Skip tour, or **any other input**: a key the tour doesn't use, a click or scroll outside the coach mark. That input is then handled exactly as if no tour were showing: `j` during step 2 still selects the next file (FR-6). Focus returns to where it was before the tour.
5. The tour never starts again by itself for that view. `⌘P` › "Tour: Pull request review" replays it any time.

### 3.3 Alternate paths

- **Palette.** The first `⌘P` (empty query, no scope chip) after onboarding starts the palette tour inside the palette. Typing ends it and the typed characters land in the query; nothing typed is lost (FR-6, Q-2).
- **Replay.** `⌘P` › "Tour: <view>" (Today, Inbox, Pull request review, Work item, Command palette) or "Tour: This view". It runs regardless of the seen flags and the master switch. If the view isn't on screen, the command switches to it first (an ordinary section switch: one history entry per spec 0018, contract C4); the tour itself never adds history entries.
- **Precondition not met.** "Tour: Pull request review" with no review open, or "Tour: Work item" with no work item showing, is shown disabled with the reason "Open a pull request first" / "Select a work item first" (palette unavailable-command row, `docs/design/command-palette.md` §4.3). "Tour: This view" on a view without a tour: disabled, "No tour for this view".
- **Turn off / reset.** Settings › General: "Show tours the first time you open a view" (switch, default on) and **Show tours again** (clears every seen flag). Palette: "Tours: Show again on first use", "Tours: Turn off first-use tours" / "Turn on…" (toggle title shows state, command-palette.md §4.5). *(Amended 2026-10-02 by 0025 FR-9: one toggle command, titled "Tours: turn on or off"-style like the other toggles)*
- **Shortcuts reference.** Every coach mark's footer says "All shortcuts ?". Pressing `?` ends the tour (FR-6) and opens the cheat sheet, as it would have anyway.

### States

| State | Behavior |
| --- | --- |
| Empty (view has nothing: inbox zero, Today on a brand-new org) | No auto start; the first use is deferred to the first visit with content (FR-2). Replay runs on the steps whose anchors are visible (FR-8), else is disabled with "Nothing to show yet". |
| Loading / first sync | No auto start while the view shows skeletons, a loading bar instead of the PR header, or "Not synced yet". Today waits for the first sync to finish. |
| Error (view failed to load) | No auto start; replay disabled with the view's error as reason. |
| Offline / stale | No effect: tours read nothing from Azure DevOps. Steps about write actions still describe them (the action's own offline state applies when used). |
| Throttled by ADO | No effect. |
| Demo mode | Tours run as in connected mode (demo is a first-use path); seen flags live in the demo directory's `settings.toml` (0009 FR-1), so they don't leak into the real install (Q-6). |
| Read-only token | Steps keep their regions; key clauses for write actions the token can't do are replaced by the view's existing read-only hint text ("Review actions need Code (Read & write)", 0004 §3 item 8). |

## 4. Functional requirements

### 4.1 Trigger and lifecycle

- **FR-1 Tours.** Five tours with stable ids: `review`, `palette`, `inbox`, `work-item`, `today`. Each has 3–5 steps; each step has an anchor id, a title, a body with key clauses, and a preferred side. The definitions, copy and anchors are in `docs/design/tours.md` §5 (single source); the code holds them as static data.
- **FR-2 First-use trigger.** A tour auto-starts when **all** hold:
  1. onboarding is complete (the shell route is showing; never on onboarding screens) and the post-onboarding hint (`crates/app/src/lib.rs` `HINT_TOAST`, 6 s) is not showing;
  2. `[tours] auto_start` is true and the tour's id is not in `[tours] seen`;
  3. the view is **ready**: `review` = header and file list painted (not the opening / not-synced / failed bar, 0004 §3.1); `work-item` = a work item detail painted with its fields (not "not available", not partly synced, 0015); `inbox` = loaded with ≥ 1 row; `today` = first sync done and ≥ 2 of its anchors visible; `palette` = opened by `⌘P` (`palette::Toggle`) with an empty query and no scope chip;
  4. no other overlay owns the keyboard: palette (except for the palette tour), cheat sheet, any menu, picker, sheet or confirm dialog, a composer or inline editor with focus, the Settings window as key window, a Ken K.O. overlay;
  5. the main window is key and visible;
  6. no input reached the window for 1 s (the palette tour: 0 s, it starts on open);
  7. no tour ended in the last 3 minutes (cooldown, Q-3);
  8. at least 2 of its steps have a visible anchor (FR-8).
  If the view stops being shown before these hold, nothing happens and the next visit tries again.
- **FR-3 Once per view.** The tour's id is added to `[tours] seen` when its first step shows, so a crash or quit mid-tour never re-shows it. The write follows 0009 FR-2 (key-wise merge, atomic, off the UI thread). Hot reload (0009 FR-4): adding an id externally suppresses that tour; removing it re-enables it.
- **FR-4 One at a time.** At most one tour shows per window. A tour never starts while another runs. Replay of a tour while another runs ends the running one first.
- **FR-5 Ending.** The tour ends on: Done; `Esc`; Skip tour; any input FR-6 passes through; the view being replaced (section switch, PR closed, palette closed); an overlay opening; the window closing. Ending restores focus to the element that had it when the tour started, or, if that element is gone, to the view's main list or the review's diff.

### 4.2 Input

- **FR-6 Keys and no lost input.** While a tour shows, the `Tour` key context binds only: `→` next, `←` back, `↩` / `Space` activate the focused coach-mark button (Next by default), `Tab` / `⇧Tab` move between Back, Next and Skip tour (wrapping, no exit, principles §6), `Esc` end. **Any other keystroke** ends the tour and is then dispatched as if the tour had never shown (same action, same target, same text insertion). A pointer down, scroll or drag outside the coach mark ends the tour and is delivered normally. Clicks inside the coach mark act on it only.
- **FR-7 Palette tour and text input.** In the palette tour the input keeps its caret; any printable character, `⌫`, paste or IME composition ends the tour and is applied to the query. `↩` / `→` / `←` / `Esc` are tour keys while the tour shows (the query is empty, so nothing is lost); `Esc` ends the tour and leaves the palette open.

### 4.3 Anchors

- **FR-8 Anchor visibility.** A step's anchor is a stable string id (`review.file-tree`) attached to an element by the owning view. An anchor is **visible** when it painted in the current frame and at least 50% of its area, and at least 24 × 24 px, lies inside the window's content area (excluding the titlebar and statusbar) and inside its scroll container's viewport. A tour's step list is filtered to visible anchors when it starts; the counter counts those ("2 of 4"). The tour never scrolls, expands or opens anything to reveal an anchor.
- **FR-9 Following the anchor.** When the anchor moves (resize, layout change, the Activity drawer opening), the coach mark re-places on the next frame. When the current step's anchor stops being visible, the tour advances to the next visible step, or ends if none remains (without the user's input being consumed).
- **FR-10 Stable anchors.** Anchor ids are constants exported by the view modules (`crates/app/src/review`, `inbox`, `today`, `workitems/detail`, `palette`), named `<view>.<region>`. Renaming or removing an anchored element is a change to this spec's §6 test fixtures: the anchor coverage test (AC-7) fails until `docs/design/tours.md` and the tour definition are updated. A spec that adds, removes or re-keys an element or key named in a tour updates that tour in the same change (0017 and 0019 do so for Today and the review).
- **FR-11 Zero cost when idle.** With no tour showing, an anchor attachment does no work beyond one check of a window-global flag; bounds are recorded only while a tour shows, or in the anchor test.

### 4.4 Keys in copy

- **FR-12 Live keys.** Every key named in a step body is a reference to an action (`review::NextFile`), rendered as a keycap from the live keymap with the cheat sheet's formatter (`crates/app/src/help/mod.rs` `binding_infos`), so copy and bindings can't drift. A referenced action without a binding in the step's view context is a test failure (AC-7), never a blank keycap at runtime.
- **FR-13 Access level.** A key clause marked as a write (vote, complete, status, comment, edit) is replaced by the view's read-only hint when the token's access level is Read-only (0001 §5.2), as in §3 States.

### 4.5 Replay and settings

- **FR-14 Commands.** Palette commands (category "Help", aliases "tour", "guide", "walkthrough"): "Tour: Today", "Tour: Inbox", "Tour: Pull request review", "Tour: Work item", "Tour: Command palette", "Tour: This view", "Tours: Show again on first use", "Tours: Turn off first-use tours" (title flips to "Turn on" when off). A tour command for another view switches to it first (§3.3); "Tour: Command palette" closes the palette as every command does, reopens it empty and starts the palette tour. Replay ignores `seen`, `auto_start` and the cooldown, needs ≥ 1 visible step, and does not change `seen`. The cheat sheet lists these commands with the others (it is generated from the palette's commands, `help::command_infos`); it gains no other change. *(Amended 2026-10-02 by 0025 FR-9: the turn-off/turn-on pair is one toggle command)*
- **FR-15 Settings.** `[tours]` in `settings.toml` (0009 FR-1, amended by the 0001/0009 task): `auto_start` (bool, default `true`), `seen` (array of tour ids, default empty). Validation (0009 FR-5): a non-array `seen` or non-string entries fall back to empty and are reported; unknown ids are kept (a newer build may know them) and ignored. Settings › General: the switch "Show tours the first time you open a view" and the button **Show tours again** (disabled while `seen` is empty).

### 4.6 Presentation

- **FR-16 Non-blocking presentation.** No scrim: the spotlighted region is outlined, nothing else is dimmed, and the rest of the window keeps receiving pointer input (Q-4). The coach mark sits beside its anchor, flipping sides to stay inside the window with an 8 px margin; only when no side has room (pane-sized anchors such as the diff) does it sit inside the anchor's top-right corner (`docs/design/tours.md` §3). At the 900 × 600 minimum window and `font_scale` 1.3 every step of every tour places (AC-12).
- **FR-17 Motion.** The coach mark's entrance and step changes use existing motion tokens; under Reduce Motion (`cx.reduce_motion()`) every change snaps (tokens §7.4). Nothing loops or pulses, in any mode. Sound: none.
- **FR-18 Accessibility.** The coach mark is a dialog (`Role::Dialog`) labeled "<Tour name> tour, step 2 of 5: <step title>", its body as description, and is announced when it shows (focus moves to Next). Buttons are labeled "Back", "Next" / "Done", "Skip tour". Keycaps carry spoken labels ("Shift V"). Contrast uses existing token pairs only (`text.*` on `bg.overlay`, `border.focus` ≥ 3:1 for the spotlight outline). Text reflows for `font_scale` 0.9–1.3.

### 4.7 Amendments to other docs (accepted 2026-10-01; applied the same day)

- **0009 FR-1 / §3 General:** `[tours]` table and the two controls (applied by the 0001/0009 task, 2026-10-01).
- **`docs/design/command-palette.md` §4.5:** a "Help" command family with the FR-14 titles; §7.2 unchanged (no new global key).
- **`docs/design/principles.md` index:** add `tours.md`. **`docs/design/tokens.md` §5.2:** add `size.coach_mark` 320 (`docs/design/tours.md` §9).
- **0017 / 0019:** declare the anchors and keys of §5 of `docs/design/tours.md` for Today and the review (agreed with their authors 2026-10-01).

## 5. ADO API surface and limits

| Purpose | Endpoint (api-version) | Scope | Cost / limit notes |
| --- | --- | --- | --- |
| — | **None.** Tours read local UI state and `settings.toml` only. | none | 0 TSTU |

- **Rate limits:** zero requests; a tour never triggers a fetch or a sync.
- **No push to local apps:** not applicable.
- **PAT scopes:** none added. Write-action clauses follow the stored access level (FR-13).

## 6. Acceptance criteria

Harness: gallery stories on the seeded demo store at `DEMO_NOW` with an injected clock and a temp `settings.toml`; key dispatch through the real keymap; reduce motion on unless stated.

- **AC-1 First use, once.** Given empty `[tours]`, when `review-open` is shown and 1 s passes with no input, then step 1 of `review` is visible and `settings.toml` has `seen = ["review"]`; when the PR is closed and another PR opened, no tour shows. (unit + story harness)
- **AC-2 Gates.** No tour while: the onboarding route shows; the post-onboarding hint shows (the tour starts 1 s after it hides); the cheat sheet, a menu, the Complete sheet or a composer is open (starts 1 s after it closes, if the view is still shown); the window is not key; a key was pressed 0.5 s ago (starts 1 s after the last input); `auto_start = false` (never); the inbox has 0 rows (starts on the first visit with ≥ 1 row). Cooldown: Today tour ends at T; Inbox shown at T + 60 s → no tour; Inbox shown again at T + 181 s → Inbox tour. (unit, injected clock)
- **AC-3 Keys.** In the `review` tour: `→` ×2 shows step 3 "3 of 5"; `←` shows step 2; `Tab` ×3 cycles Back → Next → Skip tour → Back; `↩` with Skip focused ends it; on the last step Next reads Done and `↩` ends it; `Esc` on any step ends it. (unit)
- **AC-4 No lost input.** During `review` step 2, `j` ends the tour and the selected file is the same as pressing `j` with no tour (index +1); `⇧V` ends it and opens the vote menu; a click on a file row ends it and selects that file; a scroll over the diff ends it and scrolls. During the palette tour, typing `fix` gives query `fix` and no tour; `⌘V` pastes into the query; `Esc` ends the tour and the palette stays open. (unit + story harness)
- **AC-5 Hidden anchors.** On a PR with no threads, `review` shows 4 steps ("1 of 4") and skips the thread step; a view where only 1 anchor is visible doesn't auto-start and `seen` stays unchanged; replay of a tour with 0 visible anchors is disabled with its reason. (unit)
- **AC-6 Moving anchors.** Resizing the window from 1440 to 900 during `review` step 2 re-places the coach mark beside the file tree on the next frame (bounds adjacent, not overlapping the anchor, inside the window by ≥ 8 px); hiding the current anchor advances to the next visible step without consuming input. (unit: layout assertions)
- **AC-7 Anchor and key coverage.** `crates/gallery/tests/tour_anchors.rs`: for every step of every tour, render its declared story (`review-open`, `palette-my-work`, `inbox`, `work-item-detail`, and 0017's busy-day story) with anchor recording on; each anchor is visible per FR-8. Every key reference resolves to a binding in the app keymap for the step's view context. Copy lint: title ≤ 32 characters, body ≤ 140 characters (keys counted as their label), sentence case, no exclamation mark outside literal query text (`!4821`), no emoji, ≤ 5 key references per step (a range `1–5` counts once), 3–5 steps per tour. (test, runs in CI)
- **AC-8 Replay.** "Tour: Pull request review" with a review open starts it with `seen` already containing `review` and leaves `seen` unchanged; from the Inbox it switches to Pull requests (one history entry, 0018) and starts; with no review open it is listed disabled with "Open a pull request first"; "Tour: This view" on Pipelines is disabled with "No tour for this view"; "Tours: Show again on first use" empties `seen`. (unit: palette gates)
- **AC-9 Settings.** `seen = 3` → treated as empty and reported (0009 FR-5); `seen = ["review", "future-tour"]` → `review` suppressed, `future-tour` kept on the next write; an external edit adding `inbox` suppresses the Inbox tour within one hot-reload poll (0009 FR-4); the Settings switch and button write the table and apply on the next frame. (unit: settings)
- **AC-10 Reduce motion.** With reduce motion on, showing the tour and three step changes run no animation (no `with_animation` element in flight in any frame); with it off, they use the `docs/design/tours.md` §4 motions and no animation loops after it settles. (unit: frame recorder)
- **AC-11 Accessibility.** The coach mark exposes `Role::Dialog` with the FR-18 label; focus is on Next after each step change; after `Esc`, focus is back on the element focused before the start (the PR list row in the open-from-list case). (unit: a11y tree)
- **AC-12 Gallery.** Stories, one snapshot per built-in theme: `tour-review-files` (step 2 over `review-open`), `tour-review-done` (last step), `tour-palette` (step 2 inside the palette), `tour-inbox-filters`, `tour-work-item-state`, `tour-today-summary`, `tour-narrow` (900 × 600, flipped placement), `tour-font-scale` (1.3). (snapshot test)
- **AC-13 Smoke.** `crates/gallery/tests/tours_smoke.rs` drives the real shell on the demo store with fresh settings: Today tour auto-starts after the first sync stand-in, `→` to Done; open a PR after the injected cooldown, step 2, press `j` (tour ends, file advanced); `⌘P` replays "Tour: Inbox"; frame p99 < 16 ms throughout; screenshots in `target/tours-smoke/`. (scripted smoke run)

## 7. Performance budget

| Interaction | Budget | Measured how |
| --- | --- | --- |
| Anchors with no tour showing | One flag check per anchor per frame; no allocation; frame time on `review-open` unchanged within noise (< 1%) | Frame recorder before/after, bench of the anchor element |
| Trigger met → coach mark painted | ≤ 1 frame (< 16 ms) | Frame recorder |
| Step change, re-place after resize | ≤ 1 frame | Frame recorder |
| Ending + re-dispatching the key (FR-6) | The re-dispatched action lands in the same frame as without a tour | Unit (AC-4) + frame recorder |
| Frame p99 with a tour on the 300-file PR fixture (0004 AC-3) | < 16 ms | Perf harness |
| `seen` write | Off the UI thread (0009 FR-2) | Unit |
| Cold start | No change (< 500 ms): definitions are static data; `[tours]` is read with the rest of `settings.toml` | Launch timer |
| Memory | < 64 KB for definitions and state | Perf harness |

Global rules (AGENTS.md) always apply: frame < 16 ms, Cmd+P < 50 ms on 100k items, cold start < 500 ms. Regressions are bugs.

## 8. Success signal (telemetry-free)

- **Local, visible to the user:** none needed; the seen list in `settings.toml` is the only state, and it stays on the Mac.
- **Fresh-account check (dogfood):** before release, the builder runs adoken in a fresh macOS user account in demo mode, takes no other help, and after 15 minutes can perform without the mouse: next file and mark viewed, next thread and reply, vote, mark an inbox notification done, snooze it, change a work item's state, scope a palette search to PRs. The result is recorded once in a "Verification" section appended to this spec (as 0016 §13 does).
- **Support signal:** "how do I vote / see threads / publish a draft with the keyboard?" questions from beta users stop appearing for toured actions; questions arrive with shortcut names in them.
- **Owner signal:** the owner's 2026-10-01 discoverability misses (status, draft, abandon) are found by the next new tester through the review tour's last step without help.

## 9. Risks

- **Tours as noise.** A coach mark on every first visit in the first hour annoys power users. Mitigations: 5 tours only, ≤ 5 steps, 1 s idle before start, 3 min cooldown, ends on any other input with the input kept, a master switch, never over overlays.
- **Stale copy.** Keys or regions change and the tour lies. Mitigations: keys rendered from the live keymap (FR-12), the anchor and key coverage test (AC-7), and FR-10's rule that the changing spec updates the tour.
- **Moving targets.** 0017 (Today) and 0019 (review) are being specified at the same time. Mitigations: anchor ids and keys agreed with their authors on 2026-10-01; the Today tour ships with 0017; 0019's clauses join the review tour when 0019 ships (`docs/design/tours.md` §5.1 marks them).
- **Re-dispatch fidelity.** Replaying a keystroke after ending the tour must behave exactly like the original (key context, IME, repeat). Mitigation: the tour layer only observes and ends; it doesn't consume non-tour keys, so they propagate through the normal dispatch path (FR-6). AC-4 covers actions, text, paste and pointer.
- **Anchor bookkeeping cost.** Mitigation: FR-11, recording only while a tour shows.

## 10. Open questions — resolved 2026-10-01

All recommendations accepted by the owner.

- **Q-1 Slot.** **Resolved:** v0.1, on the cut line and cut first if v0.1 slips; RICE 24 (Reach 80 · Impact 0.75 · Confidence 60% · Effort 1.5 weeks). The Today tour ships with 0017 (`docs/roadmap.md`).
- **Q-2 Palette tour starts on the first `⌘P`.** **Resolved:** yes, on the first plain `⌘P` with an empty query; typing ends it and lands in the query (FR-7).
- **Q-3 Cooldown between auto tours.** **Resolved:** 3 minutes (FR-2 item 7).
- **Q-4 Presentation.** **Resolved:** spotlight outline without a scrim; everything stays clickable and any outside input ends the tour (FR-6, FR-16).
- **Q-5 Ending marks seen.** **Resolved:** yes, any end counts as seen; replay is from `⌘P` (FR-3).
- **Q-6 Demo mode.** **Resolved:** tours run in demo mode with their own seen flags in the demo directory; recordings turn them off with the Settings switch.
- **Q-7 Changed tours.** **Resolved:** no re-show; the tour stays seen and the replay picks up new steps. A "what's new" mechanism waits in `docs/ideas.md` (§11).

## 11. Out of scope

- Tours for the PR list, Work items list, Pipelines, Settings and the Activity drawer (§3.1). Revisit if beta questions cluster on one of them.
- "What's new" tours or badges after updates (Q-7, `docs/ideas.md`). Release notes cover changes (`CHANGELOG.md`).
- Interactive tutorials that wait for the user to perform each action, practice sandboxes, video walkthroughs.
- A global shortcut for tours (`⌘K` is the row action menu; tours are rare, so `⌘P` is enough).
- Onboarding screens: no tour runs there; onboarding has its own design (`docs/design/onboarding.md`).
- Per-step "don't show again" or step-level seen tracking.

## 12. References

- Cheat sheet implementation: `crates/app/src/help/mod.rs`, `crates/app/src/help/sheet.rs`.
- Key bindings surveyed: `crates/app/src/review/pr.rs` `bindings`, `crates/app/src/review/mod.rs`, `crates/app/src/inbox/mod.rs`, `crates/app/src/today/mod.rs`, `crates/app/src/workitems/detail/mod.rs`, `crates/app/src/palette/mod.rs`, `crates/app/src/pin_actions.rs` (`⌘K`).
- Post-onboarding hint: `crates/app/src/lib.rs` (`HINT_TOAST`, `show_shell`).
- Gallery stories used by AC-7: `crates/gallery/src/lib.rs` `STORIES`.
- No Azure DevOps REST reference applies (§5).
