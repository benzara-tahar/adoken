//! Azure CLI detection, sign-in, and Entra tokens (0001 §3 Path A steps 2-4,
//! FR-2, FR-3, FR-6; `docs/design/onboarding.md` §5.1-5.2).
//!
//! Every subprocess runs in its own process group with a hard timeout (FR-2);
//! on timeout, cancel, or drop the whole group is killed, because `az` is a
//! shell script that spawns Python, and killing only the script would leave
//! Python (and `az login`'s loopback listener) behind.
//!
//! Login-shell lookup pattern adapted from zeron
//! `crates/harness/src/shell_env.rs` @ d1010657 (see THIRD_PARTY_NOTICES.md).

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use serde::Deserialize;
use time::OffsetDateTime;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use zeroize::Zeroizing;

use crate::{AuthError, AuthKind, AuthProvider, Authorization, Organization};

use super::OnboardError;

/// Cancels `az login` (and anything else long-running). `CancelToken::new()`,
/// `.clone()`, `.cancel()`.
pub type CancelToken = tokio_util::sync::CancellationToken;

/// FR-3: the MSAL-based credential cache.
pub const MIN_AZ_VERSION: AzVersion = AzVersion {
    major: 2,
    minor: 30,
    patch: 0,
};

/// Azure DevOps' Entra resource id (0001 §5.1).
pub const ADO_RESOURCE: &str = "499b84ac-1321-427f-aa17-267ca6975798";
/// Fallback resource if the PAT API rejects the ADO-resource token (0001 §5.1).
pub const MANAGEMENT_RESOURCE: &str = "https://management.core.windows.net/";

/// Where `az` usually lives when the process `PATH` doesn't say (Finder
/// launches inherit launchd's `/usr/bin:/bin:/usr/sbin:/sbin`).
pub fn known_az_locations(home: Option<&Path>) -> Vec<PathBuf> {
    let mut locations = vec![
        PathBuf::from("/opt/homebrew/bin/az"),
        PathBuf::from("/usr/local/bin/az"),
    ];
    if let Some(home) = home {
        locations.push(home.join(".local/bin/az"));
    }
    locations
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AzVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl AzVersion {
    /// `"2.83.0"`; tolerates pre-release suffixes like `2.84.0b1`.
    pub fn parse(input: &str) -> Option<Self> {
        let mut parts = input.trim().split('.').map(|part| {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u32>().ok()
        });
        Some(Self {
            major: parts.next()??,
            minor: parts.next()??,
            patch: parts.next().flatten().unwrap_or(0),
        })
    }
}

impl fmt::Display for AzVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// FR-2 hard timeouts. `az login` has none (Cancel only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AzTimeouts {
    pub shell_lookup: Duration,
    pub version: Duration,
    pub account: Duration,
    pub token: Duration,
    pub default_org: Duration,
}

impl Default for AzTimeouts {
    fn default() -> Self {
        Self {
            shell_lookup: Duration::from_secs(2),
            version: Duration::from_secs(10),
            account: Duration::from_secs(10),
            token: Duration::from_secs(20),
            default_org: Duration::from_secs(5),
        }
    }
}

/// How to find `az` (0001 §3 step 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AzLocator {
    /// Login shell for `"$SHELL" -lc 'command -v az'`; `None` skips it.
    pub shell: Option<PathBuf>,
    /// The process `PATH`.
    pub path_env: Option<OsString>,
    /// Probed after the shell lookup and `PATH`.
    pub known_locations: Vec<PathBuf>,
    /// A path the user picked ("Locate az…"); wins when executable.
    pub user_picked: Option<PathBuf>,
    pub timeouts: AzTimeouts,
}

impl AzLocator {
    /// From this process' environment.
    pub fn from_env() -> Self {
        let shell = std::env::var_os("SHELL")
            .filter(|shell| !shell.is_empty())
            .map(PathBuf::from)
            .or_else(|| cfg!(target_os = "macos").then(|| PathBuf::from("/bin/zsh")));
        Self {
            shell,
            path_env: std::env::var_os("PATH"),
            known_locations: known_az_locations(dirs::home_dir().as_deref()),
            user_picked: None,
            timeouts: AzTimeouts::default(),
        }
    }

    /// Resolve `az`: user-picked, login shell, process `PATH`, known locations.
    pub async fn locate(&self) -> Option<PathBuf> {
        if let Some(path) = self.user_picked.as_ref().filter(|path| is_executable(path)) {
            return Some(path.clone());
        }
        if let Some(path) = self.login_shell_lookup().await {
            return Some(path);
        }
        if let Some(path) = self.path_env.as_ref().and_then(find_on_path) {
            return Some(path);
        }
        self.known_locations
            .iter()
            .find(|path| is_executable(path))
            .cloned()
    }

    async fn login_shell_lookup(&self) -> Option<PathBuf> {
        let shell = self.shell.as_ref()?;
        let mut command = Command::new(shell);
        command.args(["-lc", "command -v az"]);
        if let Some(path) = &self.path_env {
            command.env("PATH", path);
        }
        let output = run(command, Some(self.timeouts.shell_lookup), None)
            .await
            .ok()?;
        // Shell init (oh-my-zsh and friends) may print noise first; take the
        // last line that is an executable absolute path.
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .rev()
            .map(str::trim)
            .filter(|line| line.starts_with('/'))
            .map(PathBuf::from)
            .find(|path| is_executable(path))
    }

    /// Locate `az` and read its version and account (FR-2). `az version` and
    /// `az account show` run concurrently to keep detection under 3 s.
    pub async fn detect(&self) -> AzStatus {
        let Some(path) = self.locate().await else {
            return AzStatus::NotInstalled;
        };
        let az = AzCli::new(path, self.path_env.clone(), self.timeouts.clone());
        az.status().await
    }
}

/// Result of detection (0001 §3 States; design §3 screen 1 card states).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AzStatus {
    NotInstalled,
    /// Older than [`MIN_AZ_VERSION`]; no token is ever requested from it.
    TooOld {
        az: AzCli,
        found: String,
    },
    NotLoggedIn {
        az: AzCli,
        version: String,
    },
    Ready {
        az: AzCli,
        version: String,
        user: String,
        tenant: String,
    },
    /// "az didn't respond" (timeout or unexpected failure), with details.
    Failed {
        reason: String,
    },
}

impl AzStatus {
    pub fn az(&self) -> Option<&AzCli> {
        match self {
            Self::TooOld { az, .. } | Self::NotLoggedIn { az, .. } | Self::Ready { az, .. } => {
                Some(az)
            }
            Self::NotInstalled | Self::Failed { .. } => None,
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

/// `az login --use-device-code` prompt, parsed from az's stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCodePrompt {
    pub url: String,
    pub code: String,
    /// az's own sentence, for display as-is.
    pub message: String,
}

/// A resolved Azure CLI executable.
#[derive(Clone, PartialEq, Eq)]
pub struct AzCli {
    path: PathBuf,
    path_env: Option<OsString>,
    timeouts: AzTimeouts,
}

impl fmt::Debug for AzCli {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AzCli").field(&self.path).finish()
    }
}

#[derive(Debug)]
enum RunError {
    Spawn(std::io::Error),
    Timeout,
    Cancelled,
}

impl AzCli {
    pub fn new(path: PathBuf, path_env: Option<OsString>, timeouts: AzTimeouts) -> Self {
        Self {
            path,
            path_env,
            timeouts,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn command(&self, args: &[&str], quiet: bool) -> Command {
        let mut command = Command::new(&self.path);
        command.args(args);
        // az's own directory first: a Finder-launched app's PATH misses it.
        let mut dirs: Vec<PathBuf> = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .into_iter()
            .collect();
        if let Some(path) = &self.path_env {
            dirs.extend(std::env::split_paths(path));
        }
        if let Ok(path) = std::env::join_paths(dirs) {
            command.env("PATH", path);
        }
        command
            .env("AZURE_CORE_COLLECT_TELEMETRY", "false")
            .env("AZURE_CORE_NO_COLOR", "true")
            .env("AZURE_CORE_LOGIN_EXPERIENCE_V2", "off");
        if quiet {
            command.env("AZURE_CORE_ONLY_SHOW_ERRORS", "true");
        }
        command
    }

    async fn run(&self, args: &[&str], limit: Duration) -> Result<Output, RunError> {
        run(self.command(args, true), Some(limit), None).await
    }

    /// Version, then account: the detection card.
    pub async fn status(&self) -> AzStatus {
        let (version, account) = futures::join!(self.version(), self.account());
        let version = match version {
            Ok(version) => version,
            Err(reason) => return AzStatus::Failed { reason },
        };
        if version < MIN_AZ_VERSION {
            return AzStatus::TooOld {
                az: self.clone(),
                found: version.to_string(),
            };
        }
        match account {
            Ok(Some(account)) => AzStatus::Ready {
                az: self.clone(),
                version: version.to_string(),
                user: account.user,
                tenant: account.tenant,
            },
            Ok(None) => AzStatus::NotLoggedIn {
                az: self.clone(),
                version: version.to_string(),
            },
            Err(reason) => AzStatus::Failed { reason },
        }
    }

    /// `az version -o json` → `"azure-cli"`.
    async fn version(&self) -> Result<AzVersion, String> {
        let output = self
            .run(&["version", "--output", "json"], self.timeouts.version)
            .await
            .map_err(|err| describe("az version", self.timeouts.version, err))?;
        if !output.status.success() {
            return Err(format!(
                "az version failed: {}",
                first_line(&output.stderr).unwrap_or("no output")
            ));
        }
        #[derive(Deserialize)]
        struct Version {
            #[serde(rename = "azure-cli")]
            azure_cli: String,
        }
        let parsed: Version = serde_json::from_slice(&output.stdout)
            .map_err(|_| "az version printed something unexpected".to_owned())?;
        AzVersion::parse(&parsed.azure_cli)
            .ok_or_else(|| format!("az reported an unreadable version `{}`", parsed.azure_cli))
    }

    /// `az account show -o json`; non-zero exit = not signed in.
    async fn account(&self) -> Result<Option<AzAccount>, String> {
        let output = self
            .run(
                &["account", "show", "--output", "json"],
                self.timeouts.account,
            )
            .await
            .map_err(|err| describe("az account show", self.timeouts.account, err))?;
        if !output.status.success() {
            return Ok(None);
        }
        #[derive(Deserialize)]
        struct User {
            name: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Account {
            user: User,
            tenant_id: String,
        }
        let account: Account = serde_json::from_slice(&output.stdout)
            .map_err(|_| "az account show printed something unexpected".to_owned())?;
        Ok(Some(AzAccount {
            user: account.user.name,
            tenant: account.tenant_id,
        }))
    }

    /// `az login` in the browser; unbounded, `cancel` kills it (FR-2).
    pub async fn login(&self, cancel: CancelToken) -> Result<(), OnboardError> {
        let command = self.command(
            &["login", "--allow-no-subscriptions", "--output", "none"],
            true,
        );
        match run(command, None, Some(&cancel)).await {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output) => Err(OnboardError::LoginFailed {
                detail: first_line(&output.stderr)
                    .unwrap_or("az login exited with an error")
                    .to_owned(),
            }),
            Err(RunError::Cancelled) => Err(OnboardError::LoginCancelled),
            Err(err) => Err(OnboardError::LoginFailed {
                detail: describe("az login", Duration::ZERO, err),
            }),
        }
    }

    /// `az login --use-device-code`; `on_prompt` receives the code to show.
    pub async fn login_device_code(
        &self,
        cancel: CancelToken,
        mut on_prompt: impl FnMut(DeviceCodePrompt) + Send,
    ) -> Result<(), OnboardError> {
        // Not quiet: az prints the device-code prompt as a warning.
        let mut command = self.command(
            &[
                "login",
                "--use-device-code",
                "--allow-no-subscriptions",
                "--output",
                "none",
            ],
            false,
        );
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|err| OnboardError::LoginFailed {
            detail: err.to_string(),
        })?;
        let _group = GroupKill::new(child.id());
        let stderr = child.stderr.take().expect("stderr is piped");
        let mut lines = BufReader::new(stderr).lines();
        let mut first_error: Option<String> = None;
        let read = async {
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(prompt) = parse_device_code(&line) {
                    on_prompt(prompt);
                } else if first_error.is_none() && !line.trim().is_empty() {
                    first_error = Some(line.trim().to_owned());
                }
            }
        };
        let status = tokio::select! {
            (_, status) = async { futures::join!(read, child.wait()) } => status,
            () = cancel.cancelled() => return Err(OnboardError::LoginCancelled),
        };
        match status {
            Ok(status) if status.success() => Ok(()),
            Ok(_) => Err(OnboardError::LoginFailed {
                detail: first_error.unwrap_or_else(|| "az login exited with an error".into()),
            }),
            Err(err) => Err(OnboardError::LoginFailed {
                detail: err.to_string(),
            }),
        }
    }

    /// `az account get-access-token --resource {resource}` (FR-6: memory only).
    pub async fn access_token(&self, resource: &str) -> Result<EntraToken, OnboardError> {
        let output = self
            .run(
                &[
                    "account",
                    "get-access-token",
                    "--resource",
                    resource,
                    "--output",
                    "json",
                ],
                self.timeouts.token,
            )
            .await
            .map_err(|err| OnboardError::AzUnavailable {
                reason: describe("az account get-access-token", self.timeouts.token, err),
            })?;
        let stdout = Zeroizing::new(output.stdout);
        if !output.status.success() {
            return Err(OnboardError::AzUnavailable {
                reason: format!(
                    "Azure CLI couldn't get a token: {}",
                    first_line(&output.stderr).unwrap_or("no details")
                ),
            });
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Token {
            access_token: String,
            // Epoch seconds, az >= 2.54; older versions only print local time.
            #[serde(rename = "expires_on")]
            expires_on: Option<serde_json::Value>,
        }
        let token: Token =
            serde_json::from_slice(&stdout).map_err(|_| OnboardError::AzUnavailable {
                reason: "az account get-access-token printed something unexpected".into(),
            })?;
        let secret = Zeroizing::new(token.access_token);
        let expires_at = token
            .expires_on
            .and_then(|value| match value {
                serde_json::Value::Number(n) => n.as_i64(),
                serde_json::Value::String(s) => s.parse().ok(),
                _ => None,
            })
            .and_then(|epoch| OffsetDateTime::from_unix_timestamp(epoch).ok())
            // Unknown: assume a short life; az tokens last ~1 h.
            .unwrap_or_else(|| OffsetDateTime::now_utc() + time::Duration::minutes(30));
        Ok(EntraToken { secret, expires_at })
    }

    /// Best effort `az devops configure --list` → default organization.
    pub async fn default_org(&self) -> Option<Organization> {
        let output = self
            .run(
                &["devops", "configure", "--list"],
                self.timeouts.default_org,
            )
            .await
            .ok()
            .filter(|output| output.status.success())?;
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once('=')?;
                (key.trim() == "organization").then(|| value.trim().to_owned())
            })
            .and_then(|value| Organization::parse(&value).ok())
    }
}

#[derive(Debug)]
struct AzAccount {
    user: String,
    tenant: String,
}

/// An Entra access token from `az`. Memory only, zeroized on drop,
/// redacted in `Debug` (FR-6).
#[derive(Clone)]
pub struct EntraToken {
    secret: Zeroizing<String>,
    expires_at: OffsetDateTime,
}

impl EntraToken {
    pub fn expires_at(&self) -> OffsetDateTime {
        self.expires_at
    }

    pub(crate) fn authorization(&self) -> Authorization {
        Authorization::bearer(&self.secret)
    }

    /// Still good for at least five minutes.
    fn is_fresh(&self) -> bool {
        self.expires_at - time::Duration::minutes(5) > OffsetDateTime::now_utc()
    }
}

impl fmt::Debug for EntraToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EntraToken")
            .field("secret", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Entra tokens from a signed-in `az`, cached per resource and refreshed
/// when they get close to expiry. Signs requests as [`AuthKind::AzCliToken`].
#[derive(Clone)]
pub struct TokenSource {
    az: AzCli,
    cache: Arc<tokio::sync::Mutex<HashMap<String, EntraToken>>>,
}

impl fmt::Debug for TokenSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSource")
            .field("az", &self.az)
            .finish_non_exhaustive()
    }
}

impl TokenSource {
    pub fn new(az: AzCli) -> Self {
        Self {
            az,
            cache: Arc::default(),
        }
    }

    pub fn az(&self) -> &AzCli {
        &self.az
    }

    pub async fn token(&self, resource: &str) -> Result<EntraToken, OnboardError> {
        let mut cache = self.cache.lock().await;
        if let Some(token) = cache.get(resource).filter(|token| token.is_fresh()) {
            return Ok(token.clone());
        }
        let token = self.az.access_token(resource).await?;
        cache.insert(resource.to_owned(), token.clone());
        Ok(token)
    }

    /// Drop every cached token (they zeroize on drop).
    pub async fn forget(&self) {
        self.cache.lock().await.clear();
    }
}

/// Requests signed with this source use the ADO-resource token.
pub(crate) struct ResourceAuth<'a> {
    pub source: &'a TokenSource,
    pub resource: &'static str,
}

impl AuthProvider for ResourceAuth<'_> {
    fn kind(&self) -> AuthKind {
        AuthKind::AzCliToken
    }

    fn authorize<'a>(
        &'a self,
        request: &'a mut reqwest::Request,
    ) -> BoxFuture<'a, Result<(), AuthError>> {
        Box::pin(async move {
            let token = self
                .source
                .token(self.resource)
                .await
                .map_err(|err| AuthError::Unavailable(err.message()))?;
            token.authorization().apply(request)
        })
    }

    fn expires_at(&self) -> Option<OffsetDateTime> {
        None
    }

    fn on_unauthorized(&self) {}
}

impl AuthProvider for TokenSource {
    fn kind(&self) -> AuthKind {
        AuthKind::AzCliToken
    }

    fn authorize<'a>(
        &'a self,
        request: &'a mut reqwest::Request,
    ) -> BoxFuture<'a, Result<(), AuthError>> {
        Box::pin(async move {
            ResourceAuth {
                source: self,
                resource: ADO_RESOURCE,
            }
            .authorize(request)
            .await
        })
    }

    fn expires_at(&self) -> Option<OffsetDateTime> {
        self.cache
            .try_lock()
            .ok()
            .and_then(|cache| cache.get(ADO_RESOURCE).map(EntraToken::expires_at))
    }

    fn on_unauthorized(&self) {
        if let Ok(mut cache) = self.cache.try_lock() {
            cache.clear();
        }
    }
}

fn describe(step: &str, limit: Duration, err: RunError) -> String {
    match err {
        RunError::Timeout => format!(
            "az didn't respond ({step} took longer than {}s)",
            limit.as_secs()
        ),
        RunError::Cancelled => format!("{step} was cancelled"),
        RunError::Spawn(err) => format!("couldn't run {step}: {err}"),
    }
}

fn first_line(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes)
        .ok()?
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
}

fn parse_device_code(line: &str) -> Option<DeviceCodePrompt> {
    let url = line
        .split_whitespace()
        .find(|word| word.starts_with("https://"))?
        .trim_end_matches(['.', ',']);
    let (_, after) = line.split_once("enter the code ")?;
    let code = after.split_whitespace().next()?;
    Some(DeviceCodePrompt {
        url: url.to_owned(),
        code: code.to_owned(),
        message: line.trim().trim_start_matches("WARNING: ").to_owned(),
    })
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

fn find_on_path(path: &OsString) -> Option<PathBuf> {
    let name = if cfg!(windows) { "az.cmd" } else { "az" };
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

/// Kills the child's whole process group when dropped (unix).
struct GroupKill(Option<u32>);

impl GroupKill {
    fn new(pid: Option<u32>) -> Self {
        Self(pid)
    }

    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for GroupKill {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0.and_then(|pid| i32::try_from(pid).ok()) {
            // SAFETY: plain syscall; the group was created for this child
            // via `process_group(0)`, so pgid == pid.
            unsafe {
                libc::killpg(pid, libc::SIGKILL);
            }
        }
    }
}

/// Run to completion under an optional deadline and cancel token. The whole
/// process group dies on timeout, cancel, or drop.
async fn run(
    mut command: Command,
    limit: Option<Duration>,
    cancel: Option<&CancelToken>,
) -> Result<Output, RunError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let child = command.spawn().map_err(RunError::Spawn)?;
    let mut group = GroupKill::new(child.id());
    let wait = child.wait_with_output();
    let deadline = async {
        match limit {
            Some(limit) => tokio::time::sleep(limit).await,
            None => futures::future::pending().await,
        }
    };
    let cancelled = async {
        match cancel {
            Some(cancel) => cancel.cancelled().await,
            None => futures::future::pending().await,
        }
    };
    let result = tokio::select! {
        output = wait => output.map_err(RunError::Spawn),
        () = deadline => Err(RunError::Timeout),
        () = cancelled => Err(RunError::Cancelled),
    };
    if result.is_ok() {
        group.disarm();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_and_compare_against_the_gate() {
        assert_eq!(
            AzVersion::parse("2.83.0"),
            Some(AzVersion {
                major: 2,
                minor: 83,
                patch: 0
            })
        );
        assert!(AzVersion::parse("2.29.1").unwrap() < MIN_AZ_VERSION);
        assert!(AzVersion::parse("2.30.0").unwrap() >= MIN_AZ_VERSION);
        assert!(AzVersion::parse("2.84.0b1").unwrap() > MIN_AZ_VERSION);
        assert!(AzVersion::parse("10.0").unwrap() > MIN_AZ_VERSION);
        assert_eq!(AzVersion::parse("dev"), None);
    }

    #[test]
    fn device_code_prompt_is_parsed_from_az_warning() {
        let line = "WARNING: To sign in, use a web browser to open the page https://microsoft.com/devicelogin and enter the code FJ7K2LMNQ to authenticate.";
        let prompt = parse_device_code(line).unwrap();
        assert_eq!(prompt.url, "https://microsoft.com/devicelogin");
        assert_eq!(prompt.code, "FJ7K2LMNQ");
        assert!(prompt.message.starts_with("To sign in"));
        assert_eq!(parse_device_code("WARNING: something else"), None);
    }

    #[test]
    fn entra_tokens_are_redacted() {
        let token = EntraToken {
            secret: Zeroizing::new("eyJ0eXAiOiJKV1Qi.secret".into()),
            expires_at: OffsetDateTime::now_utc(),
        };
        let rendered = format!("{token:?} {:?}", token.authorization());
        assert!(!rendered.contains("eyJ0eXAi"), "{rendered}");
    }
}
