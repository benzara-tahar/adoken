# Sound

Status: v0.1. Owner: ux-designer. Behavior/acceptance: `docs/specs/0007-*` (sounds) and `docs/specs/0005-*` (notification center: priorities, snooze, delivery windows, quiet hours). Tokens: [tokens.md](tokens.md) §8. Ken events: [ken.md](ken.md).

Principles: **off by default**, opt-in on the last onboarding screen or in Settings › Sounds. Sound never carries information alone — every sound has a visual counterpart. Sounds are short, soft, and original.

## 1. Settings model

```toml
[sound]
enabled = false          # master opt-in
master = 0.6             # master volume 0.0–1.0
```

That is the whole model (spec 0025 FR-13): no pack choice, no per-category or per-event switches or volumes, no background or quiet-hours options. Older files with those keys still load; the keys are ignored and never written back (0025 FR-14). The category and event gains below are fixed built-in values. Fixed rules replace the removed options: no sounds while adoken is in the background, and during quiet hours only `urgent` (P0) sounds that delivery allows (§4).

Final gain for a play = `master × category × event × file.gain`, clamped to 1.0, then a safety limiter keeps output peaks ≤ −1 dBFS (category gains: ui 0.35, notify 0.7, ken 0.6, onboarding 0.5). `enabled = false` or Quiet mode on → nothing plays; with `enabled = false` the audio device is **not opened** (no cold-start cost, no audio session).

## 2. Event map

Default = whether it plays once `sound.enabled = true` (fixed; not a setting). Priority is used for voice stealing (§5). Visual = the always-present counterpart.

### 2.1 `ui` — feedback for your own actions

| Event | Trigger | Default | Gain | Priority | Visual |
|---|---|---|---|---|---|
| `ui.action_done` | ADO confirmed a write you made: vote, comment posted, thread resolved, PR completed, work item field changed, link added or removed (spec 0015; "work item moved" with slice C in v0.2). Once per confirmed write, not per field in a batch. Not for a write that celebrates (spec 0022), which plays its `ui.celebrate_*` sound instead (amended 2026-10-01, owner notes; accepted 2026-10-01) | on | 0.8 | 2 | pending glyph clears |
| `ui.celebrate_step` | A Step celebration (spec 0022): a work item of yours confirmed moved forward into a review or resolved state. Replaces `ui.action_done` for that write | on | 0.8 | 2 | confetti puff / sparks / `sparkles` glyph ([ken.md](ken.md) §12) |
| `ui.celebrate_milestone` | A Milestone celebration (spec 0022): a work item of yours confirmed moved into Committed, Done or a completed state, or a PR you completed in adoken confirmed merged. Replaces `ui.action_done`; never plays with `ken.ko` (the K.O. replaces the celebration) | on | 0.9 | 2 | confetti burst / seal stamp / glyph + toast ([ken.md](ken.md) §12) |
| `ui.error` | A write you made failed | on | 0.9 | 4 | inline error / banner |
| `ui.toggle_on` / `ui.toggle_off` | Toggle switched (settings, palette toggles) | off | 0.5 | 1 | toggle knob |
| `ui.palette_open` | Palette opens | off | 0.4 | 1 | palette |
| `ui.copy` | Link / ID copied | off | 0.5 | 1 | "Copied" toast |
| `ui.snooze` | Notification snoozed | off | 0.6 | 1 | row leaves list |
| `ui.sync_done` | A sync *you started* finished: `⌘R` (also "Sync now" in the Activity drawer's Sync tab, the same action) or Sync this work item `⌘⇧R` | off | 0.5 | 1 | statusbar / work item header "Synced just now" |

### 2.2 `notify` — things that happened to you

Plays only for items that pass the notification center's filters and delivery rules (spec 0005). Items first seen during initial sync or backfill **never** play; only items newer than the last-seen watermark do.

| Event | Trigger | Default | Gain | Priority | Visual |
|---|---|---|---|---|---|
| `notify.mention` | You were @mentioned (PR thread, work item comment) | on | 1.0 | 3 | inbox item + OS notification |
| `notify.review_requested` | You were added as a reviewer | on | 0.9 | 3 | inbox item |
| `notify.pr_comment` | New comment on your PR or a thread you're in | on | 0.7 | 3 | inbox item |
| `notify.pr_approved` | A reviewer approved your PR (vote 10 / 5) | on | 0.8 | 3 | inbox item |
| `notify.pr_changes` | A reviewer set waiting (−5) or rejected (−10) your PR | on | 0.8 | 3 | inbox item |
| `notify.pipeline_failed` | A run you triggered, or for your PR, failed | on | 0.9 | 3 | inbox item |
| `notify.pipeline_succeeded` | A run you triggered, or for your PR, succeeded | off | 0.6 | 2 | inbox item |
| `notify.work_assigned` | A work item was assigned to you | on | 0.7 | 3 | inbox item |
| `notify.nudge` | Smart nudge surfaced (stale PR, forgotten review) — spec 0006 | off | 0.5 | 2 | My Work item |
| `notify.digest` | ≥ 3 notify events coalesced (§5), or a delivery window opened with queued items | on | 0.8 | 3 | inbox count badge |

Token events are silent: `TokenRejected`, `TokenReplaced` and `TokenReplaceFailed` (spec 0001 FR-15, FR-20) play nothing, and the former `notify.token_expiring` is removed with the expiry reminder (spec 0001 FR-18; amended 2026-10-01, owner notes; accepted 2026-10-01).

Notification priority (spec 0005) adjusts plays: `low` → never plays; `normal`, `high` → the event sound; `urgent` → the event sound with gain × 1.2 and may break through quiet hours (§4).

### 2.3 `ken` — arcade layer (requires Arcade mode on, [ken.md](ken.md); played with the Arcade sounds, §7.2)

| Event | Trigger | Default | Gain | Priority |
|---|---|---|---|---|
| `ken.combo_step` | Combo counter increments (×2 and up); playback rate rises +1 semitone per step, capped at +7 | on | 0.6 | 2 |
| `ken.combo_finish` | Combo window expires at ×3 or higher (a "finish" flourish) | on | 0.7 | 2 |
| `ken.ko` | Your PR merged (K.O.) | on | 1.0 | 3 (ducks others −6 dB for its length) |
| `ken.streak` | Daily review streak extended | on | 0.7 | 2 |
| `ken.first_strike` | First review on a teammate's PR within 1 h of it opening | on | 0.6 | 2 |
| `ken.clean_sweep` | Inbox reached zero | on | 0.6 | 2 |

### 2.4 `onboarding`

| Event | Trigger | Default | Gain |
|---|---|---|---|
| `onboarding.complete` | "Start using adoken" pressed, only if sounds were enabled on that screen | on | 0.8 |

(The onboarding previews reuse `ui.toggle_on` and `notify.mention`.)

## 3. Settings and Quiet mode

- **Settings › Sounds:** master toggle ("Sounds") and master volume slider, nothing else (spec 0025 FR-13).
- **Quiet mode** ([ken.md](ken.md) §7, spec 0025 FR-8): palette "Quiet mode: turn on or off" and "Quiet mode for 1 hour" silence every sound, along with celebrations and the Ken layer. There is no separate sound mute.
- Changes apply to the next play; no restart.

## 4. Quiet hours, snooze, delivery windows

Quiet hours / delivery windows / snooze behavior is defined in spec 0005; sound follows this matrix:

| Situation | `ui` | `notify` | `ken` | `onboarding` |
|---|---|---|---|---|
| Normal | plays | plays | plays | plays |
| Quiet hours active | plays (your own actions) | **silent**; `urgent` (P0) plays only if its delivery allows | **silent** | n/a |
| Outside a delivery window (items queued) | plays | silent while queued; when the window opens, **one** `notify.digest` for all queued items | plays | n/a |
| Item snoozed | — | silent while snoozed; when the snooze ends and the item resurfaces, its event sound plays unless quiet hours are active | — | — |
| Quiet mode on ([ken.md](ken.md) §7) | silent | silent | silent | silent |
| adoken in the background | silent | silent | silent | n/a |
| First 3 s after launch / during initial sync / backfill | plays | silent (backlog never plays) | silent | plays |

Celebration sounds (`ui.celebrate_*`) follow the `ui` column, with one exception from spec 0022: when a celebration is rate-limited to its static glyph, `ui.action_done` plays instead (FR-11).

OS notifications (Notification Center banners) are posted **without an OS sound**; adoken plays its own so there is never a double sound. OS Focus / Do Not Disturb detection is v0.2 (macOS `INFocusStatusCenter` needs the Communication Notifications entitlement); until then, adoken's quiet hours are the control.

## 5. Playback engine

- One output stream, opened lazily on the first play after `enabled = true`; closed after 60 s idle.
- On enable, the `adoken-sound` thread renders every built-in clip to mono PCM f32 at 48 kHz (rodio resamples to the device); keep in memory (budget ≤ 16 MB decoded; the built-ins are ≈ 6 MB). Playing is a buffer handoff to the mix bus, no decode on the hot path. Target ≤ 30 ms from event to audible. Disabling sounds stops the thread and frees the device and the pack.
- **Voices:** max 4 concurrent. When full, steal the oldest voice with the lowest priority; never steal a higher-priority voice (drop the new sound instead).
- **Debounce:** the same event id within 120 ms is dropped.
- **Coalescing (`notify`):** the first notify event of a burst plays immediately (0007 AC-9: within 50 ms of its banner); every notify event in the next 2 s is folded into it (0007 FR-7 / AC-10: 5 events within 1 s → exactly 1 sound). The inbox count badge is the visual for the folded events. `notify.digest` is the sound for queued items released when a delivery window opens. (An earlier draft held events 1.5 s and then chose per-event vs digest; that delayed every notification sound past AC-9.)
- **Round-robin:** events with several files cycle without immediate repeats. `pitch_jitter` applies a random playback-rate offset per play.
- **Ducking:** `ken.ko` attenuates other voices by −6 dB for its duration (20 ms ramps).
- All volume changes ramp over 20 ms (no clicks).

## 6. Sound pack format (v0.2)

User sound packs are v0.2 (spec 0007 Q-4); v0.1 has no pack choice (spec 0025 FR-13). The format below is the v0.2 plan.

Location: `<config>/sounds/<pack-id>/` (`<config>` per [tokens.md](tokens.md) §9.1). Install by dropping the folder there, or dragging a `<pack-id>.adoken-sounds.zip` onto Settings › Sounds (validated, then extracted). Hot reload with 200 ms debounce, same as themes.

```
<config>/sounds/koto-night/
  pack.toml
  mention-1.ogg
  mention-2.ogg
  done.wav
  ko.ogg
```

```toml
# pack.toml
name = "Koto Night"
author = "Your Name"
license = "CC-BY-4.0"         # REQUIRED SPDX id; packs without one are refused
version = "1.0.0"
description = "Plucked strings, very soft."
fallback = "dojo"             # "dojo" (built-in fills missing events) | "silent"

[events."notify.mention"]
files = ["mention-1.ogg", "mention-2.ogg"]
gain = 0.9                    # 0.0–1.0, default 1.0
pitch_jitter = 0.02           # ± fraction of playback rate, 0.0–0.1, default 0

[events."ui.action_done"]
files = ["done.wav"]

[events."ken.combo_step"]
files = ["combo.ogg"]
pitch_step = 1.0              # semitones per combo level (ken.combo_step only), default 1.0

[events."ken.ko"]
files = ["ko.ogg"]
```

Validation (issues listed in Settings › Sounds › "Pack issues (n)"):

| Rule | Severity |
|---|---|
| `pack.toml` parse error, missing `name` or `license` | error — pack not loaded |
| Unknown event id | warning (nearest-id suggestion, edit distance ≤ 2) |
| Missing file, unsupported format, decode failure | error for that event → fallback |
| Formats: WAV (PCM 16/24-bit, float 32), OGG Vorbis, FLAC; mono or stereo; any sample rate (resampled at load) | — |
| Duration: ≤ 2.0 s (`ken.ko`, `ken.combo_finish` ≤ 3.0 s; `onboarding.complete` ≤ 4.0 s) | error — event falls back |
| File ≤ 2 MB; pack ≤ 20 MB; ≤ 64 files | error — pack not loaded if pack limit exceeded |
| Peak > −1 dBFS | warning — limiter applied |

Authoring guidance shown in the docs link from Settings: master to around −20 LUFS integrated for notify sounds (−24 for ui), peaks ≤ −3 dBFS, 5 ms fade-in, no silence padding at the start (latency).

## 7. Built-in sounds (original)

Dojo is the one pack; the Arcade recipes play only for `ken.*` events while Ken's arcade mode is on (spec 0025 FR-13). Both are **synthesized from code** (`crates/sound/src/pack.rs` recipes on `crates/sound/src/synth.rs` primitives, deterministic seeds, 48 kHz mono), rendered once at load on the sound thread instead of shipping audio files; `cargo run -p adoken-sound --example audition -- <dojo|arcade> --wav <dir>` writes WAVs for listening. Each clip is mastered to an RMS target per category (ui −24, notify −20, ken −19 dBFS; a proxy for the LUFS guidance in §6) with peaks ≤ −3 dBFS, and every pack is checked against §6 by `Pack::validate` in tests. No samples, no third-party audio, nothing resembling any game's sound effects or announcer. The recipes below are the source of truth.

### 7.1 `dojo` (default) — wood, paper, soft bells

| Event | Recipe |
|---|---|
| `ui.action_done` | Wood tap: noise burst through band-pass 1.8 kHz (Q 6), attack 1 ms, exp decay 60 ms; + sine 1.2 kHz, decay 80 ms at −12 dB. |
| `ui.error` | Two low wood taps 90 ms apart, band-pass 700 Hz then 560 Hz, decay 90 ms. |
| `ui.toggle_on` / `off` | Paper click: noise high-pass 3 kHz, 12 ms; `on` + sine 1.6 kHz blip 30 ms, `off` + 1.1 kHz. |
| `ui.palette_open` | Air swish: pink noise, band-pass sweep 800 → 2400 Hz over 90 ms, −18 dB. |
| `ui.copy` | Single paper click (as toggle, no blip). |
| `ui.snooze` | Sine glide 900 → 600 Hz, 120 ms, soft. |
| `ui.sync_done` | Sine 1320 Hz, decay 180 ms, −10 dB. |
| `notify.mention` | Small bell: sine partials 1046 Hz (C6) + 2.76× + 5.40× (inharmonic bell ratios) at −0, −14, −22 dB, decay 600 ms; second strike at 1318 Hz (E6) after 110 ms. Two variants: second strike E6 or G6. |
| `notify.review_requested` | Bell as above, single strike at 880 Hz, decay 700 ms. |
| `notify.pr_comment` | Soft marimba: sine 784 Hz + 4× partial −18 dB, decay 250 ms. |
| `notify.pr_approved` | Marimba rising third: 659 → 784 Hz, 90 ms apart. |
| `notify.pr_changes` | Marimba falling third: 784 → 659 Hz. |
| `notify.pipeline_failed` | Low wood block 440 Hz band-pass, two hits 140 ms apart, second −4 dB. |
| `notify.pipeline_succeeded` | Marimba 1046 Hz single, decay 200 ms. |
| `notify.work_assigned` | Paper slide (noise band-pass 1.2 kHz, 150 ms swell) + marimba 698 Hz. |
| `notify.nudge` | Very soft bell 1568 Hz, decay 300 ms, −8 dB. |
| `notify.digest` | Three-note bell arpeggio C6-E6-G6, 70 ms apart, decay 500 ms. |
| `ui.celebrate_step` | Two quick wood taps rising (band-pass 1.8 kHz then 2.4 kHz, 70 ms apart, decay 60 ms) + sine 2.4 kHz sparkle, decay 150 ms at −12 dB. ≈ 0.25 s. |
| `ui.celebrate_milestone` | Marimba arpeggio C5-E5-G5-C6 (523 / 659 / 784 / 1046 Hz), 60 ms apart, each decay 300 ms; the last note adds bell partials (2.76×, 5.40× at −16, −24 dB) with decay 900 ms; under it a paper flutter (noise band-pass 4 kHz, 300 ms swell, −24 dB). ≈ 1.1 s. |
| `onboarding.complete` | Bell chord C5-G5-E6 staggered 60 ms, long decay 2.4 s, gentle low-pass 6 kHz. |

### 7.2 `arcade` — bright square/triangle waves (original chip-style)

Plays only for Ken events with Ken's arcade mode on. Same events; timbre = pulse wave (12.5 % / 25 % duty) + triangle bass, with 8-bit style pitch envelopes. E.g. `ui.action_done` = triangle blip 1.5 kHz → 2 kHz over 40 ms; `notify.mention` = pulse 25 % arpeggio C6-G6, 50 ms steps; `notify.pipeline_failed` = pulse 12.5 % descending 440 → 220 Hz over 180 ms; `ui.celebrate_step` = two triangle blips 1.2 kHz then 1.8 kHz, 50 ms apart; `ui.celebrate_milestone` = pulse 25 % arpeggio C5-E5-G5-C6 at 40 ms steps, the last note held 300 ms with 6 Hz vibrato. Every recipe lives in `crates/sound/src/pack.rs` (`arcade`).

### 7.3 `ken.*` recipes (Dojo timbre; with Ken's arcade mode on, the Arcade pulse-wave versions play)

| Event | Recipe |
|---|---|
| `ken.combo_step` | Punchy "tock": sine 520 Hz with pitch drop 1.5× → 1× in 25 ms + noise transient 8 ms; rate +1 semitone per combo level. |
| `ken.combo_finish` | Rising sweep 400 → 1600 Hz (60 ms) into a bright bell hit 1568 Hz, decay 500 ms. |
| `ken.ko` | Low boom (sine 60 Hz → 40 Hz, 400 ms, soft-clipped) + taiko-style hit (noise through low-pass 300 Hz, 150 ms) + after 180 ms a bright gong-like bell (partials 330/891/1782 Hz, decay 1.8 s). No voice. |
| `ken.streak` | Crackle (sparse noise impulses, 250 ms) under a warm rising fifth 440 → 660 Hz. |
| `ken.first_strike` | Fast whoosh (noise band-pass sweep 600 → 3 kHz, 80 ms) + single tock. |
| `ken.clean_sweep` | Broom swish (noise band-pass 2 kHz, 200 ms, amplitude swell) + bell 1318 Hz. |

No announcer voice lines in any built-in pack. User packs may add their own audio, subject to their `license`.

## 8. Accessibility

- Every sound has a visual equivalent (tables above); nothing is conveyed by sound only.
- Screen-reader users: sounds don't replace announcements; VoiceOver speech is never ducked by adoken.
- Quiet mode silences everything from the palette in one step; Settings › Sounds turns sounds off for good.
- Sounds respect the OS output device and system volume; adoken never raises system volume.
