# Ken — the arcade layer

Status: v0.1. Owner: ux-designer. Tokens: [tokens.md](tokens.md) §2.9 (`ken.*`), §6 (`z.ken`), §7 (`spring.bouncy`). Sounds: [sound.md](sound.md) §2.3, §7.3. Celebrations for finished work (§12) live here too and apply with Ken on or off; behavior: `docs/specs/0022-celebrations.md`.

拳 (*ken*) = fist. adoken's playful layer turns review work into a light arcade game: combo counters for review runs, a **K.O.** when your PR merges, daily streaks. It is:
- **Opt-in.** Off by default; offered on the last onboarding screen and in Settings › Arcade mode.
- **Never in the way.** Overlays never take focus, never intercept the pointer, and dismiss on any key (the key still does its normal job).
- **Private.** Everything is computed locally from the store. Nothing is sent to ADO, teammates, or telemetry.
- **Original.** No Capcom / Street Fighter (or any other fighting game) names, moves, characters, sprites, poses, HUD layouts, or audio. See §8.
- **Fair.** It rewards real review work, never speed-approving (§3.1).

## 1. Settings

```toml
[ken]
enabled = false
intensity = "arcade"        # "subtle" (toasts + statusbar only) | "arcade" (HUD, overlays, sparks)
naming = "english_kanji"    # "english" | "english_kanji" (English + small kanji) | "romaji_kanji"
combo = true
ko = true
streak = true
badges = true               # first strike, clean sweep, comeback, assist
screen_shake = true         # arcade only; forced off under reduce motion
particles = true            # arcade only; forced off under reduce motion
working_days = ["mon", "tue", "wed", "thu", "fri"]   # streak calendar; defaults to notification-center working days if set
```

Settings › Arcade mode shows each toggle with a live 200 × 64 preview (same demo as onboarding) and [Reset stats] (confirm dialog: "Reset all arcade stats? Combos, streaks, and K.O. history on this Mac will be cleared." [Reset] [Cancel]).

Stats accrue **only while `enabled = true`**; turning it on later doesn't retroactively score history.

## 2. Surfaces

| Surface | Subtle | Arcade | Placement |
|---|---|---|---|
| Statusbar chip | yes | yes | Statusbar right: combo `×5 疾風` (only during a combo), streak `flame` 12 px + `12` |
| Combo HUD | — | yes | Floating, top-right of the detail pane, inset `space.16`, 132 × 60, `z.ken`, pointer-transparent |
| K.O. overlay | toast instead | yes | Full window, `z.ken`, pointer-transparent |
| Badge toasts | yes | yes | Standard toast stack with a `ken` accent stripe (3 px `ken.primary` left edge) |
| Fight card | yes | yes | Weekly dialog (§5) |

## 3. Mechanics

### 3.1 Hits (what counts)

A **hit** is one qualifying review action by you, confirmed by ADO (not optimistic):

| Action | Move name (English · kanji · romaji) | Qualifies when |
|---|---|---|
| Vote Approve (10) | **Hanko Slam** · 判子 · hanko | You opened ≥ 1 changed file of that PR's latest iteration in adoken, or commented on it |
| Vote Approve with suggestions (5) | **Guiding Palm** · 導 · michibiki | same |
| Vote Wait for author (−5) | **Ready Stance** · 構え · kamae | same |
| Vote Reject (−10) | **Iron Wall** · 鉄壁 · teppeki | same |
| Post a review comment / reply | **Quick Jab** · 突き · tsuki | Comment is ≥ 12 characters and not a duplicate of your previous comment |
| Resolve a thread (fixed / won't fix / closed) | **Clean Cut** · 断 · dan | Thread has ≥ 1 comment from someone else |
| Complete a PR (you press Complete) | **Merge Wave** · 波 · nami | — (also triggers K.O. if it's your PR, §3.3) |
| Mark a changed file Viewed (first time per file version) | **Keen Eye** · 目利き · mekiki | Not on your own PR; toggling a file off and on counts once. Counts as opening the PR's files for vote qualification. |

Reserved for later (needs the `vso.build_execute` scope, not requested in v0.1): **Second Wind** · 再起 · saiki — re-queue a failed pipeline for your PR.

Actions you take in the browser (e.g. with a Read-only token) count when sync sees them, if their timestamp is within the last 15 min and you opened that PR's files in adoken within the last 24 h; they're placed in the combo at their ADO timestamp.

Anti-gaming rules: at most **3 hits per PR per combo**; changing your vote back and forth on the same PR counts once; votes on your own PR don't count; nothing counts during the first sync/backfill.

The move name is flavor text only (HUD caption, toast). It never replaces the real action label in buttons, menus, or the palette.

### 3.2 Combo

- A combo starts at the 2nd hit within the **combo window** (10 min after the previous hit) and extends with each hit; each hit resets the window to 10 min.
- Counter: `×2`, `×3`, … No maximum.
- Rank captions (shown under the numerals; wording per `naming`):

| Hits | English | Kanji · romaji |
|---|---|---|
| ×2–×4 | Flurry | 連打 · renda |
| ×5–×7 | Gale | 疾風 · shippū |
| ×8–×11 | Surge | 怒涛 · dotō |
| ×12+ | Storm | 嵐 · arashi |

  Kanji/romaji must pass a native-speaker review before release (tracked in spec 0007 acceptance).
- **Finish:** when the window expires. If the combo reached ×3+, show the finish flourish (arcade) and a toast "Combo ×7 · Gale — 4 PRs reviewed in 38 min". Below ×3 it simply fades. There is **no "combo broken" message or sound** — a combo ending is never a failure.
- Leaving adoken open overnight doesn't matter; the window is wall-clock, and quiet hours don't extend it.

### 3.3 K.O.

- Trigger: a PR **you authored** transitions to `completed` (merged), detected by sync or by your own Complete, and its `closedDate` is within the last 24 h (no K.O. storms after a long offline gap; older ones are listed in the fight card instead).
- **Double K.O.:** two of your PRs merge within 10 min → one overlay with "DOUBLE K.O." (caption) instead of two.
- **Assist** (badge, not overlay): a PR you approved (10 or 5) merged → toast "Assist · !4821 merged".
- Deferred while you're typing (any text input focused) or while the palette/dialog is open: queued until focus leaves the input or 10 s pass, whichever first; then shown. Never shown during quiet hours in arcade form (becomes a silent toast).
- **Handoff from a celebration** (amended 2026-10-01, owner notes; accepted 2026-10-01 with spec 0022): when the merge is one I completed in adoken (spec 0022 FR-1), the K.O. starts on the frame the merge is confirmed instead of waiting for the next ingest poll, and it is that merge's only celebration (no confetti, no `ui.celebrate_*` sound). The later ingest of the same merge scores it once and shows nothing again (0022 FR-6).

### 3.4 Streak

- A **streak day** = a working day (`working_days`, local time) with ≥ 1 hit.
- The streak counts consecutive streak days; non-working days are skipped (neither extend nor break).
- **Shields:** every 5 consecutive streak days earns 1 shield (max 2 held). A missed working day consumes a shield automatically; with none left, the streak resets to 0 **silently** (no notification, no sad state; the statusbar flame just disappears).
- **Pause:** palette "Pause streak" (vacation) freezes the count until "Resume streak"; statusbar flame shows `pause` glyph.
- Milestones at 5, 10, 25, 50, 100, 250 streak days → badge toast "10-day streak" + `ken.streak`. Other extensions play `ken.streak` once per day at the first hit, no toast.

### 3.5 Badges (toasts)

| Badge | Trigger | Toast copy |
|---|---|---|
| First Strike | You're the first reviewer to vote or comment on a teammate's non-draft PR within 1 h of it being published | "First strike · !4830 Add retry budget" |
| Clean Sweep | Inbox reaches zero through your actions (read/snooze/done), not by filtering; max once per day | "Clean sweep · inbox zero" |
| Comeback | Your PR's latest pipeline goes `failed` → `succeeded` after a push of yours | "Comeback · build green on !4821" |
| Assist | See §3.3 | "Assist · !4817 merged" |

## 4. Visuals and motion

All animation uses tokens; everything snaps under reduce motion (§6). Repeating effects (HUD timer bar, idle glow) run on the shared 30 fps clock and stop when the HUD unmounts.

### 4.1 Combo HUD (arcade)

```
┌───────────────────────────┐
│   ×  7                     │   numerals: SVG glyphs 40 px tall, ken.primary fill,
│   Gale 疾風                │   ken.secondary bevel, ken.outline 3 px stroke
│   ▬▬▬▬▬▬▬▬▬▬▬░░░░          │   timer bar 2 px, ken.secondary → ken.primary in last 60 s
└───────────────────────────┘
```
- Container: no background by default; a `ken.outline` @ 0.55 pill behind the caption for legibility over any background. Caption `type.body_strong` in `ken.secondary`, kanji at `type.small`.
- Appear (×2): opacity 0→1 + translate Y −8→0, `motion.quick`, `ease.out`.
- Increment: numerals SVG scale 1.35→1.0 `spring.bouncy` (≈ 640 ms settle); 4 sparks (§4.4) from the numerals; last move name flashes under the caption for 1.2 s (`fade-in`, then fade out 210 ms).
- Rank up (e.g. ×5): caption crossfade 140 ms + 8 sparks.
- Timer bar: width shrinks linearly over the remaining window.
- Finish (×3+): numerals scale 1.0→1.15 then opacity → 0 over 320 ms `ease.in`, with 12 sparks; `ken.combo_finish` plays. Below ×3: fade out 210 ms, no sound.
- Hidden when the detail pane is narrower than 480 px (statusbar chip still shows).

### 4.2 K.O. overlay (arcade)

Timeline (t = 0 at trigger), all at `z.ken`, pointer passthrough, focus untouched:

| t (ms) | Element | Motion |
|---|---|---|
| 0 | Vignette | radial `ken.outline` @ 0 → 0.35 from edges, 140 ms `ease.out` |
| 0 | "K" glyph | SVG 160 px tall, center-left; scale 1.8 → 1.0, rotate −8° → 0°, opacity 0 → 1; `spring.bouncy` |
| 90 | "." and "O" "." glyphs | same as K, staggered 90 ms each |
| 180 | Impact flash | full-window `#FFFFFF` quad opacity 0 → 0.14 → 0 over 120 ms (light appearance: `ken.outline` 0 → 0.08 → 0) |
| 180 | Screen shake | content layer offset: amplitude 6 px, 4 oscillations, exponential decay over 280 ms (if `screen_shake`) |
| 180 | Sparks | 24 particles radial burst (§4.4) |
| 320 | Caption | `type.title` `text.primary` on a `ken.outline` @ 0.7 pill: "!4821 Retry idempotency keys"; below in `type.small`: "merged into main · 3 reviewers" — `fade-in` 200 ms |
| 1400 | Exit | everything opacity → 0 + letters translate Y 0 → −12, 320 ms `ease.in` |

- Sound `ken.ko` at t = 0 (boom lands with the K).
- Any key, click, or scroll dismisses immediately (exit 90 ms) and the input is still delivered to the app.
- "DOUBLE K.O.": the word "DOUBLE" in `type.title` weight 800 `ken.secondary` appears above the letters at t = 320.
- Subtle intensity (or quiet hours): no overlay; toast with a 24 px K.O. glyph badge: "K.O. · !4821 merged into main".

### 4.3 Statusbar chip

`×7 疾風` (`type.label`, numerals tabular, `ken.primary`; caption `text.secondary`) · `flame` 12 px `ken.secondary` + streak count `text.secondary`. Increment: numerals swap with a 90 ms crossfade (no scale in the statusbar). Tooltip: "Combo ×7 · 6 min left · Streak 12 days (1 shield)".

### 4.4 Sparks

- Shape: original 4-point star SVG (8 px) and 3 px dots, colors `ken.spark` (70 %) and `ken.secondary` (30 %).
- Emission: random angle, speed 180–420 px/s, drag 3.0/s, gravity 600 px/s², lifetime 450–650 ms, opacity 1 → 0 over the last 40 %, scale 1 → 0.6.
- Budget: ≤ 48 live particles window-wide; painted as absolutely positioned SVG elements in one layer; simulation on the 30 fps shared clock, positions interpolated per frame. Must stay within the 16 ms frame budget on an M1 with a video background.

### 4.5 Fight card visuals

See §5 layout; uses the same glyphs and `ken.*` tokens over `bg.overlay`.

## 5. Weekly fight card

- Offered on the first launch of each week (working-week start) if there was ≥ 1 hit last week: toast "Your week in review is ready" [Open]. Also palette "Show fight card".
- Dialog 640 × 400 (`radius.lg`, `elevation.3`):
  - Header: "Week of Sep 15" + 拳 emblem.
  - Stat tiles (4 across): **Hits** 34 · **Best combo** ×9 Surge · **K.O.s** 3 · **Streak** 12 days.
  - Secondary row: First strikes 2 · Comebacks 1 · Assists 5 · PRs reviewed 14.
  - List: K.O.s of the week (PR id + title, merge time).
- **Share image** (build-in-public friendly): [Export image…] renders a 1200 × 675 PNG (X card ratio) with the stat tiles and a small "adoken.dev" wordmark. Export options (all default **off**): "Show repository names", "Show PR titles", "Include my background". Default export shows only numbers. File saved via save dialog; [Copy image] copies to clipboard.

## 6. Reduce motion, quiet hours, Quiet mode

| Condition | Effect |
|---|---|
| Reduce motion | No HUD pops/sparks/shake/overlay; K.O. becomes the subtle toast; HUD numbers update in place; timer bar replaced by "6 min" text. Sounds still play if enabled. |
| Quiet hours active | All `ken.*` sounds silent ([sound.md](sound.md) §4); overlays downgrade to subtle toasts; statusbar chip still updates. |
| Quiet mode | Everything ken-related hidden (HUD, overlays, toasts, chip) and silent; stats keep counting. |

Celebrations (§12) follow the same conditions: reduce motion → static glyph; quiet hours → calm style; Quiet mode → none (spec 0022 FR-7, FR-9, FR-10; spec 0025 FR-8).

## 7. Quiet mode

For demos, screen shares, pairing, focus. One switch (spec 0025 FR-8, replacing calm mode, sound mute and the celebration on/off commands): palette "Quiet mode: turn on or off" and "Quiet mode for 1 hour". While on, adoken plays no sounds and shows no celebrations (§12), Ken HUD, K.O. or Ken toasts; Inbox rows, OS banners and badges follow the notification rules as before. It lasts until turned off or until the hour runs out, and survives a restart. Statusbar shows `moon` 12 px while on (tooltip "Quiet mode until 15:40", or "Quiet mode on"). Shortcut: none by default (bindable). The commands work with Ken off. Settings › Sounds and Settings › Celebrations stay the persistent switches.

## 8. Originality rules (hard)

Enforced in review of every Ken asset, string, and sound:

- **Names never used:** Hadouken/Hadoken, Shoryuken, Tatsumaki (Senpukyaku), Shinku, Metsu, Sonic Boom, Spinning Bird Kick, Yoga Fire/Flame, Tiger Uppercut/Shot, Kikoken, Raging Demon, Shun Goku Satsu, Focus Attack, Super Combo / Ultra Combo; character names (Ryu, Ken Masters, Chun-Li, Guile, Akuma/Gouki, Gouken, Sagat, Bison/Vega, etc.); other franchises' catchphrases ("Finish him", "Fatality", "Flawless victory", "Toasty", "Here comes a new challenger", "Round 1 — Fight!", "You win / You lose", "Perfect" as an announcer call, "Continue?" countdowns, "Get ready for the next battle"). "Combo", "K.O.", "streak", "jab" are generic sports/boxing terms and are fine.
- **"Hadou-style" alternative:** our projectile-flavored move is **Merge Wave** (波 *nami*) — wave imagery drawn from ink-wash brush strokes, never a glowing ball pushed from two palms.
- **Visual tropes never used:** white or red martial-arts gi with a headband, palms-forward energy-ball pose, rising uppercut pose, character portraits with health bars, VS splash screens, pixel-art sprites of fighters, any fighting-game HUD layout.
- **Audio:** no announcer voice, no sampled game audio; built-ins are synthesized from recipes ([sound.md](sound.md) §7.3).
- **Brand:** the 拳 emblem is a brush-stroke kanji, not a character. "adoken" is the product; there is no "Ken" character or mascot.

## 9. Assets (original, in-repo)

| Asset | Path | Spec |
|---|---|---|
| Arcade glyphs `0–9`, `×`, `K`, `O`, `.` | `assets/ken/glyphs/*.svg` | 100-unit grid, 12° forward slant, chunky geometric strokes (stroke weight 18 units), top 30 % bevel band. Three layers per glyph as separate `<path>` groups with ids `fill`, `bevel`, `outline`, colored at runtime with `ken.primary`, `ken.secondary`, `ken.outline` (no baked colors). |
| Spark star | `assets/ken/spark.svg` | 4-point star, concave sides, single path. |
| Emblem 拳 | `assets/brand/emblem.svg` | Brush-stroke kanji drawn in-house, for the fight card. `emblem-strokes.svg` (the 4-path onboarding reveal) is no longer used: the Welcome screen shows the `onboarding-welcome` illustration instead (spec 0001 FR-19; amended 2026-10-01, owner notes; accepted 2026-10-01). |
| Seal 済 (celebration, arcade) | `assets/ken/seal-done.svg` | §12.3. Brush-drawn 済 (*sumi*, "settled") inside a rounded-square seal border with a slightly broken, hand-pressed edge, 100-unit grid. Two `<path>` groups with ids `fill` and `outline`, colored at runtime with `ken.primary` and `ken.outline` (no baked colors). Drawn in-house. |
| Sounds | synthesized from recipes in `crates/sound` | [sound.md](sound.md) §7. |

All assets are original, created for adoken and licensed with it (MIT). No fonts are used for arcade lettering, so there's no font licensing surface.

## 10. Accessibility

- HUD, overlay, sparks: accessibility-hidden (decorative). The statusbar chip is `Role::Status` with label "Combo 7, Gale, 6 minutes left. Streak 12 days."
- Announcements (polite, never assertive): combo finish ≥ ×3 "Combo ended: 7 hits"; K.O. "Pull request !4821 merged"; badges "First strike on pull request !4830". Increments are not announced.
- Overlay contrast: letters are large display (≥ 3:1 via `ken.outline` stroke on any background); captions sit on a `ken.outline` @ 0.7 pill (≥ 4.5:1 for `text.primary`).
- Flash: impact flash ≤ 0.14 opacity, one flash per overlay (well under 3 flashes/second, WCAG 2.3.1).
- Celebrations (§12): confetti, sparks, seal and chip glow are accessibility-hidden. Announcements follow spec 0022 FR-17 (polite, Milestones only; a K.O. merge is announced once).

## 11. Data

- Local table (owned by `crates/store`, see `docs/adr/0001-local-store.md`): `ken_hits(id, at, kind, pr_id, combo_id)`, `ken_combos(id, started_at, ended_at, hits, best_rank)`, `ken_kos(pr_id, at, double)`, `ken_streak(day, state)`.
- Derived from store change events on a background executor; the UI only reads the current combo/streak state.
- Never synced, never exported except by explicit fight-card export.

## 12. Celebrations (spec 0022)

Short effects for finished work of my own: a work item moved forward into a review state (**Step**) or into Committed, Done or a completed state (**Milestone**), and a PR I completed in adoken confirmed merged (**Milestone**). Triggers, tiers, timing, limits and settings: `docs/specs/0022-celebrations.md`. This section owns how they look, move and read. Unlike §1–§11, celebrations **don't require Arcade mode**: the calm style is the default (spec 0022 Q-1); the arcade style replaces it when Ken is on at `arcade` intensity.

### 12.1 Styles

| Tier | Calm (default; Ken off or `subtle`) | Arcade (Ken `arcade`, `particles` on) | Static (reduce motion, rate-limited) |
|---|---|---|---|
| Step | Confetti **puff**, 14 pieces (§12.2) | 12 sparks (§4.4) from the anchor | `sparkles` glyph, 3 s (§12.4) |
| Milestone | Confetti **burst**, 40 pieces + chip glow (§12.2) | **Seal stamp** + 32 sparks + chip glow (§12.3) | `sparkles` glyph, 3 s + toast (§12.5) |
| My PR merged with K.O. on | — (the K.O. replaces it, §3.3) | — | — |

Never: screen shake, vignette, impact flash, full-window overlay, lettering. Those stay K.O.-only, so the K.O. keeps its weight.

### 12.2 Confetti (calm)

- **Pieces:** paper rectangles 4 × 8 px (70 %) drawn as rotated 4-point paths, and dots 4 px (30 %). No image assets.
- **Colors** (cycled per piece, theme tokens only, so every theme recolors them): `accent.fg`, `status.success.fg`, `status.info.fg`, `status.warning.fg`, at opacity 0.9.
- **Puff (Step):** 14 pieces from the anchor's top-center, cone −140°…−40° (upward), speed 160–300 px/s, drag 3.0/s, gravity 900 px/s², spin 360–720°/s, lifetime 550–750 ms, opacity → 0 over the last 35 %. Reads as ≈ 0.7 s.
- **Burst (Milestone):** 40 pieces in two waves (24 at t = 0, 16 at t = 90 ms) from the anchor's top-center, cone −160°…−20°, speed 320–620 px/s, drag 2.2/s, gravity 900 px/s², spin 360–900°/s, lifetime 1100–1500 ms, opacity → 0 over the last 30 %. After its apex each rectangle flutters: horizontal sway ± 6 px at 3 Hz. Reads as ≈ 1.4 s.
- **Apex clamp:** launch speeds scale down so no piece's apex leaves the window (an anchor near the top, like the work item header chip, gets a lower, wider fan instead of pieces vanishing above the titlebar). Pieces leaving the window die.
- **Chip glow (Milestone, calm and arcade):** a 2 px outline in `status.success.fg` around the anchor chip, opacity 0 → 0.8 → 0 over `motion.expressive` (480 ms; in `ease.out`, out `ease.in`). Opacity only: no div scale ([tokens.md](tokens.md) §7.3).
- **Budget:** shared with §4.4: one canvas layer at `z.ken`, simulation on the shared clock with positions interpolated per frame, ≤ 64 live particles window-wide for confetti and sparks together (Ken alone ≤ 48); a burst over the cap is trimmed. Frames are requested only while something is live.

### 12.3 Seal stamp (arcade Milestone)

- Asset `assets/ken/seal-done.svg` (§9): 済 *sumi* ("settled"), 40 px, overlapping the anchor chip's right edge, rotated −6°, `ken.primary` fill with a `ken.outline` 2 px stroke.
- Motion: scale 1.6 → 1.0 and opacity 0 → 1 on `spring.bouncy` (SVG scale is allowed, [tokens.md](tokens.md) §7.3); at t = 120 ms, 32 sparks (§4.4) from the seal's center; hold to t = 900 ms; exit opacity → 0 over `motion.expressive_exit` (320 ms). Total ≤ 1.3 s.
- Step in arcade style is sparks only, no seal.
- The kanji passes the native-speaker review (§3.2) and §8.

### 12.4 Static glyph (reduce motion, rate limit)

Lucide `sparkles`, 14 px, `status.success.fg`, placed `space.4` after the state label inside the anchor chip (work item state chip, PR status chip, or the row's state badge). It appears and disappears with no transition (`motion.instant`) and stays 3 s. Same glyph and color as Today's "Merged today" row (spec 0017), so "finished" looks the same everywhere.

### 12.5 Copy

Sentence case, plain, no emoji, no exclamation marks, no praise words ("Great job", "Congrats").

| Where | Copy |
|---|---|
| Milestone toast, work item (static fallback, or no visible anchor) | Leading `sparkles` 14 px `status.success.fg`. Title "Moved to Done" (the confirmed state name). Body `type.small` `text.secondary`: "#4821 Fix timezone bug in daily settlement job". |
| Milestone toast, PR | Title "Merged into main" (target branch, `refs/heads/` stripped). Body: "!4821 Retry idempotency keys". |
| Announcement (polite) | "Moved work item 4821 to Done" · "Pull request 4821 merged into main" |
| Settings › General | Section "Celebrations". Toggle "Celebrate finished work", help "A short burst when your pull request merges or a work item moves to review or done." Sub-toggles "Pull requests I complete", "Work item state changes". Summary line "Small: Code Review, In Review, Ready for Review, Review, and resolved states. Big: Committed, Done, and completed states." with a link button "Edit in settings file". Search keywords: celebrations, confetti, effects, animation. |
| Palette | "Preview celebration" (on/off lives in Settings and Quiet mode, spec 0025 FR-8) |

Toasts use the standard stack (principles §4): 5 s, no action, no `ken` accent stripe in calm style; arcade style adds the 3 px `ken.primary` stripe like badge toasts (§2).

### 12.6 Sound

`ui.celebrate_step` and `ui.celebrate_milestone` ([sound.md](sound.md) §2.1, recipes §7.1–§7.2), replacing `ui.action_done` for that write. A K.O. plays only `ken.ko`.

### 12.7 Gallery

Stories `celebrate-step`, `celebrate-milestone`, `celebrate-pr`, `celebrate-arcade-milestone`, `celebrate-reduced`, `settings-celebrations`; snapshots per built-in theme at t = 0, 200, 600 ms with a fixed seed (spec 0022 AC-20).
