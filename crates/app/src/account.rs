//! The connected token after onboarding (spec 0001 FR-14–FR-18, spec 0009
//! FR-8/FR-9, owner decision 2026-10-01): what Settings › Account shows,
//! Replace token (one click with `az`, else the onboarding sheet), the
//! single quiet rejected/expired state, the background replacement of `az`
//! tokens, and the refused-write record (FR-17).
//!
//! `account.json` is read once at launch and kept here, so Settings ›
//! Account opens without IO or requests. Every token operation runs on the
//! Tokio runtime through the one [`Connector`] the app launched with, whose
//! live auth provider takes a verified replacement in place: sync resumes
//! on its next pass without a restart.
//!
//! Nothing here prompts: the only token actions are Settings › Account,
//! `⌘P` › Replace token…, the Activity Sync tab's rejected state and the
//! one quiet "couldn't replace" inbox item.

use std::time::Duration;

use adoken_ado::{
    AccessLevel, AutoReplace, AzStatus, Connector, OnboardError, Organization, PatMeta, PatSource,
    Replaced,
};
use adoken_model::SyncEventKind;
use adoken_sync::Phase;
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Global, Task, actions};
use gpui_tokio::Tokio;
use time::OffsetDateTime;
use tokio::sync::watch;

use crate::palette::PaletteCommand;
use crate::sync::SyncState;

actions!(
    account,
    [
        /// Replace adoken's Azure DevOps token (Settings › Account).
        ReplaceToken,
    ]
);

/// First background replacement check after launch (keeps cold start and
/// the first sync pass free of `az` subprocesses).
const AUTO_FIRST_CHECK: Duration = Duration::from_secs(60);
/// How often the background replacement asks whether it's due; the
/// once-a-day limit itself lives in `account.json`.
const AUTO_CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

/// `⌘P` › Replace token… (spec 0001 FR-16, 0009 FR-9). Connected mode only.
pub fn commands() -> Vec<PaletteCommand> {
    vec![PaletteCommand {
        title: "Replace token…",
        keys: None,
        action: Box::new(ReplaceToken),
    }]
}

pub fn init(cx: &mut App) {
    cx.on_action(|_: &ReplaceToken, cx| open_and_replace(cx));
}

/// Settings › Account, then Replace token (spec 0009 FR-9: `⌘P` › Replace
/// token… opens Settings on Account and starts the same action).
pub fn open_and_replace(cx: &mut App) {
    if model(cx).is_none() {
        return;
    }
    crate::settings::open(crate::settings::Tab::Account, cx);
    replace_token(cx);
}

/// Why sync stopped for the token (spec 0001 FR-15). Never a missing
/// scope: a work item or avatar 401 is not a rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenProblem {
    Rejected,
    /// Rejected, and `valid_to` is known and past.
    Expired(time::Date),
}

impl TokenProblem {
    /// `rejected`: the main transport latched a 401.
    pub fn of(meta: Option<&PatMeta>, rejected: bool, now: OffsetDateTime) -> Option<Self> {
        if !rejected {
            return None;
        }
        Some(
            match meta
                .filter(|meta| meta.expired(now))
                .and_then(|meta| meta.valid_to)
            {
                Some(valid_to) => Self::Expired(valid_to.date()),
                None => Self::Rejected,
            },
        )
    }

    /// The statusbar's sync item (spec 0016, danger tone).
    pub fn status_label(self) -> &'static str {
        match self {
            Self::Rejected => "Token rejected",
            Self::Expired(_) => "Token expired",
        }
    }

    /// The Activity Sync tab's empty state headline (design §10.3).
    pub fn headline(self) -> String {
        match self {
            Self::Rejected => "Azure DevOps rejected adoken's token".into(),
            Self::Expired(date) => format!("adoken's token expired on {}", date_text(date)),
        }
    }

    /// The inline cause of a write attempted meanwhile (nothing was sent).
    pub fn write_text(self) -> String {
        format!("{}. Replace it in Settings › Account.", self.headline())
    }

    /// Settings › Account's token subline (design §10.1).
    pub fn account_line(self) -> String {
        match self {
            Self::Rejected => "Rejected by Azure DevOps · sync stopped".into(),
            Self::Expired(date) => format!("Expired on {} · sync stopped", date_text(date)),
        }
    }

    /// The one inbox item per episode (spec 0001 Q-6), P1, no banner, no
    /// sound.
    pub fn inbox_summary(self) -> String {
        format!("{} · sync stopped", self.headline())
    }
}

/// "Sep 25, 2026" (design §10.1: medium style).
pub fn date_text(date: time::Date) -> String {
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][usize::from(u8::from(date.month())) - 1];
    format!("{month} {}, {}", date.day(), date.year())
}

/// What the token lets adoken do (spec 0004 AC-11, 0015 FR-28, 0001 FR-9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grants {
    /// Review actions: vote, comment, resolve, complete.
    pub can_write: bool,
    /// Edit work items and link them to pull requests.
    pub link_work_items: bool,
}

pub fn grants(meta: &PatMeta) -> Grants {
    // An unchecked scope list (very old `account.json`) counts as granted:
    // a refusal then records the scope (FR-17).
    let unknown = meta.scopes.is_empty();
    Grants {
        can_write: meta.level == AccessLevel::Review
            && (unknown || meta.has_scope(adoken_sync::writes::WRITE_SCOPE)),
        link_work_items: unknown || meta.has_scope(adoken_sync::writes::WORK_WRITE_SCOPE),
    }
}

/// Replace token… in Settings › Account (spec 0009 FR-9).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ReplaceState {
    #[default]
    Idle,
    /// The button reads "Replacing…".
    Replacing,
    /// Stays until the tab closes. `token_page`: a pasted replacement's
    /// **Open token page** (adoken can't revoke a pasted token).
    Done {
        note: String,
        token_page: Option<String>,
    },
    /// The cause, inline; the old token stays in place.
    Failed(String),
}

/// The note after a successful `az` replacement (spec 0009 FR-9).
pub fn replaced_note(replaced: &Replaced) -> String {
    match (&replaced.old_revoke_error, replaced.old_valid_to) {
        (None, _) => "New token created. The old one was revoked.".into(),
        (Some(_), Some(valid_to)) => format!(
            "New token created. adoken couldn't revoke the old one; it expires on {}.",
            date_text(valid_to.date())
        ),
        (Some(_), None) => "New token created. adoken couldn't revoke the old one.".into(),
    }
}

/// The note after a pasted replacement (design §10.2).
pub const PASTED_NOTE: &str = "New token saved. adoken can't revoke the old one for you.";

/// What Settings › Account renders (no IO).
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub meta: PatMeta,
    pub problem: Option<TokenProblem>,
    pub replace: ReplaceState,
}

pub enum AccountEvent {
    /// Show the onboarding sheet over the app (spec 0001 FR-16, design
    /// §10.2): org fixed, `level` preset.
    OpenSheet {
        org: Organization,
        level: AccessLevel,
    },
}

/// The connected account (connected mode only; demo has none).
pub struct Account {
    connector: Connector,
    meta: Option<PatMeta>,
    /// Sync latched a 401 on the main transport (FR-15).
    rejected: bool,
    replace: ReplaceState,
    job: Option<Task<()>>,
    _sync: Option<Task<()>>,
    _auto: Option<Task<()>>,
}

impl EventEmitter<AccountEvent> for Account {}

struct Link(Entity<Account>);

impl Global for Link {}

/// The launch connector and the `account.json` it read; starts the
/// background replacement.
pub fn install(connector: Connector, meta: Option<PatMeta>, cx: &mut App) -> Entity<Account> {
    let entity = cx.new(|cx| {
        let mut account = Account {
            connector,
            meta,
            rejected: false,
            replace: ReplaceState::Idle,
            job: None,
            _sync: None,
            _auto: None,
        };
        account.schedule_auto_replace(cx);
        account
    });
    cx.set_global(Link(entity.clone()));
    entity
}

pub fn model(cx: &App) -> Option<Entity<Account>> {
    cx.try_global::<Link>().map(|link| link.0.clone())
}

pub fn snapshot(cx: &App) -> Option<Snapshot> {
    let account = model(cx)?;
    let account = account.read(cx);
    Some(Snapshot {
        meta: account.meta.clone()?,
        problem: account.problem(),
        replace: account.replace.clone(),
    })
}

/// Why sync stopped for the token, if it did.
pub fn token_problem(cx: &App) -> Option<TokenProblem> {
    model(cx).and_then(|account| account.read(cx).problem())
}

/// The inline cause of a write refused because the token was rejected:
/// the expired wording when `valid_to` is past (FR-15).
pub fn rejected_write_text(fallback: String, cx: &App) -> String {
    token_problem(cx).map_or(fallback, TokenProblem::write_text)
}

/// Start Replace token: one click for an `az` token while `az` is signed
/// in, else the onboarding sheet.
pub fn replace_token(cx: &mut App) {
    if let Some(account) = model(cx) {
        account.update(cx, |account, cx| account.start_replace(cx));
    }
}

/// Change access…: the sheet with the other level preselected.
pub fn change_access(cx: &mut App) {
    if let Some(account) = model(cx) {
        account.update(cx, |account, cx| {
            if let Some(meta) = &account.meta
                && let Some(org) = meta.organization()
            {
                account.replace = ReplaceState::Idle;
                cx.emit(AccountEvent::OpenSheet {
                    org,
                    level: meta.level.other(),
                });
                cx.notify();
            }
        });
    }
}

/// The sheet saved a replacement: `replaced` from the `az` path, else a
/// verified pasted token.
pub fn sheet_replaced(meta: PatMeta, replaced: Option<Replaced>, cx: &mut App) {
    let Some(account) = model(cx) else {
        return;
    };
    account.update(cx, |account, cx| {
        let done = match &replaced {
            Some(replaced) => ReplaceState::Done {
                note: replaced_note(replaced),
                token_page: None,
            },
            None => ReplaceState::Done {
                note: PASTED_NOTE.into(),
                token_page: meta
                    .organization()
                    .map(|org| adoken_ado::token_page_url(&org).to_string()),
            },
        };
        account.applied(meta, cx);
        account.replace = done;
        cx.notify();
    });
}

/// A write's answer about `scope` (spec 0001 FR-17): a refusal for the
/// scope records it missing; the first accepted write proves an untested
/// one. Nothing happens (no IO) when it changes nothing.
pub fn record_write(scope: &'static str, refused: bool, cx: &mut App) {
    let Some(account) = model(cx) else {
        return;
    };
    let changes = account.read(cx).meta.as_ref().is_some_and(|meta| {
        if refused {
            meta.has_scope(scope)
        } else {
            meta.untested.iter().any(|id| id == scope)
        }
    });
    if !changes {
        return;
    }
    account.update(cx, |account, cx| {
        let connector = account.connector.clone();
        let job = Tokio::spawn(
            cx,
            async move { connector.record_write(scope, refused).await },
        );
        cx.spawn(async move |this, cx| match job.await {
            Ok(Ok(Some(meta))) => {
                this.update(cx, |account, cx| {
                    account.meta = Some(meta);
                    cx.notify();
                })
                .ok();
            }
            Ok(Ok(None)) => {}
            Ok(Err(error)) => eprintln!("adoken: couldn't record {scope}: {error}"),
            Err(error) => eprintln!("adoken: couldn't record {scope}: {error}"),
        })
        .detach();
    });
}

/// Follow sync's state: a 401 on the main transport is the one quiet
/// rejected state (FR-15) and one inbox item per episode (Q-6).
pub fn watch_sync(mut state: watch::Receiver<SyncState>, cx: &mut App) {
    let Some(account) = model(cx) else {
        return;
    };
    let task = cx.spawn({
        let account = account.downgrade();
        async move |cx| {
            loop {
                let rejected = matches!(
                    &*state.borrow_and_update(),
                    SyncState::Running(status) if status.phase == Phase::Unauthorized
                );
                let alive = account.update(cx, |account, cx| account.set_rejected(rejected, cx));
                if alive.is_err() || state.changed().await.is_err() {
                    return;
                }
            }
        }
    });
    account.update(cx, |account, _| account._sync = Some(task));
}

impl Account {
    pub fn meta(&self) -> Option<&PatMeta> {
        self.meta.as_ref()
    }

    pub fn grants(&self) -> Option<Grants> {
        self.meta.as_ref().map(grants)
    }

    fn problem(&self) -> Option<TokenProblem> {
        TokenProblem::of(self.meta.as_ref(), self.rejected, OffsetDateTime::now_utc())
    }

    fn set_rejected(&mut self, rejected: bool, cx: &mut Context<Self>) {
        if rejected == self.rejected {
            return;
        }
        self.rejected = rejected;
        if let Some(problem) = self.problem() {
            let episode = self.episode_key();
            crate::inbox::post_account_notice(
                SyncEventKind::TokenRejected,
                &episode,
                problem.inbox_summary(),
                cx,
            );
        }
        cx.notify();
    }

    /// One rejection per token.
    fn episode_key(&self) -> String {
        self.meta.as_ref().map_or_else(String::new, |meta| {
            meta.authorization_id
                .clone()
                .unwrap_or_else(|| meta.created_at.unix_timestamp().to_string())
        })
    }

    fn start_replace(&mut self, cx: &mut Context<Self>) {
        if self.replace == ReplaceState::Replacing {
            return;
        }
        let Some(meta) = self.meta.clone() else {
            return;
        };
        let Some(org) = meta.organization() else {
            return;
        };
        if meta.source != PatSource::Az {
            cx.emit(AccountEvent::OpenSheet {
                org,
                level: meta.level,
            });
            return;
        }
        self.replace = ReplaceState::Replacing;
        cx.notify();
        let connector = self.connector.clone();
        let job = Tokio::spawn(cx, async move {
            let az = match connector.detect_az().await {
                AzStatus::Ready { az, .. } => az,
                // Signed out, missing or too old: the sheet signs in or
                // pastes instead.
                _ => return Ok(None),
            };
            let source = connector.token_source(&az).await?;
            let (level, lifetime_days) = Connector::replace_defaults(&meta);
            connector
                .replace(&source, level, lifetime_days)
                .await
                .map(Some)
        });
        self.job = Some(cx.spawn(async move |this, cx| {
            let result = job
                .await
                .unwrap_or_else(|error| Err(OnboardError::Other(error.to_string())));
            this.update(cx, |account, cx| {
                match result {
                    Ok(Some(replaced)) => {
                        let note = replaced_note(&replaced);
                        account.applied(replaced.meta, cx);
                        account.replace = ReplaceState::Done {
                            note,
                            token_page: None,
                        };
                    }
                    Ok(None) => {
                        account.replace = ReplaceState::Idle;
                        cx.emit(AccountEvent::OpenSheet {
                            org,
                            level: account
                                .meta
                                .as_ref()
                                .map_or_else(AccessLevel::default, |m| m.level),
                        });
                    }
                    Err(error) => account.replace = ReplaceState::Failed(error.message()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// A replacement verified: new metadata, rejection cleared, sync
    /// resumes, and the inbox's token items are done (Q-6).
    fn applied(&mut self, meta: PatMeta, cx: &mut Context<Self>) {
        self.meta = Some(meta);
        self.rejected = false;
        crate::sync::token_replaced(cx);
        crate::inbox::clear_account_notices(cx);
    }

    fn schedule_auto_replace(&mut self, cx: &mut Context<Self>) {
        self._auto = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTO_FIRST_CHECK).await;
            loop {
                let due = this.read_with(cx, |account, _| {
                    account
                        .meta
                        .as_ref()
                        .is_some_and(|meta| meta.auto_replace_due(OffsetDateTime::now_utc()))
                });
                match due {
                    Err(_) => return,
                    Ok(true) => {
                        let Ok(job) = this.update(cx, |account, cx| {
                            let connector = account.connector.clone();
                            Tokio::spawn(cx, async move {
                                let outcome =
                                    connector.auto_replace(OffsetDateTime::now_utc()).await;
                                (outcome, connector.account())
                            })
                        }) else {
                            return;
                        };
                        if let Ok((outcome, meta)) = job.await {
                            let alive = this
                                .update(cx, |account, cx| account.auto_replaced(outcome, meta, cx));
                            if alive.is_err() {
                                return;
                            }
                        }
                    }
                    Ok(false) => {}
                }
                cx.background_executor().timer(AUTO_CHECK_EVERY).await;
            }
        }));
    }

    /// Tell the user once and quietly (owner decision 2026-10-01).
    fn auto_replaced(
        &mut self,
        outcome: AutoReplace,
        stored: Option<PatMeta>,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            AutoReplace::Replaced(replaced) => {
                let key = replaced
                    .meta
                    .authorization_id
                    .clone()
                    .unwrap_or_else(|| replaced.meta.created_at.unix_timestamp().to_string());
                let summary = auto_replaced_summary(&replaced.meta);
                self.applied(replaced.meta, cx);
                crate::inbox::post_account_notice(SyncEventKind::TokenReplaced, &key, summary, cx);
            }
            AutoReplace::Failed {
                warn: true,
                valid_to,
                ..
            } => {
                let key = self.episode_key();
                crate::inbox::post_account_notice(
                    SyncEventKind::TokenNotReplaced,
                    &key,
                    not_replaced_summary(valid_to),
                    cx,
                );
                self.meta = stored.or(self.meta.take());
            }
            AutoReplace::Failed { .. } | AutoReplace::NotDue => {
                self.meta = stored.or(self.meta.take());
            }
        }
        cx.notify();
    }
}

/// "adoken replaced its Azure DevOps token · the new one expires {date}".
pub fn auto_replaced_summary(meta: &PatMeta) -> String {
    match meta.valid_to {
        Some(valid_to) => format!(
            "adoken replaced its Azure DevOps token · the new one expires {}",
            date_text(valid_to.date())
        ),
        None => "adoken replaced its Azure DevOps token".into(),
    }
}

/// "adoken couldn't replace its token automatically · it expires {date}".
pub fn not_replaced_summary(valid_to: Option<OffsetDateTime>) -> String {
    match valid_to {
        Some(valid_to) => format!(
            "adoken couldn't replace its token automatically · it expires {}",
            date_text(valid_to.date())
        ),
        None => "adoken couldn't replace its token automatically".into(),
    }
}

/// Settings › Account's token subline (design §10.1), when sync hasn't
/// stopped for it.
pub fn token_line(meta: &PatMeta) -> String {
    let created = date_text(meta.created_at.date());
    let made = match (meta.source, meta.auto_replaced) {
        (PatSource::Az, true) => format!("Replaced automatically on {created}"),
        (PatSource::Az, false) => format!("Created with Azure CLI on {created}"),
        (PatSource::Pasted, _) => format!("Pasted on {created}"),
    };
    let expiry = match meta.valid_to {
        Some(valid_to) => format!("Expires {}", date_text(valid_to.date())),
        None => "No expiry date recorded".into(),
    };
    format!("{made} · {expiry}")
}

/// "This token is missing {n} permission(s) adoken uses: {consequences}."
/// (design §10.1), when any is missing.
pub fn missing_line(meta: &PatMeta) -> Option<String> {
    let missing = meta.missing();
    if missing.is_empty() {
        return None;
    }
    let consequences: Vec<&str> = missing
        .iter()
        .map(|scope| {
            scope
                .if_missing
                .unwrap_or("adoken can't work without it.")
                .trim_end_matches('.')
        })
        .collect();
    let n = missing.len();
    Some(format!(
        "This token is missing {n} permission{} adoken uses: {}.",
        if n == 1 { "" } else { "s" },
        consequences.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_ado::{AutoReplaceLog, ScopeStatus};
    use time::macros::datetime;

    fn meta(source: PatSource) -> PatMeta {
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
            valid_to: Some(datetime!(2026-12-24 10:00 UTC)),
            created_at: datetime!(2026-09-25 10:00 UTC),
            source,
            lifetime_days: Some(90),
            lifetime_capped: false,
            verified: true,
            me: None,
            auto_replaced: false,
            auto_replace: AutoReplaceLog::default(),
        }
    }

    #[test]
    fn a_401_reads_rejected_until_valid_to_has_passed() {
        let meta = meta(PatSource::Az);
        let before = datetime!(2026-12-01 10:00 UTC);
        let after = datetime!(2026-12-25 10:00 UTC);
        assert_eq!(TokenProblem::of(Some(&meta), false, after), None);
        let rejected = TokenProblem::of(Some(&meta), true, before).unwrap();
        assert_eq!(rejected.status_label(), "Token rejected");
        assert_eq!(
            rejected.write_text(),
            "Azure DevOps rejected adoken's token. Replace it in Settings › Account."
        );
        let expired = TokenProblem::of(Some(&meta), true, after).unwrap();
        assert_eq!(expired.status_label(), "Token expired");
        assert_eq!(
            expired.write_text(),
            "adoken's token expired on Dec 24, 2026. Replace it in Settings › Account."
        );
        assert_eq!(
            expired.account_line(),
            "Expired on Dec 24, 2026 · sync stopped"
        );
        // A pasted token without a date can only read "rejected".
        let mut pasted = super::tests::meta(PatSource::Pasted);
        pasted.valid_to = None;
        assert_eq!(
            TokenProblem::of(Some(&pasted), true, after),
            Some(TokenProblem::Rejected)
        );
    }

    /// Spec 0009 AC-14 and AC-15's token lines.
    #[test]
    fn token_line_names_how_it_was_made_and_its_expiry() {
        let az = meta(PatSource::Az);
        assert_eq!(
            token_line(&az),
            "Created with Azure CLI on Sep 25, 2026 · Expires Dec 24, 2026"
        );
        let mut pasted = meta(PatSource::Pasted);
        pasted.valid_to = None;
        assert_eq!(
            token_line(&pasted),
            "Pasted on Sep 25, 2026 · No expiry date recorded"
        );
        let mut auto = meta(PatSource::Az);
        auto.auto_replaced = true;
        assert!(token_line(&auto).starts_with("Replaced automatically on Sep 25, 2026"));
    }

    /// Spec 0009 AC-15: build missing, writes untested → one summary line
    /// naming pipelines.
    #[test]
    fn missing_line_lists_consequences_only_when_something_is_missing() {
        let mut meta = meta(PatSource::Pasted);
        assert_eq!(missing_line(&meta), None);
        meta.scopes.retain(|id| id != "vso.build");
        meta.untested = vec!["vso.code_write".into(), "vso.work_write".into()];
        assert_eq!(
            missing_line(&meta).as_deref(),
            Some("This token is missing 1 permission adoken uses: Pipelines won't sync.")
        );
        assert_eq!(
            meta.scope_status("vso.code_write"),
            ScopeStatus::NotCheckedYet
        );
    }

    #[test]
    fn grants_follow_level_and_write_scopes() {
        let review = meta(PatSource::Az);
        assert_eq!(
            grants(&review),
            Grants {
                can_write: true,
                link_work_items: true
            }
        );
        // A pre-2026-09-30 token: Code writes, work items don't.
        let mut old = review.clone();
        old.scopes = ["vso.project", "vso.code_write", "vso.work", "vso.build"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            grants(&old),
            Grants {
                can_write: true,
                link_work_items: false
            }
        );
        let mut read_only = review;
        read_only.level = AccessLevel::ReadOnly;
        assert!(!grants(&read_only).can_write);
    }

    #[test]
    fn replacement_notes_say_whether_the_old_token_was_revoked() {
        let mut replaced = Replaced {
            meta: meta(PatSource::Az),
            old_revoke_error: None,
            old_valid_to: Some(datetime!(2026-10-04 10:00 UTC)),
        };
        assert_eq!(
            replaced_note(&replaced),
            "New token created. The old one was revoked."
        );
        replaced.old_revoke_error = Some(OnboardError::Offline);
        assert_eq!(
            replaced_note(&replaced),
            "New token created. adoken couldn't revoke the old one; it expires on Oct 4, 2026."
        );
        assert_eq!(
            auto_replaced_summary(&replaced.meta),
            "adoken replaced its Azure DevOps token · the new one expires Dec 24, 2026"
        );
        assert_eq!(
            not_replaced_summary(Some(datetime!(2026-10-04 10:00 UTC))),
            "adoken couldn't replace its token automatically · it expires Oct 4, 2026"
        );
    }
}
