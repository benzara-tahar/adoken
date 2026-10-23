//! Onboarding backend (spec 0001): Azure CLI detection and sign-in, org and
//! project listing, least-privilege PAT minting via the PAT Lifecycle API,
//! pasted-token verification with scope probes, Keychain storage, resumable
//! metadata, Replace token (by hand and in the background), and sign-out.
//!
//! [`Connector`] is the one entry point. Every method is async and must run
//! on the Tokio runtime (the app spawns them via `gpui_tokio`); Keychain calls
//! go to Tokio's blocking pool. Secrets (PAT, Entra tokens) never leave this
//! module except into the Keychain.

mod api;
mod az;
mod error;
pub mod keychain;
mod scopes;
mod state;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use futures::future::BoxFuture;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use url::Url;

use crate::http::{Endpoints, Http, HttpError};
use crate::{
    AdoClient, AuthError, AuthKind, AuthProvider, Authorization, Organization, Pat, PatAuth,
};

pub use api::{OrgRef, Profile};
pub use az::{
    ADO_RESOURCE, AzCli, AzLocator, AzStatus, AzTimeouts, AzVersion, CancelToken, DeviceCodePrompt,
    EntraToken, MANAGEMENT_RESOURCE, MIN_AZ_VERSION, TokenSource, known_az_locations,
};
pub use error::OnboardError;
pub use keychain::{Keychain, KeychainError, MemorySecretStore, PAT_SERVICE, SecretStore};
pub use scopes::{AccessLevel, Scope, ScopeStatus, covers, scope_string, scopes};
pub use state::{
    AUTO_REPLACE_EVERY, AUTO_REPLACE_FROM, AUTO_REPLACE_WARN, AutoReplaceLog, ConnectPath,
    Identity, OnboardingState, PatMeta, PatSource, ProjectRef, Step,
};

use api::{Api, CreatedPat, PatApiError};
use az::ResourceAuth;

/// Default PAT lifetime (0001 Q-2), and the fallback when a stored token
/// doesn't record its own.
pub const DEFAULT_LIFETIME_DAYS: u32 = 90;

/// The new token's scopes as Azure DevOps returned them (`patToken.scope`),
/// else the requested set (0001 FR-8).
fn minted_scopes(created: &CreatedPat, level: AccessLevel) -> Vec<String> {
    created
        .scope
        .as_deref()
        .map(|scope| scope.split_whitespace().map(str::to_owned).collect())
        .filter(|scopes: &Vec<String>| !scopes.is_empty())
        .unwrap_or_else(|| scopes(level).iter().map(|s| s.id.to_owned()).collect())
}

/// Lifetime the PAT is retried with when the org caps it (0001 AC-7).
pub const CAPPED_LIFETIME_DAYS: u32 = 30;
const LIFESPAN_POLICY: &str = "patLifespanPolicyViolation";

/// `https://dev.azure.com/{org}/_usersSettings/tokens` (Path B step 2).
pub fn token_page_url(org: &Organization) -> Url {
    org.base_url()
        .join("_usersSettings/tokens")
        .expect("static relative path joins")
}

/// `adoken · {host-label}` for this Mac (0001 §3 step 8, Q-4).
pub fn pat_display_name() -> String {
    display_name_for_host(&gethostname::gethostname().to_string_lossy())
}

/// `adoken · {hostname}`, or `adoken · mac-{4 hex of SHA-256(hostname)}` when
/// the hostname looks like an email address (0001 Q-4).
pub fn display_name_for_host(hostname: &str) -> String {
    let hostname = hostname.trim();
    if hostname.contains('@') {
        let digest = Sha256::digest(hostname.as_bytes());
        format!("adoken · mac-{:02x}{:02x}", digest[0], digest[1])
    } else {
        format!("adoken · {hostname}")
    }
}

/// Orgs the `az` identity belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgList {
    pub me: Profile,
    /// Sorted by name, case-insensitively.
    pub orgs: Vec<OrgRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    Granted,
    /// Read works; `vso.code_write` can't be probed without side effects.
    GrantedWriteUntested,
    /// 401/403: the scope isn't on the token.
    Missing,
    /// Nothing to probe against (no project visible).
    Untested,
    /// The probe failed for another reason (shown as-is).
    Failed(String),
}

impl ProbeOutcome {
    pub fn is_granted(&self) -> bool {
        matches!(self, Self::Granted | Self::GrantedWriteUntested)
    }
}

/// One scope row's probe result (0001 §5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeProbe {
    pub scope: &'static Scope,
    pub outcome: ProbeOutcome,
}

/// Result of verifying a pasted token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteVerification {
    pub me: Identity,
    /// In [`scopes`] order for the chosen level.
    pub probes: Vec<ScopeProbe>,
    /// Saved to the Keychain and `account.json` iff every required scope was
    /// granted; `Some` means the user can continue.
    pub saved: Option<PatMeta>,
}

impl PasteVerification {
    pub fn can_continue(&self) -> bool {
        self.saved.is_some()
    }

    /// Scopes probed as missing (required and optional).
    pub fn missing(&self) -> impl Iterator<Item = &'static Scope> + '_ {
        self.probes
            .iter()
            .filter(|probe| probe.outcome == ProbeOutcome::Missing)
            .map(|probe| probe.scope)
    }
}

/// Sign-out choices (0001 §3 "Sign out").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignOut {
    /// Also revoke the PAT in Azure DevOps (az-created tokens only).
    pub revoke: bool,
    /// Also delete synced data (`<data_dir>/orgs/`, ADR 0001).
    pub delete_local_data: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignOutOutcome {
    /// Local sign-out always completes; a failed revoke is reported here.
    pub revoke_error: Option<OnboardError>,
}

/// A successful Replace token (0001 FR-16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replaced {
    pub meta: PatMeta,
    /// The new token works; revoking the old one failed (it will expire).
    pub old_revoke_error: Option<OnboardError>,
    /// The replaced token's expiry, for "couldn't revoke the old one; it
    /// expires on {date}".
    pub old_valid_to: Option<OffsetDateTime>,
}

/// What one background replacement check did (owner decision 2026-10-01).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoReplace {
    /// Not an `az` token, not within 7 days of expiry, or tried today.
    NotDue,
    /// Tell the user once, quietly.
    Replaced(Box<Replaced>),
    /// Nothing visible, unless `warn`: still failing 2 days before expiry,
    /// first time for this token (post the one quiet item).
    Failed {
        error: OnboardError,
        warn: bool,
        valid_to: Option<OffsetDateTime>,
    },
}

/// Everything a [`Connector`] talks to. Tests swap in a mock server, a fake
/// `az`, and [`MemorySecretStore`].
pub struct ConnectorParts {
    pub data_dir: PathBuf,
    pub endpoints: Endpoints,
    pub locator: AzLocator,
    pub secrets: Arc<dyn SecretStore>,
}

/// The onboarding backend. Cheap to clone.
#[derive(Clone)]
pub struct Connector {
    inner: Arc<Inner>,
}

struct Inner {
    api: Api,
    locator: AzLocator,
    secrets: Arc<dyn SecretStore>,
    data_dir: PathBuf,
    /// Serializes token-changing operations (double clicks never mint twice)
    /// and every `account.json` write.
    ops: tokio::sync::Mutex<()>,
    /// The provider [`Connector::client`] handed out (sync signs with it): a
    /// replacement that verifies is swapped into it, so sync resumes without
    /// a restart (0001 FR-15, FR-16).
    live: Mutex<Weak<PatAuth>>,
}

impl std::fmt::Debug for Connector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connector")
            .field("data_dir", &self.inner.data_dir)
            .finish_non_exhaustive()
    }
}

/// Signs probes with the pasted PAT but never trips a breaker: a 401 on a
/// probe means "scope missing", not "token dead".
struct ProbeAuth(Authorization);

impl AuthProvider for ProbeAuth {
    fn kind(&self) -> AuthKind {
        AuthKind::Pat
    }

    fn authorize<'a>(
        &'a self,
        request: &'a mut reqwest::Request,
    ) -> BoxFuture<'a, Result<(), AuthError>> {
        let result = self.0.apply(request);
        Box::pin(async move { result })
    }

    fn expires_at(&self) -> Option<OffsetDateTime> {
        None
    }

    fn on_unauthorized(&self) {}
}

struct Minted {
    created: CreatedPat,
    lifetime_days: u32,
    capped: bool,
    requested_valid_to: OffsetDateTime,
}

fn is_consent_or_policy(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "consent",
        "aadsts65001",
        "aadsts90094",
        "aadsts53003",
        "admin approval",
        "conditional access",
        "policy",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

/// `patTokenError` → user-facing error (0001 §3 step 8).
fn classify_pat_code(code: String) -> OnboardError {
    let lower = code.to_ascii_lowercase();
    if lower == "accessdenied"
        || ["denied", "restrict", "disabled", "blocked"]
            .iter()
            .any(|needle| lower.contains(needle))
    {
        OnboardError::PolicyBlocked { reason: code }
    } else {
        OnboardError::PatRejected { code }
    }
}

fn probe_outcome(result: Result<(), HttpError>) -> Result<ProbeOutcome, OnboardError> {
    match result {
        Ok(()) => Ok(ProbeOutcome::Granted),
        Err(HttpError::Unauthorized { .. } | HttpError::Forbidden { .. }) => {
            Ok(ProbeOutcome::Missing)
        }
        Err(HttpError::Offline(_)) => Err(OnboardError::Offline),
        Err(HttpError::Throttled { retry_after }) => Err(OnboardError::Throttled { retry_after }),
        Err(other) => Ok(ProbeOutcome::Failed(
            other
                .message()
                .map_or_else(|| other.to_string(), str::to_owned),
        )),
    }
}

fn storage(err: std::io::Error) -> OnboardError {
    OnboardError::Storage {
        detail: err.to_string(),
    }
}

fn keychain_error(err: KeychainError) -> OnboardError {
    OnboardError::Keychain { detail: err.0 }
}

impl Connector {
    /// Production: real Azure DevOps hosts, `az` from this environment, and
    /// the Keychain service [`PAT_SERVICE`].
    pub fn new(data_dir: PathBuf) -> Self {
        Self::with_parts(ConnectorParts {
            data_dir,
            endpoints: Endpoints::production(),
            locator: AzLocator::from_env(),
            secrets: Arc::new(Keychain::new()),
        })
    }

    pub fn with_parts(parts: ConnectorParts) -> Self {
        Self {
            inner: Arc::new(Inner {
                api: Api {
                    http: Http::new(),
                    endpoints: parts.endpoints,
                },
                locator: parts.locator,
                secrets: parts.secrets,
                data_dir: parts.data_dir,
                ops: tokio::sync::Mutex::new(()),
                live: Mutex::new(Weak::new()),
            }),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.inner.data_dir
    }

    // ── Azure CLI ──────────────────────────────────────────────────────────

    /// Locate `az`, read its version and account (FR-2, FR-3).
    pub async fn detect_az(&self) -> AzStatus {
        self.inner.locator.detect().await
    }

    /// `az login` in the browser, then re-detect. `cancel` kills it.
    pub async fn az_login(
        &self,
        az: &AzCli,
        cancel: CancelToken,
    ) -> Result<AzStatus, OnboardError> {
        az.login(cancel).await?;
        Ok(az.status().await)
    }

    /// `az login --use-device-code`; `on_prompt` gets the code to display.
    pub async fn az_login_device_code(
        &self,
        az: &AzCli,
        cancel: CancelToken,
        on_prompt: impl FnMut(DeviceCodePrompt) + Send,
    ) -> Result<AzStatus, OnboardError> {
        az.login_device_code(cancel, on_prompt).await?;
        Ok(az.status().await)
    }

    /// `az devops configure` default org, best effort.
    pub async fn default_org(&self, az: &AzCli) -> Option<Organization> {
        az.default_org().await
    }

    /// An Entra token source for a signed-in `az` (fetches the ADO token once
    /// up front so failures show here, not mid-flow). Memory only (FR-6).
    pub async fn token_source(&self, az: &AzCli) -> Result<TokenSource, OnboardError> {
        let source = TokenSource::new(az.clone());
        source.token(ADO_RESOURCE).await?;
        Ok(source)
    }

    // ── Orgs and projects ──────────────────────────────────────────────────

    /// Profiles `me`, then Accounts `memberId={me}` (0001 §3 step 5).
    pub async fn list_orgs(&self, source: &TokenSource) -> Result<OrgList, OnboardError> {
        let auth = ResourceAuth {
            source,
            resource: ADO_RESOURCE,
        };
        let api = &self.inner.api;
        let me = api
            .profile_me(&auth)
            .await
            .map_err(OnboardError::from_entra_http)?;
        let orgs = api
            .accounts(&me.id, &auth)
            .await
            .map_err(OnboardError::from_entra_http)?;
        Ok(OrgList { me, orgs })
    }

    /// All projects in `org`, with the Entra token when given, else with the
    /// saved PAT.
    pub async fn list_projects(
        &self,
        org: &Organization,
        entra: Option<&TokenSource>,
    ) -> Result<Vec<ProjectRef>, OnboardError> {
        let api = &self.inner.api;
        let not_found = |err: HttpError, map: fn(HttpError) -> OnboardError| match err {
            HttpError::NotFound { .. } => OnboardError::OrgNotFound {
                org: org.name().to_owned(),
            },
            other => map(other),
        };
        match entra {
            Some(source) => {
                let auth = ResourceAuth {
                    source,
                    resource: ADO_RESOURCE,
                };
                api.projects(org, &auth, None)
                    .await
                    .map_err(|err| not_found(err, OnboardError::from_entra_http))
            }
            None => {
                let pat = self
                    .secret_load(org)
                    .await?
                    .ok_or(OnboardError::InvalidToken)?;
                let auth = PatAuth::new(pat, None);
                api.projects(org, &auth, None)
                    .await
                    .map_err(|err| not_found(err, OnboardError::from_pat_http))
            }
        }
    }

    // ── Token state ────────────────────────────────────────────────────────

    /// Saved PAT metadata (`account.json`), if any.
    pub fn account(&self) -> Option<PatMeta> {
        state::read_account(&self.inner.data_dir).ok().flatten()
    }

    /// A client for the verified account (sync hand-off, 0001 step 11).
    /// Callers only see the [`AuthProvider`] seam (FR-1). Its provider takes
    /// every later replacement in place (FR-16): no restart, no new client.
    pub async fn client(&self) -> Result<Option<AdoClient>, OnboardError> {
        let Some(meta) = self.account().filter(|meta| meta.verified) else {
            return Ok(None);
        };
        let Some(org) = meta.organization() else {
            return Ok(None);
        };
        let Some(pat) = self.secret_load(&org).await? else {
            return Ok(None);
        };
        let auth = Arc::new(PatAuth::new(pat, meta.valid_to));
        *self.live() = Arc::downgrade(&auth);
        Ok(Some(AdoClient::new(org, auth)))
    }

    fn live(&self) -> std::sync::MutexGuard<'_, Weak<PatAuth>> {
        self.inner
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// A replacement verified: sign every live client's requests with it and
    /// clear the rejection (FR-15).
    fn swap_live(&self, pat: &Pat, valid_to: Option<OffsetDateTime>) {
        if let Some(auth) = self.live().upgrade() {
            auth.replace(pat.clone(), valid_to);
        }
    }

    // ── Path A: mint ───────────────────────────────────────────────────────

    /// Mint an org-scoped, least-privilege PAT with the Entra token, store it
    /// in the Keychain, persist metadata, and verify it via `connectionData`
    /// (0001 §3 steps 8-9). A PAT minted earlier but not yet verified is
    /// verified instead of minting another (FR-10).
    pub async fn create_pat(
        &self,
        source: &TokenSource,
        org: &Organization,
        level: AccessLevel,
        lifetime_days: u32,
    ) -> Result<PatMeta, OnboardError> {
        let _op = self.inner.ops.lock().await;
        if let Some(pending) = self.pending_for(org)
            && self.secret_load(org).await?.is_some()
        {
            return self.verify_stored(pending).await;
        }
        let minted = self.mint(source, org, level, lifetime_days).await?;
        let meta = self.store_minted(source, org, level, minted).await?;
        self.verify_stored(meta).await
    }

    /// FR-10 on relaunch: verify a minted-but-unverified PAT.
    pub async fn resume_pending(&self) -> Result<Option<PatMeta>, OnboardError> {
        let _op = self.inner.ops.lock().await;
        match self.account().filter(|meta| !meta.verified) {
            Some(meta) => self.verify_stored(meta).await.map(Some),
            None => Ok(None),
        }
    }

    fn pending_for(&self, org: &Organization) -> Option<PatMeta> {
        self.account()
            .filter(|meta| !meta.verified && meta.source == PatSource::Az && meta.org == org.name())
    }

    async fn mint(
        &self,
        source: &TokenSource,
        org: &Organization,
        level: AccessLevel,
        lifetime_days: u32,
    ) -> Result<Minted, OnboardError> {
        let scope = scope_string(level);
        let display_name = pat_display_name();
        let mut days = lifetime_days;
        let mut capped = false;
        let mut resource = ADO_RESOURCE;
        loop {
            let valid_to = OffsetDateTime::now_utc() + time::Duration::days(i64::from(days));
            let auth = ResourceAuth { source, resource };
            let result = self
                .inner
                .api
                .create_pat(org, &auth, &display_name, &scope, valid_to)
                .await;
            match result {
                Ok(created) => {
                    return Ok(Minted {
                        created,
                        lifetime_days: days,
                        capped,
                        requested_valid_to: valid_to,
                    });
                }
                Err(PatApiError::Code(code))
                    if code == LIFESPAN_POLICY && !capped && days > CAPPED_LIFETIME_DAYS =>
                {
                    log::info!(target: "adoken_ado::onboarding", "org caps PAT lifetime; retrying with {CAPPED_LIFETIME_DAYS} days");
                    days = CAPPED_LIFETIME_DAYS;
                    capped = true;
                }
                Err(PatApiError::Code(code)) => return Err(classify_pat_code(code)),
                Err(PatApiError::Http(HttpError::Unauthorized { message })) => {
                    if is_consent_or_policy(&message) {
                        return Err(OnboardError::PolicyBlocked { reason: message });
                    }
                    if resource == ADO_RESOURCE {
                        // 0001 §5.1: the management resource is the fallback.
                        resource = MANAGEMENT_RESOURCE;
                        continue;
                    }
                    return Err(OnboardError::NotInTenant);
                }
                Err(PatApiError::Http(HttpError::Forbidden { message })) => {
                    return Err(OnboardError::PolicyBlocked { reason: message });
                }
                Err(PatApiError::Http(err)) => {
                    if err.message().is_some_and(is_consent_or_policy) {
                        return Err(OnboardError::PolicyBlocked {
                            reason: err.message().unwrap_or_default().to_owned(),
                        });
                    }
                    return Err(OnboardError::from_entra_http(err));
                }
            }
        }
    }

    /// Keychain first, then metadata marked unverified, so a quit before
    /// verification resumes instead of minting again.
    async fn store_minted(
        &self,
        source: &TokenSource,
        org: &Organization,
        level: AccessLevel,
        minted: Minted,
    ) -> Result<PatMeta, OnboardError> {
        let Minted {
            created,
            lifetime_days,
            capped,
            requested_valid_to,
        } = minted;
        if let Err(err) = self.secret_save(org, &created.pat).await {
            if let Some(id) = &created.authorization_id {
                // Unstorable token: don't leave it alive in Azure DevOps.
                let _ = self.revoke(source, org, id).await;
            }
            return Err(err);
        }
        let meta = PatMeta {
            org: org.name().to_owned(),
            level,
            scopes: minted_scopes(&created, level),
            untested: Vec::new(),
            authorization_id: created.authorization_id,
            valid_to: Some(created.valid_to.unwrap_or(requested_valid_to)),
            created_at: OffsetDateTime::now_utc(),
            source: PatSource::Az,
            lifetime_days: Some(lifetime_days),
            lifetime_capped: capped,
            verified: false,
            me: None,
            auto_replaced: false,
            auto_replace: AutoReplaceLog::default(),
        };
        state::write_account(&self.inner.data_dir, &meta).map_err(storage)?;
        Ok(meta)
    }

    async fn verify_stored(&self, mut meta: PatMeta) -> Result<PatMeta, OnboardError> {
        let org = meta
            .organization()
            .ok_or_else(|| OnboardError::Other(format!("`{}` is not an organization", meta.org)))?;
        let pat = self
            .secret_load(&org)
            .await?
            .ok_or_else(|| OnboardError::Keychain {
                detail: "the token is missing from the Keychain".into(),
            })?;
        let auth = PatAuth::new(pat, meta.valid_to);
        let me = self.connection_data(&org, &auth).await?;
        meta.me = Some(me);
        meta.verified = true;
        state::write_account(&self.inner.data_dir, &meta).map_err(storage)?;
        Ok(meta)
    }

    async fn connection_data(
        &self,
        org: &Organization,
        auth: &dyn AuthProvider,
    ) -> Result<Identity, OnboardError> {
        self.inner
            .api
            .connection_data(org, auth)
            .await
            .map_err(|err| match err {
                HttpError::NotFound { .. } => OnboardError::OrgNotFound {
                    org: org.name().to_owned(),
                },
                other => OnboardError::from_pat_http(other),
            })
    }

    async fn revoke(
        &self,
        source: &TokenSource,
        org: &Organization,
        authorization_id: &str,
    ) -> Result<(), OnboardError> {
        let auth = ResourceAuth {
            source,
            resource: ADO_RESOURCE,
        };
        self.inner
            .api
            .revoke_pat(org, &auth, authorization_id)
            .await
            .map_err(OnboardError::from_entra_http)
    }

    // ── Path B: paste ──────────────────────────────────────────────────────

    /// Verify a pasted PAT: `connectionData`, then one read-only probe per
    /// scope (0001 §5.3; at most 3 in flight). Saves it when every required
    /// scope is granted, replacing any stored token (FR-16: a live client
    /// signs with it from now on). `expires_on` is the optional
    /// user-entered expiry.
    pub async fn verify_pasted(
        &self,
        org: &Organization,
        pat: Pat,
        level: AccessLevel,
        expires_on: Option<time::Date>,
    ) -> Result<PasteVerification, OnboardError> {
        let _op = self.inner.ops.lock().await;
        let auth = PatAuth::new(pat.clone(), None);
        let me = self.connection_data(org, &auth).await?;
        let probes = self.probe(org, &pat, level, &me).await?;
        let required_ok = probes
            .iter()
            .all(|probe| !probe.scope.required || probe.outcome.is_granted());
        let saved = if required_ok {
            self.secret_save(org, &pat).await?;
            let proven = |write_untested: bool| {
                probes
                    .iter()
                    .filter(move |probe| match &probe.outcome {
                        ProbeOutcome::GrantedWriteUntested => true,
                        ProbeOutcome::Granted => !write_untested,
                        _ => false,
                    })
                    .map(|probe| probe.scope.id.to_owned())
            };
            let valid_to = expires_on.map(|date| date.midnight().assume_utc());
            let meta = PatMeta {
                org: org.name().to_owned(),
                level,
                scopes: proven(false).collect(),
                untested: proven(true).collect(),
                authorization_id: None,
                valid_to,
                created_at: OffsetDateTime::now_utc(),
                source: PatSource::Pasted,
                lifetime_days: None,
                lifetime_capped: false,
                verified: true,
                me: Some(me.clone()),
                auto_replaced: false,
                auto_replace: AutoReplaceLog::default(),
            };
            state::write_account(&self.inner.data_dir, &meta).map_err(storage)?;
            self.swap_live(&pat, valid_to);
            Some(meta)
        } else {
            None
        };
        Ok(PasteVerification { me, probes, saved })
    }

    /// The pasted token's expiry, entered after verification (Path B step
    /// 5, design 2q "Expires on"); `None` clears it.
    pub async fn set_pasted_expiry(
        &self,
        expires_on: Option<time::Date>,
    ) -> Result<PatMeta, OnboardError> {
        self.update_account(|meta| {
            if meta.source != PatSource::Pasted {
                return false;
            }
            meta.valid_to = expires_on.map(|date| date.midnight().assume_utc());
            true
        })
        .await?
        .ok_or_else(|| OnboardError::Other("There is no pasted token to date.".into()))
    }

    /// A write's answer about `scope` (FR-17): refused for the scope marks
    /// it missing, so every gate and Settings › Account agree after a
    /// restart; accepted proves an untested write scope. `None` when nothing
    /// changed (the common case: no file write).
    pub async fn record_write(
        &self,
        scope: &str,
        refused: bool,
    ) -> Result<Option<PatMeta>, OnboardError> {
        self.update_account(|meta| {
            let untested = meta.untested.iter().any(|id| id == scope);
            if refused {
                if !meta.has_scope(scope) {
                    return false;
                }
                meta.scopes.retain(|id| !covers(id, scope));
                meta.untested.retain(|id| id != scope);
                true
            } else if untested {
                meta.untested.retain(|id| id != scope);
                true
            } else {
                false
            }
        })
        .await
    }

    /// Read `account.json`, let `change` edit it, write it back when it says
    /// it changed. Serialized with every other token operation.
    async fn update_account(
        &self,
        change: impl FnOnce(&mut PatMeta) -> bool,
    ) -> Result<Option<PatMeta>, OnboardError> {
        let _op = self.inner.ops.lock().await;
        let Some(mut meta) = self.account() else {
            return Ok(None);
        };
        if !change(&mut meta) {
            return Ok(None);
        }
        state::write_account(&self.inner.data_dir, &meta).map_err(storage)?;
        Ok(Some(meta))
    }

    async fn probe(
        &self,
        org: &Organization,
        pat: &Pat,
        level: AccessLevel,
        me: &Identity,
    ) -> Result<Vec<ScopeProbe>, OnboardError> {
        let auth = ProbeAuth(pat.basic_authorization());
        let api = &self.inner.api;
        let (projects, identity) = futures::join!(
            api.probe_projects(org, &auth),
            api.probe_identity(org, &me.id, &auth)
        );
        let (project_outcome, project) = match projects {
            Ok(project) => (ProbeOutcome::Granted, project),
            Err(err) => (probe_outcome(Err(err))?, None),
        };
        let identity_outcome = probe_outcome(identity)?;
        let project_id = project.map(|project| project.id);
        let project_id = project_id.as_deref();
        let (code, work, build) = futures::join!(
            api.probe_code(org, project_id, &auth),
            async {
                match project_id {
                    Some(id) => Some(api.probe_work(org, id, &auth).await),
                    None => None,
                }
            },
            async {
                match project_id {
                    Some(id) => Some(api.probe_build(org, id, &auth).await),
                    None => None,
                }
            },
        );
        let code_outcome = probe_outcome(code)?;
        let work_outcome = work.map_or(Ok(ProbeOutcome::Untested), probe_outcome)?;
        let build_outcome = build.map_or(Ok(ProbeOutcome::Untested), probe_outcome)?;
        Ok(scopes(level)
            .iter()
            .map(|scope| {
                let outcome = match scope.id {
                    "vso.project" => project_outcome.clone(),
                    "vso.code" => code_outcome.clone(),
                    "vso.code_write" => match &code_outcome {
                        ProbeOutcome::Granted => ProbeOutcome::GrantedWriteUntested,
                        other => other.clone(),
                    },
                    "vso.work" => work_outcome.clone(),
                    // Reads prove the token; the write half is proven by the
                    // first link (a 403 then explains the missing scope).
                    "vso.work_write" => match &work_outcome {
                        ProbeOutcome::Granted => ProbeOutcome::GrantedWriteUntested,
                        other => other.clone(),
                    },
                    "vso.build" => build_outcome.clone(),
                    "vso.identity" => identity_outcome.clone(),
                    _ => ProbeOutcome::Untested,
                };
                ScopeProbe { scope, outcome }
            })
            .collect())
    }

    // ── Replace and sign out ───────────────────────────────────────────────

    /// Replace token with `az` (0001 FR-16, AC-12): create a PAT with the
    /// **current** §5.2 set for `level` and `lifetime_days` → Keychain swap
    /// → verify → revoke the old one. If verify fails, the old token goes
    /// back into the Keychain and the new one is revoked. `account.json`
    /// then holds the new token's own scopes, never the old token's.
    pub async fn replace(
        &self,
        source: &TokenSource,
        level: AccessLevel,
        lifetime_days: u32,
    ) -> Result<Replaced, OnboardError> {
        let _op = self.inner.ops.lock().await;
        self.replace_locked(source, level, lifetime_days, false)
            .await
    }

    /// The stored token's level and lifetime, for one-click Replace token.
    pub fn replace_defaults(meta: &PatMeta) -> (AccessLevel, u32) {
        (
            meta.level,
            meta.lifetime_days.unwrap_or(DEFAULT_LIFETIME_DAYS),
        )
    }

    async fn replace_locked(
        &self,
        source: &TokenSource,
        level: AccessLevel,
        lifetime_days: u32,
        auto: bool,
    ) -> Result<Replaced, OnboardError> {
        let old = self
            .account()
            .ok_or_else(|| OnboardError::Other("There is no token to replace.".into()))?;
        let org = old
            .organization()
            .ok_or_else(|| OnboardError::Other(format!("`{}` is not an organization", old.org)))?;
        let old_pat = self.secret_load(&org).await?;
        let minted = self.mint(source, &org, level, lifetime_days).await?;
        let new_id = minted.created.authorization_id.clone();
        let new_pat = minted.created.pat.clone();
        let revoke_new = async |this: &Self| {
            if let Some(id) = &new_id {
                let _ = this.revoke(source, &org, id).await;
            }
        };
        if let Err(err) = self.secret_save(&org, &new_pat).await {
            revoke_new(self).await;
            return Err(err);
        }
        let valid_to = Some(minted.created.valid_to.unwrap_or(minted.requested_valid_to));
        let auth = PatAuth::new(new_pat.clone(), valid_to);
        let me = match self.connection_data(&org, &auth).await {
            Ok(me) => me,
            Err(err) => {
                match &old_pat {
                    Some(old_pat) => self.secret_save(&org, old_pat).await?,
                    None => {
                        self.secret_delete(&org).await?;
                    }
                }
                revoke_new(self).await;
                return Err(err);
            }
        };
        let meta = PatMeta {
            org: org.name().to_owned(),
            level,
            scopes: minted_scopes(&minted.created, level),
            untested: Vec::new(),
            authorization_id: new_id,
            valid_to,
            created_at: OffsetDateTime::now_utc(),
            source: PatSource::Az,
            lifetime_days: Some(minted.lifetime_days),
            lifetime_capped: minted.capped,
            verified: true,
            me: Some(me),
            auto_replaced: auto,
            auto_replace: AutoReplaceLog::default(),
        };
        state::write_account(&self.inner.data_dir, &meta).map_err(storage)?;
        self.swap_live(&new_pat, valid_to);
        let old_revoke_error = match (&old.authorization_id, old.source) {
            (Some(id), PatSource::Az) => self.revoke(source, &org, id).await.err(),
            _ => None,
        };
        Ok(Replaced {
            meta,
            old_revoke_error,
            old_valid_to: old.valid_to,
        })
    }

    /// One background replacement check at `now` (owner decision
    /// 2026-10-01): for an `az` token from 7 days before expiry, at most
    /// once a day, replace it with the same level and lifetime when `az`
    /// mints without interaction. Never opens a browser: a signed-out `az`
    /// is a failure like any other.
    pub async fn auto_replace(&self, now: OffsetDateTime) -> AutoReplace {
        let _op = self.inner.ops.lock().await;
        let Some(mut meta) = self.account().filter(|meta| meta.auto_replace_due(now)) else {
            return AutoReplace::NotDue;
        };
        meta.auto_replace.last_attempt = Some(now);
        if let Err(error) = state::write_account(&self.inner.data_dir, &meta) {
            // Without the attempt on disk "once a day" can't hold.
            return AutoReplace::Failed {
                error: storage(error),
                warn: false,
                valid_to: meta.valid_to,
            };
        }
        let attempt = async {
            let az = match self.inner.locator.detect().await {
                AzStatus::Ready { az, .. } => az,
                AzStatus::NotLoggedIn { .. } => {
                    return Err(OnboardError::AzUnavailable {
                        reason: "Azure CLI isn't signed in.".into(),
                    });
                }
                AzStatus::Failed { reason } => return Err(OnboardError::AzUnavailable { reason }),
                _ => {
                    return Err(OnboardError::AzUnavailable {
                        reason: "Azure CLI isn't available.".into(),
                    });
                }
            };
            let source = TokenSource::new(az);
            source.token(ADO_RESOURCE).await?;
            let (level, lifetime_days) = Self::replace_defaults(&meta);
            self.replace_locked(&source, level, lifetime_days, true)
                .await
        };
        match attempt.await {
            Ok(replaced) => AutoReplace::Replaced(Box::new(replaced)),
            Err(error) => {
                let warn = meta.auto_replace_warn_due(now);
                if warn {
                    meta.auto_replace.warned = true;
                    if let Err(error) = state::write_account(&self.inner.data_dir, &meta) {
                        log::warn!(target: "adoken_ado::onboarding", "couldn't record the replacement warning: {error}");
                    }
                }
                AutoReplace::Failed {
                    error,
                    warn,
                    valid_to: meta.valid_to,
                }
            }
        }
    }

    /// Sign out (0001 §3, AC-15): optionally revoke (az tokens), delete the
    /// Keychain item and saved progress, optionally delete synced data.
    /// Relaunch then starts at Welcome.
    pub async fn sign_out(
        &self,
        options: SignOut,
        az: Option<&AzCli>,
    ) -> Result<SignOutOutcome, OnboardError> {
        let _op = self.inner.ops.lock().await;
        let meta = self.account();
        let mut revoke_error = None;
        if options.revoke
            && let Some(meta) = &meta
            && meta.source == PatSource::Az
            && let (Some(id), Some(org)) = (&meta.authorization_id, meta.organization())
        {
            revoke_error = match az {
                Some(az) => self
                    .revoke(&TokenSource::new(az.clone()), &org, id)
                    .await
                    .err(),
                None => Some(OnboardError::AzUnavailable {
                    reason: "Azure CLI is needed to revoke the token in Azure DevOps.".into(),
                }),
            };
        }
        let org_name = meta
            .as_ref()
            .map(|meta| meta.org.clone())
            .or_else(|| OnboardingState::load(&self.inner.data_dir).and_then(|state| state.org));
        if let Some(org) = org_name.and_then(|name| Organization::parse(&name).ok()) {
            self.secret_delete(&org).await?;
        }
        let dir = &self.inner.data_dir;
        state::remove(dir, state::ACCOUNT_FILE).map_err(storage)?;
        state::remove(dir, state::ONBOARDING_FILE).map_err(storage)?;
        if options.delete_local_data {
            state::remove(dir, state::ORGS_DIR).map_err(storage)?;
        }
        Ok(SignOutOutcome { revoke_error })
    }

    // ── Keychain on the blocking pool ──────────────────────────────────────

    async fn secret_load(&self, org: &Organization) -> Result<Option<Pat>, OnboardError> {
        let secrets = self.inner.secrets.clone();
        let org = org.clone();
        blocking(move || secrets.load(&org)).await
    }

    async fn secret_save(&self, org: &Organization, pat: &Pat) -> Result<(), OnboardError> {
        let secrets = self.inner.secrets.clone();
        let org = org.clone();
        let pat = pat.clone();
        blocking(move || secrets.save(&org, &pat)).await
    }

    async fn secret_delete(&self, org: &Organization) -> Result<bool, OnboardError> {
        let secrets = self.inner.secrets.clone();
        let org = org.clone();
        blocking(move || secrets.delete(&org)).await
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, KeychainError> + Send + 'static,
) -> Result<T, OnboardError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| OnboardError::Keychain {
            detail: err.to_string(),
        })?
        .map_err(keychain_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_uses_hostname_unless_it_looks_like_an_email() {
        assert_eq!(
            display_name_for_host("adas-mbp.local"),
            "adoken · adas-mbp.local"
        );
        let hashed = display_name_for_host("ada@contoso.com");
        let label = hashed.strip_prefix("adoken · mac-").unwrap();
        assert_eq!(label.len(), 4);
        assert!(label.bytes().all(|b| b.is_ascii_hexdigit()));
        let digest = Sha256::digest(b"ada@contoso.com");
        assert_eq!(label, format!("{:02x}{:02x}", digest[0], digest[1]));
        assert!(!hashed.contains("contoso"));
    }

    #[test]
    fn token_page_points_at_org_user_settings() {
        let org = Organization::parse("https://dev.azure.com/fabrikam/").unwrap();
        assert_eq!(
            token_page_url(&org).as_str(),
            "https://dev.azure.com/fabrikam/_usersSettings/tokens"
        );
    }

    #[test]
    fn pat_error_codes_split_into_blocked_and_verbatim() {
        assert!(classify_pat_code("accessDenied".into()).falls_back_to_paste());
        assert_eq!(
            classify_pat_code("fullScopePatPolicyViolation".into()),
            OnboardError::PatRejected {
                code: "fullScopePatPolicyViolation".into()
            }
        );
        assert!(!classify_pat_code("globalPatPolicyViolation".into()).falls_back_to_paste());
    }
}
