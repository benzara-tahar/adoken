//! Performance budgets from AGENTS.md and spec 0003 §7, and pass/fail checks
//! against them.

use std::fmt;
use std::time::Duration;

use crate::stats::{Ms, Summary};

/// Whether a missed timing budget fails a test. `ADOKEN_PERF_ADVISORY=1`
/// (set in CI: hosted macOS runners render on a virtual GPU, so their frame
/// times say nothing about a real Mac) reports misses instead;
/// `scripts/perf.sh` on real hardware stays the gate.
pub fn enforced() -> bool {
    std::env::var_os("ADOKEN_PERF_ADVISORY").is_none()
}

/// `anyhow::ensure!` for timing budgets: bails while budgets are
/// [`enforced`], otherwise prints the miss to stderr and carries on.
#[macro_export]
macro_rules! ensure_budget {
    ($cond:expr, $($arg:tt)+) => {
        if !($cond) {
            if $crate::budget::enforced() {
                ::anyhow::bail!($($arg)+);
            }
            eprintln!("perf budget missed (advisory): {}", format_args!($($arg)+));
        }
    };
}

/// Which order statistic a budget constrains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stat {
    P50,
    P95,
    P99,
    Max,
}

impl Stat {
    pub fn of(self, summary: &Summary) -> Duration {
        match self {
            Stat::P50 => summary.p50,
            Stat::P95 => summary.p95,
            Stat::P99 => summary.p99,
            Stat::Max => summary.max,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Stat::P50 => "p50",
            Stat::P95 => "p95",
            Stat::P99 => "p99",
            Stat::Max => "max",
        }
    }
}

/// A latency limit on one statistic of one metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub metric: &'static str,
    pub stat: Stat,
    pub limit: Duration,
}

impl Budget {
    /// Process spawn → first presented frame (AGENTS.md, roadmap v0.0.1 exit).
    pub const COLD_START: Budget = Budget {
        metric: "cold start -> first frame",
        stat: Stat::P95,
        limit: Duration::from_millis(500),
    };
    /// Any interaction frame (AGENTS.md).
    pub const FRAME: Budget = Budget {
        metric: "frame draw (full redraw)",
        stat: Stat::P99,
        limit: Duration::from_millis(16),
    };
    /// Keystroke → results on 100k items, tail (spec 0003 AC-1).
    pub const SEARCH_P99: Budget = Budget {
        metric: "search query, 100k items",
        stat: Stat::P99,
        limit: Duration::from_millis(50),
    };
    /// Keystroke → results on 100k items, median (spec 0003 AC-1).
    pub const SEARCH_P50: Budget = Budget {
        metric: "search query, 100k items",
        stat: Stat::P50,
        limit: Duration::from_millis(10),
    };
    /// Index build for 100k items, off the main thread (spec 0003 AC-10).
    pub const INDEX_BUILD: Budget = Budget {
        metric: "index build, 100k items",
        stat: Stat::Max,
        limit: Duration::from_millis(300),
    };

    pub fn check(self, summary: &Summary) -> Check {
        Check {
            budget: self,
            measured: self.stat.of(summary),
            samples: summary.count,
        }
    }
}

/// The outcome of measuring one budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Check {
    pub budget: Budget,
    pub measured: Duration,
    pub samples: usize,
}

impl Check {
    /// Budgets are strict upper bounds: "< 16 ms" fails at exactly 16 ms.
    pub fn passed(&self) -> bool {
        self.measured < self.budget.limit
    }

    /// One tab-separated row for `scripts/perf.sh`:
    /// `CHECK <metric> <stat> <measured_ms> <limit_ms> <samples> <PASS|FAIL>`.
    pub fn row(&self) -> CheckRow<'_> {
        CheckRow(self)
    }
}

/// Machine-readable row form of a [`Check`]; see [`Check::row`].
pub struct CheckRow<'a>(&'a Check);

pub const ROW_TAG: &str = "CHECK";

impl fmt::Display for CheckRow<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let check = self.0;
        write!(
            f,
            "{ROW_TAG}\t{}\t{}\t{:.2}\t{:.0}\t{}\t{}",
            check.budget.metric,
            check.budget.stat.label(),
            check.measured.as_secs_f64() * 1e3,
            check.budget.limit.as_secs_f64() * 1e3,
            check.samples,
            if check.passed() { "PASS" } else { "FAIL" },
        )
    }
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} (budget < {}) {}",
            self.budget.metric,
            self.budget.stat.label(),
            Ms(self.measured),
            Ms(self.budget.limit),
            if self.passed() { "PASS" } else { "FAIL" },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(p50: u64, p95: u64, p99: u64) -> Summary {
        Summary {
            name: "t",
            count: 100,
            min: Duration::ZERO,
            p50: Duration::from_millis(p50),
            p95: Duration::from_millis(p95),
            p99: Duration::from_millis(p99),
            max: Duration::from_millis(p99 + 1),
        }
    }

    #[test]
    fn budget_is_a_strict_upper_bound() {
        assert!(!Budget::FRAME.check(&summary(1, 2, 16)).passed());
        assert!(Budget::FRAME.check(&summary(1, 2, 15)).passed());
    }

    #[test]
    fn check_reads_the_budgeted_statistic() {
        // p50 over budget but p95 under: the cold-start budget is on p95.
        let s = summary(600, 400, 400);
        assert!(Budget::COLD_START.check(&s).passed());
        // A fast median does not hide a slow tail on the p99 search budget.
        let s = summary(2, 20, 80);
        assert!(Budget::SEARCH_P50.check(&s).passed());
        assert!(!Budget::SEARCH_P99.check(&s).passed());
    }

    #[test]
    fn row_is_tab_separated_for_the_script() {
        let check = Budget::SEARCH_P99.check(&summary(2, 20, 80));
        let row = check.row().to_string();
        let cols: Vec<&str> = row.split('\t').collect();
        assert_eq!(
            cols,
            [
                "CHECK",
                "search query, 100k items",
                "p99",
                "80.00",
                "50",
                "100",
                "FAIL"
            ]
        );
    }
}
