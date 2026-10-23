# Perf note: PR review diff on a 5k-line file (spec 0004 §7, AC-3)

Measured 2026-09-25, M-series Mac (arm64), macOS 26.

## Diff compute (off the UI thread)

`cargo bench -p adoken-diff --bench diff` (criterion, bench profile):

| Case | Time | Budget |
|---|---|---|
| 5k lines, ~10 % edited | 0.77 ms | < 20 ms |
| 5k lines, fully rewritten | 2.4 ms | < 20 ms |
| Row layout, unified + split, 5k | 11 µs | once per file / fold toggle |
| Per frame: tokenize + intraline, 60 visible rows | 14 µs | part of the 16 ms frame |

The line diff interns lines to `u32` ids and runs `similar`'s Myers with a
250 ms deadline (a pathological file degrades to a valid, non-minimal script
instead of pinning an executor). Histogram was 12.8 ms / 45.6 ms on the same
inputs, so Myers is the default.

## Frame cost while scrolling

`cargo run -p adoken-gallery --example review_frames`: the whole review view
(header, tree, diff) at 1440 × 900 over a 5k-line Rust file with 40 inline
threads, drawn headless with the Metal renderer, jumping one full viewport
(48 rows) per frame for 240 frames. `Window::draw` = layout + prepaint + paint.

| Layout | Rows | p50 | p95 | max | Budget |
|---|---|---|---|---|---|
| Unified | 4,634 | 2.11 ms | 2.38 ms | 2.51 ms | 16 ms |
| Side by side | 4,492 | 3.50 ms | 3.92 ms | 4.18 ms | 16 ms |

Debug build (adoken crates at opt-level 0, dependencies at 2), so release is
lower. Cost is flat in file size: rows are laid out once per (file, layout,
folds), the list (`gpui::list`, uniform height hint) paints only visible rows,
and each row is one flex strip plus one `StyledText` with merged highlight
runs; tokens and intraline ranges are computed per visible row.

## Not yet covered

- Real trackpad input through the frame recorder (AC-3 proper) needs the
  mock-server harness; this measures the draw side only.
- Files > 20k lines or > 1 MB render collapsed ("Load anyway"), so the
  20k-line AC-3 case is only reached after an explicit expand.
- Long lines are clipped, not horizontally scrollable.
