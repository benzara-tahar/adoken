//! The Settings window's root view: left rail (search + tabs), the active
//! tab's page, keyboard handling, and the in-memory copy of `settings.toml`
//! every page edits.
//!
//! Data flow: the file is read on the background executor when the window
//! opens and whenever [`super::observe`] fires (unless our own write is
//! still in flight). A control edits one table in memory, applies it to the
//! running app at once (sound engine, background controller, motion, inbox),
//! and queues the write ([`super::write_table`]).

use std::path::PathBuf;

use adoken_notify::NotifySettings;
use adoken_theme::background::{BackgroundSettings, ViewId};
use adoken_theme::{radius, size, space, typography};
use adoken_ui::{
    ActiveTheme, ChipTone, EmptyState, Icon, IconName, Kbd, Notice, TextInput, TextInputEvent,
    Theme, TypeStyleExt,
};
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, KeyDownEvent, MouseButton,
    ScrollHandle, Subscription, Task, Window, div, prelude::*, px,
};
use toml::Table;

use super::model::{self, Motion, Tab};
use super::pages::Embeds;

const RAIL_WIDTH: f32 = 220.0;
/// Page column measure.
pub(super) const PAGE_MAX_WIDTH: f32 = 680.0;
/// Room for the macOS traffic lights above the rail.
const TITLEBAR_INSET: f32 = size::TITLEBAR;

/// The file as last read.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Load {
    Loading,
    Ready,
    /// TOML syntax error: nothing is written until the user fixes the file.
    Broken(String),
}

/// Account tab state (real mode only; demo never reads the account).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Account {
    Demo,
    /// The connected account as [`crate::account`] holds it (read at
    /// launch: opening the tab does no IO and sends nothing, 0009 FR-8).
    Live,
    ConfirmSignOut {
        org: String,
    },
    SigningOut,
    SignedOut,
    Failed(String),
}

pub struct SettingsView {
    pub(super) tab: Tab,
    pub(super) focus: FocusHandle,
    pub(super) rail_focus: FocusHandle,
    pub(super) search: Entity<TextInput>,
    pub(super) query: String,
    pub(super) load: Load,
    /// The whole file, parsed.
    pub(super) doc: Table,
    /// Validation notes from the last read (clamped values, bad keys).
    pub(super) issues: Vec<String>,
    pub(super) demo: bool,
    /// Directory holding `settings.toml` (imports, demo illustration).
    pub(super) dir: Option<PathBuf>,
    /// Backgrounds: 0 = every view, `n` = `ViewId::ALL[n - 1]`.
    pub(super) bg_scope: usize,
    pub(super) bg_error: Option<String>,
    pub(super) account: Account,
    pub(super) licenses_open: bool,
    pub(super) embeds: Embeds,
    pub(super) scroll: ScrollHandle,
    /// Gallery: Sync tab activity and clock instead of the live scheduler.
    sync_story: Option<(adoken_sync::cadence::Activity, adoken_model::Millis)>,
    /// Gallery: the Account tab's token instead of the live account.
    account_story: Option<crate::account::Snapshot>,
    _load: Option<Task<()>>,
    pub(super) _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SettingsView {
    /// The live window: reads `settings.toml` in the background.
    pub fn new(tab: Tab, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let path = super::path(cx);
        let demo = super::is_demo(cx);
        let mut this = Self::empty(
            tab,
            demo,
            path.as_ref().and_then(|p| p.parent().map(PathBuf::from)),
            window,
            cx,
        );
        this._subscriptions.push(super::observe(cx, |this, cx| {
            if !super::is_writing(cx) {
                this.reload(cx);
            }
        }));
        this.reload(cx);
        this.load_account(cx);
        this.link_inbox(cx);
        this.tick_sync(cx);
        this
    }

    /// Gallery story of the Sync tab: `activity` stands in for the running
    /// scheduler, `now` for the clock.
    pub fn sync_story(
        text: &str,
        activity: adoken_sync::cadence::Activity,
        now: adoken_model::Millis,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::story(Tab::Sync, text, false, window, cx);
        this.sync_story = Some((activity, now));
        this
    }

    /// The Sync tab's clock.
    pub(super) fn now_ms(&self) -> adoken_model::Millis {
        self.sync_story
            .as_ref()
            .map_or_else(crate::browse::now_ms, |(_, now)| *now)
    }

    /// What the scheduler last reported; `None` while sync isn't running.
    pub(super) fn sync_activity(&self, cx: &App) -> Option<adoken_sync::cadence::Activity> {
        match &self.sync_story {
            Some((activity, _)) => Some(activity.clone()),
            None => crate::sync::activity(cx).map(|a| a.borrow().clone()),
        }
    }

    /// Repaint the Sync tab every few seconds ("synced 12s ago", new
    /// passes, the measured cost); nothing runs on other tabs.
    fn tick_sync(&mut self, cx: &mut Context<Self>) {
        self._tasks.push(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(5))
                    .await;
                let alive = this.update(cx, |this, cx| {
                    if this.tab == Tab::Sync {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        }));
    }

    /// Gallery stories: `text` stands in for `settings.toml`; nothing is
    /// read or written.
    pub fn story(
        tab: Tab,
        text: &str,
        demo: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::empty(tab, demo, None, window, cx);
        this.set_text(Ok(text.to_owned()));
        if !demo {
            this.account_story = Some(crate::account::Snapshot {
                meta: story_meta(),
                problem: None,
                replace: crate::account::ReplaceState::Idle,
            });
        }
        this.show_tab(tab, window, cx);
        this
    }

    fn empty(
        tab: Tab,
        demo: bool,
        dir: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| TextInput::new("Search settings", cx));
        let search_events = cx.subscribe(&search, |this, input, event, cx| {
            if let TextInputEvent::Changed = event {
                this.query = input.read(cx).text().to_owned();
                // Keep the shown tab if it still matches; else jump.
                if !model::tab_matches(&this.query, this.tab)
                    && let Some(tab) = model::first_match(&this.query)
                {
                    this.tab = tab;
                }
                cx.notify();
            }
        });
        let _ = window;
        Self {
            tab,
            focus: cx.focus_handle(),
            rail_focus: cx.focus_handle(),
            search,
            query: String::new(),
            load: Load::Loading,
            doc: Table::new(),
            issues: Vec::new(),
            demo,
            dir,
            bg_scope: 0,
            bg_error: None,
            account: if demo { Account::Demo } else { Account::Live },
            licenses_open: false,
            embeds: Embeds::default(),
            scroll: ScrollHandle::new(),
            sync_story: None,
            account_story: None,
            _load: None,
            _tasks: Vec::new(),
            _subscriptions: vec![search_events],
        }
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// Show `tab` and move focus into its page.
    pub fn show_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = tab;
        self.scroll.set_offset(gpui::point(px(0.0), px(0.0)));
        self.embeds.ensure(tab, window, cx);
        super::remember_tab(tab, cx);
        cx.notify();
    }

    /// Put `query` in the search box (a palette command landing on one
    /// setting); an embedded control the query names takes focus.
    pub fn search_for(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.set_text(query, cx));
        self.query = query.to_owned();
        self.embeds.ensure(self.tab, window, cx);
        self.embeds.focus_for(query, window, cx);
        cx.notify();
    }

    /// Re-read the file off the UI thread.
    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(path) = super::path(cx) else {
            return;
        };
        let job = cx.background_executor().spawn(async move {
            match std::fs::read_to_string(&path) {
                Ok(text) => Ok(text),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
                Err(error) => Err(error.to_string()),
            }
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let text = job.await;
            this.update(cx, |this, cx| {
                this.set_text(text);
                cx.notify();
            })
            .ok();
        }));
    }

    fn set_text(&mut self, text: Result<String, String>) {
        match text.and_then(|text| toml::from_str::<Table>(&text).map_err(|e| e.to_string())) {
            Ok(doc) => {
                let mut issues = model::parse_nudges(&doc).1;
                issues.extend(Motion::from_document(&doc).1);
                issues.extend(super::sync_prefs::parse(&doc).1);
                issues.extend(crate::celebrate::settings_issues(&doc));
                issues.extend(crate::tours::settings::settings_issues(&doc));
                issues.extend(crate::hotkey::parse_issues(&doc));
                issues.extend(
                    crate::activity::settings::parse(&doc, crate::activity::main_window_height()).1,
                );
                if let Some(Err(error)) = doc
                    .get(adoken_notify::settings::SETTINGS_TABLE)
                    .map(|table| table.clone().try_into::<NotifySettings>())
                {
                    issues.push(format!(
                        "notify: {}; using the defaults",
                        error.message().trim()
                    ));
                }
                self.doc = doc;
                self.issues = issues;
                self.load = Load::Ready;
            }
            Err(error) => {
                self.doc = Table::new();
                self.issues.clear();
                self.load = Load::Broken(error);
            }
        }
    }

    /// Read `[name]`, change it, keep it, apply it live, queue the write.
    /// Does nothing while the file is broken (it would be overwritten).
    pub(super) fn edit_table(
        &mut self,
        name: &str,
        change: impl FnOnce(&mut Table),
        cx: &mut Context<Self>,
    ) {
        if matches!(self.load, Load::Broken(_)) {
            return;
        }
        let mut table = model::table_of(&self.doc, name);
        change(&mut table);
        self.doc
            .insert(name.to_owned(), toml::Value::Table(table.clone()));
        apply_live(name, &table, &self.doc, cx);
        super::write_table(cx, name, table);
        cx.notify();
    }

    /// Cancel a pending sign-out confirmation.
    pub(super) fn load_account_again(&mut self, cx: &mut Context<Self>) {
        self.account = Account::Live;
        cx.notify();
    }

    /// What the Account tab shows: the story's token, else the live one.
    pub(super) fn account_snapshot(&self, cx: &App) -> Option<crate::account::Snapshot> {
        self.account_story
            .clone()
            .or_else(|| crate::account::snapshot(cx))
    }

    /// Sign out (spec 0001 AC-15): deletes the Keychain item and saved
    /// setup; never revokes in Azure DevOps from here, never deletes synced
    /// data. Unreachable in demo mode.
    pub(super) fn sign_out(&mut self, cx: &mut Context<Self>) {
        if self.demo {
            return;
        }
        self.account = Account::SigningOut;
        let job = gpui_tokio::Tokio::spawn(cx, async move {
            adoken_ado::Connector::new(adoken_ado::data_dir())
                .sign_out(
                    adoken_ado::SignOut {
                        revoke: false,
                        delete_local_data: false,
                    },
                    None,
                )
                .await
        });
        self._tasks.push(cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| {
                this.account = match result {
                    Ok(Ok(_)) => Account::SignedOut,
                    Ok(Err(error)) => Account::Failed(error.to_string()),
                    Err(error) => Account::Failed(error.to_string()),
                };
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Repaint the Account tab when the account changes (Replace token,
    /// rejection, a refused write).
    fn load_account(&mut self, cx: &mut Context<Self>) {
        if self.demo {
            return;
        }
        if let Some(model) = crate::account::model(cx) {
            self._subscriptions
                .push(cx.observe(&model, |_, _, cx| cx.notify()));
        }
    }

    /// Gallery stories of Settings › Account (0009 AC-14–AC-16).
    pub fn account_story(story: AccountStory, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self::story(Tab::Account, "", false, window, cx);
        this.account_story = Some(story.snapshot());
        this
    }

    /// Repaint when the inbox's mutes change (Notifications › Muted).
    fn link_inbox(&mut self, cx: &mut Context<Self>) {
        if let Some(inbox) = crate::inbox::linked(cx) {
            self._subscriptions
                .push(cx.observe(&inbox, |_, _, cx| cx.notify()));
        }
    }

    pub(super) fn visible(&self, title: &str, keywords: &str) -> bool {
        model::row_visible(&self.query, self.tab, title, keywords)
    }

    fn step_tab(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let next = model::step_tab(self.tab, delta, &self.query);
        if next != self.tab {
            self.show_tab(next, window, cx);
        }
    }

    fn clear_search(&mut self, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| search.set_text("", cx));
        self.query.clear();
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        let in_search = self.search.focus_handle(cx).is_focused(window);
        let in_rail = self.rail_focus.is_focused(window);
        match keystroke.key.as_str() {
            "escape" => {
                cx.stop_propagation();
                if self.query.is_empty() {
                    window.remove_window();
                } else {
                    self.clear_search(cx);
                }
            }
            "enter" if in_search => {
                cx.stop_propagation();
                if let Some(tab) = model::first_match(&self.query) {
                    self.show_tab(tab, window, cx);
                }
                window.focus(&self.rail_focus, cx);
            }
            "down" if in_search => {
                cx.stop_propagation();
                window.focus(&self.rail_focus, cx);
            }
            "up" | "k" if in_rail => {
                cx.stop_propagation();
                self.step_tab(-1, window, cx);
            }
            "down" | "j" if in_rail => {
                cx.stop_propagation();
                self.step_tab(1, window, cx);
            }
            _ => {}
        }
    }

    fn render_rail(
        &self,
        theme: &Theme,
        rail_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = &theme.colors;
        let tabs =
            Tab::ALL.into_iter().map(|tab| {
                let selected = tab == self.tab;
                let enabled = model::tab_matches(&self.query, tab);
                div()
                    .id(tab.id())
                    .h(px(size::CONTROL_MD))
                    .px(px(space::S8))
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .rounded(px(radius::SM))
                    .text_color(match (selected, enabled) {
                        (_, false) => colors.text_disabled,
                        (true, true) => colors.text_primary,
                        (false, true) => colors.text_secondary,
                    })
                    .when(selected, |row| {
                        row.bg(colors.bg_selected).when(rail_focused, |row| {
                            row.border_1().border_color(colors.border_focus)
                        })
                    })
                    .when(!selected && enabled, |row| {
                        row.cursor_pointer().hover(|s| s.bg(colors.bg_hover))
                    })
                    .when(enabled, |row| {
                        row.on_click(cx.listener(move |this, _, window, cx| {
                            this.show_tab(tab, window, cx);
                            window.focus(&this.rail_focus, cx);
                        }))
                    })
                    .child(Icon::new(tab.icon()).size(size::ICON_MD).color(
                        if selected && enabled {
                            colors.accent_fg
                        } else if enabled {
                            colors.text_tertiary
                        } else {
                            colors.text_disabled
                        },
                    ))
                    .child(tab.label())
            });
        div()
            .flex_none()
            .w(px(RAIL_WIDTH))
            .h_full()
            .flex()
            .flex_col()
            .bg(colors.bg_sidebar)
            .border_r_1()
            .border_color(colors.border_subtle)
            .type_style(typography::BODY, theme)
            .child(
                div()
                    .id("settings-rail-drag")
                    .flex_none()
                    .h(px(TITLEBAR_INSET))
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
            )
            .child(
                div()
                    .flex_none()
                    .mx(px(space::S8))
                    .mb(px(space::S8))
                    .h(px(size::CONTROL_MD))
                    .px(px(space::S8))
                    .flex()
                    .items_center()
                    .gap(px(space::S6))
                    .rounded(px(radius::SM))
                    .border_1()
                    .border_color(colors.border_control)
                    .bg(colors.bg_input)
                    .child(
                        Icon::new(IconName::Search)
                            .size(size::ICON_SM)
                            .color(colors.text_tertiary),
                    )
                    .child(div().flex_1().min_w_0().child(self.search.clone()))
                    .child(Kbd::parse(super::SEARCH_KEYS).expect("static binding")),
            )
            .child(
                div()
                    .id("settings-rail")
                    .track_focus(&self.rail_focus)
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(space::S2))
                    .px(px(space::S8))
                    .children(tabs),
            )
    }

    fn render_page(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = &theme.colors;
        let mut blocks: Vec<AnyElement> = Vec::new();
        if let Load::Broken(error) = &self.load {
            let file = super::path(cx)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| adoken_settings::FILE_NAME.into());
            blocks.push(
                Notice::new(
                    ChipTone::Danger,
                    "settings.toml has a syntax error, so changes aren't saved",
                )
                .icon(IconName::CircleAlert)
                .detail(format!("{file}: {error}"))
                .into_any_element(),
            );
        }
        if let Some(error) = super::write_error(cx) {
            blocks.push(
                Notice::new(ChipTone::Danger, "Couldn't save settings")
                    .icon(IconName::CircleAlert)
                    .detail(error)
                    .into_any_element(),
            );
        }
        if !self.issues.is_empty() {
            blocks.push(
                Notice::new(
                    ChipTone::Warning,
                    "Some settings in settings.toml were adjusted",
                )
                .icon(IconName::CircleAlert)
                .detail(self.issues.join("\n"))
                .into_any_element(),
            );
        }
        let sections = self.render_tab(theme, window, cx);
        let empty = sections.is_empty();
        blocks.extend(sections);
        if empty {
            blocks.push(
                EmptyState::new(IconName::Search, "No settings match")
                    .body(format!(
                        "Nothing on {} matches “{}”.",
                        self.tab.label(),
                        self.query.trim()
                    ))
                    .into_any_element(),
            );
        }
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("settings-title-drag")
                    .flex_none()
                    .h(px(TITLEBAR_INSET))
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
            )
            .child(
                div()
                    .id("settings-page")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        div()
                            .max_w(px(PAGE_MAX_WIDTH))
                            .px(px(space::S32))
                            .pb(px(space::S48))
                            .flex()
                            .flex_col()
                            .gap(px(space::S24))
                            .child(
                                div()
                                    .text_color(colors.text_primary)
                                    .type_style(typography::TITLE, theme)
                                    .child(self.tab.label()),
                            )
                            .children(blocks),
                    ),
            )
            .into_any_element()
    }
}

/// Apply one edited table to the running app before it reaches the disk.
pub(super) fn apply_live(name: &str, table: &Table, doc: &Table, cx: &mut App) {
    match name {
        adoken_sound::SETTINGS_TABLE => {
            let (settings, _) = adoken_sound::SoundSettings::from_table(table.clone());
            crate::sound::install(settings, cx);
        }
        adoken_theme::background::SETTINGS_TABLE => {
            let (settings, _) = BackgroundSettings::from_table(table.clone());
            crate::background::set_settings(settings, cx);
        }
        super::sync_prefs::SYNC_TABLE => {
            crate::sync::set_prefs(super::sync_prefs::parse(doc).0, cx);
        }
        crate::activity::settings::TABLE => crate::activity::apply_live(table, cx),
        model::GENERAL_TABLE => {
            super::apply_motion(Motion::from_document(doc).0, cx);
        }
        adoken_notify::settings::SETTINGS_TABLE => {
            if let Ok(settings) = toml::Value::Table(table.clone()).try_into::<NotifySettings>() {
                crate::inbox::set_notify_settings(settings, cx);
            }
        }
        _ => {}
    }
}

/// Label for a background view scope.
pub(super) fn view_label(view: ViewId) -> &'static str {
    match view {
        ViewId::Onboarding => "Onboarding",
        ViewId::MyWork => "Today",
        ViewId::Inbox => "Inbox",
        ViewId::PullRequests => "Pull requests",
        ViewId::PrReview => "PR review",
        ViewId::WorkItems => "Work items",
        ViewId::Pipelines => "Pipelines",
        ViewId::SearchResults => "Search",
        ViewId::Settings => "Settings",
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let page = self.render_page(&theme, window, cx);
        let rail_focused = self.rail_focus.is_focused(window);
        div()
            .id("settings")
            .key_context(super::CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .bg(colors.bg_app)
            .text_color(colors.text_primary)
            .type_style(typography::BODY, &theme)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(
                cx.listener(|this, _: &super::NextTab, window, cx| this.step_tab(1, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &super::PreviousTab, window, cx| {
                    this.step_tab(-1, window, cx)
                }),
            )
            .on_action(|_: &crate::sync::SyncNow, _, cx| crate::sync::sync_now(cx))
            .on_action(cx.listener(|this, _: &super::FocusSearch, window, cx| {
                let handle = this.search.focus_handle(cx);
                window.focus(&handle, cx);
            }))
            .on_action(|_: &super::CloseSettings, window, _| window.remove_window())
            .on_action(|_: &crate::FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &crate::FocusPrevious, window, cx| window.focus_prev(cx))
            .child(self.render_rail(&theme, rail_focused, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(colors.bg_surface)
                    .child(page),
            )
    }
}

/// The Account tab's gallery states (design §10.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStory {
    /// An `az` Review token with every scope.
    Az,
    /// A pasted Review token: write scopes not checked yet, Build missing.
    Pasted,
    /// Sync latched a 401; no expiry known.
    Rejected,
    /// Sync latched a 401 after the known expiry.
    Expired,
    /// The background replacement made this token (owner decision 2).
    ReplacedAutomatically,
}

impl AccountStory {
    fn snapshot(self) -> crate::account::Snapshot {
        use crate::account::{ReplaceState, Snapshot, TokenProblem};
        let mut meta = story_meta();
        let mut problem = None;
        match self {
            Self::Az => {}
            Self::Pasted => {
                meta.source = adoken_ado::PatSource::Pasted;
                meta.authorization_id = None;
                meta.lifetime_days = None;
                meta.scopes.retain(|id| id != "vso.build");
                meta.untested = vec!["vso.code_write".into(), "vso.work_write".into()];
            }
            Self::Rejected => {
                meta.source = adoken_ado::PatSource::Pasted;
                meta.valid_to = None;
                problem = Some(TokenProblem::Rejected);
            }
            Self::Expired => {
                problem = meta.valid_to.map(|at| TokenProblem::Expired(at.date()));
            }
            Self::ReplacedAutomatically => meta.auto_replaced = true,
        }
        Snapshot {
            meta,
            problem,
            replace: ReplaceState::Idle,
        }
    }
}

/// The settings stories' token: an `az` Review token for fabrikam.
fn story_meta() -> adoken_ado::PatMeta {
    adoken_ado::PatMeta {
        org: "fabrikam".into(),
        level: adoken_ado::AccessLevel::Review,
        scopes: adoken_ado::scopes(adoken_ado::AccessLevel::Review)
            .iter()
            .map(|scope| scope.id.to_owned())
            .collect(),
        untested: Vec::new(),
        authorization_id: Some("story".into()),
        valid_to: time::OffsetDateTime::from_unix_timestamp(1_798_761_600).ok(),
        created_at: time::OffsetDateTime::from_unix_timestamp(1_791_000_000)
            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH),
        source: adoken_ado::PatSource::Az,
        lifetime_days: Some(90),
        lifetime_capped: false,
        verified: true,
        me: Some(adoken_ado::Identity {
            id: String::new(),
            display_name: "Mira Patel".into(),
        }),
        auto_replaced: false,
        auto_replace: Default::default(),
    }
}
