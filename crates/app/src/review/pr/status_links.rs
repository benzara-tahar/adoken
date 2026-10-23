//! The review header's status changes and linked work items (spec 0004
//! FR-16, FR-17; user decision 2026-09-30).
//!
//! - **Status:** the header's status chip opens a menu with the changes valid
//!   now (Publish / Mark as draft / Abandon… / Reactivate); `⇧S` opens it,
//!   `⇧P` publishes or marks as draft, `⇧A` asks to abandon. The header and
//!   the list's chip change on the next frame and revert with the reason if
//!   Azure DevOps refuses. Abandon asks first.
//! - **Work items:** the header lists the PR's linked work items (the
//!   store's, then Azure DevOps' own list fetched at open, which wins).
//!   Clicking one opens it in the Work items view; its unlink button asks
//!   first. "Link work item…" (`⇧W`) opens a picker searching the synced
//!   work items with the `Cmd+P` engine and query language (`#1234` and a
//!   pasted work item link work too); `↩` links. A token without
//!   `vso.work_write` gets one passive explanation instead (spec 0001
//!   FR-9, FR-14: no token action).
//!
//! Store reads and searches run on the background executor.

use std::rc::Rc;
use std::sync::Arc;

use adoken_model::WorkItem;
use adoken_sync::writes::WriteError;
use adoken_sync::writes::{Applied, ReviewWrite};
use adoken_theme::elevation;
use adoken_ui::{TextInput, TextInputEvent, Tooltip};
use gpui::{Div, Entity, ScrollStrategy, Stateful, Subscription, UniformListScrollHandle};

use super::compose::{Overlay, press};
use super::*;
use crate::palette::catalog::{Catalog, CatalogData};
use crate::palette::step_selection;
use crate::review::writes::{
    self as review_writes, LINK_SCOPE_HINT, LinkedItem, READ_ONLY_HINT, StatusAction,
};
use crate::workitems::pick::{PickRow, load_catalog, picker_rows};
use crate::workitems::{state_tone, type_style};

const MENU_WIDTH: f32 = 380.0;
const CONFIRM_WIDTH: f32 = 440.0;
const PICKER_WIDTH: f32 = 560.0;
const PICKER_ROW: f32 = size::PALETTE_ROW_COMPACT;
/// Rows before the picker scrolls; the half row hints there is more.
const PICKER_VISIBLE_ROWS: f32 = 8.5;
/// A linked item's title in the header before it ellipsizes.
const LINKED_TITLE_WIDTH: f32 = 280.0;

#[derive(Default)]
pub(in crate::review) struct LinkState {
    status_menu: Option<StatusMenu>,
    confirm: Option<Confirm>,
    picker: Option<Picker>,
    /// The Work items section's rows, by id.
    items: Vec<LinkedItem>,
    /// The PR's links per Azure DevOps, once fetched (FR-17: it wins).
    server: Option<Vec<i64>>,
    /// Work items with a link or unlink in flight.
    pending: HashSet<i64>,
    /// Azure DevOps refused a link for the token's scopes: Link explains
    /// itself, like a token known to lack them.
    scope_refused: bool,
    /// A status change is in flight (one at a time).
    status_sending: bool,
    _load: Option<Task<()>>,
}

struct StatusMenu {
    actions: Vec<StatusAction>,
    selected: usize,
    focus: FocusHandle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Confirming {
    Abandon,
    Unlink(i64),
}

struct Confirm {
    what: Confirming,
    focus: FocusHandle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerMode {
    Link,
    Unlink,
}

struct Picker {
    mode: PickerMode,
    input: Entity<TextInput>,
    /// Link mode: the synced work items, indexed (`None` while loading).
    catalog: Option<Arc<Catalog>>,
    rows: Vec<PickRow>,
    selected: usize,
    scroll: UniformListScrollHandle,
    /// Bumped per query: a slower, older search never overwrites a newer.
    generation: u64,
    _search: Option<Task<()>>,
    _catalog: Option<Task<()>>,
    _input: Subscription,
}

/// The Unlink picker's rows: linked items whose `#id title` contains
/// `text` (case-insensitive).
pub fn unlink_rows(items: &[LinkedItem], text: &str) -> Vec<PickRow> {
    let needle = text.trim().trim_start_matches('#').to_lowercase();
    items
        .iter()
        .filter(|row| {
            let title = row.item.as_ref().map_or("", |w| w.title.as_str());
            format!("{} {}", row.id, title.to_lowercase()).contains(&needle)
        })
        .map(|row| match &row.item {
            Some(item) => PickRow::Item(Box::new(item.clone())),
            None => PickRow::NotSynced(row.id),
        })
        .collect()
}

pub(in crate::review) fn status_icon(action: StatusAction) -> IconName {
    match action {
        StatusAction::Publish => IconName::GitPullRequest,
        StatusAction::MarkDraft => IconName::GitPullRequestDraft,
        StatusAction::Abandon => IconName::GitPullRequestClosed,
        StatusAction::Reactivate => IconName::RefreshCw,
    }
}

/// The key a status action is bound to (in the review and the list).
pub(in crate::review) fn status_keys(action: StatusAction) -> Option<&'static str> {
    match action {
        StatusAction::Publish | StatusAction::MarkDraft => Some(DRAFT_KEYS),
        StatusAction::Abandon => Some(ABANDON_KEYS),
        StatusAction::Reactivate => None,
    }
}

fn kbd(keys: &str) -> Kbd {
    Kbd::parse(keys).expect("static binding")
}

/// A click handler shared by the review and the list's overlays.
pub(in crate::review) type Handler<T> = Rc<dyn Fn(T, &mut Window, &mut App)>;

/// The status menu (spec 0004 FR-16), the same in the review and over the
/// list (spec 0019 FR-18); the caller positions it. `↑`/`↓`/`↩`/`Esc`
/// arrive as the review's menu actions through its key context.
pub(in crate::review) fn status_menu_panel(
    actions: &[StatusAction],
    selected: usize,
    focus: &FocusHandle,
    theme: &adoken_ui::Theme,
    pick: Handler<StatusAction>,
    close: Handler<()>,
) -> Stateful<Div> {
    let colors = &theme.colors;
    let rows = actions.iter().enumerate().map(|(i, action)| {
        let action = *action;
        let pick = pick.clone();
        div()
            .id(("status-menu", i))
            .px(px(space::S8))
            .py(px(space::S6))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .rounded(px(radius::SM))
            .cursor_pointer()
            .when(i == selected, |d| d.bg(colors.bg_selected))
            .when(i != selected, |d| d.hover(|s| s.bg(colors.bg_hover)))
            .role(gpui::Role::MenuItem)
            .aria_label(action.label())
            .child(
                Icon::new(status_icon(action))
                    .size(size::ICON_SM)
                    .color(colors.text_secondary),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_color(colors.text_primary)
                            .type_style(typography::BODY, theme)
                            .child(action.label()),
                    )
                    .child(
                        div()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::SMALL, theme)
                            .child(action.detail()),
                    ),
            )
            .children(status_keys(action).map(kbd))
            .on_click(move |_, window, cx| pick(action, window, cx))
    });
    div()
        .id("status-menu")
        .key_context(STATUS_MENU_CONTEXT)
        .track_focus(focus)
        .w(px(MENU_WIDTH))
        .p(px(space::S4))
        .rounded(px(radius::MD))
        .border_1()
        .border_color(colors.border_default)
        .bg(colors.bg_overlay)
        .shadow(theme.shadows(elevation::LEVEL_2))
        .role(gpui::Role::Menu)
        .aria_label("Change status")
        .occlude()
        .on_mouse_down_out(move |_, window, cx| close((), window, cx))
        .child(
            div()
                .px(px(space::S8))
                .py(px(space::S4))
                .text_color(colors.text_tertiary)
                .type_style(typography::LABEL, theme)
                .child("Change status"),
        )
        .children(rows)
}

/// Abandon's confirmation copy: title, body, confirm button.
pub(in crate::review) fn abandon_text(id: i64) -> (String, String, &'static str) {
    (
        format!("Abandon !{id}?"),
        "It closes without merging and leaves reviewers' lists. You can reactivate it later."
            .to_owned(),
        "Abandon",
    )
}

/// A centered confirmation over a scrim (`↩` / `Esc` arrive as the
/// review's confirm actions through its key context).
pub(in crate::review) fn confirm_dialog(
    title: String,
    body: String,
    yes: &'static str,
    focus: &FocusHandle,
    theme: &adoken_ui::Theme,
    accept: Handler<()>,
    cancel: Handler<()>,
) -> Stateful<Div> {
    let colors = &theme.colors;
    let outside = cancel.clone();
    div()
        .id("review-confirm-scrim")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(colors.bg_scrim)
        .occlude()
        .child(
            div()
                .id("review-confirm")
                .key_context(CONFIRM_CONTEXT)
                .track_focus(focus)
                .role(gpui::Role::AlertDialog)
                .aria_label(title.clone())
                .on_mouse_down_out(move |_, window, cx| outside((), window, cx))
                .w(px(CONFIRM_WIDTH))
                .flex()
                .flex_col()
                .gap(px(space::S12))
                .p(px(space::S20))
                .rounded(px(radius::LG))
                .border_1()
                .border_color(colors.border_default)
                .bg(colors.bg_overlay)
                .shadow(theme.shadows(elevation::LEVEL_3))
                .type_style(typography::BODY, theme)
                .child(
                    div()
                        .text_color(colors.text_primary)
                        .type_style(typography::HEADING, theme)
                        .child(title),
                )
                .child(div().text_color(colors.text_secondary).child(body))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(space::S8))
                        .child(
                            Button::new("confirm-yes", yes)
                                .variant(ButtonVariant::Primary)
                                .shortcut(kbd("enter"))
                                .on_press(move |window, cx| accept((), window, cx)),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("confirm-cancel", "Cancel")
                                .variant(ButtonVariant::Ghost)
                                .shortcut(kbd("escape"))
                                .on_press(move |window, cx| cancel((), window, cx)),
                        ),
                ),
        )
}

impl PrReview {
    // ── state ────────────────────────────────────────────────────────────

    pub(super) fn link_overlay(&self) -> Option<Overlay> {
        let links = &self.links;
        if links.status_menu.is_some() {
            Some(Overlay::StatusMenu)
        } else if links.confirm.is_some() {
            Some(Overlay::Confirm)
        } else if links.picker.is_some() {
            Some(Overlay::WorkItemPicker)
        } else {
            None
        }
    }

    pub(super) fn clear_link_overlays(&mut self) {
        self.links.status_menu = None;
        self.links.confirm = None;
        self.links.picker = None;
    }

    pub(super) fn close_link_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.link_overlay().is_some() {
            self.clear_link_overlays();
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// The Work items section's rows (tests, stories).
    pub fn linked_work_items(&self) -> &[LinkedItem] {
        &self.links.items
    }

    /// The picker's rows (tests).
    pub fn picker_rows(&self) -> Option<&[PickRow]> {
        self.links.picker.as_ref().map(|p| p.rows.as_slice())
    }

    /// A status change is on its way to Azure DevOps.
    pub(super) fn status_sending(&self) -> bool {
        self.links.status_sending
    }

    pub(super) fn status_actions(&self) -> Vec<StatusAction> {
        review_writes::status_actions(
            self.loaded.read_only,
            self.loaded.pr(),
            self.loaded.is_completing(),
        )
    }

    /// Link work item… is usable: write access, a token that may link, and
    /// Azure DevOps hasn't refused one for its scopes.
    fn link_blocker(&self, cx: &App) -> Option<&'static str> {
        if self.loaded.read_only {
            Some(READ_ONLY_HINT)
        } else if !review_writes::access(cx).can_link_work_items() || self.links.scope_refused {
            Some(LINK_SCOPE_HINT)
        } else {
            None
        }
    }

    fn set_rows(&mut self, stored: Vec<WorkItem>) {
        self.links.items = review_writes::linked_items(stored, self.links.server.as_deref());
    }

    /// FR-17: the store's linked items at once, then Azure DevOps' list
    /// (fetched and stored by sync's writer; the demo org has only the
    /// store's).
    pub(super) fn load_work_items(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let id = self.pr_id();
        let fetch = review_writes::access(cx).linked_work_items(self.pr_ref(), cx);
        self.links._load = Some(cx.spawn(async move |this, cx| {
            let read = |store: Store| async move { store.pr_work_items(id) };
            match cx.background_spawn(read(store.clone())).await {
                Ok(stored) => {
                    this.update(cx, |this, cx| {
                        this.set_rows(stored);
                        cx.notify();
                    })
                    .ok();
                }
                Err(error) => log::warn!("review: couldn't read !{id}'s work items: {error}"),
            }
            let Some(fetch) = fetch else {
                return;
            };
            match fetch.await {
                Ok(ids) => {
                    let stored = cx.background_spawn(read(store)).await.unwrap_or_default();
                    this.update(cx, |this, cx| {
                        this.links.server = Some(ids);
                        this.set_rows(stored);
                        cx.notify();
                    })
                    .ok();
                }
                Err(error) => log::warn!("review: couldn't fetch !{id}'s work items: {error}"),
            }
        }));
    }

    // ── status (FR-16) ───────────────────────────────────────────────────

    fn open_status_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let actions = self.status_actions();
        if actions.is_empty() {
            let pr = self.loaded.pr();
            let why = if self.loaded.read_only {
                READ_ONLY_HINT.to_owned()
            } else if self.loaded.is_completing() {
                format!("Azure DevOps is completing !{}", pr.id)
            } else {
                format!("!{} is completed", pr.id)
            };
            self.set_notice(ChipTone::Neutral, why, cx);
            return;
        }
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.clear_overlays();
        self.links.status_menu = Some(StatusMenu {
            actions,
            selected: 0,
            focus,
        });
        cx.notify();
    }

    /// A status change from the menu, a key or `Cmd+P`: explained when not
    /// valid now, confirmed first when it's Abandon.
    pub(super) fn request_status(
        &mut self,
        action: StatusAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_link_overlays(window, cx);
        let refusal = review_writes::status_refusal(
            self.loaded.read_only,
            self.loaded.pr(),
            self.loaded.is_completing(),
            action,
        );
        if let Some(why) = refusal {
            self.set_notice(ChipTone::Neutral, why, cx);
            return;
        }
        if action == StatusAction::Abandon {
            self.open_confirm(Confirming::Abandon, window, cx);
            return;
        }
        self.change_status(action, cx);
    }

    /// Send `action`: the header and the list's chip show it now; Azure
    /// DevOps' answer replaces it, a refusal reverts it with the reason.
    fn change_status(&mut self, action: StatusAction, cx: &mut Context<Self>) {
        if self.links.status_sending {
            return;
        }
        let now = self.now();
        let before = review_writes::apply_status_action(&mut self.loaded.detail.pr, action, now);
        self.links.status_sending = true;
        self.clear_notice();
        cx.emit(PrChanged(self.loaded.pr().clone()));
        cx.notify();
        let write = action.write(self.pr_ref());
        self.send(write, cx, move |this, result, cx| {
            this.links.status_sending = false;
            match result {
                Ok(Applied::Pr(pr)) => {
                    let id = pr.id;
                    if pr.status != adoken_model::PrStatus::Active {
                        // Closing drops the auto-complete I set.
                        this.loaded.auto_complete = None;
                    }
                    this.loaded.detail.pr = pr;
                    this.set_notice(ChipTone::Success, action.done(id), cx);
                }
                Ok(_) => {}
                Err(error) => {
                    review_writes::revert_status(&mut this.loaded.detail.pr, before);
                    this.set_notice(ChipTone::Danger, format!("Status not changed. {error}"), cx);
                }
            }
            cx.emit(PrChanged(this.loaded.pr().clone()));
        });
    }

    // ── confirmations ────────────────────────────────────────────────────

    fn open_confirm(&mut self, what: Confirming, window: &mut Window, cx: &mut Context<Self>) {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.clear_overlays();
        self.links.confirm = Some(Confirm { what, focus });
        cx.notify();
    }

    fn accept_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(confirm) = self.links.confirm.take() else {
            return;
        };
        window.focus(&self.focus, cx);
        match confirm.what {
            Confirming::Abandon => self.change_status(StatusAction::Abandon, cx),
            Confirming::Unlink(id) => self.unlink(id, cx),
        }
        cx.notify();
    }

    // ── work items (FR-17) ───────────────────────────────────────────────

    fn open_picker(&mut self, mode: PickerMode, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(why) = self.link_blocker(cx) {
            self.set_notice(ChipTone::Neutral, why, cx);
            return;
        }
        if mode == PickerMode::Unlink && self.links.items.is_empty() {
            self.set_notice(ChipTone::Neutral, "No work item is linked", cx);
            return;
        }
        let placeholder = match mode {
            PickerMode::Link => "Search work items, #1234, or paste a link",
            PickerMode::Unlink => "Filter linked work items",
        };
        let input = cx.new(|cx| TextInput::new(placeholder, cx));
        let events = cx.subscribe(&input, |this, input, event, cx| {
            if *event == TextInputEvent::Changed {
                let text = input.read(cx).text().to_owned();
                this.search_picker(text, cx);
            }
        });
        window.focus(&input.focus_handle(cx), cx);
        self.clear_overlays();
        self.links.picker = Some(Picker {
            mode,
            input,
            catalog: None,
            rows: Vec::new(),
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            generation: 0,
            _search: None,
            _catalog: None,
            _input: events,
        });
        match mode {
            PickerMode::Unlink => self.search_picker(String::new(), cx),
            PickerMode::Link => self.load_picker_catalog(cx),
        }
        cx.notify();
    }

    /// The synced work items, indexed off the UI thread (stories without a
    /// store use the demo dataset's).
    fn load_picker_catalog(&mut self, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let job = cx.background_spawn(async move {
            match store {
                Some(store) => load_catalog(&store).map_err(|e| e.to_string()),
                None => {
                    let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
                    let data = CatalogData::from_dataset(&ds);
                    let data = CatalogData {
                        prs: Vec::new(),
                        runs: Vec::new(),
                        repos: Vec::new(),
                        projects: Vec::new(),
                        ..data
                    };
                    Ok(Catalog::new(data, Vec::new()))
                }
            }
        });
        let task = cx.spawn(async move |this, cx| {
            let catalog = job.await;
            this.update(cx, |this, cx| {
                let Some(picker) = this.links.picker.as_mut() else {
                    return;
                };
                match catalog {
                    Ok(catalog) => {
                        picker.catalog = Some(Arc::new(catalog));
                        let text = picker.input.read(cx).text().to_owned();
                        this.search_picker(text, cx);
                    }
                    Err(error) => {
                        picker.rows = vec![PickRow::Note(
                            format!("Couldn't read work items: {error}").into(),
                        )];
                        cx.notify();
                    }
                }
            })
            .ok();
        });
        if let Some(picker) = self.links.picker.as_mut() {
            picker._catalog = Some(task);
        }
    }

    /// Re-run the picker's query: Link searches the catalog on the
    /// background executor; Unlink filters the linked items in place.
    fn search_picker(&mut self, text: String, cx: &mut Context<Self>) {
        let items = self.links.items.clone();
        let now = self.loaded.now;
        let Some(picker) = self.links.picker.as_mut() else {
            return;
        };
        picker.generation += 1;
        match picker.mode {
            PickerMode::Unlink => {
                picker.rows = unlink_rows(&items, &text);
                picker.selected = 0;
                cx.notify();
            }
            PickerMode::Link => {
                let Some(catalog) = picker.catalog.clone() else {
                    return;
                };
                let generation = picker.generation;
                let job = cx.background_spawn(async move { picker_rows(&catalog, &text, now) });
                picker._search = Some(cx.spawn(async move |this, cx| {
                    let rows = job.await;
                    this.update(cx, |this, cx| {
                        if let Some(picker) = this.links.picker.as_mut()
                            && picker.generation == generation
                        {
                            picker.rows = rows;
                            picker.selected = 0;
                            picker.scroll.scroll_to_item(0, ScrollStrategy::Top);
                            cx.notify();
                        }
                    })
                    .ok();
                }));
            }
        }
    }

    fn choose_pick(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = &self.links.picker else {
            return;
        };
        let mode = picker.mode;
        let Some(pick) = picker.rows.get(row).cloned() else {
            return;
        };
        let Some(id) = pick.id() else {
            return;
        };
        match mode {
            PickerMode::Link => {
                if self.links.items.iter().any(|r| r.id == id) {
                    self.close_link_overlays(window, cx);
                    self.set_notice(ChipTone::Neutral, format!("#{id} is already linked"), cx);
                    return;
                }
                self.close_link_overlays(window, cx);
                let item = match pick {
                    PickRow::Item(item) => Some(*item),
                    _ => None,
                };
                self.link(id, item, cx);
            }
            PickerMode::Unlink => self.open_confirm(Confirming::Unlink(id), window, cx),
        }
    }

    /// Link work item `id`: its row shows at once (pending); Azure DevOps'
    /// copy replaces it, a refusal removes it with the reason.
    fn link(&mut self, id: i64, item: Option<WorkItem>, cx: &mut Context<Self>) {
        self.links.items.push(LinkedItem { id, item });
        self.links.items.sort_by_key(|r| r.id);
        self.links.pending.insert(id);
        self.clear_notice();
        cx.notify();
        let write = ReviewWrite::LinkWorkItem {
            pr: self.pr_ref(),
            work_item: id,
        };
        self.send(write, cx, move |this, result, cx| {
            this.links.pending.remove(&id);
            match result {
                Ok(applied) => {
                    if let Applied::WorkItem(Some(stored)) = applied
                        && let Some(row) = this.links.items.iter_mut().find(|r| r.id == id)
                    {
                        row.item = Some(stored);
                    }
                    if let Some(server) = this.links.server.as_mut()
                        && !server.contains(&id)
                    {
                        server.push(id);
                    }
                    let pr = this.loaded.pr().id;
                    this.set_notice(ChipTone::Success, format!("Linked #{id} to !{pr}"), cx);
                }
                Err(error) => {
                    this.links.items.retain(|r| r.id != id);
                    if matches!(error, WriteError::MissingWorkScope { .. }) {
                        this.links.scope_refused = true;
                    }
                    this.set_notice(ChipTone::Danger, format!("Not linked. {error}"), cx);
                }
            }
        });
    }

    /// Unlink work item `id` (confirmed): its row goes at once and comes
    /// back with the reason if Azure DevOps refuses.
    fn unlink(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(at) = self.links.items.iter().position(|r| r.id == id) else {
            return;
        };
        let removed = self.links.items.remove(at);
        self.links.pending.insert(id);
        self.clear_notice();
        cx.notify();
        let write = ReviewWrite::UnlinkWorkItem {
            pr: self.pr_ref(),
            work_item: id,
        };
        self.send(write, cx, move |this, result, cx| {
            this.links.pending.remove(&id);
            match result {
                Ok(_) => {
                    if let Some(server) = this.links.server.as_mut() {
                        server.retain(|linked| *linked != id);
                    }
                    let pr = this.loaded.pr().id;
                    this.set_notice(ChipTone::Success, format!("Unlinked #{id} from !{pr}"), cx);
                }
                Err(error) => {
                    this.links.items.push(removed);
                    this.links.items.sort_by_key(|r| r.id);
                    if matches!(error, WriteError::MissingWorkScope { .. }) {
                        this.links.scope_refused = true;
                    }
                    this.set_notice(ChipTone::Danger, format!("Still linked. {error}"), cx);
                }
            }
        });
    }

    fn ask_unlink(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(why) = self.link_blocker(cx) {
            self.set_notice(ChipTone::Neutral, why, cx);
            return;
        }
        self.open_confirm(Confirming::Unlink(id), window, cx);
    }

    // ── overlay keys (dispatched by `compose`) ───────────────────────────

    pub(super) fn step_link_overlay(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(menu) = self.links.status_menu.as_mut() {
            menu.selected = step_selection(menu.selected, menu.actions.len(), delta);
            cx.notify();
        } else if let Some(picker) = self.links.picker.as_mut() {
            picker.selected = step_selection(picker.selected, picker.rows.len(), delta);
            picker
                .scroll
                .scroll_to_item(picker.selected, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    pub(super) fn confirm_link_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = &self.links.status_menu {
            let action = menu.actions[menu.selected];
            self.request_status(action, window, cx);
        } else if self.links.confirm.is_some() {
            self.accept_confirm(window, cx);
        } else if let Some(picker) = &self.links.picker {
            let row = picker.selected;
            self.choose_pick(row, window, cx);
        }
    }

    pub(super) fn on_link_actions(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        root.on_action(cx.listener(|this, _: &OpenStatusMenu, w, cx| this.open_status_menu(w, cx)))
            .on_action(cx.listener(|this, _: &ToggleDraft, w, cx| {
                let action = review_writes::draft_toggle(this.loaded.pr());
                this.request_status(action, w, cx)
            }))
            .on_action(cx.listener(|this, _: &AbandonPr, w, cx| {
                this.request_status(StatusAction::Abandon, w, cx)
            }))
            .on_action(cx.listener(|this, _: &ReactivatePr, w, cx| {
                this.request_status(StatusAction::Reactivate, w, cx)
            }))
            .on_action(cx.listener(|this, _: &LinkWorkItem, w, cx| {
                this.open_picker(PickerMode::Link, w, cx)
            }))
            .on_action(cx.listener(|this, _: &UnlinkWorkItem, w, cx| {
                this.open_picker(PickerMode::Unlink, w, cx)
            }))
    }

    // ── render ───────────────────────────────────────────────────────────

    /// The header's status control (`review.status`): a bordered menu
    /// trigger with its `⇧S` keycap always shown when a change is valid
    /// (spec 0019 FR-20), else the plain chip.
    pub(super) fn render_status_chip(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let pr = self.loaded.pr();
        let (_, _, label) = pr_status(pr.status, pr.is_draft, colors);
        let chip = if self.loaded.is_completing() {
            // Spec 0004 FR-10: Azure DevOps is merging my Complete; sync
            // flips this to Completed.
            Chip::new("Completing…").tone(ChipTone::Accent)
        } else {
            Chip::new(label).tone(if pr.is_draft {
                ChipTone::Neutral
            } else {
                ChipTone::Accent
            })
        };
        // Spec 0022 FR-8/FR-9: a confirmed merge celebrates from this chip.
        let anchor = crate::celebrate::Anchor::PullRequest(pr.id);
        let chip = div()
            .relative()
            .flex()
            .items_center()
            .gap(px(space::S4))
            .children(crate::celebrate::track(anchor, cx))
            .child(chip)
            .children(crate::celebrate::glyph(anchor, cx));
        if self.status_actions().is_empty() {
            return chip.into_any_element();
        }
        let open = self.links.status_menu.is_some();
        div()
            .id("review-status")
            .flex()
            .items_center()
            .gap(px(space::S4))
            .p(px(space::S4))
            .rounded(px(radius::SM))
            .border_1()
            .border_color(colors.border_default)
            .cursor_pointer()
            .when(open, |d| d.bg(colors.bg_selected))
            .when(!open, |d| d.hover(|s| s.bg(colors.bg_hover)))
            .role(gpui::Role::Button)
            .aria_label(format!("Status: {label}. Change status"))
            .tooltip(Tooltip::text(format!(
                "Change status · {}",
                kbd(STATUS_KEYS).label()
            )))
            .child(chip)
            .child(
                Icon::new(IconName::ChevronDown)
                    .size(size::ICON_XS)
                    .color(colors.text_tertiary),
            )
            .child(kbd(STATUS_KEYS))
            .on_click(cx.listener(|this, _, window, cx| this.open_status_menu(window, cx)))
            .into_any_element()
    }

    /// The header's Work items section (FR-17): linked items, Link work
    /// item…, or why linking isn't possible (passive, spec 0001 FR-9).
    pub(super) fn render_work_items(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let blocker = self.link_blocker(cx);
        let read_only = self.loaded.read_only;
        if read_only && self.links.items.is_empty() {
            return None;
        }
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let rows: Vec<AnyElement> = self
            .links
            .items
            .iter()
            .map(|row| self.render_linked_item(row, blocker.is_none(), cx))
            .collect();
        let empty = rows
            .is_empty()
            .then(|| div().text_color(colors.text_tertiary).child("None linked"));
        let action = match blocker {
            None => Some(
                Button::new("review-link-work-item", "Link work item…")
                    .variant(ButtonVariant::Ghost)
                    .icon(IconName::Link)
                    .shortcut(kbd(LINK_WORK_ITEM_KEYS))
                    .on_press(press(cx, |this, w, cx| {
                        this.open_picker(PickerMode::Link, w, cx)
                    }))
                    .into_any_element(),
            ),
            // Read-only: the actions row's single hint covers it (AC-11).
            Some(_) if read_only => None,
            Some(why) => Some(
                div()
                    .id("review-link-scope-hint")
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .text_color(colors.text_secondary)
                    .aria_label(why)
                    .child(
                        Icon::new(IconName::Lock)
                            .size(size::ICON_SM)
                            .color(colors.text_tertiary),
                    )
                    .child(why)
                    .into_any_element(),
            ),
        };
        Some(
            div()
                .id("review-work-items")
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(space::S8))
                .role(gpui::Role::Group)
                .aria_label("Linked work items")
                .child(
                    div()
                        .text_color(colors.text_tertiary)
                        .type_style(typography::LABEL, &theme)
                        .child("Work items"),
                )
                .children(rows)
                .children(empty)
                .children(action)
                .into_any_element(),
        )
    }

    fn render_linked_item(
        &self,
        row: &LinkedItem,
        can_unlink: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let id = row.id;
        let pending = self.links.pending.contains(&id);
        let (icon, color) = match &row.item {
            Some(item) => type_style(&item.kind, colors),
            None => (IconName::CircleDashed, colors.text_tertiary),
        };
        let title = row
            .item
            .as_ref()
            .map_or_else(|| "Not synced".to_owned(), |w| w.title.clone());
        let state = row
            .item
            .as_ref()
            .map(|w| Chip::new(w.state.clone()).tone(state_tone(w.state_category)));
        let unlink = (can_unlink && !pending).then(|| {
            div()
                .id(("review-unlink", id as u64))
                .flex_none()
                .p(px(space::S2))
                .rounded(px(radius::XS))
                .cursor_pointer()
                .hover(|s| s.bg(colors.bg_active))
                .role(gpui::Role::Button)
                .aria_label(format!("Unlink #{id}"))
                .tooltip(Tooltip::text(format!("Unlink #{id}…")))
                .child(
                    Icon::new(IconName::Unlink)
                        .size(size::ICON_XS)
                        .color(colors.text_tertiary),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.ask_unlink(id, window, cx)
                }))
        });
        div()
            .id(("review-work-item", id as u64))
            .flex()
            .items_center()
            .gap(px(space::S4))
            .h(px(size::CONTROL_SM))
            .px(px(space::S6))
            .rounded(px(radius::SM))
            .border_1()
            .border_color(colors.border_subtle)
            .cursor_pointer()
            .hover(|s| s.bg(colors.bg_hover))
            .role(gpui::Role::Link)
            .aria_label(format!("Open work item #{id}: {title}"))
            .tooltip(Tooltip::text(format!("Open #{id} in Work items")))
            .child(Icon::new(icon).size(size::ICON_SM).color(color))
            .child(
                div()
                    .flex_none()
                    .text_color(colors.text_secondary)
                    .type_style(typography::CODE_INLINE, &theme)
                    .child(format!("#{id}")),
            )
            .child(
                div()
                    .min_w_0()
                    .max_w(px(LINKED_TITLE_WIDTH))
                    .truncate()
                    .text_color(if pending || row.item.is_none() {
                        colors.text_tertiary
                    } else {
                        colors.text_primary
                    })
                    .child(title),
            )
            .children(state)
            .when(pending, |d| {
                d.child(Spinner::new().color(colors.text_tertiary))
            })
            .children(unlink)
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(OpenWorkItem(id))))
            .into_any_element()
    }

    pub(super) fn render_link_overlays(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        out.extend(self.render_status_menu(cx));
        out.extend(self.render_confirm(cx));
        out.extend(self.render_picker(cx));
        out
    }

    fn render_status_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.links.status_menu.as_ref()?;
        let theme = cx.theme().clone();
        let entity = cx.entity().downgrade();
        let pick: Handler<StatusAction> = Rc::new(move |action, window, cx| {
            entity
                .update(cx, |this, cx| this.request_status(action, window, cx))
                .ok();
        });
        let entity = cx.entity().downgrade();
        let close: Handler<()> = Rc::new(move |_, window, cx| {
            entity
                .update(cx, |this, cx| this.close_link_overlays(window, cx))
                .ok();
        });
        Some(
            status_menu_panel(
                &menu.actions,
                menu.selected,
                &menu.focus,
                &theme,
                pick,
                close,
            )
            .absolute()
            .top(px(space::S64 + space::S16))
            .left(px(space::S16))
            .into_any_element(),
        )
    }

    fn render_confirm(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let confirm = self.links.confirm.as_ref()?;
        let theme = cx.theme().clone();
        let pr = self.loaded.pr().id;
        let (title, body, yes) = match confirm.what {
            Confirming::Abandon => abandon_text(pr),
            Confirming::Unlink(id) => {
                let name = self
                    .links
                    .items
                    .iter()
                    .find(|r| r.id == id)
                    .and_then(|r| r.item.as_ref())
                    .map_or_else(String::new, |w| format!(" ({})", w.title));
                (
                    format!("Unlink #{id} from !{pr}?"),
                    format!(
                        "The work item{name} stays as it is; only its link to this pull request goes."
                    ),
                    "Unlink",
                )
            }
        };
        let entity = cx.entity().downgrade();
        let accept: Handler<()> = Rc::new(move |_, window, cx| {
            entity
                .update(cx, |this, cx| this.accept_confirm(window, cx))
                .ok();
        });
        let entity = cx.entity().downgrade();
        let cancel: Handler<()> = Rc::new(move |_, window, cx| {
            entity
                .update(cx, |this, cx| this.close_link_overlays(window, cx))
                .ok();
        });
        Some(
            confirm_dialog(title, body, yes, &confirm.focus, &theme, accept, cancel)
                .into_any_element(),
        )
    }

    fn render_picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.links.picker.as_ref()?;
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let pr = self.loaded.pr().id;
        let title = match picker.mode {
            PickerMode::Link => format!("Link a work item to !{pr}"),
            PickerMode::Unlink => format!("Unlink a work item from !{pr}"),
        };
        let visible = (picker.rows.len() as f32).clamp(1.0, PICKER_VISIBLE_ROWS);
        let rows = if picker.rows.is_empty() {
            let text = match (picker.mode, &picker.catalog) {
                (PickerMode::Link, None) => "Reading work items…",
                _ => "No work items match",
            };
            div()
                .px(px(space::S8))
                .h(px(PICKER_ROW))
                .flex()
                .items_center()
                .text_color(colors.text_tertiary)
                .child(text)
                .into_any_element()
        } else {
            uniform_list(
                "work-item-picker-rows",
                picker.rows.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|row| this.render_pick_row(row, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&picker.scroll)
            .h(px(visible * PICKER_ROW))
            .into_any_element()
        };
        let verb = match picker.mode {
            PickerMode::Link => "link",
            PickerMode::Unlink => "unlink",
        };
        let hint = |keys: &[&str], text: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(space::S4))
                .children(keys.iter().map(|k| kbd(k)))
                .child(text)
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(space::S64))
                .child(
                    div()
                        .id("work-item-picker")
                        .key_context(PICKER_CONTEXT)
                        .w(px(PICKER_WIDTH))
                        .p(px(space::S8))
                        .flex()
                        .flex_col()
                        .gap(px(space::S8))
                        .rounded(px(radius::LG))
                        .border_1()
                        .border_color(colors.border_default)
                        .bg(colors.bg_overlay)
                        .shadow(theme.shadows(elevation::LEVEL_3))
                        .text_color(colors.text_primary)
                        .type_style(typography::BODY, &theme)
                        .occlude()
                        .role(gpui::Role::Dialog)
                        .aria_label(title.clone())
                        .on_mouse_down_out(
                            cx.listener(|this, _, window, cx| this.close_link_overlays(window, cx)),
                        )
                        .child(
                            div()
                                .px(px(space::S4))
                                .text_color(colors.text_tertiary)
                                .type_style(typography::LABEL, &theme)
                                .child(title),
                        )
                        .child(
                            div()
                                .h(px(size::CONTROL_LG))
                                .flex()
                                .items_center()
                                .gap(px(space::S6))
                                .px(px(space::S8))
                                .rounded(px(radius::SM))
                                .border_1()
                                .border_color(colors.border_control)
                                .bg(colors.bg_input)
                                .child(
                                    Icon::new(IconName::Search)
                                        .size(size::ICON_SM)
                                        .color(colors.text_tertiary),
                                )
                                .child(div().flex_1().min_w_0().child(picker.input.clone())),
                        )
                        .child(rows)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(space::S12))
                                .px(px(space::S4))
                                .text_color(colors.text_tertiary)
                                .type_style(typography::SMALL, &theme)
                                .child(hint(&["up", "down"], "move"))
                                .child(hint(&["enter"], verb))
                                .child(hint(&["escape"], "close")),
                        ),
                )
                .into_any_element(),
        )
    }

    fn render_pick_row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let picker = self.links.picker.as_ref().expect("rows render while open");
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let pick = &picker.rows[row];
        let highlighted = row == picker.selected;
        let base = div()
            .id(("work-item-pick", row))
            .w_full()
            .h(px(PICKER_ROW))
            .px(px(space::S8))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .rounded(px(radius::SM));
        let linked = |id: i64| self.links.items.iter().any(|r| r.id == id);
        let row_el = match pick {
            PickRow::Note(text) => {
                return base
                    .text_color(colors.text_secondary)
                    .child(
                        Icon::new(IconName::CircleAlert)
                            .size(size::ICON_SM)
                            .color(colors.text_tertiary),
                    )
                    .child(text.clone())
                    .into_any_element();
            }
            PickRow::Item(item) => {
                let (icon, color) = type_style(&item.kind, colors);
                base.aria_label(format!("{} #{} {}", item.kind, item.id, item.title))
                    .child(Icon::new(icon).size(size::ICON_SM).color(color))
                    .child(
                        div()
                            .flex_none()
                            .text_color(colors.text_secondary)
                            .type_style(typography::CODE_INLINE, &theme)
                            .child(format!("#{}", item.id)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(item.title.clone()),
                    )
                    .child(Chip::new(item.state.clone()).tone(state_tone(item.state_category)))
                    .when(picker.mode == PickerMode::Link && linked(item.id), |d| {
                        d.child(Chip::new("Linked"))
                    })
            }
            PickRow::NotSynced(id) => base
                .aria_label(format!("Work item #{id}, not synced"))
                .child(
                    Icon::new(IconName::CircleDashed)
                        .size(size::ICON_SM)
                        .color(colors.text_tertiary),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(colors.text_secondary)
                        .type_style(typography::CODE_INLINE, &theme)
                        .child(format!("#{id}")),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(colors.text_tertiary)
                        .child("Not synced in adoken; links all the same"),
                ),
        };
        row_el
            .role(gpui::Role::MenuItem)
            .cursor_pointer()
            .when(highlighted, |d| d.bg(colors.bg_selected))
            .when(!highlighted, |d| d.hover(|s| s.bg(colors.bg_hover)))
            .on_click(cx.listener(move |this, _, window, cx| this.choose_pick(row, window, cx)))
            .into_any_element()
    }

    // ── stories ──────────────────────────────────────────────────────────

    /// Stories: the header's linked work items (demo work items `ids`,
    /// as Azure DevOps lists them).
    pub fn story_linked_items(&mut self, ids: &[i64], cx: &mut Context<Self>) {
        let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
        let stored = ds
            .work_items
            .iter()
            .filter(|w| ids.contains(&w.id))
            .cloned()
            .collect();
        self.links.server = Some(ids.to_vec());
        self.set_rows(stored);
        cx.notify();
    }

    /// Stories: the status menu open (focus stays where it is).
    pub fn story_status_menu(&mut self, cx: &mut Context<Self>) {
        self.links.status_menu = Some(StatusMenu {
            actions: self.status_actions(),
            selected: 0,
            focus: cx.focus_handle(),
        });
        cx.notify();
    }

    /// Stories: the Abandon confirmation.
    pub fn story_abandon_confirm(&mut self, cx: &mut Context<Self>) {
        self.links.confirm = Some(Confirm {
            what: Confirming::Abandon,
            focus: cx.focus_handle(),
        });
        cx.notify();
    }

    /// Stories: Azure DevOps refused a link for the token's scopes.
    pub fn story_link_scope_missing(&mut self, cx: &mut Context<Self>) {
        self.links.scope_refused = true;
        cx.notify();
    }

    /// Stories: the Link picker with `query` typed, its search done
    /// synchronously over the demo dataset.
    pub fn story_link_picker(&mut self, query: &str, cx: &mut Context<Self>) {
        let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
        let data = CatalogData {
            work_items: ds.work_items.clone(),
            org: Some(ds.org.name.clone()),
            me: Some(ds.me.clone()),
            people: ds.people.clone(),
            ..CatalogData::default()
        };
        let catalog = Catalog::new(data, Vec::new());
        let rows = picker_rows(&catalog, query, self.loaded.now);
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Search work items, #1234, or paste a link", cx);
            input.set_text(query, cx);
            input
        });
        let events = cx.subscribe(&input, |_, _, _: &TextInputEvent, _| {});
        self.links.picker = Some(Picker {
            mode: PickerMode::Link,
            input,
            catalog: Some(Arc::new(catalog)),
            rows,
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            generation: 0,
            _search: None,
            _catalog: None,
            _input: events,
        });
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> (Catalog, adoken_demo::Dataset) {
        let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
        let data = CatalogData {
            work_items: ds.work_items.clone(),
            org: Some(ds.org.name.clone()),
            me: Some(ds.me.clone()),
            people: ds.people.clone(),
            ..CatalogData::default()
        };
        (Catalog::new(data, Vec::new()), ds)
    }

    fn ids(rows: &[PickRow]) -> Vec<Option<i64>> {
        rows.iter().map(PickRow::id).collect()
    }

    #[test]
    fn fr17_picker_finds_work_items_by_id_title_and_link() {
        let (catalog, ds) = catalog();
        let wi = &ds.work_items[100];
        let now = ds.now;
        // `#id` and a bare id land on the item first.
        for query in [format!("#{}", wi.id), wi.id.to_string()] {
            let rows = picker_rows(&catalog, &query, now);
            assert_eq!(ids(&rows)[0], Some(wi.id), "{query}");
        }
        // Title words, fuzzy: only work items come back.
        let rows = picker_rows(&catalog, "timezone settlement", now);
        assert_eq!(ids(&rows)[0], Some(wi.id));
        assert!(rows.iter().all(|r| matches!(r, PickRow::Item(_))));
        // A pasted work item link of this org.
        let link = format!(
            "https://dev.azure.com/{}/Payments/_workitems/edit/{}",
            ds.org.name, wi.id
        );
        assert_eq!(ids(&picker_rows(&catalog, &link, now)), [Some(wi.id)]);
        // An id adoken hasn't synced still links.
        assert_eq!(
            picker_rows(&catalog, "#999999", now)[0],
            PickRow::NotSynced(999_999)
        );
        // Links that aren't this org's work items explain themselves.
        let pr_link = format!(
            "https://dev.azure.com/{}/Payments/_git/api/pullrequest/4312",
            ds.org.name
        );
        assert!(matches!(
            picker_rows(&catalog, &pr_link, now)[..],
            [PickRow::Note(_)]
        ));
        let other = format!("https://dev.azure.com/fabrikam/X/_workitems/edit/{}", wi.id);
        assert!(matches!(
            picker_rows(&catalog, &other, now)[..],
            [PickRow::Note(_)]
        ));
        // A PR sigil doesn't turn the picker into a PR search.
        assert!(
            picker_rows(&catalog, "!4312", now)
                .iter()
                .all(|r| !matches!(r, PickRow::Note(_)))
        );
    }

    #[test]
    fn fr17_unlink_picker_filters_linked_items() {
        let (_, ds) = catalog();
        let items = review_writes::linked_items(
            vec![ds.work_items[100].clone(), ds.work_items[101].clone()],
            Some(&[ds.work_items[100].id, ds.work_items[101].id, 424_242]),
        );
        assert_eq!(unlink_rows(&items, "").len(), 3);
        assert_eq!(
            ids(&unlink_rows(&items, "TIMEZONE")),
            [Some(ds.work_items[100].id)]
        );
        assert_eq!(ids(&unlink_rows(&items, "#424242")), [Some(424_242)]);
        assert!(unlink_rows(&items, "nothing like it").is_empty());
    }
}
