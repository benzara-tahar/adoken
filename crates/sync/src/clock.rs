//! Injectable time so scheduler and governor behavior is testable without
//! real waiting (spec 0002 §6 harness: "injected clock").

use std::sync::atomic::{AtomicI64, Ordering};

use adoken_model::Millis;
use futures::future::BoxFuture;

pub trait Clock: Send + Sync + 'static {
    fn now_ms(&self) -> Millis;
    /// Resolve after `ms` of this clock's time.
    fn sleep(&self, ms: Millis) -> BoxFuture<'static, ()>;
}

/// Wall clock + Tokio timers.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> Millis {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as Millis)
            .unwrap_or(0)
    }

    fn sleep(&self, ms: Millis) -> BoxFuture<'static, ()> {
        Box::pin(tokio::time::sleep(std::time::Duration::from_millis(
            ms.max(0) as u64,
        )))
    }
}

/// Virtual clock: `sleep` advances time instantly. Share one instance with a
/// mock server to timestamp requests in virtual time.
#[derive(Debug)]
pub struct ManualClock {
    now: AtomicI64,
}

impl ManualClock {
    pub fn new(start: Millis) -> Self {
        ManualClock {
            now: AtomicI64::new(start),
        }
    }

    pub fn advance(&self, ms: Millis) {
        self.now.fetch_add(ms.max(0), Ordering::AcqRel);
    }

    pub fn set(&self, ms: Millis) {
        self.now.fetch_max(ms, Ordering::AcqRel);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> Millis {
        self.now.load(Ordering::Acquire)
    }

    fn sleep(&self, ms: Millis) -> BoxFuture<'static, ()> {
        self.advance(ms);
        Box::pin(async {})
    }
}
