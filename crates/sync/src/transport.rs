//! The only way sync reaches the network. Reads are GETs, plus POSTs to the
//! WIQL and `workitemsbatch` endpoints (read-only queries). Writes
//! (POST/PUT/PATCH with a JSON body) exist only for user actions
//! ([`crate::writes`]); background sync never builds one.

use std::sync::Arc;

use adoken_ado::AuthProvider;
use adoken_ado::http::{Http, HttpError};
use futures::future::BoxFuture;
use reqwest::header::HeaderMap;
use url::Url;

use crate::Tier;
use crate::attribution::{Origin, PassRef};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    /// Read-only query POSTs: `…/_apis/wit/wiql` and `…/_apis/wit/workitemsbatch`.
    PostQuery,
    /// User-initiated writes ([`crate::writes`]).
    Post,
    Put,
    Patch,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::PostQuery | Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
        }
    }

    /// Changes server state (never sent by a sync tier).
    pub fn is_write(&self) -> bool {
        matches!(self, Method::Post | Method::Put | Method::Patch)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApiRequest {
    method: Method,
    url: Url,
    body: Option<serde_json::Value>,
    /// `Content-Type` of `body`.
    content_type: &'static str,
    cost_hint: Option<f64>,
    /// Conditional GET validators (`If-None-Match`, `If-Modified-Since`).
    validators: Validators,
    /// Attribution and timing the governor stamps on each attempt (spec
    /// 0016 FR-17/FR-18); never sent.
    meta: RequestMeta,
}

/// What the request recorder needs beyond the HTTP request itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RequestMeta {
    /// Explicit origin; the governor fills it from the task's
    /// [`crate::attribution`] scope when unset.
    pub origin: Option<Origin>,
    pub pass: Option<PassRef>,
    pub tier: Option<Tier>,
    /// 1-based try of the governor's retries after 429 / `Retry-After`.
    pub attempt: u32,
    /// Held by the governor before sending (slots, `Retry-After`, budget).
    pub queued_ms: u64,
    /// Sent with [`crate::Governor::send_optional`] (avatars, work item
    /// links): background failures don't light the indicator (Q-10).
    pub optional: bool,
}

/// Cache validators of a stored response (spec 0012 avatar revalidation).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

const JSON: &str = "application/json";
/// Work item updates take RFC 6902 operations under this type only.
const JSON_PATCH: &str = "application/json-patch+json";

impl ApiRequest {
    fn new(method: Method, url: Url, body: Option<serde_json::Value>) -> Self {
        ApiRequest {
            method,
            url,
            body,
            content_type: JSON,
            cost_hint: None,
            validators: Validators::default(),
            meta: RequestMeta::default(),
        }
    }

    pub fn get(url: Url) -> Self {
        Self::new(Method::Get, url, None)
    }

    /// WIQL query. Panics if `url` is not a `…/_apis/wit/wiql` endpoint: sync
    /// must never issue any other write-method request.
    pub fn wiql(url: Url, query: &str) -> Self {
        assert!(
            url.path().ends_with("/_apis/wit/wiql"),
            "POST is only allowed for WIQL queries, got {url}"
        );
        Self::new(
            Method::PostQuery,
            url,
            Some(serde_json::json!({ "query": query })),
        )
    }

    /// `workitemsbatch` read (spec 0014 FR-11: pinned work items): at most
    /// 200 ids, with relations, and `errorPolicy: omit` so a deleted item
    /// comes back as `null` instead of failing the batch. Panics on any other
    /// endpoint.
    pub fn workitems_batch(url: Url, ids: &[i64]) -> Self {
        assert!(
            url.path().ends_with("/_apis/wit/workitemsbatch"),
            "POST is only allowed for read-only queries, got {url}"
        );
        Self::new(
            Method::PostQuery,
            url,
            Some(serde_json::json!({
                "ids": ids,
                "$expand": "relations",
                "errorPolicy": "omit",
            })),
        )
    }

    /// User-initiated JSON writes ([`crate::writes`] builds these).
    pub fn post(url: Url, body: serde_json::Value) -> Self {
        Self::new(Method::Post, url, Some(body))
    }

    pub fn put(url: Url, body: serde_json::Value) -> Self {
        Self::new(Method::Put, url, Some(body))
    }

    pub fn patch(url: Url, body: serde_json::Value) -> Self {
        Self::new(Method::Patch, url, Some(body))
    }

    /// `PATCH` with a JSON Patch document (`application/json-patch+json`),
    /// the only form `_apis/wit/workitems/{id}` accepts.
    pub fn json_patch(url: Url, operations: serde_json::Value) -> Self {
        let mut request = Self::new(Method::Patch, url, Some(operations));
        request.content_type = JSON_PATCH;
        request
    }

    /// Expected TSTU cost, booked in the governor's ledger when the response
    /// carries no `X-RateLimit-Cost` (FR-9 budget).
    pub fn with_cost_hint(mut self, tstu: f64) -> Self {
        self.cost_hint = Some(tstu);
        self
    }

    pub fn cost_hint(&self) -> Option<f64> {
        self.cost_hint
    }

    /// Make a GET conditional: the server answers 304 with no body when the
    /// resource still matches.
    pub fn with_validators(mut self, validators: Validators) -> Self {
        debug_assert_eq!(self.method, Method::Get, "only GETs are conditional");
        self.validators = validators;
        self
    }

    pub fn validators(&self) -> &Validators {
        &self.validators
    }

    pub fn method(&self) -> &Method {
        &self.method
    }
    pub fn url(&self) -> &Url {
        &self.url
    }
    pub fn body(&self) -> Option<&serde_json::Value> {
        self.body.as_ref()
    }
    pub fn content_type(&self) -> &'static str {
        self.content_type
    }

    /// Attribute this request explicitly (FR-18) instead of inheriting the
    /// task's [`crate::attribution`] scope.
    pub fn with_origin(mut self, origin: Origin) -> Self {
        self.meta.origin = Some(origin);
        self
    }

    pub fn meta(&self) -> &RequestMeta {
        &self.meta
    }

    pub(crate) fn set_meta(&mut self, meta: RequestMeta) {
        self.meta = meta;
    }
}

/// Rate-limit headers (spec 0002 FR-8).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RateHeaders {
    pub retry_after_ms: Option<i64>,
    pub limit: Option<f64>,
    pub remaining: Option<f64>,
    /// Epoch seconds.
    pub reset_s: Option<i64>,
    pub resource: Option<String>,
    pub delay_ms: Option<i64>,
    pub cost: Option<f64>,
    /// `x-vss-activityid`, Azure DevOps' request correlation id, when sent
    /// (recorded beside the rate headers, spec 0016 FR-17).
    pub activity_id: Option<String>,
}

impl RateHeaders {
    pub fn parse(h: &HeaderMap) -> Self {
        let text = |name: &str| h.get(name).and_then(|v| v.to_str().ok()).map(str::trim);
        let num = |name: &str| text(name).and_then(|v| v.parse::<f64>().ok());
        RateHeaders {
            retry_after_ms: adoken_ado::http::retry_after(h).map(|d| d.as_millis() as i64),
            limit: num("x-ratelimit-limit"),
            remaining: num("x-ratelimit-remaining"),
            reset_s: num("x-ratelimit-reset").map(|v| v as i64),
            resource: text("x-ratelimit-resource").map(str::to_owned),
            delay_ms: num("x-ratelimit-delay").map(|s| (s * 1000.0) as i64),
            cost: num("x-ratelimit-cost"),
            activity_id: text("x-vss-activityid")
                .filter(|v| !v.is_empty())
                .map(str::to_owned),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ApiResponse {
    pub status: u16,
    pub rate: RateHeaders,
    pub body: Vec<u8>,
    /// `Content-Type`, `ETag`, `Last-Modified` (avatar cache, spec 0012).
    pub content_type: Option<String>,
    pub validators: Validators,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("offline: {0}")]
    Offline(String),
    #[error("unauthorized")]
    Unauthorized,
    /// The transport itself saw a `Retry-After` (e.g. the ado HTTP layer).
    #[error("throttled for {retry_after_ms} ms")]
    Throttled { retry_after_ms: i64 },
}

pub trait Transport: Send + Sync + 'static {
    fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>>;
}

fn to_reqwest(req: &ApiRequest) -> reqwest::Request {
    let method = match req.method {
        Method::Get => reqwest::Method::GET,
        Method::PostQuery | Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
    };
    let mut r = reqwest::Request::new(method, req.url.clone());
    if let Some(body) = &req.body {
        r.headers_mut().insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static(req.content_type),
        );
        *r.body_mut() = Some(serde_json::to_vec(body).expect("json").into());
    }
    let conditional = [
        (reqwest::header::IF_NONE_MATCH, &req.validators.etag),
        (
            reqwest::header::IF_MODIFIED_SINCE,
            &req.validators.last_modified,
        ),
    ];
    for (name, value) in conditional {
        if let Some(value) = value
            .as_deref()
            .and_then(|v| reqwest::header::HeaderValue::from_str(v).ok())
        {
            r.headers_mut().insert(name, value);
        }
    }
    r
}

/// Production transport: the shared ado HTTP layer (auth, 401 handling,
/// credential-redacted logs).
pub struct AdoTransport {
    pub http: Http,
    pub auth: Arc<dyn AuthProvider>,
}

impl Transport for AdoTransport {
    fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        Box::pin(async move {
            let response = self
                .http
                .execute(to_reqwest(&req), self.auth.as_ref())
                .await
                .map_err(transport_error)?;
            read_response(response).await
        })
    }
}

/// [`AdoTransport`] for optional endpoints (avatars, spec 0012 FR-9): a 401
/// or sign-in page comes back as a status instead of marking the credential
/// rejected ([`Http::execute_optional`]). Pair it with
/// [`crate::Governor::send_optional`].
pub struct OptionalAdoTransport {
    pub http: Http,
    pub auth: Arc<dyn AuthProvider>,
}

impl Transport for OptionalAdoTransport {
    fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        Box::pin(async move {
            let response = self
                .http
                .execute_optional(to_reqwest(&req), self.auth.as_ref())
                .await
                .map_err(transport_error)?;
            read_response(response).await
        })
    }
}

fn transport_error(e: HttpError) -> TransportError {
    match e {
        HttpError::Throttled { retry_after } => TransportError::Throttled {
            retry_after_ms: retry_after.as_millis() as i64,
        },
        HttpError::Unauthorized { .. } | HttpError::Auth(_) => TransportError::Unauthorized,
        other => TransportError::Offline(other.to_string()),
    }
}

/// Unauthenticated transport for the local mock ADO server in tests and
/// demos. Refuses any host other than loopback.
#[derive(Default)]
pub struct LoopbackTransport {
    client: reqwest::Client,
}

impl Transport for LoopbackTransport {
    fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        Box::pin(async move {
            let host = req.url.host_str().unwrap_or("");
            if !matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
                return Err(TransportError::Offline(format!(
                    "loopback transport refuses host {host}"
                )));
            }
            let response = self
                .client
                .execute(to_reqwest(&req))
                .await
                .map_err(|e| TransportError::Offline(e.to_string()))?;
            read_response(response).await
        })
    }
}

async fn read_response(response: reqwest::Response) -> Result<ApiResponse, TransportError> {
    let status = response.status().as_u16();
    let headers = response.headers();
    let rate = RateHeaders::parse(headers);
    let text = |name: reqwest::header::HeaderName| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let content_type = text(reqwest::header::CONTENT_TYPE);
    let validators = Validators {
        etag: text(reqwest::header::ETAG),
        last_modified: text(reqwest::header::LAST_MODIFIED),
    };
    let body = response
        .bytes()
        .await
        .map_err(|e| TransportError::Offline(e.to_string()))?
        .to_vec();
    Ok(ApiResponse {
        status,
        rate,
        body,
        content_type,
        validators,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "only allowed for WIQL")]
    fn post_is_wiql_only() {
        ApiRequest::wiql(
            Url::parse("http://127.0.0.1/o/p/_apis/git/pullrequests/1/votes").unwrap(),
            "x",
        );
    }

    #[test]
    #[should_panic(expected = "only allowed for read-only queries")]
    fn batch_post_is_workitemsbatch_only() {
        ApiRequest::workitems_batch(
            Url::parse("http://127.0.0.1/o/_apis/wit/workitems/1").unwrap(),
            &[1],
        );
    }

    #[test]
    fn read_only_query_posts_are_not_writes() {
        let batch = ApiRequest::workitems_batch(
            Url::parse("http://127.0.0.1/o/_apis/wit/workitemsbatch").unwrap(),
            &[1, 2],
        );
        assert!(!batch.method().is_write());
        let body = batch.body().unwrap();
        assert_eq!(body["ids"], serde_json::json!([1, 2]));
        assert_eq!(body["errorPolicy"], "omit");
    }

    #[test]
    fn parses_rate_headers() {
        let mut h = HeaderMap::new();
        h.insert("Retry-After", "30".parse().unwrap());
        h.insert("X-RateLimit-Limit", "200".parse().unwrap());
        h.insert("X-RateLimit-Remaining", "40".parse().unwrap());
        h.insert("X-RateLimit-Resource", "Core".parse().unwrap());
        h.insert("X-RateLimit-Cost", "0.5".parse().unwrap());
        h.insert("X-RateLimit-Delay", "1.5".parse().unwrap());
        let r = RateHeaders::parse(&h);
        assert_eq!(r.retry_after_ms, Some(30_000));
        assert_eq!(r.limit, Some(200.0));
        assert_eq!(r.remaining, Some(40.0));
        assert_eq!(r.resource.as_deref(), Some("Core"));
        assert_eq!(r.cost, Some(0.5));
        assert_eq!(r.delay_ms, Some(1500));
    }
}
