//! Azure DevOps REST client, the [`AuthProvider`] abstraction, and the
//! onboarding backend.
//!
//! Contract sources: docs/specs/0001-onboarding.md (organization input, PAT
//! format, keychain-only secrets, FR-1..FR-10) and docs/specs/0002-sync.md.
//!
//! Async code runs on Tokio (reqwest, `tokio::process`), the same stack as
//! zeron; the app bridges it into GPUI with `gpui_tokio`, so nothing here ever
//! runs on the UI thread.
//!
//! Secrets never appear in `Debug` output, logs, or error messages
//! (0001 FR-6/FR-7, 0002 AC-15), and are zeroized on drop.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock};

use base64::Engine as _;
use futures::future::BoxFuture;
use reqwest::header::{AUTHORIZATION, HeaderValue};
use time::OffsetDateTime;
use url::Url;
use zeroize::Zeroizing;

pub mod http;
pub mod onboarding;

pub use onboarding::*;

/// The app data directory from ADR 0001: `~/Library/Application Support/dev.adoken`
/// on macOS, `%APPDATA%\adoken` on Windows, `$XDG_DATA_HOME/adoken` elsewhere.
pub fn data_dir() -> PathBuf {
    let base = if cfg!(windows) {
        dirs::config_dir()
    } else {
        dirs::data_dir()
    };
    let name = if cfg!(target_os = "macos") {
        "dev.adoken"
    } else {
        "adoken"
    };
    base.unwrap_or_else(std::env::temp_dir).join(name)
}

/// The org's synced store (ADR 0001): `<data_dir>/orgs/<org>/adoken.sqlite`.
/// `<org>` is the lowercased organization name (org names are
/// case-insensitive and known before the first request, unlike the org's
/// GUID). Sign-out's "delete local data" removes `<data_dir>/orgs/`.
pub fn org_store_path(data_dir: &Path, org: &Organization) -> PathBuf {
    data_dir
        .join("orgs")
        .join(org.name().to_ascii_lowercase())
        .join("adoken.sqlite")
}

/// One Azure DevOps organization, addressed as `https://dev.azure.com/{name}/`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Organization {
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OrganizationError {
    #[error("enter an organization name or a https://dev.azure.com/<org> URL")]
    Empty,
    #[error("`{0}` is not an Azure DevOps organization URL")]
    UnsupportedUrl(String),
    #[error("organization names use letters, digits and `-`, and cannot start or end with `-`")]
    InvalidName,
}

impl Organization {
    /// Accept what users paste (0001 §3 step 5 / Path B step 1): a bare name,
    /// `https://dev.azure.com/{org}[/...]`, or legacy
    /// `https://{org}.visualstudio.com[/...]`.
    pub fn parse(input: &str) -> Result<Self, OrganizationError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(OrganizationError::Empty);
        }
        let name = if input.contains("://") {
            let url = Url::parse(input)
                .map_err(|_| OrganizationError::UnsupportedUrl(input.to_owned()))?;
            let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
            if host == "dev.azure.com" {
                url.path_segments()
                    .and_then(|mut segments| segments.next())
                    .filter(|segment| !segment.is_empty())
                    .ok_or_else(|| OrganizationError::UnsupportedUrl(input.to_owned()))?
                    .to_owned()
            } else if let Some(name) = host.strip_suffix(".visualstudio.com") {
                name.to_owned()
            } else {
                return Err(OrganizationError::UnsupportedUrl(input.to_owned()));
            }
        } else {
            input.to_owned()
        };
        let valid = !name.starts_with('-')
            && !name.ends_with('-')
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !valid {
            return Err(OrganizationError::InvalidName);
        }
        Ok(Self { name })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// `https://dev.azure.com/{org}/` — base for project-scoped REST calls.
    pub fn base_url(&self) -> Url {
        Url::parse(&format!("https://dev.azure.com/{}/", self.name))
            .expect("validated organization names form valid URLs")
    }

    /// `https://vssps.dev.azure.com/{org}/` — identities and PAT lifecycle.
    pub fn vssps_url(&self) -> Url {
        Url::parse(&format!("https://vssps.dev.azure.com/{}/", self.name))
            .expect("validated organization names form valid URLs")
    }
}

/// An `Authorization` header value. `Debug` never prints it; zeroized on drop.
#[derive(Clone, PartialEq, Eq)]
pub struct Authorization(Zeroizing<String>);

impl Authorization {
    pub(crate) fn bearer(token: &str) -> Self {
        Self(Zeroizing::new(format!("Bearer {token}")))
    }

    pub fn header_value(&self) -> &str {
        &self.0
    }

    /// Marked sensitive so `http`'s own `Debug` prints `Sensitive`.
    fn to_header(&self) -> Result<HeaderValue, AuthError> {
        let mut value = HeaderValue::from_str(&self.0)
            .map_err(|_| AuthError::Unavailable("credential is not a valid header".into()))?;
        value.set_sensitive(true);
        Ok(value)
    }

    /// Set this credential on `request`, replacing any previous one.
    pub fn apply(&self, request: &mut reqwest::Request) -> Result<(), AuthError> {
        request
            .headers_mut()
            .insert(AUTHORIZATION, self.to_header()?);
        Ok(())
    }
}

impl fmt::Debug for Authorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Authorization(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// Azure DevOps rejected this credential (HTTP 401) earlier. The provider
    /// refuses to sign more requests until a replacement token verifies
    /// (0001 FR-15, AC-13): no retry loop against a 401.
    #[error("Azure DevOps rejected adoken's token")]
    Rejected,
    /// The credential has expired; the user must re-auth.
    #[error("credentials expired")]
    Expired,
    /// The provider could not produce a credential (e.g. `az` failed).
    #[error("{0}")]
    Unavailable(String),
}

/// Which provider is active. Only `crates/ado` branches on it (FR-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthKind {
    /// Personal access token (v0.1).
    Pat,
    /// Entra token borrowed from the Azure CLI session. Used during
    /// onboarding for the Profiles/Accounts/PAT lifecycle APIs; the zero-PAT
    /// mode ships in v0.2 (0001 Q-1).
    AzCliToken,
    /// Entra ID OAuth app sign-in (Later).
    EntraOAuth,
}

/// Signs requests (0001 FR-1). PAT today; Entra ID OAuth implements the same
/// trait later, so callers never change.
pub trait AuthProvider: Send + Sync {
    fn kind(&self) -> AuthKind;

    /// Add credentials to `request`. Async because token refresh does IO.
    fn authorize<'a>(
        &'a self,
        request: &'a mut reqwest::Request,
    ) -> BoxFuture<'a, Result<(), AuthError>>;

    /// When the credential stops working, if known.
    fn expires_at(&self) -> Option<OffsetDateTime>;

    /// Called by the HTTP layer when Azure DevOps answers 401.
    fn on_unauthorized(&self);
}

/// A personal access token. Stored only in the OS keychain (AGENTS.md);
/// zeroized on drop, redacted in `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct Pat(Zeroizing<String>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PatError {
    #[error("paste a token: 84 characters (or a legacy 52-character token)")]
    Length,
    #[error("tokens contain only letters and digits")]
    Characters,
}

impl Pat {
    /// Current ADO PATs are 84 characters, legacy ones 52 (0001 Path B step 3).
    pub const LENGTHS: [usize; 2] = [84, 52];

    /// Validate a pasted token; surrounding whitespace is trimmed.
    pub fn parse(input: &str) -> Result<Self, PatError> {
        let token = input.trim();
        if !Self::LENGTHS.contains(&token.len()) {
            return Err(PatError::Length);
        }
        if !token.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(PatError::Characters);
        }
        Ok(Self(Zeroizing::new(token.to_owned())))
    }

    /// A token Azure DevOps just minted or the keychain returned. Not
    /// format-checked: the service owns the format.
    pub(crate) fn from_secret(secret: String) -> Self {
        Self(Zeroizing::new(secret))
    }

    /// The raw secret, for writing to the keychain only.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    /// ADO PAT auth: HTTP Basic with an empty user name.
    pub fn basic_authorization(&self) -> Authorization {
        let raw = Zeroizing::new(format!(":{}", self.expose_secret()));
        let encoded = Zeroizing::new(base64::engine::general_purpose::STANDARD.encode(&*raw));
        Authorization(Zeroizing::new(format!("Basic {}", *encoded)))
    }
}

impl fmt::Debug for Pat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Pat(<redacted>)")
    }
}

/// [`AuthProvider`] for a PAT. After the first 401 it stops signing requests
/// (returns [`AuthError::Rejected`] without touching the network) until
/// [`PatAuth::replace`] hands it a token that verified (0001 FR-15, AC-13).
pub struct PatAuth {
    current: RwLock<(Pat, Option<OffsetDateTime>)>,
    rejected: AtomicBool,
}

impl PatAuth {
    pub fn new(pat: Pat, expires_at: Option<OffsetDateTime>) -> Self {
        Self {
            current: RwLock::new((pat, expires_at)),
            rejected: AtomicBool::new(false),
        }
    }

    /// Whether Azure DevOps answered 401 to this token.
    pub fn is_rejected(&self) -> bool {
        self.rejected.load(Ordering::Acquire)
    }

    /// Sign with a replacement token from now on and clear the rejection
    /// latch (0001 FR-15: the state clears as soon as a replacement
    /// verifies; callers holding this provider need no restart).
    pub fn replace(&self, pat: Pat, expires_at: Option<OffsetDateTime>) {
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = (pat, expires_at);
        self.rejected.store(false, Ordering::Release);
    }

    fn current(&self) -> std::sync::RwLockReadGuard<'_, (Pat, Option<OffsetDateTime>)> {
        self.current.read().unwrap_or_else(PoisonError::into_inner)
    }
}

impl fmt::Debug for PatAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let current = self.current();
        f.debug_struct("PatAuth")
            .field("pat", &current.0)
            .field("expires_at", &current.1)
            .field("rejected", &self.is_rejected())
            .finish()
    }
}

impl AuthProvider for PatAuth {
    fn kind(&self) -> AuthKind {
        AuthKind::Pat
    }

    fn authorize<'a>(
        &'a self,
        request: &'a mut reqwest::Request,
    ) -> BoxFuture<'a, Result<(), AuthError>> {
        let result = if self.is_rejected() {
            Err(AuthError::Rejected)
        } else {
            self.current().0.basic_authorization().apply(request)
        };
        Box::pin(async move { result })
    }

    fn expires_at(&self) -> Option<OffsetDateTime> {
        self.current().1
    }

    fn on_unauthorized(&self) {
        self.rejected.store(true, Ordering::Release);
    }
}

/// Entry point for REST calls against one organization.
#[derive(Clone)]
pub struct AdoClient {
    organization: Organization,
    auth: Arc<dyn AuthProvider>,
}

impl AdoClient {
    pub fn new(organization: Organization, auth: Arc<dyn AuthProvider>) -> Self {
        Self { organization, auth }
    }

    pub fn organization(&self) -> &Organization {
        &self.organization
    }

    pub fn auth(&self) -> &Arc<dyn AuthProvider> {
        &self.auth
    }
}

impl fmt::Debug for AdoClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdoClient")
            .field("organization", &self.organization)
            .field("auth", &self.auth.kind())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organization_accepts_names_and_both_url_forms() {
        for input in [
            "contoso",
            "  contoso \n",
            "https://dev.azure.com/contoso",
            "https://dev.azure.com/contoso/",
            "https://dev.azure.com/contoso/Project/_git/repo",
            "https://contoso.visualstudio.com/",
            "https://CONTOSO.visualstudio.com/DefaultCollection",
        ] {
            let org = Organization::parse(input).unwrap();
            assert_eq!(org.name(), "contoso", "{input}");
            assert_eq!(org.base_url().as_str(), "https://dev.azure.com/contoso/");
            assert_eq!(
                org.vssps_url().as_str(),
                "https://vssps.dev.azure.com/contoso/"
            );
        }
    }

    #[test]
    fn organization_rejects_other_hosts_and_bad_names() {
        assert_eq!(Organization::parse(" "), Err(OrganizationError::Empty));
        assert!(matches!(
            Organization::parse("https://github.com/contoso"),
            Err(OrganizationError::UnsupportedUrl(_))
        ));
        assert!(matches!(
            Organization::parse("https://dev.azure.com/"),
            Err(OrganizationError::UnsupportedUrl(_))
        ));
        for name in ["-contoso", "contoso-", "con toso", "con/toso"] {
            assert_eq!(
                Organization::parse(name),
                Err(OrganizationError::InvalidName),
                "{name}"
            );
        }
    }

    #[test]
    fn pat_validates_format_and_builds_basic_auth() {
        let token = "a".repeat(84);
        let pat = Pat::parse(&format!("  {token}\n")).unwrap();
        assert_eq!(pat.expose_secret(), token);
        let expected = base64::engine::general_purpose::STANDARD.encode(format!(":{token}"));
        assert_eq!(
            pat.basic_authorization().header_value(),
            format!("Basic {expected}")
        );
        assert!(Pat::parse(&"b".repeat(52)).is_ok());
        assert_eq!(Pat::parse("short"), Err(PatError::Length));
        assert_eq!(Pat::parse(&"-".repeat(84)), Err(PatError::Characters));
    }

    #[test]
    fn secrets_never_reach_debug_output() {
        let token = "s".repeat(84);
        let pat = Pat::parse(&token).unwrap();
        let auth = PatAuth::new(pat.clone(), None);
        let client = AdoClient::new(
            Organization::parse("contoso").unwrap(),
            Arc::new(PatAuth::new(pat.clone(), None)),
        );
        let mut request = reqwest::Request::new(
            reqwest::Method::GET,
            Url::parse("https://dev.azure.com/contoso/").unwrap(),
        );
        futures::executor::block_on(auth.authorize(&mut request)).unwrap();
        let rendered = format!(
            "{pat:?} {:?} {client:?} {auth:?} {request:?}",
            pat.basic_authorization()
        );
        let basic = pat.basic_authorization();
        let encoded = basic.header_value().trim_start_matches("Basic ");
        assert!(!rendered.contains(&token), "{rendered}");
        assert!(!rendered.contains(encoded), "{rendered}");
    }

    #[test]
    fn pat_auth_signs_until_rejected_then_refuses() {
        let pat = Pat::parse(&"c".repeat(84)).unwrap();
        let provider: Arc<dyn AuthProvider> = Arc::new(PatAuth::new(pat.clone(), None));
        assert_eq!(provider.kind(), AuthKind::Pat);
        let mut request = reqwest::Request::new(
            reqwest::Method::GET,
            Url::parse("https://dev.azure.com/contoso/").unwrap(),
        );
        futures::executor::block_on(provider.authorize(&mut request)).unwrap();
        assert_eq!(
            request.headers()[AUTHORIZATION].to_str().unwrap(),
            pat.basic_authorization().header_value()
        );
        provider.on_unauthorized();
        assert_eq!(
            futures::executor::block_on(provider.authorize(&mut request)),
            Err(AuthError::Rejected)
        );
    }
}
