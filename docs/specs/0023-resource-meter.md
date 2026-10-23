# 0023 — Resource meter: adoken's own memory and CPU in the statusbar

| Field | Value |
| --- | --- |
| Status | Accepted (2026-10-01, owner: all recommendations; overrides noted) |
| Milestone | v0.1, on the cut line right after 0020 (cut second if v0.1 slips; Q-1) |
| Owner | product-manager (spec) · gpui-engineer (build) |
| Crates | `crates/app` (sampler, statusbar item, menu, palette commands, `[activity] resource_meter`), `crates/perf` (bench of the sampler), `crates/gallery` (stories) |
| Serves | **Snappy** (performance is a feature, AGENTS.md: the owner and beta users see the app's footprint at a glance and catch regressions the day they land) · **Less noise** (off by default, rounded, changes at most every 5 s, never colored) |
| Depends on | `docs/specs/0016-statusbar-activity.md` (FR-5 order, FR-6 repaint throttle, AC-S3, FR-28 `[activity]`; contract C5), `docs/specs/0009-settings.md` (General tab, `settings.toml`), `docs/specs/0003-command-palette.md` (commands) |
| Design | `docs/design/activity.md` §2.1 (order), §2.5 (width priority), §2.7 (resource meter: visuals, copy, tooltip, menu) |

> Rule: no code without an **Accepted** spec (AGENTS.md). The spec owns behavior, acceptance, and budgets; design docs own visuals, motion, sound, and copy.

## 1. Problem

Owner dogfood note (2026-10-01): "It would be nice if we could see the amount of RAM consumed by the current application, I mean the adoken application, somewhere either in the top bar or at the bottom bar, just to have an idea about how much memory this application is consuming. I'm not sure if we can do the same for the CPU because I'm not sure we can get CPU usage per process. If we can, then please add CPU usage as well." adoken's pitch is that it's lighter and faster than a browser tab on dev.azure.com, and AGENTS.md makes performance a feature. Today the only way to check is Activity Monitor: a context switch nobody makes until the fan spins, so a memory regression ships unnoticed for days.

## 2. Users and job to be done

- **Persona:** the builder (daily dogfood) and performance-curious beta users who turned the meter on; not the default user.
- **Job to be done:** "While I work, I want to glance at how much memory and CPU adoken itself uses, so I can trust it stays light and notice the day it doesn't."
- **Smallest delightful slice:** "260 MB · 1% CPU" at the right of the statusbar, the same memory number Activity Monitor shows, steady unless something real changed, with an honest tooltip.
- **Priority (RICE):** 27 = Reach 30 (power users who turn it on) · Impact 0.5 · Confidence 90% (data source measured, §7) · Effort 0.5 week. Slot: Q-1.
- **Cut:** a history graph or sparkline, per-thread or per-subsystem breakdown, an Activity drawer tab, memory warnings or thresholds, disk and network usage (the network log covers requests, 0016), energy impact (§11).

## 3. UX flow

Visuals and copy: `docs/design/activity.md` §2.7.

1. Settings › General › "Show memory and CPU in the statusbar" (off by default, Q-2), or `⌘P` › "Statusbar: Show memory and CPU". *(Amended 2026-10-02 by 0025 FR-9: one toggle command, "Statusbar: Show or hide memory and CPU")*
2. Within 2 s the statusbar's right group reads "260 MB · 1% CPU", between the Ken chip and the demo pill (FR-1). Before the first CPU interval completes it shows memory only.
3. The numbers stay put while nothing real changes (FR-6): no ticking digits.
4. Hover or keyboard focus shows the tooltip: memory as Activity Monitor counts it, resident and peak, CPU over the last 10 s and since launch, and one sentence saying memory isn't a leak meter (FR-9).
5. Click, `↩` or `Space` opens a small menu: **Copy resource usage** (one line for bug reports) and **Hide from statusbar** (FR-10). Right-click opens the same menu.

### Alternate paths

- **Narrow window or large text:** the CPU part drops first, then the whole meter hides before the essentials do (FR-8).
- **Window minimized or hidden:** sampling stops; it resumes on show with a fresh sample (FR-5).
- **Keyboard only:** the meter is the last stop in the statusbar's Tab order; everything is also a palette command.

### States

| State | Behavior |
| --- | --- |
| Empty | Not applicable: the process always has a footprint. |
| Loading / first sync | Shows like any other time; first value within 2 s of the shell's first frame (memory only until the first CPU interval). |
| Error (sampling call fails) | The item keeps the last good value; after 3 consecutive failures it hides and the menu command reports "Couldn't read memory usage" (logged once, no toast). |
| Offline / stale | Not applicable (local process data). |
| Throttled by ADO | Not applicable. |
| Demo mode | Shown when on: the numbers are the real process's. |
| Unsupported platform | Item and setting hidden until a platform source is specified (FR-4). |

## 4. Functional requirements

- **FR-1 Item and placement.** When `[activity] resource_meter` is true, the statusbar shows the resource meter in its right group, **after the Ken chip and before the demo pill** (amends 0016 FR-5: right group = network indicator / offline pill, Ken chip, resource meter, demo pill). There it moves only when the demo pill changes, never when the network indicator or the Ken chip changes width. It reads "{memory} · {cpu}% CPU" (copy in design §2.7). Statusbar only; never the titlebar (Q-5).
- **FR-2 Memory metric.** The shown memory is the process's **physical footprint**, the number Activity Monitor's Memory column shows (Q-3). The tooltip and the copied line also give the **resident size** (pages in RAM, including shared framework pages) and the **peak footprint since launch**. Measured on 2026-10-01 for a sense of scale (§7): the release build in demo mode idles at a footprint of 256–261 MB, resident 325 MB, peak 373 MB.
- **FR-3 CPU metric.** CPU is the process's user + system CPU time over wall time, as a percentage of **one core** (Activity Monitor's convention: a busy multi-threaded moment can exceed 100%), averaged over the last **10 s** of samples. The tooltip and the copied line also give total CPU time since launch.
- **FR-4 Sources.** macOS reads both metrics for its own pid only, no other process:
  - **Primary:** one `proc_pid_rusage(getpid(), RUSAGE_INFO_V4, …)` call returns `ri_phys_footprint`, `ri_resident_size`, `ri_lifetime_max_phys_footprint`, `ri_user_time` and `ri_system_time`. The two times are in Mach absolute-time units, **not** nanoseconds on Apple Silicon: convert with `mach_timebase_info` (measured 125/3 on an M4 Pro, 1/1 on Intel). Both are exposed by the `libc` crate already in the workspace (`Cargo.toml`: `libc = "0.2"`); no new dependency.
  - **Fallback** (if the primary call returns an error): `task_info(mach_task_self(), TASK_VM_INFO)` → `phys_footprint`; `task_info(…, MACH_TASK_BASIC_INFO)` → `resident_size`; `getrusage(RUSAGE_SELF)` → `ru_utime + ru_stime` (microseconds, no conversion). The fallback has no peak; the tooltip omits it.
  - `sysinfo` (a transitive dependency of the GPUI fork only) is not used: refreshing a process through it costs more than the direct call and adds nothing.
  - Windows and Linux: no source in this spec; the item and its setting are hidden there. The Windows port (v0.3) picks Task Manager's equivalent (§11).
- **FR-5 Sampling cadence and thread.** A sampler task on the background executor takes one sample every **2 s** while the main window is visible and adoken is the active app, every **10 s** while the window is visible but adoken is in the background, and **none** while the window is minimized or hidden, the setting is off, or the meter is collapsed away by FR-8. It keeps the last 6 samples (10 s of CPU deltas) in memory. The UI thread never makes the sampling call and does no work for a sample whose formatted text is unchanged.
- **FR-6 Rounding, hysteresis, rate.** Memory below 995 MB shows in MB rounded to the nearest 10 ("260 MB"); from 995 MB (which would round to 1,000), GB with one decimal ("1.0 GB", "1.2 GB"); MB and GB are 1024-based as in Activity Monitor. CPU shows whole percent, "0%" below 0.5. The shown memory changes only when the new rounded value differs **and** the raw value is ≥ 7.5 MB (GB range: 0.075 GB) from the shown one; the shown CPU changes only when the 10 s average is ≥ 1.0 point from the shown value. The meter's text changes **at most once per 5 s** (trailing edge, so a settled value shows within 5 s).
- **FR-7 Repaints (contract C5).** A change of the meter's text is delivered through the statusbar's existing coalesced update (0016 FR-6, ≤ 4 per second), so the statusbar's total repaints stay within 0016 AC-S3 (≤ 5 in any second; 0 while nothing visible changes). An unchanged text causes no notify, no layout and no repaint.
- **FR-8 Collapse order.** Amends `docs/design/activity.md` §2.5. At 900 px with `font_scale` 1.3 or a long notice, items give way in this order: (1) notice truncates; (2) **the meter drops its CPU part** ("260 MB"); (3) org truncates; (4) org hides; (5) **the meter hides**; (6) quiet-hours text → glyph; (7) demo pill short form; (8) network value hides; (9) sync label short form. The meter's width is reserved with tabular digits for "990 MB · 99% CPU" so a value change inside that range never moves another item.
- **FR-9 Honest tooltip.** Shown after 500 ms hover or on keyboard focus (principles §1.2). It says what the number is (the Activity Monitor memory number), gives resident and peak, CPU over 10 s and since launch, and says plainly that adoken keeps caches so views open instantly, that memory rises as things are opened and settles, and that a high number alone isn't a leak. Exact copy: design §2.7.
- **FR-10 Menu and commands.** Click, `↩` or `Space` on the meter, or a right-click, opens a two-row menu: **Copy resource usage** copies one line (version, memory, resident, peak, CPU 10 s, CPU total, uptime; no org, user or item data) and confirms with the existing copy toast; **Hide from statusbar** sets `resource_meter = false`. Palette commands: "Statusbar: Show memory and CPU" / "Statusbar: Hide memory and CPU" (one toggle, title shows the action, command-palette.md §4.5) and "Copy resource usage" (works while the meter is hidden; takes a fresh sample). *(Amended 2026-10-02 by 0025 FR-9: the show/hide pair is one toggle command)*
- **FR-11 Setting.** `[activity] resource_meter` (bool, default **false**, Q-2) in `settings.toml` (amends 0016 FR-28; 0009 FR-1 lists it). Settings › General: switch "Show memory and CPU in the statusbar" (0009 amendment, 2026-10-01). Live apply on the next frame (0009 FR-3), hot reload (0009 FR-4); turning it on takes a sample at once instead of waiting 2 s.
- **FR-12 Accessibility.** The meter is a button with a menu (`Role::Button`, expanded state while the menu is open) labeled with its full sentence ("adoken memory 260 megabytes, CPU 1 percent"); collapsed states keep the full label. It doesn't announce value changes (it is not a live region).
- **FR-13 No alarms.** The meter never changes tone or color, shows no thresholds, badges or notifications, and writes nothing to disk or logs (Q-6).

### 4.1 Amendments to other docs (accepted 2026-10-01; applied the same day)

- **0016 FR-5** (statusbar order) and **FR-28** (`[activity]` table): edited in place, marked amended.
- **`docs/design/activity.md`** §2.1 (order), §2.5 (collapse order), new §2.7 (meter): edited in place.
- **0009 FR-1 / §3 General:** the key and the switch (applied by the 0001/0009 task, 2026-10-01).
- **`docs/design/principles.md` §2** statusbar line: right group gains "resource meter (optional, off by default)" between the Ken combo counter and the demo pill. **`docs/design/command-palette.md` §4.5:** the two commands of FR-10.

## 5. ADO API surface and limits

| Purpose | Endpoint (api-version) | Scope | Cost / limit notes |
| --- | --- | --- | --- |
| — | **None.** The meter reads the local process's own counters. | none | 0 TSTU |

- **Rate limits:** zero requests.
- **No push to local apps:** not applicable.
- **PAT scopes:** none.

## 6. Acceptance criteria

Harness: the sampler behind a `ResourceSource` trait with a fake source (scripted footprint, resident, peak, CPU ticks, timebase) and an injected clock; the real source in a probe test; statusbar repaint counter from 0016 AC-S3.

- **AC-1 Formatting.** 260.4 MB → "260 MB"; 4.9 MB → "10 MB"; 994.9 MB → "990 MB"; 995 MB → "1.0 GB" ("1,000 MB" is never shown); 1,288 MB → "1.3 GB"; CPU 0.4 → "0%", 1.6 → "2%", 250.2 → "250%". (unit)
- **AC-2 CPU math and units.** Fake ticks with timebase 125/3: 24,000,000 ticks (1 s of CPU) over 10 s wall → "10%"; timebase 1/1 with 1,000,000,000 ns → same. The real source: a 500 ms busy loop on one thread reads 450–550 ms of CPU through both the primary and the fallback path. (unit + probe test)
- **AC-3 Hysteresis and rate.** Samples alternating 255 MB / 265 MB every 2 s for 60 s change the shown memory at most once; CPU alternating 0.2% / 0.4% shows "0%" and never changes; CPU alternating 1.2% / 1.8% changes the shown value at most once; a jump 260 → 400 MB shows "400 MB" within 5 s; no two text changes are < 5 s apart under any input. (unit, injected clock)
- **AC-4 Repaints.** 60 s of steady samples with the meter on: 0 repaints and 0 notifies from the meter. With 0016 AC-S3's fixture (200 requests and 40 progress updates in 1 s) plus a meter change in the same second: ≤ 5 statusbar repaints. (unit: repaint counter)
- **AC-5 Lifecycle and cadence.** Fake clock over 60 s: active + visible → 30 source calls (± 1); inactive + visible → 6; minimized → 0; setting off → 0; meter collapsed away by FR-8 → 0; turning the setting on → 1 call within the same frame's background task, before the 2 s timer. (unit)
- **AC-6 Off the UI thread.** The source is never called on the main thread (the fake asserts the calling thread). (unit)
- **AC-7 Collapse order.** `collapse()` (`crates/app/src/activity/statusbar.rs`) with the meter: at 900 px and `font_scale` 1.3 with a 60-character notice, the meter loses CPU before the org truncates, and hides after the org hides and before the demo pill shortens; at `font_scale` 1.0 with the 0016 §2.5 worst case plus the meter ("990 MB · 99% CPU"), nothing collapses. A value change from "260 MB · 1% CPU" to "270 MB · 2% CPU" doesn't move the network indicator or the Ken chip (layout assertion). (unit)
- **AC-8 Setting.** No `[activity] resource_meter` → no meter; `true` → shown on the next frame; `resource_meter = "yes"` → false and reported (0009 FR-5); the Settings switch and the palette toggle write the key and keep `panel_height`, `tab` and `network_indicator` intact. (unit: settings)
- **AC-9 Tooltip, menu, copy, a11y.** The tooltip text equals design §2.7 for a fixture sample; Copy resource usage puts exactly the design §2.7 line on the clipboard and shows the copy toast; Hide from statusbar sets the key to false; the a11y node is a button with the FR-12 label and expanded state while the menu is open. (unit + story)
- **AC-10 Matches Activity Monitor.** Scripted check `resource_meter_probe`: run the release build in demo mode for 60 s, then compare the meter's memory with `footprint <pid>` (macOS command-line tool) within ± 2%, and its 10 s CPU with Activity Monitor's % CPU for the process, or with two `proc_pid_rusage` reads 10 s apart converted with the Mach timebase, within ± 3 points. Don't use `top` or `ps` for CPU: on Apple Silicon they report Mach ticks unconverted (measured 2026-10-02: `top` showed 0.2% while the converted reading was 22.9%). Result recorded once in this spec. (scripted manual check; amended 2026-10-02 after verification)
- **AC-11 Gallery.** Stories, one snapshot per built-in theme: `statusbar-resource-meter` (on, with network indicator and Ken chip), `statusbar-resource-meter-narrow` (CPU dropped), `statusbar-resource-meter-gigabytes` ("1.2 GB · 3% CPU"), `statusbar-resource-meter-menu`. (snapshot test)
- **AC-12 Sampling cost.** Criterion bench in `crates/perf`: one primary-path sample (call + timebase conversion + formatting) < 5 µs on the reference Mac; the CI gate fails above that. (bench)

## 7. Performance budget

Measured 2026-10-01 on the builder's Mac (Apple M4 Pro, macOS 27.0) with a throwaway C program compiled `-O2`, 200,000 calls each, two runs:

| Call | Cost per call |
| --- | --- |
| `proc_pid_rusage(RUSAGE_INFO_V4)` (primary: footprint, resident, peak, CPU times) | 413–464 ns |
| `task_info(TASK_VM_INFO)` (fallback footprint) | 405–421 ns |
| `task_info(MACH_TASK_BASIC_INFO)` (fallback resident) | 370–381 ns |
| `getrusage(RUSAGE_SELF)` (fallback CPU) | 181–193 ns |
| Units check: a 500 ms busy loop | `getrusage` 497 ms; `ri_user_time` 11,925,728 ticks × 125/3 = 497 ms |
| adoken release build, demo store, PR list, idle 3–15 s after launch | footprint 256–261 MB, resident 325 MB, peak 373 MB, `ps` %CPU 0.0–0.7 |

At one sample per 2 s the sampling call costs ≈ 0.25 µs per second of wall time, about 0.00003% of one core: negligible even on the UI thread. It still runs on the background executor (FR-5) so that a sample with unchanged text costs the UI thread nothing at all.

| Interaction | Budget | Measured how |
| --- | --- | --- |
| One sample (call + conversion + formatting) | < 5 µs, off the UI thread | Criterion bench (AC-12) |
| Wakeups added | ≤ 0.5 / s active, ≤ 0.1 / s in the background, 0 while hidden or off | Unit with fake clock (AC-5) |
| Statusbar repaints from the meter | ≤ 1 per 5 s; 0 while the text is unchanged; total statusbar ≤ 5 / s (0016 AC-S3) | Repaint counter (AC-4) |
| Meter layout | < 0.1 ms per statusbar layout | Frame recorder |
| Memory held | < 1 KB (6 samples + formatted strings) | Unit (size_of) |
| Cold start | No change (< 500 ms): the first sample runs on the background executor after the first frame | Launch timer |
| Off by default | Zero cost: no sampler task exists while the setting is off | Unit (AC-5) |

Global rules (AGENTS.md) always apply: frame < 16 ms, Cmd+P < 50 ms on 100k items, cold start < 500 ms. Regressions are bugs.

## 8. Success signal (telemetry-free)

- **Local, visible:** the meter itself, and the copied line in bug reports.
- **Dogfood:** with the meter on, the builder notices any footprint regression of ≥ 20% between two builds on the day it lands (one line in the dogfood notes per regression found, with the build and the cause).
- **Support signal:** beta performance reports arrive with a "Copy resource usage" line instead of "it feels heavy".
- **Build in public:** a post with the meter in a real day's screenshot gets "how is it so light?" replies rather than "is it Electron?".

## 9. Risks

- **Number anxiety.** A visible number invites "why 260 MB?" and leak reports. Mitigations: off by default (Q-2), footprint rather than the larger resident size (Q-3), steady display (FR-6), no colors or thresholds (FR-13), the tooltip's plain explanation (FR-9).
- **Different tools, different numbers.** `ps` and `top`'s RSS show the resident size (325 MB in the measurement) while Activity Monitor shows the footprint (260 MB). Mitigation: the tooltip names the Activity Monitor number and gives resident beside it; AC-10 verifies against `footprint` and Activity Monitor (not `top`, whose CPU figures are unconverted Mach ticks on Apple Silicon).
- **Apple Silicon time units.** `ri_user_time` is in Mach ticks (× 125/3 on the measured M4 Pro); treating it as nanoseconds would show 1/41 of the real CPU. Mitigation: FR-4 conversion, AC-2 tests both timebases and the real source.
- **Energy.** A 2 s wakeup forever is a cost on battery. Mitigations: off by default, 10 s in the background, none while hidden (FR-5).
- **Occlusion.** [INFERENCE] The GPUI fork reports minimized and hidden windows; whether it reports a fully covered window is unverified. If not, a covered-but-visible window samples at the background rate (10 s), which stays within budget.
- **Graphics memory attribution.** [unverified] Whether Metal buffers count toward the footprint on every macOS version is not documented on a page found; AC-10's comparison with `footprint` is what the meter promises, nothing more.

## 10. Open questions — resolved 2026-10-01

All recommendations accepted by the owner.

- **Q-1 Slot.** **Resolved:** v0.1, on the cut line right after 0020 tours, cut second if v0.1 slips; RICE 27 (Reach 30 · Impact 0.5 · Confidence 90% · Effort 0.5 week) (`docs/roadmap.md`).
- **Q-2 Default visibility.** **Resolved:** off by default; on from Settings › General or `⌘P` (FR-11; 0009 FR-1).
- **Q-3 Which memory number.** **Resolved:** the physical footprint, with resident and peak in the tooltip (FR-2, FR-9).
- **Q-4 CPU always shown.** **Resolved:** yes, "· 1% CPU" whenever the meter is on; it drops first when space is short (FR-3, FR-8).
- **Q-5 Placement.** **Resolved:** the statusbar, after the Ken chip and before the demo pill (FR-1; 0016 FR-5).
- **Q-6 No thresholds.** **Resolved:** the meter never turns warning or red and never notifies (FR-13).

## 11. Out of scope

- History graph or sparkline, per-subsystem breakdown (caches, store, GPU textures), memory warnings: not requested; noise. Revisit only if dogfood shows the plain number isn't enough to spot regressions.
- An Activity drawer tab for resources (0016's drawer stays Sync + Network).
- Windows and Linux sources: v0.3 port spec (Windows likely `GetProcessMemoryInfo` + `GetProcessTimes`; Linux `/proc/self/status` + `/proc/self/stat`), to be measured there.
- Child processes (`az` during onboarding, spec 0001) and system processes: the meter is adoken's own process only.
- Energy impact, disk usage, network usage (the network log covers requests, 0016).

## 12. References

- `proc_pid_rusage`, `rusage_info_v4`, `ri_phys_footprint`, `ri_lifetime_max_phys_footprint`: macOS SDK `usr/include/sys/resource.h` and `libproc.h` (Xcode command-line tools SDK for macOS 27.0, read 2026-10-01); Rust bindings in `libc` 0.2.189 `src/unix/bsd/apple/mod.rs`.
- `task_info` flavors `TASK_VM_INFO`, `MACH_TASK_BASIC_INFO`, `mach_timebase_info`: macOS SDK `mach/task_info.h`, `mach/mach_time.h`.
- `getrusage(2)`: macOS man page.
- Statusbar collapse code: `crates/app/src/activity/statusbar.rs` `BarParts`, `Collapse`, `collapse`.
- Measurement program (throwaway, not committed): 200,000-call loops of each call above, `clang -O2`, run twice on 2026-10-01.
