//! The one HTTP layer every Azure DevOps call goes through.
//!
//! - Signs requests through an [`AuthProvider`]; a 401 is reported to the
//!   provider (which then stops signing) and never retried.
//! - Honors `Retry-After` on 429/503: the host is marked throttled until the
//!   deadline and requests to it fail fast with [`HttpError::Throttled`]
//!   without touching the network ("never auto-hammer", 0001 §3 States).
//! - Debug logs carry method, URL and status only; headers are rendered
//!   through [`redacted_headers`], which never prints `Authorization` (FR-7).
//! - Error bodies are reduced to Azure DevOps' `message` field; response
//!   bodies are never logged (the PAT create response carries the secret).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, RETRY_AFTER};
use reqwest::{Method, Request, Response, StatusCode};
use serde::de::DeserializeOwned;
use url::Url;

use crate::{AuthError, AuthProvider, Organization};

/// Base URLs of the Azure DevOps hosts adoken talks to. Tests point all of
/// them at one mock server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    /// `https://app.vssps.visualstudio.com/` — Profiles and Accounts.
    pub app_vssps: Url,
    /// `https://dev.azure.com/` — `{org}/…` REST.
    pub dev: Url,
    /// `https://vssps.dev.azure.com/` — `{org}/…` identities and PAT lifecycle.
    pub vssps: Url,
}

impl Endpoints {
    pub fn production() -> Self {
        Self {
            app_vssps: Url::parse("https://app.vssps.visualstudio.com/").expect("static URL"),
            dev: Url::parse("https://dev.azure.com/").expect("static URL"),
            vssps: Url::parse("https://vssps.dev.azure.com/").expect("static URL"),
        }
    }

    /// Every host under one base: `{base}app/`, `{base}dev/`, `{base}vssps/`.
    pub fn under(base: &Url) -> Self {
        let mut base = base.clone();
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        let join = |path: &str| base.join(path).expect("relative path joins");
        Self {
            app_vssps: join("app/"),
            dev: join("dev/"),
            vssps: join("vssps/"),
        }
    }

    /// `{dev}{org}/`
    pub fn org(&self, org: &Organization) -> Url {
        self.dev
            .join(&format!("{}/", org.name()))
            .expect("validated organization names join")
    }

    /// `{vssps}{org}/`
    pub fn org_vssps(&self, org: &Organization) -> Url {
        self.vssps
            .join(&format!("{}/", org.name()))
            .expect("validated organization names join")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HttpError {
    /// The host asked us to wait (`Retry-After`), or is still inside that window.
    #[error("throttled; retry in {}s", retry_after.as_secs())]
    Throttled { retry_after: Duration },
    /// 401, or a redirect/HTML page that is Azure DevOps' sign-in screen.
    /// `message` is Azure DevOps' / Entra's explanation when there is one.
    #[error("unauthorized")]
    Unauthorized { message: String },
    #[error("forbidden: {message}")]
    Forbidden { message: String },
    #[error("not found: {message}")]
    NotFound { message: String },
    #[error("HTTP {status}: {message}")]
    Status { status: u16, message: String },
    /// DNS, connect, TLS, or timeout: nothing reached the service.
    #[error("offline: {0}")]
    Offline(String),
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("unexpected response: {0}")]
    Decode(String),
}

impl HttpError {
    /// The Azure DevOps `message` of an error response, if any.
    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Forbidden { message }
            | Self::NotFound { message }
            | Self::Status { message, .. }
            | Self::Unauthorized { message } => Some(message),
            _ => None,
        }
    }
}

/// Shared HTTP client with the per-host `Retry-After` governor. Cheap to clone.
///
/// The reqwest client is built on first send, not in `new`: loading the macOS
/// native root certificates takes ~100 ms, and `Http` is constructed on the UI
/// thread before the first frame. Sends always run on the Tokio runtime.
#[derive(Clone)]
pub struct Http {
    client: Arc<OnceLock<reqwest::Client>>,
    throttled_until: Arc<Mutex<HashMap<String, Instant>>>,
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Http {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Http").finish_non_exhaustive()
    }
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_MESSAGE_CHARS: usize = 300;

impl Http {
    pub fn new() -> Self {
        Self {
            client: Arc::default(),
            throttled_until: Arc::default(),
        }
    }

    fn client(&self) -> &reqwest::Client {
        self.client.get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(concat!("adoken/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(CONNECT_TIMEOUT)
                .timeout(REQUEST_TIMEOUT)
                // A redirect on an API call is the sign-in page: treat as 401,
                // and never forward credentials to another host.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("reqwest client builds with static config")
        })
    }

    pub fn get(&self, url: Url) -> Request {
        Request::new(Method::GET, url)
    }

    pub fn delete(&self, url: Url) -> Request {
        Request::new(Method::DELETE, url)
    }

    pub fn post_json(&self, url: Url, body: &serde_json::Value) -> Request {
        let mut request = Request::new(Method::POST, url);
        request.headers_mut().insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        *request.body_mut() = Some(
            serde_json::to_vec(body)
                .expect("JSON values serialize")
                .into(),
        );
        request
    }

    /// Sign and send `request`. Non-2xx statuses become [`HttpError`]s.
    pub async fn send(
        &self,
        request: Request,
        auth: &dyn AuthProvider,
    ) -> Result<Response, HttpError> {
        let response = self.execute(request, auth).await?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let message = error_message(response).await;
        Err(match status {
            StatusCode::FORBIDDEN => HttpError::Forbidden { message },
            StatusCode::NOT_FOUND => HttpError::NotFound { message },
            _ => HttpError::Status {
                status: status.as_u16(),
                message,
            },
        })
    }

    /// Sign and send `request`, handling throttling and 401 (reported to
    /// `auth`, never retried). Other statuses are returned as-is, for APIs
    /// whose error bodies carry structured codes (PAT lifecycle).
    pub async fn execute(
        &self,
        request: Request,
        auth: &dyn AuthProvider,
    ) -> Result<Response, HttpError> {
        self.execute_inner(request, auth, true).await
    }

    /// [`Self::execute`] for optional endpoints (avatars, spec 0012 FR-9): a
    /// 401 or sign-in page is returned like any other status and never
    /// reported to `auth`, so one refusing endpoint can't mark the
    /// credential rejected for everything else.
    pub async fn execute_optional(
        &self,
        request: Request,
        auth: &dyn AuthProvider,
    ) -> Result<Response, HttpError> {
        self.execute_inner(request, auth, false).await
    }

    async fn execute_inner(
        &self,
        mut request: Request,
        auth: &dyn AuthProvider,
        report_unauthorized: bool,
    ) -> Result<Response, HttpError> {
        let host = host_key(request.url());
        if let Some(retry_after) = self.throttle_remaining(&host) {
            return Err(HttpError::Throttled { retry_after });
        }
        auth.authorize(&mut request).await?;
        let method = request.method().clone();
        let url = request.url().clone();
        log::debug!(target: "adoken_ado::http", "{method} {url}");
        log::trace!(target: "adoken_ado::http", "headers: {}", redacted_headers(request.headers()));

        let response = self.client().execute(request).await.map_err(|err| {
            log::debug!(target: "adoken_ado::http", "{method} {url} failed: {}", transport_reason(&err));
            HttpError::Offline(transport_reason(&err))
        })?;
        let status = response.status();
        log::debug!(target: "adoken_ado::http", "{method} {url} -> {}", status.as_u16());

        if (status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE)
            && let Some(retry_after) = retry_after(response.headers())
        {
            self.throttle(host, retry_after);
            return Err(HttpError::Throttled { retry_after });
        }
        // 203 / redirects / HTML are Azure DevOps' sign-in page.
        let sign_in_page = status == StatusCode::NON_AUTHORITATIVE_INFORMATION
            // 304 answers a conditional GET (avatar revalidation), not a login.
            || (status.is_redirection() && status != StatusCode::NOT_MODIFIED)
            || (status.is_success() && is_html(response.headers()));
        if report_unauthorized && (status == StatusCode::UNAUTHORIZED || sign_in_page) {
            auth.on_unauthorized();
            let message = if sign_in_page {
                String::new()
            } else {
                error_message(response).await
            };
            return Err(HttpError::Unauthorized { message });
        }
        Ok(response)
    }

    /// [`Self::send`] and decode a JSON body.
    pub async fn json<T: DeserializeOwned>(
        &self,
        request: Request,
        auth: &dyn AuthProvider,
    ) -> Result<T, HttpError> {
        let response = self.send(request, auth).await?;
        decode(response).await
    }

    fn throttle_remaining(&self, host: &str) -> Option<Duration> {
        let mut map = self
            .throttled_until
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let until = *map.get(host)?;
        let now = Instant::now();
        if until > now {
            Some(until - now)
        } else {
            map.remove(host);
            None
        }
    }

    fn throttle(&self, host: String, retry_after: Duration) {
        self.throttled_until
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(host, Instant::now() + retry_after);
    }
}

pub(crate) async fn decode<T: DeserializeOwned>(response: Response) -> Result<T, HttpError> {
    let bytes = response
        .bytes()
        .await
        .map_err(|err| HttpError::Offline(transport_reason(&err)))?;
    // Never echo the body: it may carry a freshly minted secret.
    serde_json::from_slice(&bytes).map_err(|err| HttpError::Decode(err.to_string()))
}

fn host_key(url: &Url) -> String {
    format!(
        "{}:{}",
        url.host_str().unwrap_or_default(),
        url.port_or_known_default().unwrap_or_default()
    )
}

fn is_html(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"))
}

fn transport_reason(err: &reqwest::Error) -> String {
    let kind = if err.is_timeout() {
        "timed out"
    } else if err.is_connect() {
        "could not connect"
    } else {
        "request failed"
    };
    match err.url() {
        Some(url) => format!("{kind} ({})", url.host_str().unwrap_or_default()),
        None => kind.to_owned(),
    }
}

/// `Retry-After` as delta-seconds or an HTTP date.
pub fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let at = httpdate::parse_http_date(value).ok()?;
    Some(at.duration_since(SystemTime::now()).unwrap_or_default())
}

/// Header names and values for debug logs, with credentials redacted.
pub fn redacted_headers(headers: &HeaderMap) -> String {
    let mut out = String::new();
    for (name, value) in headers {
        if !out.is_empty() {
            out.push_str(", ");
        }
        let shown = if name == AUTHORIZATION || value.is_sensitive() {
            "<redacted>"
        } else {
            value.to_str().unwrap_or("<binary>")
        };
        let _ = write!(out, "{name}: {shown}");
    }
    out
}

async fn error_message(response: Response) -> String {
    #[derive(serde::Deserialize)]
    struct AdoError {
        message: Option<String>,
    }
    let status = response.status();
    let bytes = response.bytes().await.unwrap_or_default();
    let message = serde_json::from_slice::<AdoError>(&bytes)
        .ok()
        .and_then(|error| error.message)
        .unwrap_or_else(|| {
            status
                .canonical_reason()
                .unwrap_or("request failed")
                .to_owned()
        });
    message.chars().take(MAX_MESSAGE_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    #[test]
    fn redacted_headers_hide_credentials() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Basic c2VjcmV0"));
        headers.insert("x-custom", HeaderValue::from_static("visible"));
        let rendered = redacted_headers(&headers);
        assert!(!rendered.contains("c2VjcmV0"), "{rendered}");
        assert!(rendered.contains("authorization: <redacted>"), "{rendered}");
        assert!(rendered.contains("x-custom: visible"), "{rendered}");
    }

    #[test]
    fn retry_after_accepts_seconds_and_dates() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("20"));
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(20)));
        let later = SystemTime::now() + Duration::from_secs(120);
        headers.insert(
            RETRY_AFTER,
            HeaderValue::from_str(&httpdate::fmt_http_date(later)).unwrap(),
        );
        let parsed = retry_after(&headers).unwrap();
        assert!(parsed > Duration::from_secs(100) && parsed <= Duration::from_secs(120));
        headers.insert(RETRY_AFTER, HeaderValue::from_static("soon"));
        assert_eq!(retry_after(&headers), None);
    }

    #[test]
    fn endpoints_under_one_base_keep_org_paths() {
        let endpoints = Endpoints::under(&Url::parse("http://127.0.0.1:9000").unwrap());
        let org = Organization::parse("fabrikam").unwrap();
        assert_eq!(
            endpoints.org(&org).as_str(),
            "http://127.0.0.1:9000/dev/fabrikam/"
        );
        assert_eq!(
            endpoints.org_vssps(&org).as_str(),
            "http://127.0.0.1:9000/vssps/fabrikam/"
        );
        assert_eq!(endpoints.app_vssps.as_str(), "http://127.0.0.1:9000/app/");
    }
}
