//! Rate governor (spec 0002 FR-7..FR-10). Every request goes through it.
//!
//! - ≤ 4 in-flight requests per host, admitted by tier (user > hot > warm >
//!   cold, [`admission`]); one of the four is kept for user requests.
//! - `Retry-After` (any status): no request to the host until it elapses;
//!   the throttled request is retried after the window.
//! - `X-RateLimit-Remaining / Limit` < 50% → [`Pressure::Slow`];
//!   ≤ 20% → [`Pressure::HotOnly`] until `X-RateLimit-Reset`.
//! - 429 without `Retry-After` (TF400733) → backoff 30 s, doubling, 5 min cap.
//! - Self-imposed budget: ≤ 300 requests and ≤ 50 TSTU per sliding 5 min.
//! - 401 → every later request fails fast with [`GovError::Unauthorized`].
//! - A local ledger of requests and cost per tier (FR-10).

mod admission;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use adoken_model::Millis;
use adoken_model::time::{MINUTE, SECOND};

use admission::Admission;
pub use admission::MAX_BYPASS;

use crate::Tier;
use crate::clock::Clock;
use crate::transport::{ApiRequest, ApiResponse, RequestMeta, Transport, TransportError};

pub const MAX_IN_FLIGHT: usize = 4;
/// Of [`MAX_IN_FLIGHT`], what hot/warm/cold may hold at once: the last slot
/// is the user's (FR-7).
pub const BACKGROUND_IN_FLIGHT: usize = MAX_IN_FLIGHT - 1;
pub const WINDOW_MS: Millis = 5 * MINUTE;
pub const MAX_REQUESTS_PER_WINDOW: usize = 300;
pub const MAX_COST_PER_WINDOW: f64 = 50.0;
pub const BACKOFF_START_MS: Millis = 30 * SECOND;
pub const BACKOFF_CAP_MS: Millis = 5 * MINUTE;
/// A throttled request is retried at most this many times.
const MAX_THROTTLE_RETRIES: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pressure {
    Normal,
    /// < 50% remaining: warm/cold intervals ×4, cold backfill paused.
    Slow,
    /// < 20% remaining: hot tier only, at background cadence.
    HotOnly,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum GovError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("offline: {0}")]
    Offline(String),
    #[error("throttled until {until}")]
    Throttled { until: Millis },
    /// The tier may not run under the current pressure.
    #[error("deferred by rate pressure")]
    Deferred,
    /// `message` is Azure DevOps' error text (`{"message": …}`), if any.
    #[error("HTTP {status} for {url}")]
    Status {
        status: u16,
        url: String,
        message: Option<String>,
    },
}

/// Snapshot for the status pill / debug ledger.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GovSnapshot {
    pub paused_until: Option<Millis>,
    pub pressure_until: Option<Millis>,
    pub resource: Option<String>,
    pub unauthorized: bool,
    /// (tier, requests, summed cost) over the last hour.
    pub ledger: Vec<(Tier, usize, f64)>,
    /// `X-RateLimit-Remaining / Limit` in percent, when last sent.
    pub remaining_pct: Option<f64>,
}

struct State {
    paused_until: Millis,
    backoff_ms: Millis,
    pressure: Pressure,
    pressure_until: Millis,
    resource: Option<String>,
    remaining_pct: Option<f64>,
    unauthorized: bool,
    /// (at, tier, cost) for the last hour.
    ledger: VecDeque<(Millis, Tier, f64)>,
    /// TSTU booked per tier since start (index by [`tier_ix`]).
    spent: [f64; 4],
}

pub struct Governor {
    clock: Arc<dyn Clock>,
    in_flight: Admission,
    state: Mutex<State>,
}

/// Why a request sent now would wait before going out (the review's
/// loading copy, spec 0004 States).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    /// `Retry-After` or 429 backoff (FR-8) until then.
    Paused { until: Millis },
    /// The 5-minute budget (FR-9) is spent until then.
    Budget { until: Millis },
    /// Every slot is taken and sync holds some of them.
    Busy,
}

impl Governor {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Governor {
            clock,
            in_flight: Admission::new(MAX_IN_FLIGHT, BACKGROUND_IN_FLIGHT),
            state: Mutex::new(State {
                paused_until: 0,
                backoff_ms: 0,
                pressure: Pressure::Normal,
                pressure_until: 0,
                resource: None,
                remaining_pct: None,
                unauthorized: false,
                ledger: VecDeque::new(),
                spent: [0.0; 4],
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Current pressure (decays to `Normal` once `X-RateLimit-Reset` passes).
    pub fn pressure(&self) -> Pressure {
        let now = self.clock.now_ms();
        let mut s = self.lock();
        if s.pressure != Pressure::Normal && s.pressure_until > 0 && now >= s.pressure_until {
            s.pressure = Pressure::Normal;
            s.pressure_until = 0;
        }
        s.pressure
    }

    pub fn allows(&self, tier: Tier) -> bool {
        match self.pressure() {
            Pressure::HotOnly => matches!(tier, Tier::Hot | Tier::User),
            _ => true,
        }
    }

    pub fn is_unauthorized(&self) -> bool {
        self.lock().unauthorized
    }

    /// A replacement token verified (spec 0001 FR-15): requests may go out
    /// again. The transport's auth provider already signs with the new one.
    pub fn token_replaced(&self) {
        self.lock().unauthorized = false;
    }

    pub fn snapshot(&self) -> GovSnapshot {
        let now = self.clock.now_ms();
        let s = self.lock();
        let mut ledger: Vec<(Tier, usize, f64)> = Vec::new();
        for (_, tier, cost) in s.ledger.iter().filter(|(at, ..)| *at > now - 60 * MINUTE) {
            match ledger.iter_mut().find(|(t, ..)| t == tier) {
                Some(row) => {
                    row.1 += 1;
                    row.2 += cost;
                }
                None => ledger.push((*tier, 1, *cost)),
            }
        }
        GovSnapshot {
            paused_until: (s.paused_until > now).then_some(s.paused_until),
            pressure_until: (s.pressure_until > now).then_some(s.pressure_until),
            resource: s.resource.clone(),
            unauthorized: s.unauthorized,
            ledger,
            remaining_pct: s.remaining_pct,
        }
    }

    /// Summed TSTU over the sliding 5-minute window (FR-9): measured
    /// `X-RateLimit-Cost`, else the request's cost hint.
    pub fn window_cost(&self) -> f64 {
        let since = self.clock.now_ms() - WINDOW_MS;
        self.lock()
            .ledger
            .iter()
            .filter(|(at, ..)| *at > since)
            .map(|(.., cost)| cost)
            .sum()
    }

    /// TSTU booked at `tier` since the governor started; the scheduler
    /// measures a pass as the difference (spec 0002 FR-4a estimate).
    pub fn spent(&self, tier: Tier) -> f64 {
        self.lock().spent[tier_ix(tier)]
    }

    /// Wait for the host window/budget, send, and interpret rate headers.
    pub async fn send(
        &self,
        transport: &dyn Transport,
        tier: Tier,
        req: ApiRequest,
    ) -> Result<ApiResponse, GovError> {
        self.send_inner(transport, tier, req, false).await
    }

    /// [`Self::send`] for optional endpoints (avatars, spec 0012 FR-9): a
    /// 401 or sign-in page is an ordinary [`GovError::Status`] and never
    /// latches [`GovError::Unauthorized`] for sync.
    pub async fn send_optional(
        &self,
        transport: &dyn Transport,
        tier: Tier,
        req: ApiRequest,
    ) -> Result<ApiResponse, GovError> {
        self.send_inner(transport, tier, req, true).await
    }

    async fn send_inner(
        &self,
        transport: &dyn Transport,
        tier: Tier,
        req: ApiRequest,
        optional: bool,
    ) -> Result<ApiResponse, GovError> {
        // FR-18: the origin of the work this request belongs to, stamped on
        // every attempt with its number and queue wait (FR-17).
        let attribution = crate::attribution::current();
        let origin = req.meta().origin.or(attribution.as_ref().map(|a| a.origin));
        let pass = attribution.and_then(|a| a.pass);
        let mut attempts = 0;
        loop {
            let queued_from = self.clock.now_ms();
            if self.is_unauthorized() {
                return Err(GovError::Unauthorized);
            }
            if !self.allows(tier) {
                return Err(GovError::Deferred);
            }
            self.wait_for_window().await;
            let _permit = self.in_flight.acquire(tier).await;
            let sent_at = self.clock.now_ms();
            let mut attempt = req.clone();
            attempt.set_meta(RequestMeta {
                origin,
                pass: pass.as_ref().map(|p| p.pass),
                tier: Some(tier),
                attempt: attempts + 1,
                queued_ms: (sent_at - queued_from).max(0) as u64,
                optional,
            });
            let result = transport.send(attempt).await;
            let now = self.clock.now_ms().max(sent_at);
            if let Some(p) = &pass {
                let booked = match &result {
                    Ok(resp) => resp.rate.cost.or(req.cost_hint()).unwrap_or(0.0),
                    Err(_) => 0.0,
                };
                p.counters.add(booked);
            }
            match result {
                Err(TransportError::Unauthorized) => {
                    if !optional {
                        self.lock().unauthorized = true;
                    }
                    self.record(sent_at, tier, 0.0);
                    return Err(GovError::Unauthorized);
                }
                Err(TransportError::Offline(e)) => return Err(GovError::Offline(e)),
                Err(TransportError::Throttled { retry_after_ms }) => {
                    let until = now + retry_after_ms;
                    self.pause_until(until);
                    attempts += 1;
                    if attempts > MAX_THROTTLE_RETRIES {
                        return Err(GovError::Throttled { until });
                    }
                }
                Ok(resp) => {
                    let cost = resp.rate.cost.or(req.cost_hint()).unwrap_or(0.0);
                    self.record(sent_at, tier, cost);
                    self.observe(now, &resp);
                    match resp.status {
                        // 203 is Azure DevOps' sign-in page.
                        401 | 203 if !optional => {
                            self.lock().unauthorized = true;
                            return Err(GovError::Unauthorized);
                        }
                        // 304: a conditional GET's cached copy is still valid.
                        200..=299 | 304 => {
                            self.lock().backoff_ms = 0;
                            return Ok(resp);
                        }
                        429 | 503 => {
                            let until = {
                                let mut s = self.lock();
                                let until = match resp.rate.retry_after_ms {
                                    Some(ms) => now + ms,
                                    None => {
                                        s.backoff_ms = if s.backoff_ms == 0 {
                                            BACKOFF_START_MS
                                        } else {
                                            (s.backoff_ms * 2).min(BACKOFF_CAP_MS)
                                        };
                                        now + s.backoff_ms
                                    }
                                };
                                s.paused_until = s.paused_until.max(until);
                                until
                            };
                            attempts += 1;
                            if attempts > MAX_THROTTLE_RETRIES {
                                return Err(GovError::Throttled { until });
                            }
                        }
                        status => {
                            return Err(GovError::Status {
                                status,
                                url: req.url().to_string(),
                                message: error_message(&resp.body),
                            });
                        }
                    }
                }
            }
        }
    }

    fn pause_until(&self, until: Millis) {
        let mut s = self.lock();
        s.paused_until = s.paused_until.max(until);
    }

    fn record(&self, at: Millis, tier: Tier, cost: f64) {
        let mut s = self.lock();
        s.spent[tier_ix(tier)] += cost;
        s.ledger.push_back((at, tier, cost));
        while s
            .ledger
            .front()
            .is_some_and(|(t, ..)| *t <= at - 60 * MINUTE)
        {
            s.ledger.pop_front();
        }
    }

    fn observe(&self, now: Millis, resp: &ApiResponse) {
        let r = &resp.rate;
        let mut s = self.lock();
        if let Some(ms) = r.retry_after_ms {
            s.paused_until = s.paused_until.max(now + ms);
        }
        if let Some(ms) = r.delay_ms.filter(|d| *d > 0) {
            // Server is already delaying us: back off at least as long.
            s.paused_until = s.paused_until.max(now + ms);
        }
        if r.resource.is_some() {
            s.resource = r.resource.clone();
        }
        if let (Some(limit), Some(remaining)) = (r.limit, r.remaining)
            && limit > 0.0
        {
            let ratio = remaining / limit;
            s.remaining_pct = Some(ratio * 100.0);
            // AC-4 treats exactly 20% (40/200) as hot-only.
            let pressure = if ratio <= 0.2 {
                Pressure::HotOnly
            } else if ratio < 0.5 {
                Pressure::Slow
            } else {
                Pressure::Normal
            };
            s.pressure = pressure;
            s.pressure_until = if pressure == Pressure::Normal {
                0
            } else {
                r.reset_s.map(|t| t * 1000).unwrap_or(now + WINDOW_MS)
            };
        }
    }

    /// When the host's `Retry-After` / backoff pause ends, and when the
    /// spent 5-minute budget frees up, if either is in the future.
    fn window(&self, now: Millis) -> (Option<Millis>, Option<Millis>) {
        let s = self.lock();
        let paused = (s.paused_until > now).then_some(s.paused_until);
        let recent = s.ledger.iter().filter(|(t, ..)| *t > now - WINDOW_MS);
        let oldest = recent.clone().next().map(|(t, ..)| *t);
        let (count, cost) = recent.fold((0, 0.0), |(n, c), (.., cost)| (n + 1, c + cost));
        let budget = (count >= MAX_REQUESTS_PER_WINDOW || cost >= MAX_COST_PER_WINDOW)
            .then_some(oldest)
            .flatten()
            .map(|oldest| oldest + WINDOW_MS + 1);
        (paused, budget)
    }

    /// Why a request sent now would wait, if it would.
    pub fn hold(&self) -> Option<Hold> {
        let (paused, budget) = self.window(self.clock.now_ms());
        if let Some(until) = paused {
            return Some(Hold::Paused { until });
        }
        if let Some(until) = budget {
            return Some(Hold::Budget { until });
        }
        let o = self.in_flight.occupancy();
        (o.in_flight >= MAX_IN_FLIGHT && o.background > 0).then_some(Hold::Busy)
    }

    /// Sleep past `Retry-After` / backoff windows and the self-imposed budget.
    async fn wait_for_window(&self) {
        loop {
            let now = self.clock.now_ms();
            let (paused, budget) = self.window(now);
            let Some(until) = paused.max(budget) else {
                return;
            };
            self.clock.sleep(until - now).await;
        }
    }
}

fn tier_ix(tier: Tier) -> usize {
    match tier {
        Tier::User => 0,
        Tier::Hot => 1,
        Tier::Warm => 2,
        Tier::Cold => 3,
    }
}

/// Azure DevOps error bodies: `{"$id": "1", "message": "TF401181: …", …}`.
fn error_message(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()?
        .get("message")?
        .as_str()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;

    /// The review's "waiting" copy: a pause outranks a spent budget, and
    /// both clear once their time passes.
    #[test]
    fn hold_names_the_pause_then_the_budget() {
        let clock = Arc::new(ManualClock::new(1_000_000));
        let gov = Governor::new(clock.clone());
        assert_eq!(gov.hold(), None);
        let now = clock.now_ms();
        for _ in 0..MAX_REQUESTS_PER_WINDOW {
            gov.record(now, Tier::Cold, 0.0);
        }
        let freed = now + WINDOW_MS + 1;
        assert_eq!(gov.hold(), Some(Hold::Budget { until: freed }));
        gov.pause_until(now + 30 * SECOND);
        assert_eq!(
            gov.hold(),
            Some(Hold::Paused {
                until: now + 30 * SECOND
            })
        );
        clock.advance(30 * SECOND);
        assert_eq!(gov.hold(), Some(Hold::Budget { until: freed }));
        clock.advance(WINDOW_MS);
        assert_eq!(gov.hold(), None);
    }
}
