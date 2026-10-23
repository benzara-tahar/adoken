//! FR-7 admission: at most [`super::MAX_IN_FLIGHT`] requests in flight per
//! host, handed out by tier (user > hot > warm > cold), first come first
//! served within a tier.
//!
//! - One slot is the user's: background tiers hold at most
//!   [`super::BACKGROUND_IN_FLIGHT`], so a user request never waits a full
//!   round trip behind sync.
//! - No starvation: a waiter passed over [`MAX_BYPASS`] times while a slot
//!   was free for it goes next, whatever its tier.

use std::pin::pin;
use std::sync::{Mutex, MutexGuard};

use tokio::sync::Notify;

use crate::Tier;

/// Admissions of later, higher-priority waiters a request tolerates before
/// it goes next regardless of tier.
pub const MAX_BYPASS: u32 = 8;

pub(super) struct Admission {
    capacity: usize,
    /// Slots background tiers may hold at once; the rest are the user's.
    background_capacity: usize,
    queue: Mutex<Queue>,
    /// Signalled whenever a slot frees or the head of the queue changes.
    changed: Notify,
}

#[derive(Default)]
struct Queue {
    in_flight: usize,
    background: usize,
    next_seq: u64,
    waiting: Vec<Waiter>,
}

struct Waiter {
    seq: u64,
    tier: Tier,
    bypassed: u32,
}

/// Lower goes first (spec 0002 FR-7).
fn rank(tier: Tier) -> u8 {
    match tier {
        Tier::User => 0,
        Tier::Hot => 1,
        Tier::Warm => 2,
        Tier::Cold => 3,
    }
}

fn is_background(tier: Tier) -> bool {
    tier != Tier::User
}

/// In-flight counts, for [`super::Governor::hold`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Occupancy {
    pub in_flight: usize,
    pub background: usize,
    pub waiting: usize,
}

impl Queue {
    fn eligible(&self, waiter: &Waiter, background_capacity: usize) -> bool {
        !is_background(waiter.tier) || self.background < background_capacity
    }

    /// The waiter a free slot goes to now, if any.
    fn next(&self, capacity: usize, background_capacity: usize) -> Option<u64> {
        if self.in_flight >= capacity {
            return None;
        }
        let eligible = || {
            self.waiting
                .iter()
                .filter(|w| self.eligible(w, background_capacity))
        };
        eligible()
            .filter(|w| w.bypassed >= MAX_BYPASS)
            .min_by_key(|w| w.seq)
            .or_else(|| eligible().min_by_key(|w| (rank(w.tier), w.seq)))
            .map(|w| w.seq)
    }

    fn admit(&mut self, seq: u64, background_capacity: usize) {
        let Some(ix) = self.waiting.iter().position(|w| w.seq == seq) else {
            return;
        };
        let admitted = self.waiting.swap_remove(ix);
        // Earlier waiters that could have taken this slot were passed over.
        let passed: Vec<bool> = self
            .waiting
            .iter()
            .map(|w| w.seq < seq && self.eligible(w, background_capacity))
            .collect();
        for (w, passed) in self.waiting.iter_mut().zip(passed) {
            if passed {
                w.bypassed += 1;
            }
        }
        self.in_flight += 1;
        if is_background(admitted.tier) {
            self.background += 1;
        }
    }
}

impl Admission {
    pub(super) fn new(capacity: usize, background_capacity: usize) -> Self {
        Admission {
            capacity,
            background_capacity: background_capacity.min(capacity),
            queue: Mutex::new(Queue::default()),
            changed: Notify::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(super) fn occupancy(&self) -> Occupancy {
        let q = self.lock();
        Occupancy {
            in_flight: q.in_flight,
            background: q.background,
            waiting: q.waiting.len(),
        }
    }

    /// Wait for a slot at `tier`'s priority. Dropping the future before it
    /// resolves leaves the queue.
    pub(super) async fn acquire(&self, tier: Tier) -> Permit<'_> {
        let seq = {
            let mut q = self.lock();
            let seq = q.next_seq;
            q.next_seq += 1;
            q.waiting.push(Waiter {
                seq,
                tier,
                bypassed: 0,
            });
            seq
        };
        let mut queued = Queued {
            admission: self,
            seq: Some(seq),
        };
        loop {
            // Registered before the check, so a release in between wakes us.
            let mut changed = pin!(self.changed.notified());
            changed.as_mut().enable();
            {
                let mut q = self.lock();
                if q.next(self.capacity, self.background_capacity) == Some(seq) {
                    q.admit(seq, self.background_capacity);
                    let more = q.next(self.capacity, self.background_capacity).is_some();
                    drop(q);
                    queued.seq = None;
                    if more {
                        self.changed.notify_waiters();
                    }
                    return Permit {
                        admission: self,
                        background: is_background(tier),
                    };
                }
            }
            changed.await;
        }
    }
}

/// A queued waiter; leaves the queue if its future is dropped.
struct Queued<'a> {
    admission: &'a Admission,
    seq: Option<u64>,
}

impl Drop for Queued<'_> {
    fn drop(&mut self) {
        if let Some(seq) = self.seq {
            self.admission.lock().waiting.retain(|w| w.seq != seq);
            self.admission.changed.notify_waiters();
        }
    }
}

/// One in-flight slot; released on drop.
pub(super) struct Permit<'a> {
    admission: &'a Admission,
    background: bool,
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        {
            let mut q = self.admission.lock();
            q.in_flight -= 1;
            if self.background {
                q.background -= 1;
            }
        }
        self.admission.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::{mpsc, oneshot};

    use super::*;

    /// Queue `tiers` in order behind a full house, then release one slot at
    /// a time: the order they're admitted in.
    async fn admission_order(capacity: usize, background: usize, tiers: &[Tier]) -> Vec<usize> {
        let admission = Arc::new(Admission::new(capacity, background));
        let (hold_tx, mut hold_rx) = mpsc::unbounded_channel::<oneshot::Sender<()>>();
        // Every slot held by a user request.
        let mut holders = Vec::new();
        for _ in 0..capacity {
            let (a, tx) = (admission.clone(), hold_tx.clone());
            holders.push(tokio::spawn(async move {
                let _permit = a.acquire(Tier::User).await;
                let (release, wait) = tokio::sync::oneshot::channel();
                tx.send(release).unwrap();
                let _ = wait.await;
            }));
        }
        let mut releases = Vec::new();
        for _ in 0..capacity {
            releases.push(hold_rx.recv().await.unwrap());
        }
        let (done_tx, mut done_rx) =
            mpsc::unbounded_channel::<(usize, tokio::sync::oneshot::Sender<()>)>();
        for (ix, tier) in tiers.iter().copied().enumerate() {
            let (a, tx) = (admission.clone(), done_tx.clone());
            tokio::spawn(async move {
                let _permit = a.acquire(tier).await;
                let (release, wait) = tokio::sync::oneshot::channel();
                tx.send((ix, release)).unwrap();
                let _ = wait.await;
            });
            // Enqueue strictly in order.
            while admission.occupancy().waiting < ix + 1 {
                tokio::task::yield_now().await;
            }
        }
        let mut order = Vec::new();
        // One slot at a time: release a holder, see who got in, release it.
        releases.pop().unwrap().send(()).unwrap();
        for _ in 0..tiers.len() {
            let (ix, release) = tokio::time::timeout(Duration::from_secs(5), done_rx.recv())
                .await
                .expect("a waiter is admitted")
                .unwrap();
            order.push(ix);
            release.send(()).unwrap();
        }
        order
    }

    #[tokio::test]
    async fn a_user_request_behind_many_cold_ones_goes_first() {
        let mut tiers = vec![Tier::Cold; 20];
        tiers.push(Tier::User);
        let order = admission_order(4, 4, &tiers).await;
        assert_eq!(order[0], 20, "{order:?}");
    }

    #[tokio::test]
    async fn tiers_are_served_by_priority_then_arrival() {
        let tiers = [
            Tier::Cold,
            Tier::Warm,
            Tier::Hot,
            Tier::Cold,
            Tier::User,
            Tier::Hot,
        ];
        let order = admission_order(4, 4, &tiers).await;
        assert_eq!(order, [4, 2, 5, 1, 0, 3]);
    }

    #[tokio::test]
    async fn cold_is_not_starved_by_a_steady_stream_of_user_requests() {
        let mut tiers = vec![Tier::Cold];
        tiers.extend([Tier::User; 30]);
        let order = admission_order(4, 4, &tiers).await;
        let cold_at = order.iter().position(|ix| *ix == 0).unwrap();
        assert_eq!(cold_at, MAX_BYPASS as usize, "{order:?}");
    }

    /// Background tiers can't take the user's slot: with three background
    /// requests in flight, a user request is admitted at once and a fourth
    /// background one waits.
    #[tokio::test]
    async fn the_last_slot_is_kept_for_the_user() {
        let admission = Admission::new(4, 3);
        let bg: Vec<Permit<'_>> = futures::future::join_all(
            [Tier::Cold, Tier::Warm, Tier::Hot].map(|t| admission.acquire(t)),
        )
        .await;
        assert_eq!(admission.occupancy().background, 3);
        let more_bg = tokio::time::timeout(Duration::from_millis(50), admission.acquire(Tier::Hot));
        assert!(
            more_bg.await.is_err(),
            "background waits for a background slot"
        );
        assert_eq!(
            admission.occupancy().waiting,
            0,
            "a dropped waiter leaves the queue"
        );
        let user = tokio::time::timeout(Duration::from_millis(50), admission.acquire(Tier::User))
            .await
            .expect("the user's slot is free");
        assert_eq!(admission.occupancy().in_flight, 4);
        drop(user);
        drop(bg);
        assert_eq!(admission.occupancy(), Occupancy::default());
    }
}
