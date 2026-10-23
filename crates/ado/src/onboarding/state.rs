//! Non-secret persistence (0001 FR-8, FR-10).
//!
//! Two small JSON files in the data dir, next to the org stores
//! ([`crate::org_store_path`]), which must be readable before any store opens:
//! - `account.json`: [`PatMeta`] (written only by [`super::Connector`]).
//! - `onboarding.json`: [`OnboardingState`] (written by the onboarding UI).
//!
//! Separate files so the UI's step saves never race the backend's metadata
//! writes. Neither ever holds a secret. Directory `0700`, files `0600`
//! (ADR 0001), written atomically (temp file + rename).

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use time::OffsetDateTime;

use crate::Organization;

use super::{AccessLevel, Scope, ScopeStatus, covers, scopes};

pub(crate) const ACCOUNT_FILE: &str = "account.json";
pub(crate) const ONBOARDING_FILE: &str = "onboarding.json";
/// ADR 0001: per-org synced data lives under here.
pub(crate) const ORGS_DIR: &str = "orgs";

/// How the PAT was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatSource {
    Az,
    Pasted,
}

/// `connectionData.authenticatedUser`: who "me" is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub id: String,
    pub display_name: String,
}

/// Non-secret facts about the stored PAT (FR-8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatMeta {
    pub org: String,
    pub level: AccessLevel,
    /// Scope ids the token holds: Azure DevOps' own list from the create
    /// response (az), or the ones a probe or a write proved (pasted).
    pub scopes: Vec<String>,
    /// Pasted tokens: write scopes in [`Self::scopes`] whose write half
    /// isn't proven yet (0001 §5.3 "Write untested").
    #[serde(default)]
    pub untested: Vec<String>,
    /// PAT Lifecycle id; `None` for pasted tokens.
    pub authorization_id: Option<String>,
    /// Expiry from Azure DevOps (az) or the user (pasted, optional).
    #[serde(with = "time::serde::rfc3339::option")]
    pub valid_to: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub source: PatSource,
    /// Requested lifetime (az), reused by Replace token.
    pub lifetime_days: Option<u32>,
    /// The org capped the lifetime, so adoken used 30 days (0001 AC-7).
    pub lifetime_capped: bool,
    /// `connectionData` succeeded with this token (FR-10: minted but
    /// unverified tokens are re-verified, never re-minted).
    pub verified: bool,
    pub me: Option<Identity>,
    /// Created by the background replacement (owner decision 2026-10-01):
    /// Settings › Account says "Replaced automatically on {created}".
    #[serde(default)]
    pub auto_replaced: bool,
    /// Background replacement bookkeeping for this token.
    #[serde(default)]
    pub auto_replace: AutoReplaceLog,
}

/// When the background replacement last tried, and whether the one quiet
/// "couldn't replace" item was posted (owner decision 2026-10-01).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoReplaceLog {
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_attempt: Option<OffsetDateTime>,
    #[serde(default)]
    pub warned: bool,
}

impl PatMeta {
    pub fn organization(&self) -> Option<Organization> {
        Organization::parse(&self.org).ok()
    }

    /// The token may call what needs `id` (a `_write` scope covers its read
    /// scope), as far as adoken knows.
    pub fn has_scope(&self, id: &str) -> bool {
        self.scopes.iter().any(|granted| covers(granted, id))
    }

    /// One §5.2 row's state for this token (0009 FR-8).
    pub fn scope_status(&self, id: &str) -> ScopeStatus {
        if !self.has_scope(id) {
            ScopeStatus::Missing
        } else if self.untested.iter().any(|untested| untested == id) {
            ScopeStatus::NotCheckedYet
        } else {
            ScopeStatus::Granted
        }
    }

    /// The level's scopes this token lacks, in dialog order.
    pub fn missing(&self) -> Vec<&'static Scope> {
        scopes(self.level)
            .iter()
            .filter(|scope| self.scope_status(scope.id) == ScopeStatus::Missing)
            .collect()
    }

    /// `valid_to` is known and past.
    pub fn expired(&self, now: OffsetDateTime) -> bool {
        self.valid_to.is_some_and(|valid_to| valid_to <= now)
    }

    /// The background replacement may try now (owner decision 2026-10-01):
    /// an `az` token, from 7 days before `valid_to`, at most once a day.
    pub fn auto_replace_due(&self, now: OffsetDateTime) -> bool {
        let Some(valid_to) = self.valid_to else {
            return false;
        };
        self.source == PatSource::Az
            && self.verified
            && now >= valid_to - AUTO_REPLACE_FROM
            && self
                .auto_replace
                .last_attempt
                .is_none_or(|last| now - last >= AUTO_REPLACE_EVERY)
    }

    /// A failed background replacement posts its one quiet item when it is
    /// still failing 2 days before expiry, once per token.
    pub fn auto_replace_warn_due(&self, now: OffsetDateTime) -> bool {
        !self.auto_replace.warned
            && self
                .valid_to
                .is_some_and(|valid_to| now >= valid_to - AUTO_REPLACE_WARN)
    }
}

/// The background replacement starts this long before `valid_to`.
pub const AUTO_REPLACE_FROM: time::Duration = time::Duration::days(7);
/// At most one background attempt per this long.
pub const AUTO_REPLACE_EVERY: time::Duration = time::Duration::days(1);
/// Still failing this long before `valid_to`: post the one quiet item.
pub const AUTO_REPLACE_WARN: time::Duration = time::Duration::days(2);

/// Onboarding screens (`docs/design/onboarding.md` §1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    #[default]
    Welcome,
    Connect,
    AzSignIn,
    PickOrg,
    ReviewAccess,
    CreateInBrowser,
    PasteVerify,
    Projects,
    FirstSync,
    MakeItYours,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectPath {
    Az,
    Paste,
}

/// A project in the picker and the saved selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_update_time: Option<OffsetDateTime>,
}

/// Resumable onboarding progress (FR-10). Holds no secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OnboardingState {
    pub step: Step,
    pub path: Option<ConnectPath>,
    pub org: Option<String>,
    pub level: AccessLevel,
    pub lifetime_days: u32,
    pub projects: Vec<ProjectRef>,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            step: Step::Welcome,
            path: None,
            org: None,
            level: AccessLevel::Review,
            lifetime_days: 90,
            projects: Vec::new(),
        }
    }
}

impl OnboardingState {
    /// `None` when there is no saved progress (or it can't be read).
    pub fn load(data_dir: &Path) -> Option<Self> {
        read_json(&data_dir.join(ONBOARDING_FILE)).ok().flatten()
    }

    pub fn save(&self, data_dir: &Path) -> io::Result<()> {
        write_json(data_dir, ONBOARDING_FILE, self)
    }
}

pub(crate) fn read_account(data_dir: &Path) -> io::Result<Option<PatMeta>> {
    read_json(&data_dir.join(ACCOUNT_FILE))
}

pub(crate) fn write_account(data_dir: &Path, meta: &PatMeta) -> io::Result<()> {
    write_json(data_dir, ACCOUNT_FILE, meta)
}

/// Remove `name` in `data_dir`, treating "already gone" as success.
pub(crate) fn remove(data_dir: &Path, name: &str) -> io::Result<()> {
    let path = data_dir.join(name);
    let result = if path.is_dir() {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    };
    match result {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn write_json<T: Serialize>(data_dir: &Path, name: &str, value: &T) -> io::Result<()> {
    create_private_dir(data_dir)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let tmp: PathBuf = data_dir.join(format!(".{name}.tmp"));
    write_private(&tmp, &bytes)?;
    std::fs::rename(&tmp, data_dir.join(name))
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_state_round_trips_and_defaults_to_welcome() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(OnboardingState::load(dir.path()), None);
        let state = OnboardingState {
            step: Step::Projects,
            path: Some(ConnectPath::Az),
            org: Some("fabrikam".into()),
            level: AccessLevel::ReadOnly,
            lifetime_days: 30,
            projects: vec![ProjectRef {
                id: "p1".into(),
                name: "Payments".into(),
                description: None,
                last_update_time: None,
            }],
        };
        state.save(dir.path()).unwrap();
        assert_eq!(OnboardingState::load(dir.path()), Some(state));
        // Older/partial files fill the rest from defaults.
        std::fs::write(dir.path().join(ONBOARDING_FILE), r#"{"step":"connect"}"#).unwrap();
        let loaded = OnboardingState::load(dir.path()).unwrap();
        assert_eq!(loaded.step, Step::Connect);
        assert_eq!(loaded.lifetime_days, 90);
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("dev.adoken");
        OnboardingState::default().save(&data).unwrap();
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&data), 0o700);
        assert_eq!(mode(&data.join(ONBOARDING_FILE)), 0o600);
    }

    fn meta(now: OffsetDateTime) -> PatMeta {
        PatMeta {
            org: "fabrikam".into(),
            level: AccessLevel::Review,
            scopes: [
                "vso.build",
                "vso.code_write",
                "vso.identity",
                "vso.project",
                "vso.work_write",
            ]
            .map(String::from)
            .to_vec(),
            untested: Vec::new(),
            authorization_id: Some("auth".into()),
            valid_to: Some(now + time::Duration::days(90)),
            created_at: now,
            source: PatSource::Az,
            lifetime_days: Some(90),
            lifetime_capped: false,
            verified: true,
            me: None,
            auto_replaced: false,
            auto_replace: AutoReplaceLog::default(),
        }
    }

    #[test]
    fn scope_status_reads_granted_untested_and_missing() {
        let now = OffsetDateTime::now_utc();
        let mut meta = meta(now);
        assert!(meta.missing().is_empty());
        // A pasted token: build refused by its probe, writes untested.
        meta.source = PatSource::Pasted;
        meta.scopes.retain(|id| id != "vso.build");
        meta.untested = vec!["vso.code_write".into(), "vso.work_write".into()];
        assert_eq!(meta.scope_status("vso.build"), ScopeStatus::Missing);
        assert_eq!(
            meta.scope_status("vso.code_write"),
            ScopeStatus::NotCheckedYet
        );
        assert_eq!(meta.scope_status("vso.project"), ScopeStatus::Granted);
        // The read half of a write scope is covered.
        assert!(meta.has_scope("vso.work"));
        let missing: Vec<_> = meta.missing().iter().map(|s| s.id).collect();
        assert_eq!(missing, ["vso.build"]);
    }

    #[test]
    fn account_files_from_before_the_amendment_still_load() {
        let json = r#"{"org":"fabrikam","level":"review","scopes":["vso.code_write"],
            "authorization_id":null,"valid_to":null,"created_at":"2026-09-25T10:00:00Z",
            "source":"pasted","lifetime_days":null,"lifetime_capped":false,"verified":true,
            "me":null}"#;
        let meta: PatMeta = serde_json::from_str(json).unwrap();
        assert!(meta.untested.is_empty());
        assert!(!meta.auto_replaced);
        assert_eq!(meta.auto_replace, AutoReplaceLog::default());
    }

    #[test]
    fn auto_replace_starts_seven_days_out_and_tries_once_a_day() {
        let created = OffsetDateTime::now_utc();
        let mut meta = meta(created);
        let valid_to = meta.valid_to.unwrap();
        let day = time::Duration::days(1);
        assert!(!meta.auto_replace_due(valid_to - 8 * day));
        assert!(meta.auto_replace_due(valid_to - 7 * day));
        meta.auto_replace.last_attempt = Some(valid_to - 7 * day);
        assert!(!meta.auto_replace_due(valid_to - 6 * day - time::Duration::hours(1)));
        assert!(meta.auto_replace_due(valid_to - 6 * day));
        // Only `az` tokens with a known expiry.
        meta.auto_replace.last_attempt = None;
        meta.source = PatSource::Pasted;
        assert!(!meta.auto_replace_due(valid_to - day));
        meta.source = PatSource::Az;
        meta.valid_to = None;
        assert!(!meta.auto_replace_due(created + 365 * day));
    }

    #[test]
    fn the_quiet_warning_is_due_two_days_out_once() {
        let mut meta = meta(OffsetDateTime::now_utc());
        let valid_to = meta.valid_to.unwrap();
        assert!(!meta.auto_replace_warn_due(valid_to - time::Duration::days(3)));
        assert!(meta.auto_replace_warn_due(valid_to - time::Duration::days(2)));
        meta.auto_replace.warned = true;
        assert!(!meta.auto_replace_warn_due(valid_to - time::Duration::hours(1)));
    }
}
