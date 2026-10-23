//! Onboarding: connect Azure DevOps (docs/specs/0001-onboarding.md,
//! docs/design/onboarding.md).
//!
//! [`flow::Flow`] owns every decision; this view renders it, forwards input,
//! and runs its [`flow::Effect`]s. All backend work (az subprocesses, HTTP,
//! Keychain) goes through `adoken_ado::Connector` on the Tokio runtime via
//! `gpui_tokio`, and file writes on the background executor, so the UI
//! thread never waits. The backend owns the spec's timeouts; the view adds
//! the 5-minute `az login` limit (design 1a).

mod flow;
mod story;
mod view;

use std::time::{Duration, Instant};

use adoken_ado::{
    AccessLevel, AzCli, AzStatus, CancelToken, ConnectPath, Connector, OnboardError,
    OnboardingState, Organization, Pat, PatMeta, Replaced, TokenSource,
};
use adoken_ui::TextInput;
use adoken_ui::text_input::TextInputEvent;
use gpui::{
    App, AppContext as _, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable,
    KeyBinding, ScrollHandle, Subscription, Task, Window, actions,
};
use gpui_tokio::Tokio;

pub use flow::Screen;
use flow::{Effect, Flow, Load, Verifying};
pub use story::Story;
pub(crate) use view::scope_row;

actions!(
    onboarding,
    [
        /// Run the current screen's primary action.
        Confirm,
        /// Go to the previous onboarding screen.
        Back,
        /// Move to the next option or row.
        SelectNext,
        /// Move to the previous option or row.
        SelectPrev,
        /// Choose the first option (Use Azure CLI).
        ChooseFirst,
        /// Choose the second option (Paste a token).
        ChooseSecond,
        /// Toggle the focused checkbox row.
        ToggleRow,
        /// Select every visible project.
        SelectAllRows,
        /// Clear the project selection.
        ClearRows,
        /// Paste a token from anywhere on the token screens.
        PasteToken,
        /// Cancel the running step or close a disclosure.
        Cancel,
    ]
);

const KEY_CONTEXT: &str = "Onboarding";
/// Single-key bindings must not fire while typing in a field.
const NAV_CONTEXT: &str = "Onboarding && !TextInput";
pub const BACK_KEYS: &str = "secondary-[";

pub fn bindings() -> Vec<KeyBinding> {
    let cx = Some(KEY_CONTEXT);
    let nav = Some(NAV_CONTEXT);
    vec![
        KeyBinding::new("enter", Confirm, cx),
        KeyBinding::new(BACK_KEYS, Back, cx),
        KeyBinding::new("secondary-left", Back, cx),
        KeyBinding::new("down", SelectNext, cx),
        KeyBinding::new("up", SelectPrev, cx),
        KeyBinding::new("escape", Cancel, cx),
        KeyBinding::new("j", SelectNext, nav),
        KeyBinding::new("k", SelectPrev, nav),
        KeyBinding::new("1", ChooseFirst, nav),
        KeyBinding::new("2", ChooseSecond, nav),
        KeyBinding::new("space", ToggleRow, nav),
        KeyBinding::new("secondary-a", SelectAllRows, nav),
        KeyBinding::new("secondary-shift-a", ClearRows, nav),
        KeyBinding::new("secondary-v", PasteToken, nav),
    ]
}

/// Emitted when the user leaves onboarding for the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnboardingEvent {
    Finished,
    /// The sheet saved a replacement (spec 0001 FR-16): `replaced` on the
    /// `az` path, `None` for a pasted token.
    Replaced {
        meta: Box<PatMeta>,
        replaced: Option<Box<Replaced>>,
    },
    /// The sheet was closed without replacing anything.
    Closed,
}

/// How onboarding was entered.
pub enum Start {
    Fresh,
    Resume {
        state: Box<OnboardingState>,
        account: Option<Box<PatMeta>>,
    },
    /// Replace token / Change access over the app (design §10.2).
    Sheet {
        org: Organization,
        level: AccessLevel,
    },
}

pub struct Onboarding {
    flow: Flow,
    connector: Connector,
    focus: FocusHandle,

    org_filter: Entity<TextInput>,
    manual_org: Entity<TextInput>,
    paste_org: Entity<TextInput>,
    token: Entity<TextInput>,
    project_filter: Entity<TextInput>,
    /// 2q "Expires on (optional)".
    expires: Entity<TextInput>,
    token_revealed: bool,
    org_scroll: ScrollHandle,
    project_scroll: ScrollHandle,

    /// Entra token source from `az` for steps 2a–2b; memory only (FR-6),
    /// dropped (and zeroized by the backend) when onboarding ends.
    token_source: Option<TokenSource>,
    sign_in_cancel: Option<CancelToken>,
    /// The sheet's `az` replacement, for the Account tab's note.
    replaced: Option<Replaced>,
    /// The token 2q saved (sheet), with its expiry once entered.
    pasted: Option<PatMeta>,
    /// The new token's name on 2b (`adoken · {host}`), read once.
    token_name: gpui::SharedString,

    /// Welcome intro played or skipped (design §6: any key jumps to the end).
    welcome_settled: bool,
    /// "Why these?" disclosure (design §4) and az failure details.
    why_open: bool,
    details_open: bool,
    /// Verified scope rows revealed so far (80 ms stagger, design §6).
    revealed: usize,
    /// Bumped on every rejected token; keys the field shake.
    shakes: u64,

    detect_task: Option<Task<()>>,
    sign_in_task: Option<Task<()>>,
    sign_in_timeout: Option<Task<()>>,
    prompt_task: Option<Task<()>>,
    orgs_task: Option<Task<()>>,
    create_task: Option<Task<()>>,
    verify_task: Option<Task<()>>,
    projects_task: Option<Task<()>>,
    advance_task: Option<Task<()>>,
    reveal_task: Option<Task<()>>,
    tick_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<OnboardingEvent> for Onboarding {}

impl Focusable for Onboarding {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Spacing between verified scope rows resolving (design §6).
const REVEAL_STAGGER: Duration = Duration::from_millis(80);

impl Onboarding {
    pub fn new(
        connector: Connector,
        start: Start,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let now = Instant::now();
        let (flow, effects) = match &start {
            Start::Fresh => Flow::start(now),
            Start::Resume { state, account } => Flow::resume(state, account.as_deref(), now),
            Start::Sheet { org, level } => Flow::sheet(org.clone(), *level, now),
        };
        let mut this = Self::with_flow(connector, flow, window, cx);
        this.run(effects, window, cx);
        // Design 1: the az card stays quiet for 150 ms, then shows a spinner.
        this.tick_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(flow::DETECTION_QUIET).await;
            this.update(cx, |this, cx| {
                this.tick_task = None;
                cx.notify();
            })
            .ok();
        }));
        this
    }

    /// The view over `flow`, before any of its effects run.
    fn with_flow(
        connector: Connector,
        flow: Flow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let org_filter = cx.new(|cx| TextInput::new("Filter organizations", cx));
        let manual_org = cx.new(|cx| TextInput::new("organization", cx));
        let paste_org = cx.new(|cx| TextInput::new("organization", cx));
        let token = cx.new(|cx| {
            let mut input = TextInput::new("Paste token (⌘V)", cx);
            input.set_masked(true, cx);
            input
        });
        let project_filter = cx.new(|cx| TextInput::new("Filter projects", cx));
        let expires =
            cx.new(|cx| TextInput::new("Date Azure DevOps showed when you created it", cx));
        paste_org.update(cx, |input, cx| input.set_text(&flow.paste_org_text, cx));

        let subscriptions = vec![
            cx.subscribe_in(&org_filter, window, |this, input, event, _, cx| {
                if matches!(event, TextInputEvent::Changed) {
                    let text = input.read(cx).text().to_owned();
                    this.flow.set_org_filter(&text);
                    cx.notify();
                }
            }),
            cx.subscribe_in(&manual_org, window, |this, input, event, _, cx| {
                if matches!(event, TextInputEvent::Changed) {
                    let text = input.read(cx).text().to_owned();
                    this.flow.set_manual_org(&text);
                    cx.notify();
                }
            }),
            cx.subscribe_in(&paste_org, window, |this, input, event, _, cx| {
                if matches!(event, TextInputEvent::Changed) {
                    let text = input.read(cx).text().to_owned();
                    this.flow.set_paste_org(&text);
                    cx.notify();
                }
            }),
            cx.subscribe_in(&token, window, |this, _, event, window, cx| {
                // Design 2q: pasting verifies immediately; typing needs ↩.
                match event {
                    TextInputEvent::Pasted => this.apply(window, cx, |flow, _| flow.verify()),
                    TextInputEvent::Changed => {
                        if matches!(this.flow.verifying, Verifying::Malformed(_)) {
                            this.flow.verifying = Verifying::Idle;
                        }
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&project_filter, window, |this, input, event, _, cx| {
                if matches!(event, TextInputEvent::Changed) {
                    let text = input.read(cx).text().to_owned();
                    if let Some(picker) = this.flow.picker_mut() {
                        picker.set_filter(&text);
                    }
                    cx.notify();
                }
            }),
            cx.subscribe_in(&expires, window, |this, input, event, _, cx| {
                if matches!(event, TextInputEvent::Changed) {
                    let text = input.read(cx).text().to_owned();
                    this.flow.set_expires_text(&text);
                    cx.notify();
                }
            }),
        ];

        Self {
            flow,
            connector,
            focus: cx.focus_handle(),
            org_filter,
            manual_org,
            paste_org,
            token,
            project_filter,
            expires,
            token_revealed: false,
            org_scroll: ScrollHandle::new(),
            project_scroll: ScrollHandle::new(),
            token_source: None,
            sign_in_cancel: None,
            replaced: None,
            pasted: None,
            token_name: adoken_ado::pat_display_name().into(),
            welcome_settled: false,
            why_open: false,
            details_open: false,
            revealed: 0,
            shakes: 0,
            detect_task: None,
            sign_in_task: None,
            sign_in_timeout: None,
            prompt_task: None,
            orgs_task: None,
            create_task: None,
            verify_task: None,
            projects_task: None,
            advance_task: None,
            reveal_task: None,
            tick_task: None,
            _subscriptions: subscriptions,
        }
    }

    /// Run a flow transition, its effects, and the screen-change focus move.
    fn apply(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut Flow, Instant) -> Vec<Effect>,
    ) {
        let before = (self.flow.screen, self.flow.transition);
        let effects = update(&mut self.flow, Instant::now());
        self.run(effects, window, cx);
        if (self.flow.screen, self.flow.transition) != before {
            self.screen_changed(window, cx);
        }
        cx.notify();
    }

    /// Focus the new screen's field, else the flow root (design §7: focus
    /// moves with the step).
    fn screen_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.why_open = false;
        self.details_open = false;
        self.welcome_settled = true;
        let input = match self.flow.screen {
            Screen::PickOrg if self.flow.manual_org => Some(self.manual_org.clone()),
            Screen::CreateInBrowser if self.flow.paste_org_text.is_empty() => {
                Some(self.paste_org.clone())
            }
            Screen::PasteVerify if !self.token_saved() => Some(self.token.clone()),
            _ => None,
        };
        if let Screen::CreateInBrowser = self.flow.screen {
            let text = self.flow.paste_org_text.clone();
            self.paste_org
                .update(cx, |input, cx| input.set_text(&text, cx));
        }
        match input {
            Some(input) => window.focus(&input.focus_handle(cx), cx),
            None => window.focus(&self.focus, cx),
        }
    }

    fn token_saved(&self) -> bool {
        matches!(&self.flow.verifying, Verifying::Done(result) if result.saved.is_some())
    }

    /// Execute effects; results come back through `apply`.
    fn run(&mut self, effects: Vec<Effect>, window: &mut Window, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                Effect::Detect => self.detect(window, cx),
                Effect::SignIn { device_code } => self.sign_in(device_code, window, cx),
                Effect::CancelSignIn => {
                    if let Some(cancel) = self.sign_in_cancel.take() {
                        cancel.cancel();
                    }
                    self.sign_in_timeout = None;
                    self.prompt_task = None;
                }
                Effect::LoadOrgs => self.load_orgs(window, cx),
                Effect::CreatePat {
                    org,
                    level,
                    lifetime_days,
                } => self.create_pat(org, level, lifetime_days, window, cx),
                Effect::Verify { org, level } => self.verify(org, level, window, cx),
                Effect::ForgetPastedToken => {
                    self.token_revealed = false;
                    self.token.update(cx, |input, cx| input.set_text("", cx));
                }
                Effect::LoadProjects { org } => self.load_projects(org, window, cx),
                Effect::OpenUrl(url) => cx.open_url(&url),
                Effect::CopyText(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
                Effect::AdvanceAfter(delay) => {
                    self.advance_task = Some(cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(delay).await;
                        this.update_in(cx, |this, window, cx| {
                            this.apply(window, cx, |flow, _| flow.advance())
                        })
                        .ok();
                    }));
                }
                Effect::Persist => {
                    // The sheet never touches saved progress (design §10.2).
                    if !self.flow.sheet {
                        self.persist(cx);
                    }
                }
                Effect::Finish => {
                    // FR-6: the Entra token source dies with onboarding.
                    self.token_source = None;
                    cx.emit(OnboardingEvent::Finished);
                }
                Effect::ReplacePat {
                    level,
                    lifetime_days,
                } => self.replace_pat(level, lifetime_days, window, cx),
                Effect::SaveExpiry(date) => self.save_expiry(date, cx),
                Effect::SheetDone => self.sheet_done(cx),
            }
        }
    }

    fn detect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let connector = self.connector.clone();
        let job = Tokio::spawn(cx, async move { connector.detect_az().await });
        self.detect_task = Some(cx.spawn_in(window, async move |this, cx| {
            let status = job.await.unwrap_or_else(|error| AzStatus::Failed {
                reason: error.to_string(),
            });
            this.update_in(cx, |this, window, cx| {
                this.apply(window, cx, |flow, _| {
                    flow.detected(status);
                    Vec::new()
                })
            })
            .ok();
        }));
    }

    fn detected_az(&self) -> Option<AzCli> {
        self.flow.detection.status().and_then(AzStatus::az).cloned()
    }

    fn sign_in(&mut self, device_code: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(az) = self.detected_az() else {
            self.apply(window, cx, |flow, _| {
                flow.signed_in(Err(OnboardError::AzUnavailable {
                    reason: "Azure CLI wasn't found. Check again, or paste a token instead.".into(),
                }))
            });
            return;
        };
        let cancel = CancelToken::new();
        self.sign_in_cancel = Some(cancel.clone());
        let connector = self.connector.clone();
        let job = if device_code {
            let (prompts, mut received) = futures::channel::mpsc::unbounded();
            self.prompt_task = Some(cx.spawn_in(window, async move |this, cx| {
                use futures::StreamExt as _;
                while let Some(prompt) = received.next().await {
                    this.update(cx, |this, cx| {
                        this.flow.device_code(prompt);
                        cx.notify();
                    })
                    .ok();
                }
            }));
            Tokio::spawn(cx, async move {
                connector
                    .az_login_device_code(&az, cancel, move |prompt| {
                        prompts.unbounded_send(prompt).ok();
                    })
                    .await
            })
        } else {
            Tokio::spawn(cx, async move { connector.az_login(&az, cancel).await })
        };
        self.sign_in_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = join_result(job.await);
            this.update_in(cx, |this, window, cx| {
                this.sign_in_timeout = None;
                this.sign_in_cancel = None;
                this.apply(window, cx, |flow, _| flow.signed_in(result))
            })
            .ok();
        }));
        self.sign_in_timeout = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(flow::SIGN_IN_TIMEOUT).await;
            this.update_in(cx, |this, window, cx| {
                this.apply(window, cx, |flow, _| flow.sign_in_timed_out())
            })
            .ok();
        }));
    }

    /// The Entra token source, obtained from the detected `az` (re-detecting
    /// after a relaunch, since it is never persisted).
    fn source_job(
        &self,
        cx: &mut Context<Self>,
    ) -> Task<Result<Result<TokenSource, OnboardError>, gpui_tokio::JoinError>> {
        let connector = self.connector.clone();
        let existing = self.token_source.clone();
        let known = match self.flow.detection.status() {
            Some(AzStatus::Ready { az, .. }) => Some(az.clone()),
            _ => None,
        };
        Tokio::spawn(cx, async move {
            if let Some(source) = existing {
                return Ok(source);
            }
            let az = match known {
                Some(az) => az,
                None => match connector.detect_az().await {
                    AzStatus::Ready { az, .. } => az,
                    AzStatus::NotLoggedIn { .. } => {
                        return Err(OnboardError::AzUnavailable {
                            reason: "Azure CLI isn't signed in. Go back and sign in, or paste a token instead.".into(),
                        });
                    }
                    AzStatus::Failed { reason } => {
                        return Err(OnboardError::AzUnavailable { reason });
                    }
                    _ => {
                        return Err(OnboardError::AzUnavailable {
                            reason: "Azure CLI isn't available. Paste a token instead.".into(),
                        });
                    }
                },
            };
            connector.token_source(&az).await
        })
    }

    fn load_orgs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let connector = self.connector.clone();
        let source = self.source_job(cx);
        self.orgs_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = match join_result(source.await) {
                Ok(source) => {
                    let job = Tokio::spawn(cx, async move {
                        let default = connector.default_org(source.az()).await;
                        let orgs = connector.list_orgs(&source).await;
                        (source, orgs, default)
                    });
                    match job.await {
                        Ok(done) => Ok(done),
                        Err(error) => Err(OnboardError::Other(error.to_string())),
                    }
                }
                Err(error) => Err(error),
            };
            this.update_in(cx, |this, window, cx| {
                let now = Instant::now();
                match result {
                    Ok((source, orgs, default)) => {
                        this.token_source = Some(source);
                        let many = matches!(&orgs, Ok(list) if list.orgs.len() > flow::ORG_FILTER_THRESHOLD);
                        this.flow.orgs_loaded(orgs, default, now);
                        if this.flow.screen == Screen::PickOrg {
                            if this.flow.manual_org {
                                window.focus(&this.manual_org.focus_handle(cx), cx);
                            } else if many {
                                window.focus(&this.org_filter.focus_handle(cx), cx);
                            }
                        }
                    }
                    Err(error) => this.flow.orgs_loaded(Err(error), None, now),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn create_pat(
        &mut self,
        org: adoken_ado::Organization,
        level: AccessLevel,
        lifetime_days: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let connector = self.connector.clone();
        let source = self.source_job(cx);
        self.create_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = match join_result(source.await) {
                Ok(source) => {
                    let job = Tokio::spawn(cx, async move {
                        let meta = connector
                            .create_pat(&source, &org, level, lifetime_days)
                            .await;
                        (source, meta)
                    });
                    match job.await {
                        Ok((source, meta)) => {
                            this.update(cx, |this, _| this.token_source = Some(source))
                                .ok();
                            meta
                        }
                        Err(error) => Err(OnboardError::Other(error.to_string())),
                    }
                }
                Err(error) => Err(error),
            };
            this.update_in(cx, |this, window, cx| {
                this.apply(window, cx, |flow, now| flow.pat_created(result, now))
            })
            .ok();
        }));
    }

    /// The sheet's `az` path (spec 0001 FR-16): create with the current set
    /// → swap → verify → revoke the old token.
    fn replace_pat(
        &mut self,
        level: AccessLevel,
        lifetime_days: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let connector = self.connector.clone();
        let source = self.source_job(cx);
        self.create_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = match join_result(source.await) {
                Ok(source) => {
                    let job = Tokio::spawn(cx, async move {
                        let replaced = connector.replace(&source, level, lifetime_days).await;
                        (source, replaced)
                    });
                    match job.await {
                        Ok((source, replaced)) => {
                            this.update(cx, |this, _| this.token_source = Some(source))
                                .ok();
                            replaced
                        }
                        Err(error) => Err(OnboardError::Other(error.to_string())),
                    }
                }
                Err(error) => Err(error),
            };
            this.update_in(cx, |this, window, cx| {
                let meta = result
                    .as_ref()
                    .map(|replaced| replaced.meta.clone())
                    .map_err(Clone::clone);
                this.replaced = result.ok();
                this.apply(window, cx, |flow, now| flow.pat_created(meta, now))
            })
            .ok();
        }));
    }

    fn verify(
        &mut self,
        org: adoken_ado::Organization,
        level: AccessLevel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pat = match Pat::parse(self.token.read(cx).text()) {
            Ok(pat) => pat,
            Err(error) => {
                self.flow.malformed_token(error.to_string());
                self.shakes += 1;
                return;
            }
        };
        self.revealed = 0;
        self.reveal_task = None;
        let connector = self.connector.clone();
        let job = Tokio::spawn(cx, async move {
            connector.verify_pasted(&org, pat, level, None).await
        });
        self.verify_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = join_result(job.await);
            this.update_in(cx, |this, window, cx| {
                if matches!(result, Err(OnboardError::InvalidToken)) {
                    this.shakes += 1;
                }
                this.pasted = result.as_ref().ok().and_then(|v| v.saved.clone());
                this.apply(window, cx, |flow, now| flow.verified(result, now));
                this.start_reveal(window, cx);
            })
            .ok();
        }));
    }

    /// "Expires on" saved with Continue (Path B step 5), off the UI thread.
    fn save_expiry(&mut self, date: time::Date, cx: &mut Context<Self>) {
        if let Some(meta) = &mut self.pasted {
            meta.valid_to = Some(date.midnight().assume_utc());
        }
        let connector = self.connector.clone();
        let job = Tokio::spawn(
            cx,
            async move { connector.set_pasted_expiry(Some(date)).await },
        );
        cx.background_spawn(async move {
            if let Ok(Err(error)) = job.await {
                eprintln!("adoken: couldn't save the token's expiry date: {error}");
            }
        })
        .detach();
    }

    /// The sheet saved a replacement: hand it to the app and close.
    fn sheet_done(&mut self, cx: &mut Context<Self>) {
        self.token_source = None;
        let meta = match &self.replaced {
            Some(replaced) => Some(replaced.meta.clone()),
            None => self.pasted.clone(),
        };
        if let Some(meta) = meta {
            cx.emit(OnboardingEvent::Replaced {
                meta: Box::new(meta),
                replaced: self.replaced.take().map(Box::new),
            });
        }
    }

    /// Reveal verified rows one by one; all at once under reduce motion.
    fn start_reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Verifying::Done(result) = &self.flow.verifying else {
            return;
        };
        let total = result.probes.len();
        if cx.reduce_motion() {
            self.revealed = total;
            return;
        }
        self.revealed = 0;
        self.reveal_task = Some(cx.spawn_in(window, async move |this, cx| {
            for _ in 0..total {
                cx.background_executor().timer(REVEAL_STAGGER).await;
                let alive = this.update(cx, |this, cx| {
                    this.revealed += 1;
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
    }

    fn load_projects(
        &mut self,
        org: adoken_ado::Organization,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let connector = self.connector.clone();
        let job = Tokio::spawn(cx, async move { connector.list_projects(&org, None).await });
        self.projects_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = join_result(job.await);
            this.update_in(cx, |this, window, cx| {
                let many = matches!(&result, Ok(p) if p.len() > flow::PROJECT_PRESELECT_MAX);
                this.flow.projects_loaded(result, Instant::now());
                this.project_filter
                    .update(cx, |input, cx| input.set_text("", cx));
                if many && this.flow.screen == Screen::Projects {
                    window.focus(&this.project_filter.focus_handle(cx), cx);
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Save non-secret progress off the UI thread (FR-10).
    fn persist(&self, cx: &mut Context<Self>) {
        let state = self.flow.state();
        let dir = self.connector.data_dir().to_owned();
        cx.background_spawn(async move {
            if let Err(error) = state.save(&dir) {
                eprintln!("adoken: couldn't save onboarding progress: {error}");
            }
        })
        .detach();
    }

    /// Re-render once a second while a `Retry-After` countdown is showing.
    fn tick_while_counting(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let counting = [
            match &self.flow.minting {
                flow::Minting::Failed(f) => f.retry_in(now),
                _ => None,
            },
            match &self.flow.orgs {
                Load::Failed(f) => f.retry_in(now),
                _ => None,
            },
            match &self.flow.projects {
                Load::Failed(f) => f.retry_in(now),
                _ => None,
            },
            match &self.flow.verifying {
                Verifying::Failed(f) => f.retry_in(now),
                _ => None,
            },
        ]
        .iter()
        .any(Option::is_some);
        if counting && self.tick_task.is_none() {
            self.tick_task = Some(cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                this.update(cx, |this, cx| {
                    this.tick_task = None;
                    cx.notify();
                })
                .ok();
            }));
        }
    }

    // Actions ------------------------------------------------------------

    /// Whether a control other than the flow root or a text field has focus
    /// (e.g. a Tab-focused button): its own Enter/Space handling wins.
    fn control_focused(&self, window: &Window, cx: &App) -> bool {
        let Some(focused) = window.focused(cx) else {
            return false;
        };
        focused != self.focus
            && ![
                &self.org_filter,
                &self.manual_org,
                &self.paste_org,
                &self.token,
                &self.project_filter,
                &self.expires,
            ]
            .iter()
            .any(|input| input.focus_handle(cx) == focused)
    }

    fn on_confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        if self.control_focused(window, cx) {
            cx.propagate();
            return;
        }
        self.welcome_settled = true;
        self.apply(window, cx, |flow, now| flow.confirm(now));
    }

    fn on_back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        self.apply(window, cx, |flow, _| flow.back());
    }

    fn on_move(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        match self.flow.screen {
            Screen::Connect => {
                self.flow.toggle_choice();
                cx.notify();
            }
            Screen::PickOrg if !self.flow.manual_org => {
                self.flow.move_org(delta);
                self.org_scroll.scroll_to_item(self.flow.org_cursor);
                cx.notify();
            }
            Screen::Projects => {
                if let Some(picker) = self.flow.picker_mut() {
                    picker.move_cursor(delta);
                    let cursor = picker.cursor;
                    self.project_scroll.scroll_to_item(cursor);
                }
                cx.notify();
            }
            _ => cx.propagate(),
        }
        let _ = window;
    }

    fn on_choose(&mut self, path: ConnectPath, cx: &mut Context<Self>) {
        if self.flow.screen == Screen::Connect {
            self.flow.select_choice(path);
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    fn on_toggle(&mut self, _: &ToggleRow, window: &mut Window, cx: &mut Context<Self>) {
        if self.control_focused(window, cx) {
            cx.propagate();
            return;
        }
        if let Some(picker) = self.flow.picker_mut() {
            picker.toggle(None);
            cx.notify();
        }
    }

    fn on_select_all(&mut self, _: &SelectAllRows, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(picker) = self.flow.picker_mut() {
            picker.select_visible();
            cx.notify();
        }
    }

    fn on_clear(&mut self, _: &ClearRows, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(picker) = self.flow.picker_mut() {
            picker.clear();
            cx.notify();
        }
    }

    /// ⌘V on 2p/2q outside a field: put the clipboard in the token field and
    /// verify (design §8).
    fn on_paste(&mut self, _: &PasteToken, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(
            self.flow.screen,
            Screen::CreateInBrowser | Screen::PasteVerify
        ) || self.token_saved()
        {
            cx.propagate();
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.token
            .update(cx, |input, cx| input.set_text(text.trim(), cx));
        self.apply(window, cx, |flow, _| flow.pasted());
    }

    fn on_cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        if self.why_open || self.details_open {
            self.why_open = false;
            self.details_open = false;
            cx.notify();
            return;
        }
        if self.flow.screen == Screen::PickOrg && self.flow.manual_org {
            self.flow.manual_org = false;
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        // Esc never exits onboarding (design §8); it only cancels az login.
        // The sheet over the app closes when there's nothing to cancel.
        let waiting = matches!(self.flow.sign_in, flow::SignIn::Waiting { .. });
        if self.flow.sheet && !waiting {
            self.close_sheet(cx);
            return;
        }
        self.apply(window, cx, |flow, _| flow.cancel_sign_in());
    }

    /// Close the sheet without replacing anything.
    fn close_sheet(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = self.sign_in_cancel.take() {
            cancel.cancel();
        }
        self.token_source = None;
        cx.emit(OnboardingEvent::Closed);
    }
}

/// Flatten a Tokio join failure into the flow's error type.
fn join_result<T>(
    result: Result<Result<T, OnboardError>, gpui_tokio::JoinError>,
) -> Result<T, OnboardError> {
    result.unwrap_or_else(|error| Err(OnboardError::Other(error.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_key_bindings_never_fire_while_typing_in_a_field() {
        // `j`, `1`, `space`, ⌘V must reach the text field, not the flow.
        let predicate = gpui::KeyBindingContextPredicate::parse(NAV_CONTEXT).unwrap();
        let mut root = gpui::KeyContext::default();
        root.add(KEY_CONTEXT);
        let mut field = gpui::KeyContext::default();
        field.add(adoken_ui::text_input::KEY_CONTEXT);
        assert!(predicate.eval(&[root.clone()]));
        assert!(!predicate.eval(&[root, field]));
    }
}
