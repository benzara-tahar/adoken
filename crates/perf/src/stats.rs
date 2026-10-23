//! Latency samples and nearest-rank percentiles.

use std::fmt;
use std::time::Duration;

/// A named series of latency samples (frame durations, query latencies, launches).
#[derive(Debug, Clone)]
pub struct Samples {
    name: &'static str,
    durations: Vec<Duration>,
}

impl Samples {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            durations: Vec::new(),
        }
    }

    pub fn with_capacity(name: &'static str, capacity: usize) -> Self {
        Self {
            name,
            durations: Vec::with_capacity(capacity),
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn record(&mut self, duration: Duration) {
        self.durations.push(duration);
    }

    pub fn len(&self) -> usize {
        self.durations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.durations.is_empty()
    }

    pub fn as_slice(&self) -> &[Duration] {
        &self.durations
    }

    pub fn clear(&mut self) {
        self.durations.clear();
    }

    /// Percentiles over everything recorded so far. `None` when empty.
    pub fn summary(&self) -> Option<Summary> {
        Summary::of(self.name, &self.durations)
    }
}

/// Order statistics of a sample series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub name: &'static str,
    pub count: usize,
    pub min: Duration,
    pub p50: Duration,
    pub p95: Duration,
    pub p99: Duration,
    pub max: Duration,
}

impl Summary {
    pub fn of(name: &'static str, durations: &[Duration]) -> Option<Self> {
        if durations.is_empty() {
            return None;
        }
        let mut sorted = durations.to_vec();
        sorted.sort_unstable();
        Some(Self {
            name,
            count: sorted.len(),
            min: sorted[0],
            p50: percentile(&sorted, 50),
            p95: percentile(&sorted, 95),
            p99: percentile(&sorted, 99),
            max: sorted[sorted.len() - 1],
        })
    }
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} n={} min={} p50={} p95={} p99={} max={}",
            self.name,
            self.count,
            Ms(self.min),
            Ms(self.p50),
            Ms(self.p95),
            Ms(self.p99),
            Ms(self.max),
        )
    }
}

/// Nearest-rank percentile of an ascending, non-empty slice: the smallest
/// sample such that at least `pct`% of samples are ≤ it. Never interpolates,
/// so every reported value is a latency that actually happened.
pub fn percentile(sorted: &[Duration], pct: u32) -> Duration {
    assert!(!sorted.is_empty(), "percentile of an empty series");
    assert!((1..=100).contains(&pct), "percentile {pct} out of 1..=100");
    let rank = (sorted.len() * pct as usize).div_ceil(100);
    sorted[rank - 1]
}

/// Formats a duration as milliseconds with two decimals (`12.34ms`).
#[derive(Debug, Clone, Copy)]
pub struct Ms(pub Duration);

impl fmt::Display for Ms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.2}ms", self.0.as_secs_f64() * 1e3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(values: &[u64]) -> Vec<Duration> {
        values.iter().map(|&v| Duration::from_millis(v)).collect()
    }

    #[test]
    fn nearest_rank_on_one_to_hundred() {
        let sorted = ms(&(1..=100).collect::<Vec<_>>());
        assert_eq!(percentile(&sorted, 50), Duration::from_millis(50));
        assert_eq!(percentile(&sorted, 95), Duration::from_millis(95));
        assert_eq!(percentile(&sorted, 99), Duration::from_millis(99));
        assert_eq!(percentile(&sorted, 100), Duration::from_millis(100));
    }

    #[test]
    fn small_series_tail_is_the_max() {
        // With 10 samples p95 and p99 must land on the worst sample, not on an
        // interpolated value below it: the budget check is against real tails.
        let sorted = ms(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 400]);
        assert_eq!(percentile(&sorted, 50), Duration::from_millis(5));
        assert_eq!(percentile(&sorted, 95), Duration::from_millis(400));
        assert_eq!(percentile(&sorted, 99), Duration::from_millis(400));
    }

    #[test]
    fn single_sample_is_every_percentile() {
        let sorted = ms(&[7]);
        for pct in [1, 50, 95, 99, 100] {
            assert_eq!(percentile(&sorted, pct), Duration::from_millis(7));
        }
    }

    #[test]
    fn summary_is_order_independent() {
        let mut samples = Samples::new("q");
        for v in [30, 10, 20] {
            samples.record(Duration::from_millis(v));
        }
        let s = samples.summary().unwrap();
        assert_eq!(s.count, 3);
        assert_eq!(s.min, Duration::from_millis(10));
        assert_eq!(s.p50, Duration::from_millis(20));
        assert_eq!(s.max, Duration::from_millis(30));
    }

    #[test]
    fn empty_series_has_no_summary() {
        assert!(Samples::new("empty").summary().is_none());
    }
}
