//! Spec 0001 §6 backend acceptance tests: a fake `az` on a controlled PATH
//! returning fixture output per subcommand, and a mock HTTP server standing
//! in for every Azure DevOps host. Nothing here touches real `az` or ADO.

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use adoken_ado::http::{Endpoints, Http, HttpError};
use adoken_ado::{
    AccessLevel, AuthError, AutoReplace, AzLocator, AzStatus, AzTimeouts, CancelToken, Connector,
    ConnectorParts, Keychain, MemorySecretStore, OnboardError, OnboardingState, Organization, Pat,
    PatAuth, PatSource, ProbeOutcome, ScopeStatus, SecretStore, SignOut, Step,
};
use base64::Engine as _;
use serde_json::{Value, json};
use tempfile::TempDir;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ENTRA: &str = "ENTRASECRETeyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiJ9fake";
const ORG: &str = "fabrikam";

fn pat_value(tag: &str) -> String {
    format!("{tag:0<84}")
}

// ── Log capture (AC-9: no secret in logs) ─────────────────────────────────

struct Capture(Mutex<Vec<String>>);

impl Capture {
    fn text(&self) -> String {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .join("\n")
    }
}

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!("{} {}", record.target(), record.args()));
    }
    fn flush(&self) {}
}

static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));
static LOGS: LazyLock<&'static Capture> = LazyLock::new(|| {
    log::set_logger(&CAPTURE).expect("only this test binary installs a logger");
    log::set_max_level(log::LevelFilter::Trace);
    &CAPTURE
});

fn logs() -> &'static Capture {
    &LOGS
}

// ── Fake az ────────────────────────────────────────────────────────────────
//
// macOS scans every new executable on its first run (~250 ms, serialized
// system-wide), which would make parallel tests trip the FR-2 timeouts. So the
// scripts are written once per machine under CARGO_TARGET_TMPDIR and each test
// hardlinks them (same inode, no rescan). Scripts find their per-test fixture
// directory relative to `$0`.

const AZ_SCRIPT: &str = r#"#!/bin/sh
D="${0%/opt/homebrew/bin/az}/fixtures"
echo "$*" >> "$D/calls.log"
case "$1" in
  version) key=version ;;
  account) case "$2" in show) key=account_show ;; get-access-token) key=token ;; *) key=unknown ;; esac ;;
  login) key=login ;;
  devops) key=devops ;;
  *) key=unknown ;;
esac
if [ -f "$D/$key.sleep" ]; then /bin/sleep "$(/bin/cat "$D/$key.sleep")"; fi
if [ -f "$D/$key.stdout" ]; then /bin/cat "$D/$key.stdout"; fi
if [ -f "$D/$key.stderr" ]; then /bin/cat "$D/$key.stderr" >&2; fi
code=$(/bin/cat "$D/$key.exit" 2>/dev/null || echo 0)
if [ "$key" = login ] && [ "$code" = 0 ]; then
  /bin/cp "$D/account_ok.stdout" "$D/account_show.stdout"
  echo 0 > "$D/account_show.exit"
fi
exit "$code"
"#;
/// A login shell whose rc files only print noise.
const NOISY_SHELL: &str = "#!/bin/sh\necho 'oh-my-zsh: updating plugins'\nexit 0\n";
/// A login shell that knows where this test's az is, after some noise.
const LOGIN_SHELL: &str =
    "#!/bin/sh\necho 'Welcome back!'\necho \"${0%/login-shell}/opt/homebrew/bin/az\"\n";

static MASTERS: LazyLock<PathBuf> = LazyLock::new(|| {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fake-az");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, contents) in [
        ("az", AZ_SCRIPT),
        ("noisy-shell", NOISY_SHELL),
        ("login-shell", LOGIN_SHELL),
    ] {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(contents) {
            let tmp = dir.join(format!(".{name}.tmp"));
            write_executable(&tmp, contents);
            std::fs::rename(&tmp, &path).unwrap();
        }
    }
    dir
});

fn link_master(name: &str, dest: &Path) {
    std::fs::hard_link(MASTERS.join(name), dest).unwrap();
}

struct FakeAz {
    dir: TempDir,
    az: PathBuf,
    fixtures: PathBuf,
}

impl FakeAz {
    /// `az` at `<tmp>/opt/homebrew/bin/az` only.
    fn new() -> Self {
        let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let bin = dir.path().join("opt/homebrew/bin");
        let fixtures = dir.path().join("fixtures");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&fixtures).unwrap();
        let az = bin.join("az");
        link_master("az", &az);
        let fake = Self { dir, az, fixtures };
        fake.set("account_ok", &account_json(), 0);
        fake
    }

    /// A login shell script (`noisy-shell` or `login-shell`) next to `opt/`.
    fn shell(&self, name: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        link_master(name, &path);
        path
    }

    fn signed_in(version: &str) -> Self {
        let fake = Self::new();
        fake.set("version", &version_json(version), 0);
        fake.set("account_show", &account_json(), 0);
        fake.set("token", &token_json(), 0);
        fake
    }

    fn set(&self, key: &str, stdout: &str, exit: i32) {
        std::fs::write(self.fixtures.join(format!("{key}.stdout")), stdout).unwrap();
        std::fs::write(self.fixtures.join(format!("{key}.exit")), exit.to_string()).unwrap();
    }

    fn set_stderr(&self, key: &str, stderr: &str) {
        std::fs::write(self.fixtures.join(format!("{key}.stderr")), stderr).unwrap();
    }

    fn sleep(&self, key: &str, seconds: u32) {
        std::fs::write(
            self.fixtures.join(format!("{key}.sleep")),
            seconds.to_string(),
        )
        .unwrap();
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.fixtures.join("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Finder-launch conditions: `PATH=/usr/bin:/bin`, no login shell;
    /// `az` is only at the known Homebrew location.
    fn locator(&self) -> AzLocator {
        AzLocator {
            shell: None,
            path_env: Some("/usr/bin:/bin".into()),
            known_locations: vec![self.az.clone()],
            user_picked: None,
            timeouts: AzTimeouts::default(),
        }
    }
}

fn write_executable(path: &Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, contents).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn version_json(version: &str) -> String {
    json!({ "azure-cli": version, "azure-cli-core": version, "extensions": {} }).to_string()
}

fn account_json() -> String {
    json!({
        "user": { "name": "ada@contoso.com", "type": "user" },
        "tenantId": "tenant-1234",
        "name": "N/A(tenant level account)"
    })
    .to_string()
}

fn token_json() -> String {
    let expires = OffsetDateTime::now_utc().unix_timestamp() + 3600;
    json!({
        "accessToken": ENTRA,
        "expiresOn": "2026-09-25 14:00:00.000000",
        "expires_on": expires,
        "tenant": "tenant-1234",
        "tokenType": "Bearer"
    })
    .to_string()
}

// ── Harness ───────────────────────────────────────────────────────────────

struct Harness {
    server: MockServer,
    az: FakeAz,
    data: TempDir,
    secrets: Arc<MemorySecretStore>,
    connector: Connector,
}

impl Harness {
    async fn new() -> Self {
        Self::with_az(FakeAz::signed_in("2.83.0")).await
    }

    async fn with_az(az: FakeAz) -> Self {
        logs();
        let server = MockServer::start().await;
        let data = tempfile::tempdir().unwrap();
        let secrets = Arc::new(MemorySecretStore::new());
        let connector = connector(&server, &az, data.path(), secrets.clone());
        Self {
            server,
            az,
            data,
            secrets,
            connector,
        }
    }

    /// A fresh process: same data dir, keychain, and server.
    fn relaunch(&self) -> Connector {
        connector(
            &self.server,
            &self.az,
            self.data.path(),
            self.secrets.clone(),
        )
    }

    fn org(&self) -> Organization {
        Organization::parse(ORG).unwrap()
    }

    async fn requests(&self, verb: &str, url_path: &str) -> Vec<wiremock::Request> {
        self.server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.method.as_str() == verb && request.url.path() == url_path)
            .collect()
    }

    async fn token_source(&self) -> adoken_ado::TokenSource {
        let status = self.connector.detect_az().await;
        let az = status.az().expect("fake az detected").clone();
        self.connector.token_source(&az).await.unwrap()
    }
}

fn connector(
    server: &MockServer,
    az: &FakeAz,
    data: &Path,
    secrets: Arc<dyn SecretStore>,
) -> Connector {
    Connector::with_parts(ConnectorParts {
        data_dir: data.join("dev.adoken"),
        endpoints: Endpoints::under(&url::Url::parse(&server.uri()).unwrap()),
        locator: az.locator(),
        secrets,
    })
}

const PATS_PATH: &str = "/vssps/fabrikam/_apis/tokens/pats";
const CONNECTION_DATA_PATH: &str = "/dev/fabrikam/_apis/connectionData";

fn basic(token: &str) -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!(":{token}"))
    )
}

fn pat_created(token: &str, authorization_id: &str, days: i64) -> ResponseTemplate {
    let valid_to = (OffsetDateTime::now_utc() + time::Duration::days(days))
        .format(&Rfc3339)
        .unwrap();
    ResponseTemplate::new(200).set_body_json(json!({
        "patToken": {
            "displayName": "adoken · test",
            "validTo": valid_to,
            "scope": "vso.project vso.code_write vso.work vso.build vso.identity",
            "targetAccounts": ["org-id"],
            "validFrom": "2026-09-25T10:00:00Z",
            "authorizationId": authorization_id,
            "token": token
        },
        "patTokenError": "none"
    }))
}

fn pat_error(code: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "patToken": null, "patTokenError": code }))
}

fn connection_data() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "authenticatedUser": { "id": "user-1", "providerDisplayName": "Ada Lovelace" },
        "authorizedUser": { "id": "user-1" },
        "instanceId": "instance"
    }))
}

async fn mount_connection_data_for(server: &MockServer, token: &str) {
    Mock::given(method("GET"))
        .and(path(CONNECTION_DATA_PATH))
        .and(header("authorization", basic(token).as_str()))
        .respond_with(connection_data())
        .mount(server)
        .await;
}

fn create_bodies(requests: &[wiremock::Request]) -> Vec<Value> {
    requests
        .iter()
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect()
}

fn valid_to_of(body: &Value) -> OffsetDateTime {
    OffsetDateTime::parse(body["validTo"].as_str().unwrap(), &Rfc3339).unwrap()
}

fn assert_near(actual: OffsetDateTime, expected: OffsetDateTime) {
    let delta = (actual - expected).abs();
    assert!(delta < time::Duration::minutes(1), "{actual} vs {expected}");
}

/// Every file under `dir`, for byte searches.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            out.push(path);
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

// ── Detection: AC-1, AC-2, AC-3, FR-2 ──────────────────────────────────────

#[tokio::test]
async fn ac1_detects_version_and_signed_in_user_quickly() {
    let harness = Harness::new().await;
    let started = Instant::now();
    let status = harness.connector.detect_az().await;
    assert!(started.elapsed() < Duration::from_secs(3));
    match status {
        AzStatus::Ready {
            version,
            user,
            tenant,
            ..
        } => {
            assert_eq!(version, "2.83.0");
            assert_eq!(user, "ada@contoso.com");
            assert_eq!(tenant, "tenant-1234");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn ac2_finds_az_at_homebrew_when_path_is_finder_minimal() {
    let az = FakeAz::signed_in("2.83.0");
    // A login shell that only prints rc-file noise, no az.
    let shell = az.shell("noisy-shell");
    let mut locator = az.locator();
    locator.shell = Some(shell);
    let found = locator.locate().await.unwrap();
    assert_eq!(found, az.az);
    assert!(locator.detect().await.is_ready());

    // Nothing anywhere → not installed.
    let mut empty = locator.clone();
    empty.known_locations = vec![az.dir.path().join("nope/az")];
    assert_eq!(empty.detect().await, AzStatus::NotInstalled);
}

#[tokio::test]
async fn login_shell_lookup_wins_and_skips_rc_noise() {
    let az = FakeAz::signed_in("2.83.0");
    let shell = az.shell("login-shell");
    let locator = AzLocator {
        shell: Some(shell),
        path_env: Some("/usr/bin:/bin".into()),
        known_locations: vec![],
        user_picked: None,
        timeouts: AzTimeouts::default(),
    };
    assert_eq!(locator.locate().await, Some(az.az.clone()));
}

#[tokio::test]
async fn ac3_too_old_az_is_gated_without_token_calls() {
    let az = FakeAz::signed_in("2.29.1");
    let harness = Harness::with_az(az).await;
    match harness.connector.detect_az().await {
        AzStatus::TooOld { found, .. } => assert_eq!(found, "2.29.1"),
        other => panic!("{other:?}"),
    }
    assert!(
        harness
            .az
            .calls()
            .iter()
            .all(|call| !call.contains("get-access-token")),
        "{:?}",
        harness.az.calls()
    );
}

#[tokio::test]
async fn fr2_hung_az_times_out_as_failed() {
    let az = FakeAz::signed_in("2.83.0");
    az.sleep("version", 5);
    let mut locator = az.locator();
    locator.timeouts.version = Duration::from_millis(300);
    let started = Instant::now();
    match locator.detect().await {
        AzStatus::Failed { reason } => assert!(reason.contains("didn't respond"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert!(started.elapsed() < Duration::from_secs(3));
}

// ── Sign-in: AC-4 ──────────────────────────────────────────────────────────

#[tokio::test]
async fn ac4_signed_out_runs_az_login_then_is_ready() {
    let az = FakeAz::signed_in("2.83.0");
    az.set("account_show", "", 1);
    az.set_stderr(
        "account_show",
        "ERROR: Please run 'az login' to setup account.",
    );
    az.set("login", "", 0);
    let harness = Harness::with_az(az).await;
    let status = harness.connector.detect_az().await;
    let AzStatus::NotLoggedIn { az, version } = status else {
        panic!("{status:?}");
    };
    assert_eq!(version, "2.83.0");
    let after = harness
        .connector
        .az_login(&az, CancelToken::new())
        .await
        .unwrap();
    assert!(after.is_ready(), "{after:?}");
    assert!(
        harness
            .az
            .calls()
            .iter()
            .any(|call| call.starts_with("login --allow-no-subscriptions"))
    );
}

#[tokio::test]
async fn az_login_cancel_kills_it_promptly() {
    let az = FakeAz::signed_in("2.83.0");
    az.set("account_show", "", 1);
    az.set("login", "", 0);
    az.sleep("login", 30);
    let harness = Harness::with_az(az).await;
    let status = harness.connector.detect_az().await;
    let az = status.az().unwrap().clone();
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        trigger.cancel();
    });
    let started = Instant::now();
    let result = harness.connector.az_login(&az, cancel).await;
    assert_eq!(result, Err(OnboardError::LoginCancelled));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn az_login_failure_reports_first_stderr_line() {
    let az = FakeAz::signed_in("2.83.0");
    az.set("account_show", "", 1);
    az.set("login", "", 1);
    az.set_stderr("login", "\nAADSTS50076: MFA required\nmore\n");
    let harness = Harness::with_az(az).await;
    let az = harness.connector.detect_az().await.az().unwrap().clone();
    assert_eq!(
        harness.connector.az_login(&az, CancelToken::new()).await,
        Err(OnboardError::LoginFailed {
            detail: "AADSTS50076: MFA required".into()
        })
    );
}

#[tokio::test]
async fn default_org_comes_from_az_devops_configure() {
    let az = FakeAz::signed_in("2.83.0");
    az.set(
        "devops",
        "organization = https://dev.azure.com/contoso/\nproject = Payments\n",
        0,
    );
    let harness = Harness::with_az(az).await;
    let az = harness.connector.detect_az().await.az().unwrap().clone();
    assert_eq!(
        harness.connector.default_org(&az).await,
        Some(Organization::parse("contoso").unwrap())
    );
}

// ── Orgs: AC-5 ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn ac5_lists_all_orgs_via_profile_then_accounts_with_entra_token() {
    let harness = Harness::new().await;
    let bearer = format!("Bearer {ENTRA}");
    Mock::given(method("GET"))
        .and(path("/app/_apis/profile/profiles/me"))
        .and(header("authorization", bearer.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "member-42", "displayName": "Ada Lovelace", "emailAddress": "ada@contoso.com"
        })))
        .expect(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("GET"))
        .and(path("/app/_apis/accounts"))
        .and(query_param("memberId", "member-42"))
        .and(header("authorization", bearer.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "count": 3,
            "value": [
                { "accountId": "1", "accountName": "fabrikam" },
                { "accountId": "2", "accountName": "Contoso" },
                { "accountId": "3", "accountName": "adatum" }
            ]
        })))
        .expect(1)
        .mount(&harness.server)
        .await;
    let source = harness.token_source().await;
    let list = harness.connector.list_orgs(&source).await.unwrap();
    assert_eq!(list.me.id, "member-42");
    let names: Vec<_> = list.orgs.iter().map(|org| org.name.as_str()).collect();
    assert_eq!(names, ["adatum", "Contoso", "fabrikam"]);
    assert!(list.orgs.iter().all(|org| org.organization().is_some()));
    assert_eq!(
        Organization::parse("https://dev.azure.com/fabrikam/")
            .unwrap()
            .name(),
        "fabrikam"
    );
}

#[tokio::test]
async fn projects_are_paged_by_continuation_token() {
    let harness = Harness::new().await;
    Mock::given(method("GET"))
        .and(path("/dev/fabrikam/_apis/projects"))
        .and(query_param("continuationToken", "page2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "count": 1, "value": [{ "id": "p2", "name": "Mobile" }]
        })))
        .mount(&harness.server)
        .await;
    Mock::given(method("GET"))
        .and(path("/dev/fabrikam/_apis/projects"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ms-continuationtoken", "page2")
                .set_body_json(json!({
                    "count": 1,
                    "value": [{
                        "id": "p1", "name": "Payments", "description": "Money",
                        "lastUpdateTime": "2026-09-20T08:00:00.123Z"
                    }]
                })),
        )
        .mount(&harness.server)
        .await;
    let source = harness.token_source().await;
    let projects = harness
        .connector
        .list_projects(&harness.org(), Some(&source))
        .await
        .unwrap();
    let names: Vec<_> = projects.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Payments", "Mobile"]);
    assert_eq!(projects[0].description.as_deref(), Some("Money"));
    assert!(projects[0].last_update_time.is_some());
}

// ── Mint: AC-6, AC-7, AC-8, AC-9 ───────────────────────────────────────────

#[tokio::test]
async fn ac6_create_request_is_org_scoped_least_privilege_90_days() {
    let harness = Harness::new().await;
    let token = pat_value("PATSECRETac6");
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .and(query_param("api-version", "7.1-preview.1"))
        .and(header("authorization", format!("Bearer {ENTRA}").as_str()))
        .respond_with(pat_created(&token, "auth-1", 90))
        .expect(1)
        .mount(&harness.server)
        .await;
    mount_connection_data_for(&harness.server, &token).await;
    let source = harness.token_source().await;
    let meta = harness
        .connector
        .create_pat(&source, &harness.org(), AccessLevel::Review, 90)
        .await
        .unwrap();

    let creates = harness.requests("POST", PATS_PATH).await;
    let bodies = create_bodies(&creates);
    assert_eq!(bodies.len(), 1);
    let body = &bodies[0];
    assert_eq!(body["allOrgs"], json!(false));
    let mut scopes: Vec<_> = body["scope"].as_str().unwrap().split(' ').collect();
    scopes.sort_unstable();
    assert_eq!(
        scopes,
        [
            "vso.build",
            "vso.code_write",
            "vso.identity",
            "vso.project",
            "vso.work_write"
        ]
    );
    assert_near(
        valid_to_of(body),
        OffsetDateTime::now_utc() + time::Duration::days(90),
    );
    assert!(
        body["displayName"]
            .as_str()
            .unwrap()
            .starts_with("adoken · ")
    );
    assert!(meta.verified);
    assert!(!meta.lifetime_capped);
    assert_eq!(meta.me.as_ref().unwrap().id, "user-1");
}

#[tokio::test]
async fn ac7_lifespan_policy_retries_exactly_once_with_30_days() {
    let harness = Harness::new().await;
    let token = pat_value("PATSECRETac7");
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(pat_error("patLifespanPolicyViolation"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&harness.server)
        .await;
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(pat_created(&token, "auth-7", 30))
        .mount(&harness.server)
        .await;
    mount_connection_data_for(&harness.server, &token).await;
    let source = harness.token_source().await;
    let meta = harness
        .connector
        .create_pat(&source, &harness.org(), AccessLevel::Review, 180)
        .await
        .unwrap();
    let bodies = create_bodies(&harness.requests("POST", PATS_PATH).await);
    assert_eq!(bodies.len(), 2);
    assert_near(
        valid_to_of(&bodies[0]),
        OffsetDateTime::now_utc() + time::Duration::days(180),
    );
    assert_near(
        valid_to_of(&bodies[1]),
        OffsetDateTime::now_utc() + time::Duration::days(30),
    );
    assert!(meta.lifetime_capped);
    assert_eq!(meta.lifetime_days, Some(30));
}

#[tokio::test]
async fn ac7_second_lifespan_rejection_is_shown_not_retried() {
    let harness = Harness::new().await;
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(pat_error("patLifespanPolicyViolation"))
        .mount(&harness.server)
        .await;
    let source = harness.token_source().await;
    let result = harness
        .connector
        .create_pat(&source, &harness.org(), AccessLevel::Review, 90)
        .await;
    assert_eq!(
        result,
        Err(OnboardError::PatRejected {
            code: "patLifespanPolicyViolation".into()
        })
    );
    assert_eq!(harness.requests("POST", PATS_PATH).await.len(), 2);
}

#[tokio::test]
async fn ac8_access_denied_switches_to_paste_without_more_creates() {
    let harness = Harness::new().await;
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(pat_error("accessDenied"))
        .mount(&harness.server)
        .await;
    let source = harness.token_source().await;
    let err = harness
        .connector
        .create_pat(&source, &harness.org(), AccessLevel::Review, 90)
        .await
        .unwrap_err();
    assert!(err.falls_back_to_paste(), "{err:?}");
    assert!(err.message().contains("accessDenied"), "{}", err.message());
    assert_eq!(harness.requests("POST", PATS_PATH).await.len(), 1);
    assert_eq!(harness.connector.account(), None);
    assert_eq!(harness.secrets.load(&harness.org()), Ok(None));
}

#[tokio::test]
async fn admin_consent_error_is_a_policy_block() {
    let harness = Harness::new().await;
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "message": "AADSTS65001: The user or administrator has not consented to use the application."
        })))
        .mount(&harness.server)
        .await;
    let source = harness.token_source().await;
    let err = harness
        .connector
        .create_pat(&source, &harness.org(), AccessLevel::Review, 90)
        .await
        .unwrap_err();
    assert!(err.falls_back_to_paste(), "{err:?}");
    assert_eq!(harness.requests("POST", PATS_PATH).await.len(), 1);
}

#[tokio::test]
async fn rejected_entra_token_falls_back_to_management_resource_then_reports_tenant() {
    let harness = Harness::new().await;
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(ResponseTemplate::new(401))
        .mount(&harness.server)
        .await;
    let source = harness.token_source().await;
    let err = harness
        .connector
        .create_pat(&source, &harness.org(), AccessLevel::ReadOnly, 90)
        .await
        .unwrap_err();
    assert_eq!(err, OnboardError::NotInTenant);
    assert_eq!(harness.requests("POST", PATS_PATH).await.len(), 2);
    assert!(
        harness
            .az
            .calls()
            .iter()
            .any(|call| call.contains("https://management.core.windows.net/"))
    );
}

async fn ac9_scenario(harness: &Harness, secrets: &dyn SecretStore) {
    let token = pat_value("PATSECRETac9");
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(pat_created(&token, "auth-9", 90))
        .mount(&harness.server)
        .await;
    mount_connection_data_for(&harness.server, &token).await;
    let source = harness.token_source().await;
    let meta = harness
        .connector
        .create_pat(&source, &harness.org(), AccessLevel::Review, 90)
        .await
        .unwrap();
    OnboardingState {
        step: Step::Projects,
        org: Some(ORG.into()),
        ..OnboardingState::default()
    }
    .save(harness.connector.data_dir())
    .unwrap();

    assert_eq!(
        secrets
            .load(&harness.org())
            .unwrap()
            .unwrap()
            .expose_secret(),
        token
    );
    let account: Value = serde_json::from_slice(
        &std::fs::read(harness.connector.data_dir().join("account.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(account["authorization_id"], "auth-9");
    assert_eq!(account["source"], "az");
    assert!(account["valid_to"].is_string());
    assert_eq!(account["scopes"].as_array().unwrap().len(), 5);
    assert_eq!(meta.source, PatSource::Az);

    let secrets_to_find = [
        token.as_bytes().to_vec(),
        ENTRA.as_bytes().to_vec(),
        basic(&token).into_bytes(),
    ];
    let files = files_under(harness.data.path());
    assert!(files.len() >= 2, "{files:?}");
    for file in files {
        let bytes = std::fs::read(&file).unwrap();
        for secret in &secrets_to_find {
            assert!(
                !contains(&bytes, secret),
                "secret found in {}",
                file.display()
            );
        }
    }
    let captured = logs().text();
    assert!(captured.contains("adoken_ado::http"), "debug logging is on");
    for secret in &secrets_to_find {
        assert!(
            !contains(captured.as_bytes(), secret),
            "secret found in logs"
        );
    }
}

#[tokio::test]
async fn ac9_token_only_in_keychain_never_in_files_or_logs() {
    let harness = Harness::new().await;
    ac9_scenario(&harness, harness.secrets.as_ref()).await;
}

/// AC-9 against the real macOS Keychain under a test-only service (0001 §6).
#[cfg(target_os = "macos")]
#[tokio::test]
async fn ac9_real_keychain_item_under_test_service() {
    let mut harness = Harness::new().await;
    let keychain = Arc::new(Keychain::with_service(format!(
        "dev.adoken.pat.test.ac9.{}",
        std::process::id()
    )));
    harness.connector = connector(
        &harness.server,
        &harness.az,
        harness.data.path(),
        keychain.clone(),
    );
    ac9_scenario(&harness, keychain.as_ref()).await;
    assert_eq!(keychain.delete(&harness.org()), Ok(true));
}

// ── Paste: AC-10 ───────────────────────────────────────────────────────────

async fn mount_probes(server: &MockServer, token: &str, build_status: u16, project_status: u16) {
    let auth = basic(token);
    mount_connection_data_for(server, token).await;
    Mock::given(method("GET"))
        .and(path("/dev/fabrikam/_apis/projects"))
        .and(query_param("$top", "1"))
        .and(header("authorization", auth.as_str()))
        .respond_with(ResponseTemplate::new(project_status).set_body_json(json!({
            "count": 1, "value": [{ "id": "proj-1", "name": "Payments" }]
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/dev/fabrikam/proj-1/_apis/git/repositories"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "count": 0, "value": [] })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/dev/fabrikam/_apis/git/repositories"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "count": 0, "value": [] })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/dev/fabrikam/proj-1/_apis/wit/wiql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "workItems": [] })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/dev/fabrikam/proj-1/_apis/build/builds"))
        .respond_with(ResponseTemplate::new(build_status).set_body_json(json!({
            "count": 0, "value": [],
            "message": "TF400813: The user is not authorized to access this resource."
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/vssps/fabrikam/_apis/identities"))
        .and(query_param("identityIds", "user-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "count": 1, "value": [] })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn ac10_missing_build_scope_is_listed_and_user_can_continue() {
    for build_status in [401, 403] {
        let harness = Harness::new().await;
        let token = pat_value("PASTEDac10");
        mount_probes(&harness.server, &token, build_status, 200).await;
        let result = harness
            .connector
            .verify_pasted(
                &harness.org(),
                Pat::parse(&token).unwrap(),
                AccessLevel::Review,
                None,
            )
            .await
            .unwrap();
        let missing: Vec<_> = result.missing().map(|scope| scope.title).collect();
        assert_eq!(missing, ["Build (Read)"], "status {build_status}");
        assert!(result.can_continue());
        let outcome = |id: &str| {
            result
                .probes
                .iter()
                .find(|probe| probe.scope.id == id)
                .unwrap()
                .outcome
                .clone()
        };
        assert_eq!(
            outcome("vso.code_write"),
            ProbeOutcome::GrantedWriteUntested
        );
        assert_eq!(outcome("vso.project"), ProbeOutcome::Granted);
        assert_eq!(outcome("vso.identity"), ProbeOutcome::Granted);
        // Review level: reads prove the work scope, the link write is
        // checked on first use (user decision 2026-09-30).
        assert_eq!(
            outcome("vso.work_write"),
            ProbeOutcome::GrantedWriteUntested
        );
        let meta = result.saved.unwrap();
        assert_eq!(meta.source, PatSource::Pasted);
        assert!(!meta.has_scope("vso.build"), "pipelines disabled");
        assert!(meta.has_scope("vso.work_write"));
        assert_eq!(result.me.display_name, "Ada Lovelace");
        assert_eq!(
            harness.secrets.load(&harness.org()).unwrap(),
            Some(Pat::parse(&token).unwrap())
        );
    }
}

#[tokio::test]
async fn paste_missing_required_scope_is_not_saved() {
    let harness = Harness::new().await;
    let token = pat_value("PASTEDnoproj");
    mount_probes(&harness.server, &token, 200, 401).await;
    let result = harness
        .connector
        .verify_pasted(
            &harness.org(),
            Pat::parse(&token).unwrap(),
            AccessLevel::ReadOnly,
            None,
        )
        .await
        .unwrap();
    assert!(!result.can_continue());
    let missing: Vec<_> = result.missing().map(|scope| scope.id).collect();
    assert_eq!(missing, ["vso.project"]);
    assert_eq!(harness.secrets.load(&harness.org()), Ok(None));
    assert_eq!(harness.connector.account(), None);
}

#[tokio::test]
async fn paste_invalid_token_is_rejected() {
    let harness = Harness::new().await;
    Mock::given(method("GET"))
        .and(path(CONNECTION_DATA_PATH))
        .respond_with(ResponseTemplate::new(401))
        .mount(&harness.server)
        .await;
    let result = harness
        .connector
        .verify_pasted(
            &harness.org(),
            Pat::parse(&pat_value("BAD")).unwrap(),
            AccessLevel::Review,
            None,
        )
        .await;
    assert_eq!(result, Err(OnboardError::InvalidToken));
}

// ── Replace token: AC-12, AC-21, background replacement ─────────────────────

/// The scopes a token minted before 2026-09-30 got (0001 AC-21).
const OLD_REVIEW_SCOPES: &str = "vso.project vso.code_write vso.work vso.build vso.identity";
const REVIEW_SCOPES: &str = "vso.build vso.code_write vso.identity vso.project vso.work_write";

fn pat_created_with(
    token: &str,
    authorization_id: &str,
    valid_to: OffsetDateTime,
    scope: &str,
) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "patToken": {
            "displayName": "adoken · test",
            "validTo": valid_to.format(&Rfc3339).unwrap(),
            "scope": scope,
            "targetAccounts": ["org-id"],
            "validFrom": "2026-09-25T10:00:00Z",
            "authorizationId": authorization_id,
            "token": token
        },
        "patTokenError": "none"
    }))
}

async fn mint_initial(harness: &Harness, token: &str, authorization_id: &str) {
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(pat_created(token, authorization_id, 90))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&harness.server)
        .await;
    mount_connection_data_for(&harness.server, token).await;
    let source = harness.token_source().await;
    harness
        .connector
        .create_pat(&source, &harness.org(), AccessLevel::Review, 90)
        .await
        .unwrap();
}

#[tokio::test]
async fn ac12_replace_creates_swaps_verifies_then_revokes_old_in_order() {
    let harness = Harness::new().await;
    let old = pat_value("OLDPAT");
    let new = pat_value("NEWPAT");
    mint_initial(&harness, &old, "auth-old").await;
    let new_valid_to = OffsetDateTime::now_utc() + time::Duration::days(90);
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(pat_created_with(
            &new,
            "auth-new",
            new_valid_to,
            REVIEW_SCOPES,
        ))
        .mount(&harness.server)
        .await;
    mount_connection_data_for(&harness.server, &new).await;
    Mock::given(method("DELETE"))
        .and(path(PATS_PATH))
        .respond_with(ResponseTemplate::new(204))
        .mount(&harness.server)
        .await;

    let before = OffsetDateTime::now_utc();
    let source = harness.token_source().await;
    let replaced = harness
        .connector
        .replace(&source, AccessLevel::Review, 90)
        .await
        .unwrap();
    assert_eq!(replaced.old_revoke_error, None);
    assert_eq!(replaced.meta.authorization_id.as_deref(), Some("auth-new"));
    assert_eq!(
        harness.secrets.load(&harness.org()).unwrap(),
        Some(Pat::parse(&new).unwrap())
    );
    // account.json: the new id, expiry, creation time and the response's
    // scopes.
    let stored = harness.relaunch().account().unwrap();
    assert_eq!(stored.authorization_id.as_deref(), Some("auth-new"));
    assert_eq!(
        stored.valid_to.map(OffsetDateTime::unix_timestamp),
        Some(new_valid_to.unix_timestamp())
    );
    assert!(stored.created_at >= before);
    assert_eq!(stored.scopes.join(" "), REVIEW_SCOPES);
    assert!(!stored.auto_replaced);

    let requests = harness.server.received_requests().await.unwrap();
    let new_basic = basic(&new);
    let sequence: Vec<String> = requests
        .iter()
        .filter_map(|request| {
            let p = request.url.path();
            if p == PATS_PATH && request.method.as_str() == "POST" {
                Some("create".into())
            } else if p == CONNECTION_DATA_PATH
                && request
                    .headers
                    .get("authorization")
                    .unwrap()
                    .to_str()
                    .unwrap()
                    == new_basic
            {
                Some("verify-new".into())
            } else if p == PATS_PATH && request.method.as_str() == "DELETE" {
                let id = request
                    .url
                    .query_pairs()
                    .find(|(key, _)| key == "authorizationId")
                    .unwrap()
                    .1
                    .into_owned();
                Some(format!("revoke:{id}"))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        sequence,
        ["create", "create", "verify-new", "revoke:auth-old"]
    );
}

#[tokio::test]
async fn ac12_failed_verify_keeps_old_token_and_revokes_new() {
    let harness = Harness::new().await;
    let old = pat_value("OLDPAT2");
    let new = pat_value("NEWPAT2");
    mint_initial(&harness, &old, "auth-old").await;
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(pat_created(&new, "auth-new", 90))
        .mount(&harness.server)
        .await;
    Mock::given(method("GET"))
        .and(path(CONNECTION_DATA_PATH))
        .and(header("authorization", basic(&new).as_str()))
        .respond_with(ResponseTemplate::new(401))
        .mount(&harness.server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(PATS_PATH))
        .respond_with(ResponseTemplate::new(204))
        .mount(&harness.server)
        .await;

    let source = harness.token_source().await;
    assert_eq!(
        harness
            .connector
            .replace(&source, AccessLevel::Review, 90)
            .await
            .unwrap_err(),
        OnboardError::InvalidToken
    );
    assert_eq!(
        harness.secrets.load(&harness.org()).unwrap(),
        Some(Pat::parse(&old).unwrap())
    );
    let revokes: Vec<String> = harness
        .requests("DELETE", PATS_PATH)
        .await
        .iter()
        .map(|request| request.url.query().unwrap().to_owned())
        .collect();
    assert_eq!(revokes.len(), 1);
    assert!(
        revokes[0].contains("authorizationId=auth-new"),
        "{revokes:?}"
    );
    let meta = harness.connector.account().unwrap();
    assert_eq!(meta.authorization_id.as_deref(), Some("auth-old"));
}

fn scope_of(request: &wiremock::Request) -> String {
    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
    body["scope"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn ac21_replace_upgrades_an_old_token_and_the_live_client_signs_with_it() {
    let harness = Harness::new().await;
    let old = pat_value("OLDSET");
    let new = pat_value("NEWSET");
    // A Review token minted before vso.work_write joined §5.2.
    mint_initial(&harness, &old, "auth-old").await;
    assert_eq!(
        harness.connector.account().unwrap().scopes.join(" "),
        OLD_REVIEW_SCOPES
    );
    assert!(
        !harness
            .connector
            .account()
            .unwrap()
            .has_scope("vso.work_write")
    );
    // Sync's client, rejected by a 401 before the replacement.
    let client = harness.connector.client().await.unwrap().unwrap();
    client.auth().on_unauthorized();

    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(pat_created_with(
            &new,
            "auth-new",
            OffsetDateTime::now_utc() + time::Duration::days(90),
            REVIEW_SCOPES,
        ))
        .mount(&harness.server)
        .await;
    mount_connection_data_for(&harness.server, &new).await;
    Mock::given(method("DELETE"))
        .and(path(PATS_PATH))
        .respond_with(ResponseTemplate::new(204))
        .mount(&harness.server)
        .await;
    let source = harness.token_source().await;
    let replaced = harness
        .connector
        .replace(&source, AccessLevel::Review, 90)
        .await
        .unwrap();

    let creates = harness.requests("POST", PATS_PATH).await;
    let mut requested: Vec<String> = scope_of(creates.last().unwrap())
        .split(' ')
        .map(str::to_owned)
        .collect();
    requested.sort();
    assert_eq!(requested.join(" "), REVIEW_SCOPES);
    assert!(replaced.meta.has_scope("vso.work_write"));
    assert!(
        harness
            .relaunch()
            .account()
            .unwrap()
            .has_scope("vso.work_write")
    );
    // No restart: the same client signs with the new token again.
    let mut request = reqwest::Request::new(
        reqwest::Method::GET,
        url::Url::parse("https://dev.azure.com/fabrikam/_apis/projects").unwrap(),
    );
    client.auth().authorize(&mut request).await.unwrap();
    assert_eq!(
        request.headers()["authorization"].to_str().unwrap(),
        basic(&new)
    );
}

#[tokio::test]
async fn background_replacement_runs_once_a_day_from_seven_days_out() {
    let harness = Harness::new().await;
    let old = pat_value("SOONOLD");
    let new = pat_value("SOONNEW");
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(pat_created(&old, "auth-old", 5))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&harness.server)
        .await;
    mount_connection_data_for(&harness.server, &old).await;
    let source = harness.token_source().await;
    let first = harness
        .connector
        .create_pat(&source, &harness.org(), AccessLevel::Review, 30)
        .await
        .unwrap();
    let valid_to = first.valid_to.unwrap();
    let now = OffsetDateTime::now_utc();
    // 8 days out: nothing happens, nothing is sent.
    let creates = harness.requests("POST", PATS_PATH).await.len();
    assert_eq!(
        harness
            .connector
            .auto_replace(valid_to - time::Duration::days(8))
            .await,
        AutoReplace::NotDue
    );
    assert_eq!(harness.requests("POST", PATS_PATH).await.len(), creates);

    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(pat_created_with(
            &new,
            "auth-new",
            now + time::Duration::days(30),
            REVIEW_SCOPES,
        ))
        .mount(&harness.server)
        .await;
    mount_connection_data_for(&harness.server, &new).await;
    Mock::given(method("DELETE"))
        .and(path(PATS_PATH))
        .respond_with(ResponseTemplate::new(204))
        .mount(&harness.server)
        .await;
    let AutoReplace::Replaced(replaced) = harness.connector.auto_replace(now).await else {
        panic!("expected a background replacement");
    };
    assert!(replaced.meta.auto_replaced);
    assert_eq!(replaced.meta.level, AccessLevel::Review);
    assert_eq!(replaced.meta.lifetime_days, Some(30), "same lifetime");
    assert_eq!(
        harness.secrets.load(&harness.org()).unwrap(),
        Some(Pat::parse(&new).unwrap())
    );
    // The new token is 30 days out: not due again.
    assert_eq!(
        harness.connector.auto_replace(now).await,
        AutoReplace::NotDue
    );
}

#[tokio::test]
async fn background_replacement_with_az_signed_out_warns_once_two_days_out() {
    let harness = Harness::new().await;
    let old = pat_value("LATEOLD");
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(pat_created(&old, "auth-old", 1))
        .mount(&harness.server)
        .await;
    mount_connection_data_for(&harness.server, &old).await;
    let source = harness.token_source().await;
    let first = harness
        .connector
        .create_pat(&source, &harness.org(), AccessLevel::Review, 90)
        .await
        .unwrap();
    let valid_to = first.valid_to.unwrap();
    harness.az.set("account_show", "", 1);
    let creates = harness.requests("POST", PATS_PATH).await.len();

    let four_days_out = valid_to - time::Duration::days(4);
    let AutoReplace::Failed { warn, .. } = harness.connector.auto_replace(four_days_out).await
    else {
        panic!("expected a failed attempt");
    };
    assert!(!warn, "nothing visible before 2 days out");
    // Same day again: not even tried.
    assert_eq!(
        harness
            .connector
            .auto_replace(four_days_out + time::Duration::hours(3))
            .await,
        AutoReplace::NotDue
    );
    let day_out = valid_to - time::Duration::days(1);
    let AutoReplace::Failed { warn, .. } = harness.connector.auto_replace(day_out).await else {
        panic!("expected a failed attempt");
    };
    assert!(warn, "one quiet item 2 days out");
    let AutoReplace::Failed { warn, .. } = harness
        .connector
        .auto_replace(day_out + time::Duration::days(1))
        .await
    else {
        panic!("expected a failed attempt");
    };
    assert!(!warn, "never repeated");
    // `az` never minted and never opened a browser.
    assert_eq!(harness.requests("POST", PATS_PATH).await.len(), creates);
    assert!(
        !harness
            .az
            .calls()
            .iter()
            .any(|call| call.starts_with("login"))
    );
}

#[tokio::test]
async fn refused_writes_mark_the_scope_missing_and_accepted_ones_prove_it() {
    let harness = Harness::new().await;
    let token = pat_value("PASTEWRITES");
    mount_connection_data_for(&harness.server, &token).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "count": 1, "value": [{ "id": "p1", "name": "Payments" }]
        })))
        .mount(&harness.server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "workItems": [] })))
        .mount(&harness.server)
        .await;
    let verification = harness
        .connector
        .verify_pasted(
            &harness.org(),
            Pat::parse(&token).unwrap(),
            AccessLevel::Review,
            None,
        )
        .await
        .unwrap();
    let meta = verification.saved.unwrap();
    assert_eq!(
        meta.scope_status("vso.code_write"),
        ScopeStatus::NotCheckedYet
    );
    assert_eq!(
        meta.scope_status("vso.work_write"),
        ScopeStatus::NotCheckedYet
    );

    // AC-23: the first vote gets a scope 403.
    let meta = harness
        .connector
        .record_write("vso.code_write", true)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(meta.scope_status("vso.code_write"), ScopeStatus::Missing);
    // A work item edit went through.
    harness
        .connector
        .record_write("vso.work_write", false)
        .await
        .unwrap();
    assert_eq!(
        harness
            .connector
            .record_write("vso.work_write", false)
            .await,
        Ok(None),
        "already proven: no file write"
    );
    let relaunched = harness.relaunch().account().unwrap();
    assert_eq!(
        relaunched.scope_status("vso.code_write"),
        ScopeStatus::Missing
    );
    assert_eq!(
        relaunched.scope_status("vso.work_write"),
        ScopeStatus::Granted
    );
}

// ── 401 breaker: AC-13 ─────────────────────────────────────────────────────

#[tokio::test]
async fn ac13_after_a_401_no_more_requests_reach_the_org() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let endpoints = Endpoints::under(&url::Url::parse(&server.uri()).unwrap());
    let org = Organization::parse(ORG).unwrap();
    let http = Http::new();
    let auth = PatAuth::new(Pat::parse(&pat_value("DEAD")).unwrap(), None);
    let url = endpoints.org(&org).join("_apis/projects").unwrap();
    assert!(matches!(
        http.send(http.get(url.clone()), &auth).await,
        Err(HttpError::Unauthorized { .. })
    ));
    for _ in 0..20 {
        assert_eq!(
            http.send(http.get(url.clone()), &auth).await.unwrap_err(),
            HttpError::Auth(AuthError::Rejected)
        );
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    assert!(auth.is_rejected());
}

#[tokio::test]
async fn retry_after_is_honored_without_hammering() {
    let harness = Harness::new().await;
    Mock::given(method("GET"))
        .and(path("/dev/fabrikam/_apis/projects"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "20"))
        .mount(&harness.server)
        .await;
    let source = harness.token_source().await;
    for _ in 0..3 {
        match harness
            .connector
            .list_projects(&harness.org(), Some(&source))
            .await
        {
            Err(OnboardError::Throttled { retry_after }) => {
                assert!(
                    retry_after > Duration::from_secs(15) && retry_after <= Duration::from_secs(20)
                );
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(
        harness
            .requests("GET", "/dev/fabrikam/_apis/projects")
            .await
            .len(),
        1
    );
}

// ── Resume: AC-14 ──────────────────────────────────────────────────────────

#[tokio::test]
async fn ac14_quit_between_mint_and_verify_resumes_without_reminting() {
    let harness = Harness::new().await;
    let token = pat_value("PATSECRETac14");
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(pat_created(&token, "auth-14", 90))
        .mount(&harness.server)
        .await;
    // Verification can't complete before "quit".
    Mock::given(method("GET"))
        .and(path(CONNECTION_DATA_PATH))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&harness.server)
        .await;
    mount_connection_data_for(&harness.server, &token).await;
    let source = harness.token_source().await;
    assert!(
        harness
            .connector
            .create_pat(&source, &harness.org(), AccessLevel::Review, 90)
            .await
            .is_err()
    );
    let pending = harness.connector.account().unwrap();
    assert!(!pending.verified);

    // Relaunch: resume verifies; a repeated create verifies too.
    let relaunched = harness.relaunch();
    let resumed = relaunched.resume_pending().await.unwrap().unwrap();
    assert!(resumed.verified);
    assert_eq!(resumed.authorization_id.as_deref(), Some("auth-14"));
    assert_eq!(relaunched.resume_pending().await, Ok(None));
    assert_eq!(harness.requests("POST", PATS_PATH).await.len(), 1);
}

#[tokio::test]
async fn ac14_create_after_relaunch_verifies_pending_instead_of_minting() {
    let harness = Harness::new().await;
    let token = pat_value("PATSECRETac14b");
    Mock::given(method("POST"))
        .and(path(PATS_PATH))
        .respond_with(pat_created(&token, "auth-14b", 90))
        .mount(&harness.server)
        .await;
    Mock::given(method("GET"))
        .and(path(CONNECTION_DATA_PATH))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&harness.server)
        .await;
    mount_connection_data_for(&harness.server, &token).await;
    let source = harness.token_source().await;
    let _ = harness
        .connector
        .create_pat(&source, &harness.org(), AccessLevel::Review, 90)
        .await;
    let meta = harness
        .relaunch()
        .create_pat(&source, &harness.org(), AccessLevel::Review, 90)
        .await
        .unwrap();
    assert!(meta.verified);
    assert_eq!(harness.requests("POST", PATS_PATH).await.len(), 1);
}

// ── Sign out: AC-15 ────────────────────────────────────────────────────────

#[tokio::test]
async fn ac15_sign_out_revokes_once_and_deletes_everything() {
    let harness = Harness::new().await;
    mint_initial(&harness, &pat_value("PATSECRETac15"), "auth-15").await;
    Mock::given(method("DELETE"))
        .and(path(PATS_PATH))
        .and(query_param("authorizationId", "auth-15"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&harness.server)
        .await;
    let data_dir = harness.connector.data_dir().to_path_buf();
    OnboardingState {
        step: Step::Done,
        org: Some(ORG.into()),
        ..OnboardingState::default()
    }
    .save(&data_dir)
    .unwrap();
    let synced = data_dir.join("orgs/org-id/adoken.sqlite");
    std::fs::create_dir_all(synced.parent().unwrap()).unwrap();
    std::fs::write(&synced, b"db").unwrap();

    let az = harness.connector.detect_az().await.az().unwrap().clone();
    let outcome = harness
        .connector
        .sign_out(
            SignOut {
                revoke: true,
                delete_local_data: true,
            },
            Some(&az),
        )
        .await
        .unwrap();
    assert_eq!(outcome.revoke_error, None);
    assert_eq!(harness.secrets.load(&harness.org()), Ok(None));
    assert!(!data_dir.join("orgs").exists());
    assert_eq!(harness.connector.account(), None);
    // Relaunch shows Welcome.
    assert_eq!(OnboardingState::load(&data_dir), None);
    assert_eq!(
        OnboardingState::load(&data_dir).unwrap_or_default().step,
        Step::Welcome
    );
    assert_eq!(harness.requests("DELETE", PATS_PATH).await.len(), 1);
}
