//! Hand-written multi-file diffs with anchored review threads for a few demo
//! PRs (Rust + TypeScript), including threads left on an older iteration
//! whose lines moved (mappable) or were deleted ("comments on older code").

use adoken_model::{
    ChangeType, Comment, LineRange, Millis, PersonId, PrFileChange, PrThread, ThreadAnchor,
    ThreadStatus, time::HOUR, time::MINUTE,
};

use crate::rng::object_id;

/// Materialized rich PR content.
pub struct RichContent {
    pub files: Vec<PrFileChange>,
    pub blobs: Vec<(String, String)>,
    pub threads: Vec<PrThread>,
    /// Number of iterations the threads assume (≥ 2).
    pub iterations: u32,
}

pub struct RichSpec {
    pub repo: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub branch: &'static str,
}

pub const RICH: &[RichSpec] = &[
    RichSpec {
        repo: "payments-api",
        title: "Retry idempotent charge calls with jittered backoff",
        description: "## Why\nCharge calls fail with transient 502s from the gateway a few times a day, and we surface them to shoppers as hard declines.\n\n## What\n- New `retry` module: exponential backoff with full jitter, capped at 3 attempts / 2s total\n- `ChargeService::charge` retries only idempotent calls (every charge carries an idempotency key)\n- Refunds are **not** retried yet (follow-up)\n\n## Testing\n- Unit tests for the backoff schedule\n- Soak test in staging with 5% injected 502s: 0 shopper-visible failures over 2h",
        branch: "refs/heads/feature/charge-retry-backoff",
    },
    RichSpec {
        repo: "checkout-web",
        title: "Debounce address autocomplete and cancel stale requests",
        description: "Typing an address fired one Places request per keystroke and could render results for an older query after a newer one.\n\n- `useDebounced` hook (250 ms)\n- `AbortController` per request; stale responses are dropped\n- Removes the legacy jQuery autocomplete\n- Renames `utils/format.ts` → `utils/formatAddress.ts`",
        branch: "refs/heads/fix/address-autocomplete-debounce",
    },
    RichSpec {
        repo: "ledger-service",
        title: "Split posting engine: validate currency before balancing",
        description: "Part 1 of the ledger engine split. Moves journal entry types into `journal.rs` and validates currency codes before balancing, so mixed-currency postings fail fast with a clear error.\n\n`engine.rs` is large; the interesting hunks are in `post_entry` and the new `check_currency`.",
        branch: "refs/heads/feature/ledger-engine-split",
    },
];

// ── PR 1: Rust retry ──────────────────────────────────────────────────────

const CHARGE_BASE: &str = r#"use crate::client::{Gateway, GatewayError};
use crate::model::{Charge, ChargeRequest};

/// Charges a card through the configured gateway.
pub struct ChargeService {
    gateway: Gateway,
}

impl ChargeService {
    pub fn new(gateway: Gateway) -> Self {
        Self { gateway }
    }

    pub async fn charge(&self, req: &ChargeRequest) -> Result<Charge, GatewayError> {
        let response = self.gateway.post("/v1/charges", req).await?;
        if response.status == 409 {
            // Duplicate request: the gateway already has this charge.
            return self.gateway.get_charge(&req.idempotency_key).await;
        }
        response.json::<Charge>()
    }

    pub async fn refund(&self, charge: &Charge, amount_minor: i64) -> Result<(), GatewayError> {
        if amount_minor > charge.amount_minor {
            return Err(GatewayError::InvalidAmount);
        }
        self.gateway
            .post("/v1/refunds", &(charge.id.as_str(), amount_minor))
            .await?;
        Ok(())
    }
}
"#;

const CHARGE_ITER1: &str = r#"use crate::client::{Gateway, GatewayError};
use crate::model::{Charge, ChargeRequest};

/// Charges a card through the configured gateway.
pub struct ChargeService {
    gateway: Gateway,
}

impl ChargeService {
    pub fn new(gateway: Gateway) -> Self {
        Self { gateway }
    }

    pub async fn charge(&self, req: &ChargeRequest) -> Result<Charge, GatewayError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let response = match self.gateway.post("/v1/charges", req).await {
                Ok(response) => response,
                Err(_) if attempt < 3 => {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    continue;
                }
                Err(e) => return Err(e),
            };
            if response.status == 409 {
                // Duplicate request: the gateway already has this charge.
                return self.gateway.get_charge(&req.idempotency_key).await;
            }
            return response.json::<Charge>();
        }
    }

    pub async fn refund(&self, charge: &Charge, amount_minor: i64) -> Result<(), GatewayError> {
        if amount_minor > charge.amount_minor {
            return Err(GatewayError::InvalidAmount);
        }
        self.gateway
            .post("/v1/refunds", &(charge.id.as_str(), amount_minor))
            .await?;
        Ok(())
    }
}
"#;

const CHARGE_FINAL: &str = r#"use crate::client::{Gateway, GatewayError};
use crate::model::{Charge, ChargeRequest};
use crate::retry::{Backoff, retry_idempotent};

/// Charges a card through the configured gateway.
///
/// Every charge carries an idempotency key, so transient gateway failures are
/// retried with jittered exponential backoff (see `retry.rs`).
pub struct ChargeService {
    gateway: Gateway,
    backoff: Backoff,
}

impl ChargeService {
    pub fn new(gateway: Gateway) -> Self {
        Self {
            gateway,
            backoff: Backoff::default(),
        }
    }

    pub async fn charge(&self, req: &ChargeRequest) -> Result<Charge, GatewayError> {
        retry_idempotent(&self.backoff, || async {
            let response = self.gateway.post("/v1/charges", req).await?;
            if response.status == 409 {
                // Duplicate request: the gateway already has this charge.
                return self.gateway.get_charge(&req.idempotency_key).await;
            }
            response.json::<Charge>()
        })
        .await
    }

    pub async fn refund(&self, charge: &Charge, amount_minor: i64) -> Result<(), GatewayError> {
        if amount_minor > charge.amount_minor {
            return Err(GatewayError::InvalidAmount);
        }
        self.gateway
            .post("/v1/refunds", &(charge.id.as_str(), amount_minor))
            .await?;
        Ok(())
    }
}
"#;

const RETRY_NEW: &str = r#"//! Jittered exponential backoff for idempotent gateway calls.

use std::future::Future;
use std::time::Duration;

use crate::client::GatewayError;

/// Backoff schedule: `base * 2^attempt`, full jitter, capped.
#[derive(Clone, Debug)]
pub struct Backoff {
    pub base: Duration,
    pub cap: Duration,
    pub max_attempts: u32,
    /// Total time budget across all attempts.
    pub budget: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            base: Duration::from_millis(100),
            cap: Duration::from_millis(800),
            max_attempts: 3,
            budget: Duration::from_secs(2),
        }
    }
}

impl Backoff {
    /// Delay before retry number `attempt` (1-based), before jitter.
    pub fn delay(&self, attempt: u32) -> Duration {
        let exp = self.base.saturating_mul(1 << attempt.min(16));
        exp.min(self.cap)
    }
}

/// Runs `op` until it succeeds, fails permanently, or the budget runs out.
pub async fn retry_idempotent<T, F, Fut>(backoff: &Backoff, mut op: F) -> Result<T, GatewayError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, GatewayError>>,
{
    let started = tokio::time::Instant::now();
    let mut attempt = 0;
    loop {
        attempt += 1;
        match op().await {
            Ok(value) => return Ok(value),
            Err(e) if !e.is_transient() || attempt >= backoff.max_attempts => return Err(e),
            Err(e) => {
                let delay = jitter(backoff.delay(attempt));
                if started.elapsed() + delay > backoff.budget {
                    return Err(e);
                }
                tracing::debug!(attempt, ?delay, "retrying charge after transient error");
                tokio::time::sleep(delay).await;
            }
        }
    }
}

fn jitter(max: Duration) -> Duration {
    let nanos = max.as_nanos() as u64;
    Duration::from_nanos(fastrand::u64(0..=nanos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_is_capped() {
        let b = Backoff::default();
        assert_eq!(b.delay(1), Duration::from_millis(200));
        assert_eq!(b.delay(10), b.cap);
    }
}
"#;

const LIB_BASE: &str = r#"pub mod charge;
pub mod client;
pub mod model;
pub mod webhook;
"#;

const LIB_NEW: &str = r#"pub mod charge;
pub mod client;
pub mod model;
pub mod retry;
pub mod webhook;
"#;

const CARGO_BASE: &str = r#"[package]
name = "payments-api"
version = "2.14.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["rt-multi-thread", "time"] }
tracing = "0.1"
"#;

const CARGO_NEW: &str = r#"[package]
name = "payments-api"
version = "2.15.0"
edition = "2024"

[dependencies]
fastrand = "2"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["rt-multi-thread", "time"] }
tracing = "0.1"
"#;

// ── PR 2: TypeScript autocomplete ─────────────────────────────────────────

const FIELD_BASE: &str = r#"import { useEffect, useState } from "react";
import { searchPlaces, type Place } from "../api/places";
import { formatAddress } from "../utils/format";

type Props = {
  value: string;
  onSelect: (place: Place) => void;
};

export function AddressField({ value, onSelect }: Props) {
  const [query, setQuery] = useState(value);
  const [results, setResults] = useState<Place[]>([]);

  useEffect(() => {
    if (query.length < 3) {
      setResults([]);
      return;
    }
    searchPlaces(query).then(setResults);
  }, [query]);

  return (
    <div className="address-field">
      <input
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder="Start typing your address"
      />
      <ul role="listbox">
        {results.map((place) => (
          <li key={place.id} role="option" onClick={() => onSelect(place)}>
            {formatAddress(place)}
          </li>
        ))}
      </ul>
    </div>
  );
}
"#;

const FIELD_NEW: &str = r#"import { useEffect, useState } from "react";
import { searchPlaces, type Place } from "../api/places";
import { useDebounced } from "../hooks/useDebounced";
import { formatAddress } from "../utils/formatAddress";

type Props = {
  value: string;
  onSelect: (place: Place) => void;
};

const MIN_QUERY = 3;

export function AddressField({ value, onSelect }: Props) {
  const [query, setQuery] = useState(value);
  const [results, setResults] = useState<Place[]>([]);
  const debounced = useDebounced(query, 250);

  useEffect(() => {
    if (debounced.length < MIN_QUERY) {
      setResults([]);
      return;
    }
    const controller = new AbortController();
    searchPlaces(debounced, controller.signal)
      .then(setResults)
      .catch((err) => {
        if (err.name !== "AbortError") throw err;
      });
    return () => controller.abort();
  }, [debounced]);

  return (
    <div className="address-field">
      <input
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder="Start typing your address"
        aria-autocomplete="list"
        aria-expanded={results.length > 0}
      />
      <ul role="listbox">
        {results.map((place) => (
          <li key={place.id} role="option" onClick={() => onSelect(place)}>
            {formatAddress(place)}
          </li>
        ))}
      </ul>
    </div>
  );
}
"#;

const DEBOUNCE_NEW: &str = r#"import { useEffect, useState } from "react";

/** Returns `value` after it has been stable for `delayMs`. */
export function useDebounced<T>(value: T, delayMs: number): T {
  const [debounced, setDebounced] = useState(value);

  useEffect(() => {
    const id = window.setTimeout(() => setDebounced(value), delayMs);
    return () => window.clearTimeout(id);
  }, [value, delayMs]);

  return debounced;
}
"#;

const PLACES_BASE: &str = r#"export type Place = {
  id: string;
  line1: string;
  city: string;
  postcode: string;
  country: string;
};

const ENDPOINT = "/api/places/search";

export async function searchPlaces(query: string): Promise<Place[]> {
  const res = await fetch(`${ENDPOINT}?q=${encodeURIComponent(query)}`);
  if (!res.ok) {
    return [];
  }
  const body = await res.json();
  return body.results as Place[];
}
"#;

const PLACES_NEW: &str = r#"export type Place = {
  id: string;
  line1: string;
  city: string;
  postcode: string;
  country: string;
};

const ENDPOINT = "/api/places/search";

export async function searchPlaces(query: string, signal?: AbortSignal): Promise<Place[]> {
  const res = await fetch(`${ENDPOINT}?q=${encodeURIComponent(query)}`, { signal });
  if (!res.ok) {
    throw new Error(`places search failed: ${res.status}`);
  }
  const body: { results: Place[] } = await res.json();
  return body.results;
}
"#;

const LEGACY_JS: &str = r##"// Legacy jQuery autocomplete, replaced by AddressField.
$(function () {
  $("#address").autocomplete({
    source: "/api/places/search",
    minLength: 3,
    select: function (event, ui) {
      $("#address").val(ui.item.label);
    },
  });
});
"##;

const FORMAT_BASE: &str = r#"import type { Place } from "../api/places";

export function formatAddress(place: Place): string {
  return [place.line1, place.city, place.postcode].join(", ");
}
"#;

const FORMAT_NEW: &str = r#"import type { Place } from "../api/places";

export function formatAddress(place: Place): string {
  return [place.line1, `${place.postcode} ${place.city}`, place.country]
    .filter(Boolean)
    .join(", ");
}
"#;

// ── PR 3: large Rust file ─────────────────────────────────────────────────

const ACCOUNT_KINDS: &[&str] = &[
    "cash",
    "receivable",
    "payable",
    "revenue",
    "refund",
    "fee",
    "tax",
    "payout",
    "reserve",
    "chargeback",
    "fx_gain",
    "fx_loss",
    "interest",
    "adjustment",
    "suspense",
];

/// ~3k-line `engine.rs`; `new` adds currency validation in each handler.
fn engine_source(new: bool) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    s.push_str("//! Double-entry posting engine.\n\nuse crate::journal::{Entry, Line};\nuse crate::money::{Currency, Money};\n\n");
    if new {
        s.push_str("/// Rejects entries whose lines mix currencies.\npub fn check_currency(entry: &Entry) -> Result<Currency, PostError> {\n    let currency = entry.lines[0].amount.currency;\n    if entry.lines.iter().any(|l| l.amount.currency != currency) {\n        return Err(PostError::MixedCurrency);\n    }\n    Ok(currency)\n}\n\n");
    }
    s.push_str("#[derive(Debug, PartialEq)]\npub enum PostError {\n    Unbalanced,\n    UnknownAccount(String),\n");
    if new {
        s.push_str("    MixedCurrency,\n");
    }
    s.push_str("}\n\n");
    for round in 0..9 {
        for kind in ACCOUNT_KINDS {
            let name = format!("{kind}_{round}");
            let _ = writeln!(s, "/// Posts a `{kind}` entry (bucket {round}).");
            let _ = writeln!(
                s,
                "pub fn post_{name}(entry: &Entry) -> Result<Vec<Line>, PostError> {{"
            );
            if new {
                let _ = writeln!(s, "    let currency = check_currency(entry)?;");
            }
            let _ = writeln!(
                s,
                "    let mut debit = Money::zero(entry.lines[0].amount.currency);"
            );
            let _ = writeln!(
                s,
                "    let mut credit = Money::zero(entry.lines[0].amount.currency);"
            );
            let _ = writeln!(s, "    for line in &entry.lines {{");
            let _ = writeln!(s, "        if !line.account.starts_with(\"{kind}\") {{");
            let _ = writeln!(
                s,
                "            return Err(PostError::UnknownAccount(line.account.clone()));"
            );
            let _ = writeln!(s, "        }}");
            let _ = writeln!(s, "        if line.amount.is_debit() {{");
            let _ = writeln!(s, "            debit = debit + line.amount;");
            let _ = writeln!(s, "        }} else {{");
            let _ = writeln!(s, "            credit = credit + line.amount;");
            let _ = writeln!(s, "        }}");
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s, "    if debit != credit {{");
            let _ = writeln!(s, "        return Err(PostError::Unbalanced);");
            let _ = writeln!(s, "    }}");
            if new {
                let _ = writeln!(s, "    debug_assert_eq!(debit.currency, currency);");
            }
            let _ = writeln!(s, "    Ok(entry.lines.clone())");
            let _ = writeln!(s, "}}\n");
        }
    }
    s
}

const JOURNAL_NEW: &str = r#"//! Journal entry types, moved out of `engine.rs`.

use crate::money::Money;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub account: String,
    pub amount: Money,
    pub memo: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: u64,
    pub lines: Vec<Line>,
}
"#;

// ── assembly ──────────────────────────────────────────────────────────────

fn line_of(content: &str, needle: &str) -> u32 {
    content
        .lines()
        .position(|l| l.contains(needle))
        .map(|i| i as u32 + 1)
        .unwrap_or_else(|| panic!("demo code: {needle:?} not found"))
}

struct Files {
    files: Vec<PrFileChange>,
    blobs: Vec<(String, String)>,
}

impl Files {
    fn blob(&mut self, content: &str) -> String {
        let id = object_id(content);
        if !self.blobs.iter().any(|(b, _)| *b == id) {
            self.blobs.push((id.clone(), content.to_owned()));
        }
        id
    }

    fn change(&mut self, path: &str, old: Option<&str>, new: Option<&str>) {
        let change_type = match (old, new) {
            (None, _) => ChangeType::Add,
            (_, None) => ChangeType::Delete,
            _ => ChangeType::Edit,
        };
        let old_content = old.map(|c| self.blob(c));
        let new_content = new.map(|c| self.blob(c));
        // Azure DevOps numbers a PR's changed files 1, 2, … per iteration.
        let change_tracking_id = Some(self.files.len() as u32 + 1);
        self.files.push(PrFileChange {
            path: path.to_owned(),
            original_path: None,
            change_type,
            old_content,
            new_content,
            change_tracking_id,
        });
    }
}

/// Thread builder input: (author, body, minutes after thread start).
type Msg<'a> = (&'a PersonId, String, i64);

#[allow(clippy::too_many_arguments)]
fn thread(
    pr_id: i64,
    id: i64,
    status: ThreadStatus,
    path: Option<(&str, u32, u32, Option<String>)>,
    iteration: Option<u32>,
    start: Millis,
    msgs: Vec<Msg<'_>>,
) -> PrThread {
    let comments: Vec<Comment> = msgs
        .into_iter()
        .enumerate()
        .map(|(i, (author, body, after))| Comment {
            id: i as i64 + 1,
            parent_id: (i > 0).then_some(1),
            author: author.clone(),
            body,
            created: start + after * MINUTE,
            updated: start + after * MINUTE,
            is_system: false,
            is_deleted: false,
        })
        .collect();
    let updated = comments.last().map(|c| c.updated).unwrap_or(start);
    PrThread {
        id,
        pr_id,
        status,
        anchor: path.map(|(file_path, start, end, blob)| ThreadAnchor {
            file_path: file_path.to_owned(),
            right: Some(LineRange { start, end }),
            left: None,
            iteration_blob: blob,
        }),
        iteration,
        comments,
        updated,
    }
}

/// Participants: `author` of the PR, `me`, and two other reviewers.
pub struct Cast<'a> {
    pub author: &'a PersonId,
    pub me: &'a PersonId,
    pub r1: &'a PersonId,
    pub r2: &'a PersonId,
}

pub fn build(which: usize, pr_id: i64, created: Millis, cast: &Cast<'_>) -> RichContent {
    let mention = |id: &PersonId| format!("@<{id}>");
    let mut f = Files {
        files: Vec::new(),
        blobs: Vec::new(),
    };
    let t0 = created + 2 * HOUR;
    let (threads, iterations) = match which {
        0 => {
            f.change("/Cargo.toml", Some(CARGO_BASE), Some(CARGO_NEW));
            f.change("/src/charge.rs", Some(CHARGE_BASE), Some(CHARGE_FINAL));
            f.change("/src/lib.rs", Some(LIB_BASE), Some(LIB_NEW));
            f.change("/src/retry.rs", None, Some(RETRY_NEW));
            let iter1 = f.blob(CHARGE_ITER1);
            let fin = f.blob(CHARGE_FINAL);
            let sleep = line_of(CHARGE_ITER1, "tokio::time::sleep");
            let dup = line_of(CHARGE_ITER1, "if response.status == 409");
            let retry_call = line_of(CHARGE_FINAL, "retry_idempotent(&self.backoff");
            let budget = line_of(RETRY_NEW, "if started.elapsed() + delay > backoff.budget");
            let threads = vec![
                thread(
                    pr_id,
                    1,
                    ThreadStatus::Fixed,
                    Some(("/src/charge.rs", sleep, sleep, Some(iter1.clone()))),
                    Some(1),
                    t0,
                    vec![
                        (
                            cast.r1,
                            "A fixed 200 ms sleep without jitter will synchronize retries across pods during a gateway blip. Can we use full jitter?".into(),
                            0,
                        ),
                        (cast.author, "Good call — moved this into a `retry` module with full jitter.".into(), 95),
                    ],
                ),
                thread(
                    pr_id,
                    2,
                    ThreadStatus::Active,
                    Some(("/src/charge.rs", dup, dup + 3, Some(iter1))),
                    Some(1),
                    t0 + 10 * MINUTE,
                    vec![
                        (
                            cast.r2,
                            format!(
                                "{} should a 409 really short-circuit the retry loop? If the first attempt timed out after the gateway accepted it, we'd fetch a charge that might still be pending.",
                                mention(cast.me)
                            ),
                            0,
                        ),
                        (cast.author, "The gateway only returns 409 once the original request completed, so `get_charge` sees the final state. I'll add a comment.".into(), 40),
                    ],
                ),
                thread(
                    pr_id,
                    3,
                    ThreadStatus::Active,
                    Some(("/src/charge.rs", retry_call, retry_call, Some(fin))),
                    Some(2),
                    t0 + 26 * HOUR,
                    vec![(
                        cast.r1,
                        "What's the worst-case total delay here? Checkout has a 10 s budget end to end.".into(),
                        0,
                    )],
                ),
                thread(
                    pr_id,
                    4,
                    ThreadStatus::Active,
                    Some(("/src/retry.rs", budget, budget + 2, None)),
                    Some(2),
                    t0 + 27 * HOUR,
                    vec![(
                        cast.r2,
                        "nit: `elapsed() + delay` can overshoot by one op latency. Probably fine, but worth a doc comment on `budget`.".into(),
                        0,
                    )],
                ),
                thread(
                    pr_id,
                    5,
                    ThreadStatus::Active,
                    None,
                    None,
                    t0 + 28 * HOUR,
                    vec![(
                        cast.r1,
                        "Overall LGTM once the max-delay question is answered. Nice soak test numbers.".into(),
                        0,
                    )],
                ),
            ];
            (threads, 2)
        }
        1 => {
            f.change(
                "/src/components/AddressField.tsx",
                Some(FIELD_BASE),
                Some(FIELD_NEW),
            );
            f.change("/src/hooks/useDebounced.ts", None, Some(DEBOUNCE_NEW));
            f.change("/src/api/places.ts", Some(PLACES_BASE), Some(PLACES_NEW));
            f.change("/src/legacy/autocomplete.js", Some(LEGACY_JS), None);
            let old = f.blob(FORMAT_BASE);
            let new = f.blob(FORMAT_NEW);
            let change_tracking_id = Some(f.files.len() as u32 + 1);
            f.files.push(PrFileChange {
                path: "/src/utils/formatAddress.ts".into(),
                original_path: Some("/src/utils/format.ts".into()),
                change_type: ChangeType::Rename,
                old_content: Some(old),
                new_content: Some(new),
                change_tracking_id,
            });
            let abort = line_of(FIELD_NEW, "const controller = new AbortController()");
            let throw = line_of(PLACES_NEW, "throw new Error");
            let delay = line_of(FIELD_NEW, "useDebounced(query, 250)");
            let threads = vec![
                thread(
                    pr_id,
                    1,
                    ThreadStatus::Active,
                    Some(("/src/api/places.ts", throw, throw, None)),
                    Some(1),
                    t0,
                    vec![
                        (
                            cast.me,
                            "Throwing here changes behavior for the checkout summary page, which also calls `searchPlaces` and expects `[]` on failure.".into(),
                            0,
                        ),
                        (cast.author, "Ah, I missed that caller. Will wrap it there.".into(), 30),
                    ],
                ),
                thread(
                    pr_id,
                    2,
                    ThreadStatus::Fixed,
                    Some(("/src/components/AddressField.tsx", abort, abort + 5, None)),
                    Some(1),
                    t0 + 20 * MINUTE,
                    vec![(
                        cast.r1,
                        "Love the AbortController. Can we also ignore results when `debounced` changed while awaiting?".into(),
                        0,
                    )],
                ),
                thread(
                    pr_id,
                    3,
                    ThreadStatus::Active,
                    Some(("/src/components/AddressField.tsx", delay, delay, None)),
                    Some(1),
                    t0 + HOUR,
                    vec![(
                        cast.r2,
                        format!("{} 250 ms felt sluggish in the UX study; 150 ms?", mention(cast.me)),
                        0,
                    )],
                ),
            ];
            (threads, 1)
        }
        _ => {
            let old = engine_source(false);
            let new = engine_source(true);
            f.change("/src/engine.rs", Some(&old), Some(&new));
            f.change("/src/journal.rs", None, Some(JOURNAL_NEW));
            let check = line_of(&new, "pub fn check_currency");
            let deep = line_of(&new, "pub fn post_tax_6");
            let threads = vec![
                thread(
                    pr_id,
                    1,
                    ThreadStatus::Active,
                    Some(("/src/engine.rs", check, check + 6, None)),
                    Some(1),
                    t0,
                    vec![(
                        cast.r1,
                        "`entry.lines[0]` panics on an empty entry. Return `PostError::Unbalanced` for empty entries first?".into(),
                        0,
                    )],
                ),
                thread(
                    pr_id,
                    2,
                    ThreadStatus::Pending,
                    Some(("/src/engine.rs", deep, deep + 1, None)),
                    Some(1),
                    t0 + 3 * HOUR,
                    vec![(
                        cast.r2,
                        "These 135 handlers are identical apart from the prefix. Follow-up to generate them with a macro?".into(),
                        0,
                    )],
                ),
            ];
            (threads, 1)
        }
    };
    RichContent {
        files: f.files,
        blobs: f.blobs,
        threads,
        iterations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_iteration_anchors_move_or_vanish() {
        // Thread 2 anchors a line that still exists (moved) in the final file;
        // thread 1 anchors a line deleted by iteration 2.
        let dup = line_of(CHARGE_ITER1, "if response.status == 409");
        let dup_final = line_of(CHARGE_FINAL, "if response.status == 409");
        assert_ne!(dup, dup_final);
        assert!(!CHARGE_FINAL.contains("tokio::time::sleep"));
        let lines = engine_source(true).lines().count();
        assert!((2500..4000).contains(&lines), "{lines}");
    }
}
