#!/usr/bin/env bash
# Perf harness (roadmap v0.0.1, spec 0003 AC-1): release-builds the app and the
# runners in crates/perf, measures cold start, frame draw and search latency,
# and prints a budget table. Exits non-zero if any budget fails.
#
# Env knobs:
#   RUNS=20           cold-start launches (after 1 warm-up)
#   FRAMES=120        forced full redraws timed in one extra launch (0 = skip)
#   ITEMS=100000      synthetic corpus size
#   QUERIES=500       synthetic query set size
#   ROUNDS=3          timed passes over the query set
set -euo pipefail

cd "$(dirname "$0")/.."

RUNS=${RUNS:-20}
FRAMES=${FRAMES:-120}
ITEMS=${ITEMS:-100000}
QUERIES=${QUERIES:-500}
ROUNDS=${ROUNDS:-3}
TARGET_DIR=${CARGO_TARGET_DIR:-target}
BIN="$TARGET_DIR/release"

echo "==> cargo build --release (adoken, perf runners)"
cargo build --release --quiet -p adoken -p adoken-perf --bins

log=$(mktemp -t adoken-perf)
trap 'rm -f "$log"' EXIT

echo
echo "==> cold start + frames"
"$BIN/perf-cold-start" "$BIN/adoken" --runs "$RUNS" --frames "$FRAMES" | tee -a "$log"

echo
echo "==> search"
"$BIN/perf-search" --items "$ITEMS" --queries "$QUERIES" --rounds "$ROUNDS" | tee -a "$log"

echo
echo "==> budgets ($(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m))"
awk -F'\t' '
  BEGIN {
    fmt = "%-28s %-5s %12s %10s %6s  %s\n"
    printf fmt, "metric", "stat", "measured", "budget", "n", "result"
  }
  $1 == "CHECK" {
    printf fmt, $2, $3, sprintf("%.2f ms", $4), "< " $5 " ms", $6, $7
    if ($7 != "PASS") failed++
    rows++
  }
  END {
    if (rows == 0) { print "no budget rows were produced"; exit 2 }
    if (failed) { printf "%d of %d budgets FAILED\n", failed, rows; exit 1 }
    printf "all %d budgets pass\n", rows
  }
' "$log"
