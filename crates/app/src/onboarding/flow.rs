//! The onboarding state machine (docs/specs/0001-onboarding.md §3,
//! docs/design/onboarding.md §1–§3). Pure: no gpui, no IO. User input and
//! backend results come in as method calls; everything that needs IO goes
//! out as [`Effect`]s the view runs on background executors, feeding the
//! results back in. Secrets never pass through here: the pasted token stays
//! in the masked input and goes straight to the backend.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use adoken_ado::{
    AccessLevel, AzStatus, ConnectPath, DeviceCodePrompt, OnboardError, OnboardingState, OrgList,
    OrgRef, Organization, PasteVerification, PatMeta, ProbeOutcome, ProjectRef, Scope, Step,
};

/// Screens in flow order (design §1). Sync and Make-it-yours land with
/// spec 0002 and the preferences work; until then Projects hands off to Done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    Welcome,
    Connect,
    SignIn,
    PickOrg,
    ReviewAccess,
    CreateInBrowser,
    PasteVerify,
    Projects,
    Done,
}

/// Step indicator labels (design §2); screen 0 has no indicator.
pub const INDICATOR: [&str; 3] = ["Connect", "Token", "Projects"];
/// The Replace token sheet's indicator (design §10.2).
pub const SHEET_INDICATOR: [&str; 2] = ["Connect", "Token"];

impl Screen {
    /// Active indicator dot; `Done` shows every dot done.
    pub fn indicator(self) -> Option<usize> {
        match self {
            Self::Welcome => None,
            Self::Connect | Self::SignIn => Some(0),
            Self::PickOrg | Self::ReviewAccess | Self::CreateInBrowser | Self::PasteVerify => {
                Some(1)
            }
            Self::Projects => Some(2),
            Self::Done => Some(INDICATOR.len()),
        }
    }

    pub fn step(self) -> Step {
        match self {
            Self::Welcome => Step::Welcome,
            Self::Connect => Step::Connect,
            Self::SignIn => Step::AzSignIn,
            Self::PickOrg => Step::PickOrg,
            Self::ReviewAccess => Step::ReviewAccess,
            Self::CreateInBrowser => Step::CreateInBrowser,
            Self::PasteVerify => Step::PasteVerify,
            Self::Projects => Step::Projects,
            Self::Done => Step::Done,
        }
    }
}

/// Token lifetimes offered (spec 0001 Q-2, Q-8), default 90.
pub const LIFETIMES: [u32; 4] = [30, 90, 180, 365];
pub const DEFAULT_LIFETIME: u32 = adoken_ado::DEFAULT_LIFETIME_DAYS;
/// Show the org filter above this many orgs (design 2a).
pub const ORG_FILTER_THRESHOLD: usize = 6;
/// Preselect every project up to this many; beyond it, none (design 3).
pub const PROJECT_PRESELECT_MAX: usize = 8;
/// Pause on a success check before auto-advancing (design 1a, 2b).
pub const ADVANCE_DELAY: Duration = Duration::from_millis(400);
/// `az login` gives up after this (design 1a).
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// The Azure CLI card shows nothing for this long, then a spinner (design 1).
pub const DETECTION_QUIET: Duration = Duration::from_millis(150);

/// Work for the view to run off the UI thread, then report back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Run az detection, then call [`Flow::detected`].
    Detect,
    /// `az login` (browser or device code), then [`Flow::signed_in`].
    SignIn {
        device_code: bool,
    },
    /// Kill the running `az login`.
    CancelSignIn,
    /// Entra token (re-detecting az if needed) + orgs, then
    /// [`Flow::orgs_loaded`].
    LoadOrgs,
    /// Mint + store + verify, then [`Flow::pat_created`].
    CreatePat {
        org: Organization,
        level: AccessLevel,
        lifetime_days: u32,
    },
    /// Verify the token in the paste field, then [`Flow::verified`].
    Verify {
        org: Organization,
        level: AccessLevel,
    },
    /// Clear the paste field (after the token was saved).
    ForgetPastedToken,
    /// List projects with the stored PAT, then [`Flow::projects_loaded`].
    LoadProjects {
        org: Organization,
    },
    OpenUrl(String),
    CopyText(String),
    /// Call [`Flow::advance`] after this long.
    AdvanceAfter(Duration),
    /// Save [`Flow::state`] (non-secret). Never in the sheet: the stored
    /// onboarding progress stays "done".
    Persist,
    /// Onboarding is complete; show the app.
    Finish,
    /// The sheet's replacement (`az`) for the stored token: create with the
    /// current set → swap → verify → revoke old, then [`Flow::pat_created`].
    ReplacePat {
        level: AccessLevel,
        lifetime_days: u32,
    },
    /// Save the pasted token's expiry date (Path B step 5).
    SaveExpiry(time::Date),
    /// The sheet saved a replacement: close it (design §10.2).
    SheetDone,
}

/// A failed network step. Throttling carries a countdown; the step's button
/// stays disabled until then (spec §3 States: never auto-hammer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub error: OnboardError,
    retry_at: Option<Instant>,
}

impl Failure {
    pub fn new(error: OnboardError, now: Instant) -> Self {
        let retry_at = match &error {
            OnboardError::Throttled { retry_after } => Some(now + *retry_after),
            _ => None,
        };
        Self { error, retry_at }
    }

    /// Whole seconds left before retry is allowed; `None` when allowed now.
    pub fn retry_in(&self, now: Instant) -> Option<u64> {
        let left = self.retry_at?.checked_duration_since(now)?;
        (!left.is_zero()).then(|| left.as_secs_f64().ceil() as u64)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Load<T> {
    Idle,
    Loading,
    Loaded(T),
    Failed(Failure),
}

impl<T> Load<T> {
    pub fn needs_load(&self) -> bool {
        matches!(self, Self::Idle | Self::Failed(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    Checking { since: Instant },
    Done(AzStatus),
}

impl Detection {
    pub fn status(&self) -> Option<&AzStatus> {
        match self {
            Self::Checking { .. } => None,
            Self::Done(status) => Some(status),
        }
    }

    /// The Azure CLI card can be chosen (ready, or installed but signed out).
    pub fn az_usable(&self) -> bool {
        matches!(
            self.status(),
            Some(AzStatus::Ready { .. } | AzStatus::NotLoggedIn { .. })
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignIn {
    Idle,
    Waiting {
        device_code: bool,
        prompt: Option<DeviceCodePrompt>,
    },
    Failed(OnboardError),
    TimedOut,
    Succeeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Minting {
    Idle,
    Creating,
    Created(PatMeta),
    Failed(Failure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verifying {
    Idle,
    Running,
    Done(Box<PasteVerification>),
    /// The field isn't a token (length/characters); nothing was sent.
    Malformed(String),
    Failed(Failure),
}

/// What a finished verification means for the flow (design 2q results).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifySummary<'a> {
    pub saved: bool,
    pub missing_required: Vec<&'static Scope>,
    pub missing_optional: Vec<&'static Scope>,
    pub who: &'a str,
}

pub fn summarize(result: &PasteVerification) -> VerifySummary<'_> {
    let missing = |required: bool| {
        result
            .probes
            .iter()
            .filter(|probe| probe.scope.required == required)
            .filter(|probe| {
                matches!(
                    probe.outcome,
                    ProbeOutcome::Missing | ProbeOutcome::Failed(_)
                )
            })
            .map(|probe| probe.scope)
            .collect::<Vec<_>>()
    };
    VerifySummary {
        saved: result.saved.is_some(),
        missing_required: missing(true),
        missing_optional: missing(false),
        who: &result.me.display_name,
    }
}

/// Case-insensitive subsequence match (`fbrk` finds `fabrikam`).
pub fn fuzzy_contains(haystack: &str, needle: &str) -> bool {
    let mut hay = haystack.chars().flat_map(char::to_lowercase);
    needle
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| !c.is_whitespace())
        .all(|n| hay.any(|h| h == n))
}

/// Orgs to list for `filter`: the az default first (design 2a), otherwise
/// in API order.
pub fn visible_orgs<'a>(
    orgs: &'a [OrgRef],
    default: Option<&Organization>,
    filter: &str,
) -> Vec<&'a OrgRef> {
    let is_default =
        |org: &OrgRef| default.is_some_and(|d| d.name().eq_ignore_ascii_case(&org.name));
    let mut visible: Vec<&OrgRef> = orgs
        .iter()
        .filter(|org| fuzzy_contains(&org.name, filter))
        .collect();
    visible.sort_by_key(|org| !is_default(org));
    visible
}

/// The project picker (design 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPicker {
    /// Most recently updated first when there are many (design 3).
    pub projects: Vec<ProjectRef>,
    pub selected: HashSet<String>,
    pub filter: String,
    pub cursor: usize,
}

impl ProjectPicker {
    /// Default selection: all when ≤ 8, else none; previously saved
    /// choices win when resuming.
    pub fn new(mut projects: Vec<ProjectRef>, saved: &[ProjectRef]) -> Self {
        let many = projects.len() > PROJECT_PRESELECT_MAX;
        if many {
            projects.sort_by_key(|p| std::cmp::Reverse(p.last_update_time));
        }
        let selected = if !saved.is_empty() {
            saved
                .iter()
                .filter(|s| projects.iter().any(|p| p.id == s.id))
                .map(|s| s.id.clone())
                .collect()
        } else if many {
            HashSet::new()
        } else {
            projects.iter().map(|p| p.id.clone()).collect()
        };
        Self {
            projects,
            selected,
            filter: String::new(),
            cursor: 0,
        }
    }

    /// Indices into `projects` matching the filter.
    pub fn visible(&self) -> Vec<usize> {
        self.projects
            .iter()
            .enumerate()
            .filter(|(_, p)| fuzzy_contains(&p.name, &self.filter))
            .map(|(ix, _)| ix)
            .collect()
    }

    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_owned();
        self.cursor = 0;
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let len = self.visible().len();
        if len == 0 {
            return;
        }
        self.cursor = (self.cursor as isize + delta).rem_euclid(len as isize) as usize;
    }

    /// Toggle the row at `visible_ix` (the focused row when `None`).
    pub fn toggle(&mut self, visible_ix: Option<usize>) {
        let visible = self.visible();
        let Some(&ix) = visible.get(visible_ix.unwrap_or(self.cursor)) else {
            return;
        };
        if let Some(visible_ix) = visible_ix {
            self.cursor = visible_ix;
        }
        let id = &self.projects[ix].id;
        if !self.selected.remove(id) {
            self.selected.insert(id.clone());
        }
    }

    /// ⌘A: select every project in the filtered set.
    pub fn select_visible(&mut self) {
        for ix in self.visible() {
            self.selected.insert(self.projects[ix].id.clone());
        }
    }

    /// ⌘⇧A: clear the selection.
    pub fn clear(&mut self) {
        self.selected.clear();
    }

    pub fn chosen(&self) -> Vec<ProjectRef> {
        self.projects
            .iter()
            .filter(|p| self.selected.contains(&p.id))
            .cloned()
            .collect()
    }
}

/// Screen-change direction, for the slide (design §6: Back is mirrored).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Back,
}

#[derive(Debug, Clone)]
pub struct Flow {
    pub screen: Screen,
    history: Vec<Screen>,
    /// The screen shown before this one (indicator animation).
    pub previous: Option<Screen>,
    pub direction: Direction,
    /// Bumped on every screen change; keys enter animations.
    pub transition: u64,

    pub detection: Detection,
    /// Connect screen choice (design 1: `1`/`2`, `↑ ↓`).
    pub choice: ConnectPath,
    /// The user picked a card, so detection results no longer move the
    /// selection.
    choice_pinned: bool,
    pub path: Option<ConnectPath>,
    pub sign_in: SignIn,

    pub orgs: Load<OrgList>,
    pub default_org: Option<Organization>,
    pub org_filter: String,
    pub org_cursor: usize,
    /// "Enter it manually" is open.
    pub manual_org: bool,
    pub manual_org_text: String,
    pub org_error: Option<String>,
    pub org: Option<Organization>,

    pub level: AccessLevel,
    pub lifetime_days: u32,
    pub minting: Minting,

    /// Why the paste path was forced (policy block), shown on 2p.
    pub paste_reason: Option<String>,
    pub paste_org_text: String,
    pub paste_org_error: Option<String>,
    pub verifying: Verifying,
    /// Scopes the user was sent back to fix (design 2q "Fix token").
    pub highlight: Vec<&'static str>,
    /// 2q "Expires on (optional)": the field's text and why it doesn't
    /// parse.
    pub expires_text: String,
    pub expires_error: Option<String>,

    pub projects: Load<ProjectPicker>,
    saved_projects: Vec<ProjectRef>,
    /// The Replace token sheet (spec 0001 FR-16): org fixed, no Welcome,
    /// org picker, Projects or Done; the stored progress isn't touched.
    pub sheet: bool,
}

/// Parse the "Expires on" field: `2026-12-24`, `12/24/2026` or
/// `Dec 24, 2026`; empty is fine.
pub fn parse_expiry(text: &str) -> Result<Option<time::Date>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let invalid = || format!("\"{text}\" isn't a date. Try 2026-12-24.");
    let month_of = |name: &str| {
        let name = name.get(..3)?.to_ascii_lowercase();
        [
            "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
        ]
        .iter()
        .position(|m| *m == name)
        .and_then(|ix| time::Month::try_from(ix as u8 + 1).ok())
    };
    let numbers: Vec<&str> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect();
    let (year, month, day) = match numbers.as_slice() {
        [y, m, d] if y.len() == 4 => (
            y.parse().ok(),
            m.parse::<u8>()
                .ok()
                .and_then(|m| time::Month::try_from(m).ok()),
            d.parse().ok(),
        ),
        [m, d, y] if y.len() == 4 && m.chars().all(|c| c.is_ascii_digit()) => (
            y.parse().ok(),
            m.parse::<u8>()
                .ok()
                .and_then(|m| time::Month::try_from(m).ok()),
            d.parse().ok(),
        ),
        [m, d, y] if y.len() == 4 => (y.parse().ok(), month_of(m), d.parse().ok()),
        _ => return Err(invalid()),
    };
    match (year, month, day) {
        (Some(year), Some(month), Some(day)) => time::Date::from_calendar_date(year, month, day)
            .map(Some)
            .map_err(|_| invalid()),
        _ => Err(invalid()),
    }
}

impl Flow {
    /// The Replace token sheet (spec 0001 FR-16, design §10.2): screen 1
    /// with `org` fixed and `level` preset.
    pub fn sheet(org: Organization, level: AccessLevel, now: Instant) -> (Self, Vec<Effect>) {
        let (mut flow, effects) = Self::start(now);
        flow.screen = Screen::Connect;
        flow.paste_org_text = org.name().to_owned();
        flow.org = Some(org);
        flow.level = level;
        flow.sheet = true;
        (flow, effects)
    }

    /// The step indicator's labels.
    pub fn indicator(&self) -> &'static [&'static str] {
        if self.sheet {
            &SHEET_INDICATOR
        } else {
            &INDICATOR
        }
    }

    /// "Expires on" changed.
    pub fn set_expires_text(&mut self, text: &str) {
        self.expires_text = text.to_owned();
        self.expires_error = None;
    }

    /// 2q's Continue: save the optional date, then Projects (or close the
    /// sheet). A date that doesn't parse stops here with its reason.
    fn continue_after_paste(&mut self) -> Vec<Effect> {
        let mut effects = match parse_expiry(&self.expires_text) {
            Ok(Some(date)) => vec![Effect::SaveExpiry(date)],
            Ok(None) => Vec::new(),
            Err(error) => {
                self.expires_error = Some(error);
                return Vec::new();
            }
        };
        if self.sheet {
            effects.push(Effect::SheetDone);
        } else {
            effects.extend(self.go(Screen::Projects));
        }
        effects
    }
}

impl Flow {
    /// A fresh flow on Welcome; detection starts immediately (design 0).
    pub fn start(now: Instant) -> (Self, Vec<Effect>) {
        let flow = Self {
            screen: Screen::Welcome,
            history: Vec::new(),
            previous: None,
            direction: Direction::Forward,
            transition: 0,
            detection: Detection::Checking { since: now },
            choice: ConnectPath::Az,
            choice_pinned: false,
            path: None,
            sign_in: SignIn::Idle,
            orgs: Load::Idle,
            default_org: None,
            org_filter: String::new(),
            org_cursor: 0,
            manual_org: false,
            manual_org_text: String::new(),
            org_error: None,
            org: None,
            level: AccessLevel::default(),
            lifetime_days: DEFAULT_LIFETIME,
            minting: Minting::Idle,
            paste_reason: None,
            paste_org_text: String::new(),
            paste_org_error: None,
            verifying: Verifying::Idle,
            highlight: Vec::new(),
            projects: Load::Idle,
            saved_projects: Vec::new(),
            expires_text: String::new(),
            expires_error: None,
            sheet: false,
        };
        (flow, vec![Effect::Detect])
    }

    /// Resume at the first incomplete step (FR-10, design §1 "Resume").
    /// `account` is the stored, verified token's metadata, if any: with it
    /// the token step is complete whatever the saved step says.
    pub fn resume(
        state: &OnboardingState,
        account: Option<&PatMeta>,
        now: Instant,
    ) -> (Self, Vec<Effect>) {
        let (mut flow, mut effects) = Self::start(now);
        flow.level = state.level;
        flow.lifetime_days = if LIFETIMES.contains(&state.lifetime_days) {
            state.lifetime_days
        } else {
            DEFAULT_LIFETIME
        };
        flow.path = state.path;
        flow.choice = state.path.unwrap_or(ConnectPath::Az);
        flow.choice_pinned = state.path.is_some();
        flow.saved_projects = state.projects.clone();
        let saved_org = state
            .org
            .as_deref()
            .and_then(|o| Organization::parse(o).ok());
        if let Some(org) = &saved_org {
            flow.paste_org_text = org.name().to_owned();
        }

        let token_org = account
            .filter(|meta| meta.verified)
            .and_then(PatMeta::organization);
        let (screen, trail): (Screen, &[Screen]) = if let Some(org) = token_org {
            flow.org = Some(org);
            flow.level = account.map_or(flow.level, |meta| meta.level);
            let screen = match state.step {
                Step::FirstSync | Step::MakeItYours | Step::Done => Screen::Done,
                _ => Screen::Projects,
            };
            (screen, &[Screen::Welcome, Screen::Connect])
        } else {
            match state.step {
                Step::Welcome => (Screen::Welcome, &[]),
                Step::PickOrg | Step::ReviewAccess if state.path != Some(ConnectPath::Paste) => {
                    match saved_org {
                        Some(org) if state.step == Step::ReviewAccess => {
                            flow.org = Some(org);
                            (
                                Screen::ReviewAccess,
                                &[Screen::Welcome, Screen::Connect, Screen::PickOrg],
                            )
                        }
                        _ => (Screen::PickOrg, &[Screen::Welcome, Screen::Connect]),
                    }
                }
                Step::CreateInBrowser | Step::PasteVerify => {
                    (Screen::CreateInBrowser, &[Screen::Welcome, Screen::Connect])
                }
                // Signed-in state, sync and prefs all need a token first.
                _ => (Screen::Connect, &[Screen::Welcome]),
            }
        };
        flow.history = trail.to_vec();
        flow.screen = screen;
        effects.extend(flow.enter());
        (flow, effects)
    }

    /// Non-secret progress to persist.
    pub fn state(&self) -> OnboardingState {
        OnboardingState {
            step: self.screen.step(),
            path: self.path,
            org: self
                .org
                .as_ref()
                .map(|o| o.name().to_owned())
                .or_else(|| (!self.paste_org_text.is_empty()).then(|| self.paste_org_text.clone())),
            level: self.level,
            lifetime_days: self.lifetime_days,
            projects: match &self.projects {
                Load::Loaded(picker) => picker.chosen(),
                _ => self.saved_projects.clone(),
            },
        }
    }

    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty() && self.screen != Screen::Done
    }

    fn go(&mut self, screen: Screen) -> Vec<Effect> {
        if screen == self.screen {
            return Vec::new();
        }
        self.history.push(self.screen);
        self.previous = Some(self.screen);
        self.direction = Direction::Forward;
        self.screen = screen;
        self.transition += 1;
        let mut effects = self.enter();
        effects.push(Effect::Persist);
        effects
    }

    /// Loads a screen needs when it becomes current.
    fn enter(&mut self) -> Vec<Effect> {
        match self.screen {
            Screen::PickOrg if self.orgs.needs_load() => {
                self.orgs = Load::Loading;
                vec![Effect::LoadOrgs]
            }
            Screen::Projects if self.projects.needs_load() => match self.org.clone() {
                Some(org) => {
                    self.projects = Load::Loading;
                    vec![Effect::LoadProjects { org }]
                }
                None => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// `⌘[`: previous screen. Never leaves onboarding; cancels a pending
    /// sign-in.
    pub fn back(&mut self) -> Vec<Effect> {
        if !self.can_go_back() {
            return Vec::new();
        }
        let mut effects = Vec::new();
        if matches!(self.sign_in, SignIn::Waiting { .. }) {
            self.sign_in = SignIn::Idle;
            effects.push(Effect::CancelSignIn);
        }
        let Some(screen) = self.history.pop() else {
            return effects;
        };
        self.previous = Some(self.screen);
        self.direction = Direction::Back;
        self.screen = screen;
        self.transition += 1;
        effects.extend(self.enter());
        effects.push(Effect::Persist);
        effects
    }

    /// `↩`: the current screen's primary action.
    pub fn confirm(&mut self, now: Instant) -> Vec<Effect> {
        match self.screen {
            Screen::Welcome => self.go(Screen::Connect),
            Screen::Connect => self.choose_and_continue(self.choice),
            Screen::SignIn => match self.sign_in {
                SignIn::Waiting { .. } => Vec::new(),
                SignIn::Succeeded => self.advance(),
                _ => self.start_sign_in(false),
            },
            Screen::PickOrg => self.pick_org(),
            Screen::ReviewAccess => self.create_token(now),
            Screen::CreateInBrowser => self.open_paste_verify(),
            Screen::PasteVerify => match &self.verifying {
                Verifying::Done(result) if result.saved.is_some() => self.continue_after_paste(),
                Verifying::Running => Vec::new(),
                Verifying::Failed(failure) if failure.retry_in(now).is_some() => Vec::new(),
                _ => self.verify(),
            },
            Screen::Projects => self.finish_projects(),
            Screen::Done => vec![Effect::Persist, Effect::Finish],
        }
    }

    // Connect ------------------------------------------------------------

    pub fn detected(&mut self, status: AzStatus) {
        self.detection = Detection::Done(status);
        if !self.choice_pinned {
            self.choice = if self.detection.az_usable() {
                ConnectPath::Az
            } else {
                ConnectPath::Paste
            };
        }
    }

    /// "Check again" (design 1): rerun all detection.
    pub fn detect_again(&mut self, now: Instant) -> Vec<Effect> {
        if matches!(self.detection, Detection::Checking { .. }) {
            return Vec::new();
        }
        self.detection = Detection::Checking { since: now };
        vec![Effect::Detect]
    }

    /// `1`/`2`/`↑ ↓`/click. A disabled Azure CLI card is still selectable
    /// (focusable with its reason), but Continue won't take it.
    pub fn select_choice(&mut self, choice: ConnectPath) {
        self.choice = choice;
        self.choice_pinned = true;
    }

    pub fn toggle_choice(&mut self) {
        self.select_choice(match self.choice {
            ConnectPath::Az => ConnectPath::Paste,
            ConnectPath::Paste => ConnectPath::Az,
        });
    }

    /// Continue with `choice`; an unusable Azure CLI leaves everything as is.
    pub fn choose_and_continue(&mut self, choice: ConnectPath) -> Vec<Effect> {
        if choice == ConnectPath::Paste || self.detection.az_usable() {
            self.select_choice(choice);
        }
        match choice {
            ConnectPath::Paste => {
                self.path = Some(ConnectPath::Paste);
                self.go(Screen::CreateInBrowser)
            }
            ConnectPath::Az => match self.detection.status() {
                Some(AzStatus::Ready { .. }) => {
                    self.path = Some(ConnectPath::Az);
                    // The sheet's org is fixed (design §10.2: 2a skipped).
                    self.go(if self.sheet {
                        Screen::ReviewAccess
                    } else {
                        Screen::PickOrg
                    })
                }
                Some(AzStatus::NotLoggedIn { .. }) => {
                    self.path = Some(ConnectPath::Az);
                    self.go(Screen::SignIn)
                }
                _ => Vec::new(),
            },
        }
    }

    // Sign in ------------------------------------------------------------

    pub fn start_sign_in(&mut self, device_code: bool) -> Vec<Effect> {
        if matches!(self.sign_in, SignIn::Waiting { .. }) {
            return Vec::new();
        }
        self.sign_in = SignIn::Waiting {
            device_code,
            prompt: None,
        };
        vec![Effect::SignIn { device_code }]
    }

    pub fn device_code(&mut self, prompt: DeviceCodePrompt) {
        if let SignIn::Waiting {
            device_code: true,
            prompt: slot,
        } = &mut self.sign_in
        {
            *slot = Some(prompt);
        }
    }

    /// `az login` finished (AC-4: success continues without further clicks).
    pub fn signed_in(&mut self, result: Result<AzStatus, OnboardError>) -> Vec<Effect> {
        if !matches!(self.sign_in, SignIn::Waiting { .. }) {
            // Cancelled or timed out meanwhile; ignore the late result.
            return Vec::new();
        }
        match result {
            Ok(status @ AzStatus::Ready { .. }) => {
                self.detection = Detection::Done(status);
                self.sign_in = SignIn::Succeeded;
                vec![Effect::AdvanceAfter(ADVANCE_DELAY)]
            }
            Ok(status) => {
                self.detection = Detection::Done(status);
                self.sign_in = SignIn::Failed(OnboardError::LoginFailed {
                    detail: "az still reports no signed-in account".into(),
                });
                Vec::new()
            }
            Err(OnboardError::LoginCancelled) => {
                self.sign_in = SignIn::Idle;
                Vec::new()
            }
            Err(error) => {
                self.sign_in = SignIn::Failed(error);
                Vec::new()
            }
        }
    }

    /// `Esc`/[Cancel]: kill `az login`, return to the choice (spec §3 States).
    pub fn cancel_sign_in(&mut self) -> Vec<Effect> {
        if !matches!(self.sign_in, SignIn::Waiting { .. }) {
            return Vec::new();
        }
        self.sign_in = SignIn::Idle;
        let mut effects = vec![Effect::CancelSignIn];
        effects.extend(self.back());
        effects
    }

    pub fn sign_in_timed_out(&mut self) -> Vec<Effect> {
        if !matches!(self.sign_in, SignIn::Waiting { .. }) {
            return Vec::new();
        }
        self.sign_in = SignIn::TimedOut;
        vec![Effect::CancelSignIn]
    }

    /// Delayed auto-advance after a success check.
    pub fn advance(&mut self) -> Vec<Effect> {
        match (self.screen, &self.sign_in, &self.minting) {
            (Screen::SignIn, SignIn::Succeeded, _) if self.sheet => self.go(Screen::ReviewAccess),
            (Screen::SignIn, SignIn::Succeeded, _) => self.go(Screen::PickOrg),
            (Screen::ReviewAccess, _, Minting::Created(_)) if self.sheet => vec![Effect::SheetDone],
            (Screen::ReviewAccess, _, Minting::Created(_)) => self.go(Screen::Projects),
            _ => Vec::new(),
        }
    }

    /// "Paste a token instead" from any az screen.
    pub fn use_paste_path(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        if matches!(self.sign_in, SignIn::Waiting { .. }) {
            self.sign_in = SignIn::Idle;
            effects.push(Effect::CancelSignIn);
        }
        self.path = Some(ConnectPath::Paste);
        self.choice = ConnectPath::Paste;
        self.choice_pinned = true;
        if let Some(org) = &self.org {
            self.paste_org_text = org.name().to_owned();
        }
        effects.extend(self.go(Screen::CreateInBrowser));
        effects
    }

    // Organization -------------------------------------------------------

    pub fn orgs_loaded(
        &mut self,
        result: Result<OrgList, OnboardError>,
        default_org: Option<Organization>,
        now: Instant,
    ) {
        match result {
            Ok(list) => {
                self.default_org = default_org;
                let preferred = self.org.clone().or_else(|| self.default_org.clone());
                let visible = visible_orgs(&list.orgs, self.default_org.as_ref(), "");
                self.org_cursor = preferred
                    .and_then(|org| {
                        visible
                            .iter()
                            .position(|o| o.name.eq_ignore_ascii_case(org.name()))
                    })
                    .unwrap_or(0);
                // No orgs: open manual entry right away (design 2a empty).
                if list.orgs.is_empty() {
                    self.manual_org = true;
                }
                self.orgs = Load::Loaded(list);
            }
            Err(error) => self.orgs = Load::Failed(Failure::new(error, now)),
        }
    }

    pub fn retry_orgs(&mut self, now: Instant) -> Vec<Effect> {
        match &self.orgs {
            Load::Failed(failure) if failure.retry_in(now).is_none() => {
                self.orgs = Load::Loading;
                vec![Effect::LoadOrgs]
            }
            _ => Vec::new(),
        }
    }

    pub fn visible_orgs(&self) -> Vec<&OrgRef> {
        match &self.orgs {
            Load::Loaded(list) => {
                visible_orgs(&list.orgs, self.default_org.as_ref(), &self.org_filter)
            }
            _ => Vec::new(),
        }
    }

    pub fn set_org_filter(&mut self, filter: &str) {
        self.org_filter = filter.to_owned();
        self.org_cursor = 0;
    }

    pub fn move_org(&mut self, delta: isize) {
        let len = self.visible_orgs().len();
        if len > 0 {
            self.org_cursor = (self.org_cursor as isize + delta).rem_euclid(len as isize) as usize;
        }
    }

    pub fn select_org(&mut self, visible_ix: usize) {
        self.org_cursor = visible_ix;
        self.manual_org = false;
    }

    pub fn open_manual_org(&mut self) {
        self.manual_org = true;
        self.org_error = None;
    }

    pub fn set_manual_org(&mut self, text: &str) {
        self.manual_org_text = text.to_owned();
        self.org_error = None;
    }

    fn pick_org(&mut self) -> Vec<Effect> {
        let org = if self.manual_org {
            match Organization::parse(&self.manual_org_text) {
                Ok(org) => org,
                Err(error) => {
                    self.org_error = Some(error.to_string());
                    return Vec::new();
                }
            }
        } else {
            match self
                .visible_orgs()
                .get(self.org_cursor)
                .and_then(|org| org.organization())
            {
                Some(org) => org,
                None => return Vec::new(),
            }
        };
        if self.org.as_ref() != Some(&org) {
            self.minting = Minting::Idle;
            self.projects = Load::Idle;
        }
        self.org = Some(org);
        self.go(Screen::ReviewAccess)
    }

    // Access + create ----------------------------------------------------

    pub fn set_level(&mut self, level: AccessLevel) {
        if matches!(self.minting, Minting::Creating | Minting::Created(_)) {
            return;
        }
        self.level = level;
    }

    pub fn set_lifetime(&mut self, days: u32) {
        if matches!(self.minting, Minting::Creating | Minting::Created(_)) {
            return;
        }
        if LIFETIMES.contains(&days) {
            self.lifetime_days = days;
        }
    }

    fn create_token(&mut self, now: Instant) -> Vec<Effect> {
        match &self.minting {
            Minting::Creating => return Vec::new(),
            Minting::Created(_) => return self.advance(),
            Minting::Failed(failure) if failure.retry_in(now).is_some() => return Vec::new(),
            _ => {}
        }
        let Some(org) = self.org.clone() else {
            return Vec::new();
        };
        self.minting = Minting::Creating;
        if self.sheet {
            return vec![Effect::ReplacePat {
                level: self.level,
                lifetime_days: self.lifetime_days,
            }];
        }
        vec![Effect::CreatePat {
            org,
            level: self.level,
            lifetime_days: self.lifetime_days,
        }]
    }

    /// Mint finished. Policy blocks switch to the paste path with the
    /// reason (AC-8); the 30-day cap snaps the lifetime control (AC-7).
    pub fn pat_created(
        &mut self,
        result: Result<PatMeta, OnboardError>,
        now: Instant,
    ) -> Vec<Effect> {
        if !matches!(self.minting, Minting::Creating) {
            return Vec::new();
        }
        match result {
            Ok(meta) => {
                if meta.lifetime_capped {
                    self.lifetime_days = meta.lifetime_days.unwrap_or(30);
                }
                self.minting = Minting::Created(meta);
                vec![Effect::Persist, Effect::AdvanceAfter(ADVANCE_DELAY)]
            }
            Err(error) if error.falls_back_to_paste() => {
                self.minting = Minting::Idle;
                self.paste_reason = Some(error.message());
                self.use_paste_path()
            }
            Err(error) => {
                self.minting = Minting::Failed(Failure::new(error, now));
                Vec::new()
            }
        }
    }

    // Paste path ---------------------------------------------------------

    pub fn set_paste_org(&mut self, text: &str) {
        self.paste_org_text = text.to_owned();
        self.paste_org_error = None;
    }

    /// Validate the org field (design 2p: on blur / before use).
    pub fn paste_org(&mut self) -> Option<Organization> {
        match Organization::parse(&self.paste_org_text) {
            Ok(org) => {
                self.paste_org_error = None;
                Some(org)
            }
            Err(error) => {
                self.paste_org_error = Some(error.to_string());
                None
            }
        }
    }

    pub fn open_token_page(&mut self) -> Vec<Effect> {
        match self.paste_org() {
            Some(org) => vec![Effect::OpenUrl(
                adoken_ado::token_page_url(&org).to_string(),
            )],
            None => Vec::new(),
        }
    }

    pub fn copy_scope(&self, id: &str) -> Vec<Effect> {
        vec![Effect::CopyText(id.to_owned())]
    }

    fn open_paste_verify(&mut self) -> Vec<Effect> {
        match self.paste_org() {
            Some(org) => {
                self.org = Some(org);
                self.go(Screen::PasteVerify)
            }
            None => Vec::new(),
        }
    }

    /// ⌘V anywhere on 2p/2q: jump to 2q and verify what was pasted.
    pub fn pasted(&mut self) -> Vec<Effect> {
        match self.screen {
            Screen::CreateInBrowser => {
                let mut effects = self.open_paste_verify();
                if self.screen == Screen::PasteVerify {
                    effects.extend(self.verify());
                }
                effects
            }
            Screen::PasteVerify => self.verify(),
            _ => Vec::new(),
        }
    }

    pub fn verify(&mut self) -> Vec<Effect> {
        if matches!(self.verifying, Verifying::Running) {
            return Vec::new();
        }
        let Some(org) = self.org.clone() else {
            return Vec::new();
        };
        self.verifying = Verifying::Running;
        vec![Effect::Verify {
            org,
            level: self.level,
        }]
    }

    /// The field didn't parse as a token; nothing was sent.
    pub fn malformed_token(&mut self, message: String) {
        self.verifying = Verifying::Malformed(message);
    }

    pub fn verified(
        &mut self,
        result: Result<PasteVerification, OnboardError>,
        now: Instant,
    ) -> Vec<Effect> {
        if !matches!(self.verifying, Verifying::Running) {
            return Vec::new();
        }
        match result {
            Ok(verification) => {
                let saved = verification.saved.is_some();
                self.highlight.clear();
                self.verifying = Verifying::Done(Box::new(verification));
                if saved {
                    // Design 2q: the value never renders after success.
                    vec![Effect::ForgetPastedToken, Effect::Persist]
                } else {
                    Vec::new()
                }
            }
            Err(error) => {
                self.verifying = Verifying::Failed(Failure::new(error, now));
                Vec::new()
            }
        }
    }

    /// "Fix token": back to 2p with the missing rows highlighted.
    pub fn fix_token(&mut self) -> Vec<Effect> {
        if let Verifying::Done(result) = &self.verifying {
            let summary = summarize(result);
            self.highlight = summary
                .missing_required
                .iter()
                .chain(&summary.missing_optional)
                .map(|scope| scope.id)
                .collect();
        }
        self.verifying = Verifying::Idle;
        self.back()
    }

    // Projects -----------------------------------------------------------

    pub fn projects_loaded(&mut self, result: Result<Vec<ProjectRef>, OnboardError>, now: Instant) {
        self.projects = match result {
            Ok(projects) => Load::Loaded(ProjectPicker::new(projects, &self.saved_projects)),
            Err(error) => Load::Failed(Failure::new(error, now)),
        };
    }

    pub fn retry_projects(&mut self, now: Instant) -> Vec<Effect> {
        match (&self.projects, self.org.clone()) {
            (Load::Failed(failure), Some(org)) if failure.retry_in(now).is_none() => {
                self.projects = Load::Loading;
                vec![Effect::LoadProjects { org }]
            }
            _ => Vec::new(),
        }
    }

    pub fn picker_mut(&mut self) -> Option<&mut ProjectPicker> {
        match &mut self.projects {
            Load::Loaded(picker) => Some(picker),
            _ => None,
        }
    }

    fn finish_projects(&mut self) -> Vec<Effect> {
        let Load::Loaded(picker) = &self.projects else {
            return Vec::new();
        };
        if picker.selected.is_empty() {
            return Vec::new();
        }
        self.saved_projects = picker.chosen();
        self.go(Screen::Done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_ado::{AzCli, Identity, PatSource, Profile, ScopeProbe, scopes};

    fn now() -> Instant {
        Instant::now()
    }

    fn az() -> AzCli {
        AzCli::new("/opt/homebrew/bin/az".into(), None, Default::default())
    }

    fn ready() -> AzStatus {
        AzStatus::Ready {
            az: az(),
            version: "2.83.0".into(),
            user: "ada@contoso.com".into(),
            tenant: "t".into(),
        }
    }

    fn signed_out() -> AzStatus {
        AzStatus::NotLoggedIn {
            az: az(),
            version: "2.83.0".into(),
        }
    }

    fn org(name: &str) -> OrgRef {
        OrgRef {
            id: format!("id-{name}"),
            name: name.into(),
        }
    }

    fn org_list(names: &[&str]) -> OrgList {
        OrgList {
            me: Profile {
                id: "me".into(),
                display_name: "Ada".into(),
                email: Some("ada@contoso.com".into()),
            },
            orgs: names.iter().map(|n| org(n)).collect(),
        }
    }

    fn meta(org: &str, capped: bool) -> PatMeta {
        PatMeta {
            org: org.into(),
            level: AccessLevel::Review,
            scopes: Vec::new(),
            untested: Vec::new(),
            authorization_id: Some("auth".into()),
            valid_to: None,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            source: PatSource::Az,
            lifetime_days: Some(if capped { 30 } else { 90 }),
            lifetime_capped: capped,
            verified: true,
            me: None,
            auto_replaced: false,
            auto_replace: Default::default(),
        }
    }

    /// Spec 0001 AC-22 / design §10.2: the sheet starts on Connect with the
    /// org fixed, skips the org picker, replaces instead of minting a second
    /// token, and closes instead of showing Projects; nothing is persisted.
    #[test]
    fn the_replace_sheet_skips_welcome_org_and_projects() {
        let fabrikam = Organization::parse("fabrikam").unwrap();
        let (mut flow, effects) = Flow::sheet(fabrikam.clone(), AccessLevel::ReadOnly, now());
        assert_eq!(effects, [Effect::Detect]);
        assert_eq!(flow.screen, Screen::Connect);
        assert!(!flow.can_go_back(), "no Welcome behind it");
        assert_eq!(flow.indicator(), ["Connect", "Token"]);
        assert_eq!(flow.level, AccessLevel::ReadOnly);
        flow.detected(ready());
        flow.confirm(now());
        assert_eq!(flow.screen, Screen::ReviewAccess, "2a skipped");
        assert_eq!(flow.org, Some(fabrikam));
        let effects = flow.confirm(now());
        assert_eq!(
            effects,
            [Effect::ReplacePat {
                level: AccessLevel::ReadOnly,
                lifetime_days: DEFAULT_LIFETIME
            }]
        );
        flow.pat_created(Ok(meta("fabrikam", false)), now());
        assert_eq!(flow.advance(), [Effect::SheetDone]);
        assert_eq!(flow.screen, Screen::ReviewAccess, "never Projects");

        // Paste path: 2p keeps the org, 2q's Continue closes the sheet.
        let (mut flow, _) = Flow::sheet(
            Organization::parse("fabrikam").unwrap(),
            AccessLevel::Review,
            now(),
        );
        flow.detected(signed_out());
        flow.choose_and_continue(ConnectPath::Paste);
        assert_eq!(flow.screen, Screen::CreateInBrowser);
        flow.confirm(now());
        assert_eq!(flow.screen, Screen::PasteVerify);
        flow.verify();
        flow.verified(Ok(verification(AccessLevel::Review, &[], true)), now());
        flow.set_expires_text("Dec 24, 2026");
        assert_eq!(
            flow.confirm(now()),
            [
                Effect::SaveExpiry(time::macros::date!(2026 - 12 - 24)),
                Effect::SheetDone
            ]
        );
    }

    #[test]
    fn expiry_accepts_common_date_forms_and_rejects_others() {
        let dec24 = Some(time::macros::date!(2026 - 12 - 24));
        assert_eq!(parse_expiry(""), Ok(None));
        assert_eq!(parse_expiry(" 2026-12-24 "), Ok(dec24));
        assert_eq!(parse_expiry("12/24/2026"), Ok(dec24));
        assert_eq!(parse_expiry("Dec 24, 2026"), Ok(dec24));
        assert_eq!(parse_expiry("december 24 2026"), Ok(dec24));
        assert!(parse_expiry("2026-02-30").is_err());
        assert!(parse_expiry("next week").is_err());
    }

    #[test]
    fn an_unparsable_expiry_stops_continue_with_its_reason() {
        let (mut flow, _) = Flow::start(now());
        flow.detected(signed_out());
        flow.confirm(now());
        flow.choose_and_continue(ConnectPath::Paste);
        flow.set_paste_org("fabrikam");
        flow.confirm(now());
        flow.verify();
        flow.verified(Ok(verification(AccessLevel::Review, &[], true)), now());
        flow.set_expires_text("soon");
        assert!(flow.confirm(now()).is_empty());
        assert_eq!(flow.screen, Screen::PasteVerify);
        assert!(flow.expires_error.is_some());
        flow.set_expires_text("");
        flow.confirm(now());
        assert_eq!(flow.screen, Screen::Projects);
    }

    fn project(id: &str, updated: i64) -> ProjectRef {
        ProjectRef {
            id: id.into(),
            name: format!("Project {id}"),
            description: None,
            last_update_time: Some(
                time::OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(updated),
            ),
        }
    }

    fn verification(level: AccessLevel, missing: &[&str], saved: bool) -> PasteVerification {
        PasteVerification {
            me: Identity {
                id: "me".into(),
                display_name: "Dana Kim".into(),
            },
            probes: scopes(level)
                .iter()
                .map(|scope| ScopeProbe {
                    scope,
                    outcome: if missing.contains(&scope.id) {
                        ProbeOutcome::Missing
                    } else {
                        ProbeOutcome::Granted
                    },
                })
                .collect(),
            saved: saved.then(|| meta("fabrikam", false)),
        }
    }

    /// Welcome → Connect with az ready → PickOrg with orgs loaded.
    fn at_pick_org(names: &[&str]) -> Flow {
        let (mut flow, effects) = Flow::start(now());
        assert_eq!(effects, [Effect::Detect]);
        flow.detected(ready());
        flow.confirm(now());
        let effects = flow.confirm(now());
        assert_eq!(flow.screen, Screen::PickOrg);
        assert!(effects.contains(&Effect::LoadOrgs));
        flow.orgs_loaded(Ok(org_list(names)), None, now());
        flow
    }

    #[test]
    fn detection_result_preselects_the_usable_card_until_the_user_chooses() {
        let (mut flow, _) = Flow::start(now());
        flow.detected(AzStatus::NotInstalled);
        assert_eq!(
            flow.choice,
            ConnectPath::Paste,
            "disabled az → paste preselected"
        );
        flow.detect_again(now());
        flow.detected(ready());
        assert_eq!(flow.choice, ConnectPath::Az);

        flow.select_choice(ConnectPath::Paste);
        flow.detect_again(now());
        flow.detected(ready());
        assert_eq!(flow.choice, ConnectPath::Paste, "a user choice sticks");
    }

    #[test]
    fn too_old_az_cannot_be_continued_and_never_requests_a_token() {
        // AC-3: too old → paste offered, no token work.
        let (mut flow, _) = Flow::start(now());
        flow.detected(AzStatus::TooOld {
            az: az(),
            found: "2.29.1".into(),
        });
        flow.confirm(now());
        let effects = flow.choose_and_continue(ConnectPath::Az);
        assert!(effects.is_empty());
        assert_eq!(flow.screen, Screen::Connect);
        flow.confirm(now());
        assert_eq!(flow.screen, Screen::CreateInBrowser);
    }

    #[test]
    fn signed_out_az_signs_in_then_advances_to_orgs_without_clicks() {
        // AC-4.
        let (mut flow, _) = Flow::start(now());
        flow.detected(signed_out());
        flow.confirm(now());
        flow.confirm(now());
        assert_eq!(flow.screen, Screen::SignIn);
        assert_eq!(flow.confirm(now()), [Effect::SignIn { device_code: false }]);
        assert!(
            flow.confirm(now()).is_empty(),
            "no second az login while waiting"
        );
        assert_eq!(
            flow.signed_in(Ok(ready())),
            [Effect::AdvanceAfter(ADVANCE_DELAY)]
        );
        let effects = flow.advance();
        assert_eq!(flow.screen, Screen::PickOrg);
        assert!(effects.contains(&Effect::LoadOrgs));
    }

    #[test]
    fn cancelling_sign_in_kills_az_and_returns_to_the_choice() {
        let (mut flow, _) = Flow::start(now());
        flow.detected(signed_out());
        flow.confirm(now());
        flow.confirm(now());
        flow.confirm(now());
        let effects = flow.cancel_sign_in();
        assert!(effects.contains(&Effect::CancelSignIn));
        assert_eq!(flow.screen, Screen::Connect);
        // A late result from the killed process changes nothing.
        assert!(flow.signed_in(Ok(ready())).is_empty());
        assert_eq!(flow.screen, Screen::Connect);
    }

    #[test]
    fn sign_in_timeout_and_failure_keep_the_user_on_the_screen() {
        let (mut flow, _) = Flow::start(now());
        flow.detected(signed_out());
        flow.confirm(now());
        flow.confirm(now());
        flow.confirm(now());
        assert_eq!(flow.sign_in_timed_out(), [Effect::CancelSignIn]);
        assert_eq!(flow.sign_in, SignIn::TimedOut);
        flow.confirm(now());
        let failed = OnboardError::LoginFailed {
            detail: "AADSTS50076".into(),
        };
        flow.signed_in(Err(failed.clone()));
        assert_eq!(flow.sign_in, SignIn::Failed(failed));
        assert_eq!(flow.screen, Screen::SignIn);
    }

    #[test]
    fn org_filter_is_case_insensitive_and_manual_urls_resolve() {
        // AC-5.
        let mut flow = at_pick_org(&["Contoso", "fabrikam", "Northwind"]);
        assert_eq!(flow.visible_orgs().len(), 3);
        flow.set_org_filter("FAB");
        let names: Vec<_> = flow.visible_orgs().iter().map(|o| o.name.clone()).collect();
        assert_eq!(names, ["fabrikam"]);

        flow.set_org_filter("");
        flow.open_manual_org();
        flow.set_manual_org("https://dev.azure.com/fabrikam/");
        flow.confirm(now());
        assert_eq!(flow.screen, Screen::ReviewAccess);
        assert_eq!(flow.org.as_ref().map(Organization::name), Some("fabrikam"));
    }

    #[test]
    fn invalid_manual_org_shows_an_error_and_stays() {
        let mut flow = at_pick_org(&["contoso"]);
        flow.open_manual_org();
        flow.set_manual_org("https://example.com/foo");
        assert!(flow.confirm(now()).is_empty());
        assert!(flow.org_error.is_some());
        assert_eq!(flow.screen, Screen::PickOrg);
    }

    #[test]
    fn az_default_org_is_listed_first_and_preselected() {
        let (mut flow, _) = Flow::start(now());
        flow.detected(ready());
        flow.confirm(now());
        flow.confirm(now());
        flow.orgs_loaded(
            Ok(org_list(&["alpha", "beta", "gamma"])),
            Organization::parse("gamma").ok(),
            now(),
        );
        assert_eq!(flow.visible_orgs()[0].name, "gamma");
        assert_eq!(flow.org_cursor, 0);
        flow.move_org(-1);
        assert_eq!(flow.org_cursor, 2, "wraps");
    }

    #[test]
    fn creating_a_token_sends_level_and_lifetime_once() {
        let mut flow = at_pick_org(&["fabrikam"]);
        flow.confirm(now());
        flow.set_level(AccessLevel::ReadOnly);
        flow.set_lifetime(180);
        flow.set_lifetime(45); // not offered: ignored
        let effects = flow.confirm(now());
        assert_eq!(
            effects,
            [Effect::CreatePat {
                org: Organization::parse("fabrikam").unwrap(),
                level: AccessLevel::ReadOnly,
                lifetime_days: 180,
            }]
        );
        assert!(flow.confirm(now()).is_empty(), "no double mint");
        flow.set_level(AccessLevel::Review);
        assert_eq!(flow.level, AccessLevel::ReadOnly, "locked while creating");
    }

    #[test]
    fn lifetime_cap_snaps_to_30_and_success_advances_to_projects() {
        // AC-7 (the retry itself is the backend's): the control shows 30.
        let mut flow = at_pick_org(&["fabrikam"]);
        flow.confirm(now());
        flow.confirm(now());
        let effects = flow.pat_created(Ok(meta("fabrikam", true)), now());
        assert_eq!(flow.lifetime_days, 30);
        assert!(effects.contains(&Effect::AdvanceAfter(ADVANCE_DELAY)));
        let effects = flow.advance();
        assert_eq!(flow.screen, Screen::Projects);
        assert!(effects.contains(&Effect::LoadProjects {
            org: Organization::parse("fabrikam").unwrap()
        }));
    }

    #[test]
    fn policy_block_switches_to_paste_with_the_reason_and_no_retry() {
        // AC-8.
        let mut flow = at_pick_org(&["fabrikam"]);
        flow.confirm(now());
        flow.confirm(now());
        let effects = flow.pat_created(
            Err(OnboardError::PolicyBlocked {
                reason: "accessDenied".into(),
            }),
            now(),
        );
        assert_eq!(flow.screen, Screen::CreateInBrowser);
        assert!(
            flow.paste_reason
                .as_deref()
                .unwrap()
                .contains("accessDenied")
        );
        assert_eq!(flow.paste_org_text, "fabrikam");
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::CreatePat { .. }))
        );
    }

    #[test]
    fn throttled_create_waits_out_retry_after() {
        let mut flow = at_pick_org(&["fabrikam"]);
        flow.confirm(now());
        flow.confirm(now());
        let t0 = now();
        flow.pat_created(
            Err(OnboardError::Throttled {
                retry_after: Duration::from_secs(20),
            }),
            t0,
        );
        let Minting::Failed(failure) = &flow.minting else {
            panic!("expected failure");
        };
        assert_eq!(failure.retry_in(t0), Some(20));
        assert!(flow.confirm(t0 + Duration::from_secs(5)).is_empty());
        assert!(matches!(
            flow.confirm(t0 + Duration::from_secs(21)).as_slice(),
            [Effect::CreatePat { .. }]
        ));
    }

    #[test]
    fn paste_path_validates_org_and_pasting_jumps_to_verify() {
        let (mut flow, _) = Flow::start(now());
        flow.detected(AzStatus::NotInstalled);
        flow.confirm(now());
        flow.confirm(now());
        assert_eq!(flow.screen, Screen::CreateInBrowser);
        assert!(flow.open_token_page().is_empty());
        assert!(flow.paste_org_error.is_some());

        flow.set_paste_org("https://dev.azure.com/contoso/Payments/_git/api");
        assert_eq!(
            flow.open_token_page(),
            [Effect::OpenUrl(
                "https://dev.azure.com/contoso/_usersSettings/tokens".into()
            )]
        );
        let effects = flow.pasted();
        assert_eq!(flow.screen, Screen::PasteVerify);
        assert!(effects.contains(&Effect::Verify {
            org: Organization::parse("contoso").unwrap(),
            level: AccessLevel::Review,
        }));
    }

    #[test]
    fn optional_scope_missing_can_continue_required_missing_cannot() {
        // AC-10: Build missing → listed, user can continue.
        let mut flow = at_paste_verify();
        flow.verify();
        let effects = flow.verified(
            Ok(verification(AccessLevel::Review, &["vso.build"], true)),
            now(),
        );
        assert!(effects.contains(&Effect::ForgetPastedToken));
        let Verifying::Done(result) = &flow.verifying else {
            panic!()
        };
        let summary = summarize(result);
        assert_eq!(
            summary
                .missing_optional
                .iter()
                .map(|s| s.title)
                .collect::<Vec<_>>(),
            ["Build (Read)"]
        );
        assert!(summary.missing_required.is_empty());
        flow.confirm(now());
        assert_eq!(flow.screen, Screen::Projects);

        let mut flow = at_paste_verify();
        flow.verify();
        flow.verified(
            Ok(verification(
                AccessLevel::Review,
                &["vso.code_write"],
                false,
            )),
            now(),
        );
        let effects = flow.confirm(now());
        assert_eq!(flow.screen, Screen::PasteVerify, "required missing blocks");
        assert!(matches!(effects.as_slice(), [Effect::Verify { .. }]));
    }

    #[test]
    fn fix_token_returns_to_the_checklist_with_missing_rows_highlighted() {
        let mut flow = at_paste_verify();
        flow.verify();
        flow.verified(
            Ok(verification(AccessLevel::Review, &["vso.build"], true)),
            now(),
        );
        flow.fix_token();
        assert_eq!(flow.screen, Screen::CreateInBrowser);
        assert_eq!(flow.highlight, ["vso.build"]);
    }

    fn at_paste_verify() -> Flow {
        let (mut flow, _) = Flow::start(now());
        flow.detected(AzStatus::NotInstalled);
        flow.confirm(now());
        flow.confirm(now());
        flow.set_paste_org("fabrikam");
        flow.confirm(now());
        assert_eq!(flow.screen, Screen::PasteVerify);
        flow
    }

    #[test]
    fn projects_default_selection_depends_on_count_and_zero_blocks_finish() {
        let few: Vec<_> = (0..3).map(|i| project(&i.to_string(), i)).collect();
        assert_eq!(ProjectPicker::new(few, &[]).selected.len(), 3);

        let many: Vec<_> = (0..9).map(|i| project(&i.to_string(), i)).collect();
        let mut picker = ProjectPicker::new(many, &[]);
        assert!(picker.selected.is_empty());
        assert_eq!(picker.projects[0].id, "8", "most recently updated first");

        picker.set_filter("project 1");
        picker.select_visible();
        assert_eq!(picker.selected.len(), 1);
        picker.set_filter("");
        picker.toggle(None);
        assert_eq!(picker.selected.len(), 2);
        picker.toggle(None);
        assert_eq!(picker.selected.len(), 1);
        picker.clear();
        assert!(picker.selected.is_empty());
    }

    #[test]
    fn projects_screen_needs_one_selection_then_finishes() {
        let mut flow = at_paste_verify();
        flow.verify();
        flow.verified(Ok(verification(AccessLevel::Review, &[], true)), now());
        flow.confirm(now());
        flow.projects_loaded(Ok(vec![project("a", 1)]), now());
        flow.picker_mut().unwrap().clear();
        assert!(flow.confirm(now()).is_empty());
        flow.picker_mut().unwrap().toggle(Some(0));
        flow.confirm(now());
        assert_eq!(flow.screen, Screen::Done);
        assert_eq!(flow.state().projects.len(), 1);
        assert_eq!(flow.state().step, Step::Done);
        assert!(flow.confirm(now()).contains(&Effect::Finish));
    }

    #[test]
    fn back_walks_history_and_never_leaves_onboarding() {
        let mut flow = at_pick_org(&["fabrikam"]);
        flow.back();
        assert_eq!(flow.screen, Screen::Connect);
        assert_eq!(flow.direction, Direction::Back);
        flow.back();
        assert_eq!(flow.screen, Screen::Welcome);
        assert!(flow.back().is_empty());
        assert_eq!(flow.screen, Screen::Welcome);
    }

    #[test]
    fn resume_skips_the_token_step_when_a_verified_token_exists() {
        let state = OnboardingState {
            step: Step::ReviewAccess,
            path: Some(ConnectPath::Az),
            org: Some("fabrikam".into()),
            ..OnboardingState::default()
        };
        let (flow, effects) = Flow::resume(&state, Some(&meta("fabrikam", false)), now());
        assert_eq!(flow.screen, Screen::Projects);
        assert!(effects.contains(&Effect::LoadProjects {
            org: Organization::parse("fabrikam").unwrap()
        }));
        assert!(flow.can_go_back());
    }

    #[test]
    fn resume_without_a_token_restarts_at_the_token_step() {
        let az_path = OnboardingState {
            step: Step::ReviewAccess,
            path: Some(ConnectPath::Az),
            org: Some("fabrikam".into()),
            lifetime_days: 180,
            ..OnboardingState::default()
        };
        let (flow, _) = Flow::resume(&az_path, None, now());
        assert_eq!(flow.screen, Screen::ReviewAccess);
        assert_eq!(flow.lifetime_days, 180);

        let paste = OnboardingState {
            step: Step::PasteVerify,
            path: Some(ConnectPath::Paste),
            org: Some("contoso".into()),
            ..OnboardingState::default()
        };
        let (flow, _) = Flow::resume(&paste, None, now());
        assert_eq!(flow.screen, Screen::CreateInBrowser);
        assert_eq!(flow.paste_org_text, "contoso");

        let late = OnboardingState {
            step: Step::Projects,
            ..OnboardingState::default()
        };
        let (flow, _) = Flow::resume(&late, None, now());
        assert_eq!(flow.screen, Screen::Connect);
    }

    #[test]
    fn fuzzy_matching_is_an_ordered_case_insensitive_subsequence() {
        assert!(fuzzy_contains("Fabrikam", "fbrk"));
        assert!(fuzzy_contains("Fabrikam", ""));
        assert!(!fuzzy_contains("Fabrikam", "kf"));
    }
}
