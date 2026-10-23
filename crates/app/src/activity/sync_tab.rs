//! Activity › Sync (spec 0016 FR-7–FR-15; activity.md §4): the toolbar
//! with Sync now / Pause / Sync settings…, the throttled banner, the six
//! kind rows, and Recent passes with their changed items.

use adoken_sync::activity::{KindRow, RowState};
use adoken_sync::cadence::Pause;
use adoken_theme::{radius, size, space, typography};
use adoken_ui::action_menu::{MenuItem, MenuPosition};
use adoken_ui::{
    Button, ButtonVariant, EmptyState, Icon, IconName, Kbd, Theme, Tooltip, TypeStyleExt,
};
use gpui::{AnyElement, Context, Hsla, MouseButton, Window, div, prelude::*, px};

use super::model::ActivityEvent;
use super::panel::{ActivityPanel, Pane};
use super::passes::{self, Glyph, ItemKind, PassLine, PassOutcome};
use crate::palette::catalog::Target;

/// Two columns from this drawer width (activity.md §4.1).
const TWO_COLUMNS: f32 = 760.0;
const KINDS_WIDTH: f32 = 300.0;
const PASS_ROW: f32 = 28.0;

/// One selectable or decorative row of the passes list.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum PassRow {
    Line(usize),
    Group {
        kind: ItemKind,
        count: usize,
    },
    Item {
        line: usize,
        kind: ItemKind,
        id: i64,
        word: String,
    },
    More {
        more: u32,
    },
}

impl PassRow {
    fn selectable(&self) -> bool {
        matches!(self, PassRow::Line(_) | PassRow::Item { .. })
    }
}

/// The passes list flattened with expanded passes' items.
pub(super) fn pass_rows(activity: &super::Activity) -> Vec<PassRow> {
    let mut rows = Vec::new();
    for (ix, line) in activity.lines().iter().enumerate() {
        rows.push(PassRow::Line(ix));
        let PassLine::One(pass) = line else { continue };
        if !activity.is_expanded(pass.id) {
            continue;
        }
        for (kind, changes) in passes::items(&pass.changes) {
            rows.push(PassRow::Group {
                kind,
                count: (changes.new + changes.updated + changes.removed) as usize,
            });
            for (id, word) in &changes.ids {
                rows.push(PassRow::Item {
                    line: ix,
                    kind,
                    id: *id,
                    word: passes::change_word(word),
                });
            }
            if changes.more > 0 {
                rows.push(PassRow::More { more: changes.more });
            }
        }
    }
    rows
}

pub(super) fn glyph_style(glyph: Glyph, theme: &Theme) -> (IconName, Hsla) {
    let c = &theme.colors;
    match glyph {
        Glyph::Synced => (IconName::CircleCheck, c.status_success_fg),
        Glyph::Syncing => (IconName::LoaderCircle, c.status_info_fg),
        Glyph::NotSynced => (IconName::CircleDotDashed, c.text_tertiary),
        Glyph::Manual => (IconName::CircleDashed, c.text_tertiary),
        Glyph::Paused => (IconName::Pause, c.text_secondary),
        Glyph::Slowed => (IconName::Timer, c.status_warning_fg),
        Glyph::Throttled => (IconName::Clock, c.status_warning_fg),
        Glyph::Failed => (IconName::CircleX, c.status_danger_fg),
        Glyph::Offline => (IconName::WifiOff, c.text_tertiary),
        Glyph::Locked => (IconName::Lock, c.text_tertiary),
        Glyph::LockedDanger => (IconName::Lock, c.status_danger_fg),
        Glyph::Pinned => (IconName::Pin, c.text_tertiary),
    }
}

impl ActivityPanel {
    pub(super) fn render_sync(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = &theme.colors;
        let activity = self.activity.read(cx);
        let demo = activity.is_demo();
        let now = activity.now();
        let offset = activity.offset();
        let width = activity.content_width();
        let state = activity.sync_state().cloned();
        // No token / token rejected / no projects replace kinds and passes
        // (activity.md §7.1).
        let empty = match &state {
            Some(crate::sync::SyncState::NoToken) => Some(
                EmptyState::new(IconName::KeyRound, "No token in Keychain")
                    .body("adoken can't sync without one. Nothing was sent to Azure DevOps."),
            ),
            Some(crate::sync::SyncState::NoProjects) => Some(
                EmptyState::new(IconName::FolderKanban, "No projects selected")
                    .body("Pick the projects adoken should sync in Settings › Projects."),
            ),
            Some(crate::sync::SyncState::Running(s))
                if s.phase == adoken_sync::Phase::Unauthorized =>
            {
                // Spec 0001 FR-15, design §10.3: the one token action
                // outside Settings › Account and ⌘P.
                let headline = crate::account::token_problem(cx)
                    .unwrap_or(crate::account::TokenProblem::Rejected)
                    .headline();
                Some(
                    EmptyState::new(IconName::KeyRound, headline)
                        .body("Sync stopped. Your local data is kept.")
                        .action(
                            adoken_ui::Button::new("sync-replace-token", "Replace token…")
                                .variant(adoken_ui::ButtonVariant::Primary)
                                .on_press(|_, cx| crate::account::open_and_replace(cx)),
                        ),
                )
            }
            _ => None,
        };
        let toolbar = self.render_sync_toolbar(theme, demo, window, cx);
        if let Some(empty) = empty {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(toolbar)
                .child(div().flex_1().min_h_0().child(empty))
                .into_any_element();
        }
        let rows = self.activity.read(cx).rows(cx);
        let banner = rows.as_ref().and_then(|(inputs, rows)| {
            rows.iter().find_map(|r| match &r.state {
                RowState::WaitingForAdo { until, resource } => Some((
                    *until,
                    resource.clone().or_else(|| inputs.gov.resource.clone()),
                )),
                _ => None,
            })
        });
        let kinds = self.render_kinds(theme, rows.as_ref(), demo, offset, now, window, cx);
        let passes = self.render_passes(theme, demo, offset, now, window, cx);
        let two = width >= TWO_COLUMNS;
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .when_some(banner, |d, (until, resource)| {
                d.child(
                    div()
                        .flex_none()
                        .h(px(size::CONTROL_MD))
                        .px(px(space::S12))
                        .flex()
                        .items_center()
                        .gap(px(space::S8))
                        .bg(colors.status_warning_wash)
                        .text_color(colors.text_primary)
                        .type_style(typography::SMALL, theme)
                        .child(
                            Icon::new(IconName::CircleAlert)
                                .size(size::ICON_SM)
                                .color(colors.status_warning_fg),
                        )
                        .child(format!(
                            "Azure DevOps is throttling requests · sync resumes in {}",
                            passes::countdown(until - now)
                        ))
                        .when_some(resource, |d, resource| {
                            d.child("· resource:").child(
                                div()
                                    .type_style(typography::CODE_INLINE, theme)
                                    .child(resource),
                            )
                        }),
                )
            })
            .child(if two {
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .w(px(KINDS_WIDTH))
                            .h_full()
                            .flex_none()
                            .border_r_1()
                            .border_color(colors.border_subtle)
                            .child(kinds),
                    )
                    .child(div().flex_1().min_w_0().h_full().child(passes))
                    .into_any_element()
            } else {
                div()
                    .id("activity-sync-one-column")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(kinds)
                    .child(
                        div()
                            .h(px(24.0))
                            .px(px(space::S12))
                            .flex()
                            .items_center()
                            .text_color(colors.text_secondary)
                            .type_style(typography::LABEL, theme)
                            .child("Recent passes"),
                    )
                    .child(passes)
                    .into_any_element()
            })
            .into_any_element()
    }

    fn render_sync_toolbar(
        &mut self,
        theme: &Theme,
        demo: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let colors = &theme.colors;
        let activity = self.activity.read(cx);
        let now = activity.now();
        let offset = activity.offset();
        let pause = crate::sync::prefs(cx).pause;
        let paused = pause.active(now);
        let offline = matches!(
            activity.sync_state(),
            Some(crate::sync::SyncState::Running(s)) if s.phase == adoken_sync::Phase::Offline
        );
        let rows = activity.rows(cx);
        let sentence = if demo {
            // Design §4.1: the toolbar repeats the statusbar sentence; the
            // demo explanation lives in the note row above the kinds.
            "Demo data".to_owned()
        } else if paused {
            match pause {
                Pause::Until(at) => format!(
                    "Sync paused until {} · ⌘R still syncs once",
                    passes::clock(at, offset)
                ),
                _ => "Sync paused · ⌘R still syncs once".to_owned(),
            }
        } else {
            let last = rows
                .as_ref()
                .and_then(|(_, rows)| rows.iter().filter_map(|r| r.last_ok).max());
            let next = rows.as_ref().and_then(|(_, rows)| {
                rows.iter()
                    .filter_map(|r| r.next_due)
                    .filter(|at| *at > now)
                    .min()
            });
            let mut parts = vec![match last {
                Some(at) => format!("Synced {}", passes::ago(now - at)),
                None => "Not synced yet".to_owned(),
            }];
            parts.extend(next.map(|at| format!("next in {}", passes::next_in(at - now))));
            parts.join(" · ")
        };
        let actions = passes::sync_actions(demo, offline);
        let reason = actions.reason;
        let sync_now = Button::new("activity-sync-now", "Sync now")
            .variant(ButtonVariant::Secondary)
            .icon(IconName::RefreshCw)
            .shortcut(Kbd::parse(crate::sync::SYNC_NOW_KEYS).expect("static binding"))
            .disabled(!actions.sync_now)
            .tooltip(reason.unwrap_or("Sync now ⌘R"))
            .on_press(|window, cx| window.dispatch_action(Box::new(crate::sync::SyncNow), cx));
        let pause_button = if paused {
            Button::new("activity-resume", "Resume")
                .variant(ButtonVariant::Primary)
                .icon(IconName::Play)
                .disabled(!actions.pause)
                .on_press(|_, cx| crate::sync::set_pause(Pause::Off, cx))
        } else {
            let weak = cx.entity().downgrade();
            Button::new("activity-pause", "Pause")
                .variant(ButtonVariant::Secondary)
                .icon(IconName::Pause)
                .disabled(!actions.pause)
                .on_press(move |window, cx| {
                    weak.update(cx, |this, cx| this.open_pause_menu(window, cx))
                        .ok();
                })
        };
        div()
            .id("activity-sync-toolbar")
            .role(gpui::Role::Toolbar)
            .aria_label(match reason {
                Some(reason) => format!("Sync actions unavailable. {reason}"),
                None => "Sync actions".to_owned(),
            })
            .flex_none()
            .h(px(size::DRAWER_HEADER))
            .px(px(space::S12))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .border_b_1()
            .border_color(colors.border_subtle)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(colors.text_secondary)
                    .child(sentence),
            )
            .child(sync_now)
            .child(pause_button)
            .child(
                Button::new("activity-sync-settings", "Sync settings…")
                    .variant(ButtonVariant::Ghost)
                    .on_press(|window, cx| {
                        window.dispatch_action(Box::new(crate::sync::OpenSyncSettings), cx)
                    }),
            )
    }

    pub(super) fn open_pause_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = vec![
            MenuItem::new("Pause for 1 hour", |window, cx| {
                window.dispatch_action(Box::new(crate::sync::PauseSyncForHour), cx)
            }),
            MenuItem::new("Pause until resumed", |_, cx| {
                crate::sync::set_pause(Pause::UntilResumed, cx)
            }),
        ];
        let position = MenuPosition::at_pointer(window.mouse_position());
        self.menu.open(
            "Pause sync",
            items,
            position,
            adoken_theme::z::DROPDOWN,
            |this: &mut Self| &mut this.menu,
            window,
            cx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn render_kinds(
        &mut self,
        theme: &Theme,
        rows: Option<&(adoken_sync::activity::SyncInputs, [KindRow; 6])>,
        demo: bool,
        offset: time::UtcOffset,
        now: adoken_model::Millis,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = &theme.colors;
        let focused = self.kinds_focus.is_focused(window);
        let mut list = div()
            .id("activity-kinds")
            .track_focus(&self.kinds_focus)
            .role(gpui::Role::List)
            .aria_label("Kinds of data")
            .size_full()
            .overflow_y_scroll()
            .py(px(space::S4))
            .flex()
            .flex_col()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| window.focus(&this.kinds_focus, cx)),
            );
        if demo {
            list = list.child(
                div()
                    .px(px(space::S12))
                    .py(px(space::S4))
                    .flex()
                    .items_start()
                    .gap(px(space::S8))
                    .text_color(colors.text_secondary)
                    .type_style(typography::SMALL, theme)
                    .child(
                        div().pt(px(space::S2)).child(
                            Icon::new(IconName::Info)
                                .size(size::ICON_SM)
                                .color(colors.text_tertiary),
                        ),
                    )
                    .child(div().flex_1().min_w_0().child(
                        "Demo mode: nothing syncs. A local feed adds fake activity every 30 s.",
                    )),
            );
        }
        let Some((inputs, rows)) = rows else {
            // No running sync (demo, or before the token is read).
            for kind in [
                adoken_sync::activity::RowKind::MyPrs,
                adoken_sync::activity::RowKind::AllPrs,
                adoken_sync::activity::RowKind::WorkItems,
                adoken_sync::activity::RowKind::Pipelines,
                adoken_sync::activity::RowKind::Index,
                adoken_sync::activity::RowKind::Pinned,
            ] {
                let line = if demo {
                    "Demo data isn't synced"
                } else {
                    "Not synced yet"
                };
                list = list.child(kind_row(
                    theme,
                    kind as usize,
                    passes::kind_title(kind),
                    None,
                    line.into(),
                    Glyph::NotSynced,
                    format!("{}, {line}", passes::kind_title(kind)),
                    false,
                    false,
                ));
            }
            return list.into_any_element();
        };
        for (ix, row) in rows.iter().enumerate() {
            let text = passes::row_text(row, inputs, offset, now);
            let selected = ix == self.kinds_cursor;
            list = list.child(
                kind_row(
                    theme,
                    ix,
                    text.title,
                    text.cadence.clone(),
                    text.line.clone(),
                    text.glyph,
                    text.label.clone(),
                    selected,
                    focused,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.kinds_cursor = ix;
                    window.focus(&this.kinds_focus, cx);
                    cx.notify();
                })),
            );
        }
        list.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_passes(
        &mut self,
        theme: &Theme,
        demo: bool,
        offset: time::UtcOffset,
        now: adoken_model::Millis,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = &theme.colors;
        let activity = self.activity.read(cx);
        let rows = pass_rows(activity);
        if rows.is_empty() {
            let mut empty = EmptyState::new(IconName::RefreshCw, "No passes yet").body(if demo {
                "Demo feed activity shows up here when the feed adds some (it checks every 30 s)."
            } else {
                "The first sync pass shows up here as it runs."
            });
            if !demo {
                empty = empty.action(
                    Button::new("activity-empty-sync-now", "Sync now")
                        .shortcut(Kbd::parse(crate::sync::SYNC_NOW_KEYS).expect("static binding"))
                        .on_press(|window, cx| {
                            window.dispatch_action(Box::new(crate::sync::SyncNow), cx)
                        }),
                );
            }
            return div().size_full().child(empty).into_any_element();
        }
        let focused = self.passes_focus.is_focused(window);
        let two = activity.content_width() >= TWO_COLUMNS;
        let mut list = div()
            .id("activity-passes")
            .track_focus(&self.passes_focus)
            .role(gpui::Role::List)
            .aria_label("Recent passes")
            .size_full()
            .overflow_y_scroll()
            .py(px(space::S4))
            .flex()
            .flex_col()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| window.focus(&this.passes_focus, cx)),
            );
        for (ix, row) in rows.iter().enumerate() {
            let selected = ix == self.pass_cursor && row.selectable();
            let element = match row {
                PassRow::Line(line) => {
                    let line = self.activity.read(cx).lines()[*line].clone();
                    self.render_pass_line(theme, &line, selected, focused, two, offset, now, ix, cx)
                }
                PassRow::Group { kind, count } => div()
                    .h(px(24.0))
                    .pl(px(space::S24 + space::S12))
                    .flex()
                    .items_center()
                    .text_color(colors.text_secondary)
                    .type_style(typography::LABEL, theme)
                    .child(format!("{} {count}", passes::group_title(*kind)))
                    .into_any_element(),
                PassRow::Item { kind, id, word, .. } => {
                    let title = self.activity.read(cx).title(*kind, *id).cloned();
                    let gone = matches!(title, Some(None));
                    let (kind, id, word) = (*kind, *id, word.clone());
                    let (icon, tint) = match kind {
                        ItemKind::Pr => (IconName::GitPullRequest, colors.text_secondary),
                        ItemKind::WorkItem => (IconName::ListTodo, colors.text_secondary),
                        ItemKind::Run => (IconName::Workflow, colors.text_secondary),
                    };
                    let reference = passes::item_ref(kind, id);
                    let title_text = match &title {
                        Some(Some(t)) => t.clone(),
                        Some(None) => "Not in adoken anymore".to_owned(),
                        None => String::new(),
                    };
                    let word_text = if gone {
                        "No longer synced".to_owned()
                    } else {
                        word.clone()
                    };
                    div()
                        .id(("activity-item", ix))
                        .role(gpui::Role::ListItem)
                        .aria_label(format!(
                            "{} {id}, {title_text}, {word_text}",
                            match kind {
                                ItemKind::Pr => "Pull request",
                                ItemKind::WorkItem => "Work item",
                                ItemKind::Run => "Pipeline run",
                            }
                        ))
                        .aria_selected(selected)
                        .h(px(PASS_ROW))
                        .mx(px(space::S4))
                        .pl(px(space::S24 + space::S8))
                        .pr(px(space::S8))
                        .flex()
                        .items_center()
                        .gap(px(space::S8))
                        .rounded(px(radius::SM))
                        .when(selected, |d| {
                            d.bg(if focused {
                                colors.bg_selected
                            } else {
                                colors.bg_selected_muted
                            })
                        })
                        .when(!selected, |d| d.hover(|d| d.bg(colors.bg_hover)))
                        .cursor_pointer()
                        .child(Icon::new(icon).size(size::ICON_SM).color(if gone {
                            colors.text_tertiary
                        } else {
                            tint
                        }))
                        .child(
                            div()
                                .flex_none()
                                .text_color(colors.text_secondary)
                                .type_style(typography::CODE_INLINE, theme)
                                .child(reference),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(if gone {
                                    colors.text_tertiary
                                } else {
                                    colors.text_primary
                                })
                                .child(title_text),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_color(colors.text_secondary)
                                .type_style(typography::LABEL, theme)
                                .child(word_text),
                        )
                        .when(gone, |d| d.tooltip(Tooltip::text("Not in adoken anymore")))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.pass_cursor = ix;
                            window.focus(&this.passes_focus, cx);
                            if !gone {
                                this.open_item(kind, id, cx);
                            }
                            cx.notify();
                        }))
                        .into_any_element()
                }
                PassRow::More { more } => div()
                    .h(px(24.0))
                    .pl(px(space::S24 + space::S12))
                    .flex()
                    .items_center()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, theme)
                    .child(format!("+{more} more"))
                    .into_any_element(),
            };
            list = list.child(element);
        }
        list.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_pass_line(
        &self,
        theme: &Theme,
        line: &PassLine,
        selected: bool,
        focused: bool,
        two: bool,
        offset: time::UtcOffset,
        now: adoken_model::Millis,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = &theme.colors;
        let activity = self.activity.read(cx);
        let (time_text, origin, user, outcome, meta, key, expandable, tooltip) = match line {
            PassLine::One(pass) => {
                let (origin, user) = passes::origin_text(pass);
                let outcome = passes::outcome(pass);
                let time = if matches!(outcome, PassOutcome::Running { .. }) {
                    "now".to_owned()
                } else {
                    super::net::time_text(pass.started, offset, false)
                };
                let expandable = !passes::items(&pass.changes).is_empty();
                (
                    time,
                    origin,
                    user,
                    outcome,
                    passes::meta_text(pass),
                    pass.id,
                    expandable,
                    format!(
                        "{} · {}",
                        passes::meta_text(pass),
                        passes::lanes_text(pass.lanes)
                    ),
                )
            }
            PassLine::Folded {
                ids,
                oldest,
                newest,
                lanes,
            } => (
                format!(
                    "{}–{}",
                    passes::clock(*oldest, offset),
                    passes::clock(*newest, offset)
                ),
                passes::lanes_text(*lanes),
                false,
                PassOutcome::NoChanges,
                String::new(),
                ids[0],
                false,
                format!("{} passes with no changes", ids.len()),
            ),
        };
        let _ = now;
        let expanded = activity.is_expanded(key);
        let (lead, summary, summary_color): (AnyElement, String, Hsla) = match &outcome {
            PassOutcome::Running { requests } => (
                Icon::new(IconName::LoaderCircle)
                    .size(size::ICON_XS)
                    .color(colors.status_info_fg)
                    .into_any_element(),
                format!("Syncing… {requests} requests so far"),
                colors.text_secondary,
            ),
            PassOutcome::Failed(text) => (
                Icon::new(IconName::CircleX)
                    .size(size::ICON_XS)
                    .color(colors.status_danger_fg)
                    .into_any_element(),
                text.clone(),
                colors.text_primary,
            ),
            PassOutcome::Throttled => (
                Icon::new(IconName::Clock)
                    .size(size::ICON_XS)
                    .color(colors.status_warning_fg)
                    .into_any_element(),
                "Waiting for Azure DevOps…".into(),
                colors.text_primary,
            ),
            PassOutcome::Stopped => (
                Icon::new(IconName::WifiOff)
                    .size(size::ICON_XS)
                    .color(colors.text_tertiary)
                    .into_any_element(),
                "Stopped: offline".into(),
                colors.text_tertiary,
            ),
            PassOutcome::Changes(parts) => (
                if expandable {
                    Icon::new(if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size(size::ICON_XS)
                    .color(colors.text_tertiary)
                    .into_any_element()
                } else {
                    div().size(px(size::ICON_XS)).into_any_element()
                },
                parts.join(" · "),
                colors.text_primary,
            ),
            PassOutcome::NoChanges => (
                div().size(px(size::ICON_XS)).into_any_element(),
                match line {
                    PassLine::Folded { ids, .. } => format!("No changes · {} passes", ids.len()),
                    _ => "No changes".into(),
                },
                colors.text_tertiary,
            ),
        };
        let label = format!(
            "{time_text}, {origin}, {summary}{}",
            if two {
                format!(", {meta}")
            } else {
                String::new()
            }
        );
        div()
            .id(("activity-pass", ix))
            .role(gpui::Role::ListItem)
            .aria_label(label)
            .when(expandable, |d| d.aria_expanded(expanded))
            .aria_selected(selected)
            .h(px(PASS_ROW))
            .mx(px(space::S4))
            .px(px(space::S8))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .rounded(px(radius::SM))
            .type_style(typography::SMALL, theme)
            .when(selected, |d| {
                d.bg(if focused {
                    colors.bg_selected
                } else {
                    colors.bg_selected_muted
                })
            })
            .when(!selected, |d| d.hover(|d| d.bg(colors.bg_hover)))
            .cursor_pointer()
            .child(lead)
            .child(
                div()
                    .flex_none()
                    .w(px(if matches!(line, PassLine::Folded { .. }) {
                        88.0
                    } else {
                        56.0
                    }))
                    .text_color(colors.text_tertiary)
                    .child(time_text),
            )
            .child(
                div()
                    .max_w(gpui::relative(0.4))
                    .min_w_0()
                    .truncate()
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .text_color(colors.text_secondary)
                    .when(user, |d| {
                        d.child(
                            Icon::new(IconName::User)
                                .size(size::ICON_XS)
                                .color(colors.text_secondary),
                        )
                    })
                    .child(origin),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(summary_color)
                    .child(summary),
            )
            .when(two && !meta.is_empty(), |d| {
                d.child(
                    div()
                        .flex_none()
                        .text_color(colors.text_tertiary)
                        .type_style(typography::LABEL, theme)
                        .child(meta),
                )
            })
            .tooltip(Tooltip::text(tooltip))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.pass_cursor = ix;
                window.focus(&this.passes_focus, cx);
                if expandable {
                    this.activity
                        .update(cx, |a, cx| a.toggle_expanded(key, None, cx));
                }
                cx.notify();
            }))
            .into_any_element()
    }

    /// `↩` on a changed item: its canonical view, the drawer stays open
    /// and keeps the keyboard (FR-13, AC-A5).
    pub(super) fn open_item(&mut self, kind: ItemKind, id: i64, cx: &mut Context<Self>) {
        let target = match kind {
            ItemKind::Pr => Target::PullRequest(id),
            ItemKind::WorkItem => Target::WorkItem(id),
            ItemKind::Run => Target::Run(id),
        };
        self.emit(ActivityEvent::Open(target), cx);
    }

    /// Sync tab keys: move in the focused list.
    pub(super) fn sync_move(&mut self, delta: isize, window: &Window, cx: &mut Context<Self>) {
        match self.pane(window) {
            Pane::Passes => {
                let rows = pass_rows(self.activity.read(cx));
                if rows.is_empty() {
                    return;
                }
                let mut ix = self.pass_cursor.min(rows.len() - 1) as isize;
                let step = delta.signum();
                let mut remaining = delta.abs();
                while remaining > 0 {
                    let mut next = ix + step;
                    while next >= 0
                        && (next as usize) < rows.len()
                        && !rows[next as usize].selectable()
                    {
                        next += step;
                    }
                    if next < 0 || next as usize >= rows.len() {
                        break;
                    }
                    ix = next;
                    remaining -= 1;
                }
                self.pass_cursor = ix as usize;
            }
            _ => {
                self.kinds_cursor = (self.kinds_cursor as isize + delta).clamp(0, 5) as usize;
            }
        }
        cx.notify();
    }

    pub(super) fn sync_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.pane(window) {
            Pane::Passes => {
                let rows = pass_rows(self.activity.read(cx));
                match rows.get(self.pass_cursor) {
                    Some(PassRow::Line(line)) => {
                        let key = self.activity.read(cx).lines()[*line].key();
                        self.activity
                            .update(cx, |a, cx| a.toggle_expanded(key, None, cx));
                    }
                    Some(PassRow::Item { kind, id, .. }) => {
                        let gone = matches!(self.activity.read(cx).title(*kind, *id), Some(None));
                        if !gone {
                            self.open_item(*kind, *id, cx);
                        }
                    }
                    _ => {}
                }
            }
            _ => window.dispatch_action(Box::new(crate::sync::OpenSyncSettings), cx),
        }
    }

    /// `→` / `←` on a pass row expands / collapses it.
    pub(super) fn sync_expand(&mut self, expand: bool, window: &Window, cx: &mut Context<Self>) {
        if self.pane(window) != Pane::Passes {
            return;
        }
        let rows = pass_rows(self.activity.read(cx));
        let line = match rows.get(self.pass_cursor) {
            Some(PassRow::Line(line)) => *line,
            Some(PassRow::Item { line, .. }) if !expand => *line,
            _ => return,
        };
        let key = self.activity.read(cx).lines()[line].key();
        self.activity
            .update(cx, |a, cx| a.toggle_expanded(key, Some(expand), cx));
        // Collapsing from an item lands on its pass.
        let rows = pass_rows(self.activity.read(cx));
        if let Some(ix) = rows.iter().position(|r| *r == PassRow::Line(line)) {
            self.pass_cursor = ix;
        }
        cx.notify();
    }
}

#[allow(clippy::too_many_arguments)]
fn kind_row(
    theme: &Theme,
    ix: usize,
    title: &'static str,
    cadence: Option<String>,
    line: String,
    glyph: Glyph,
    label: String,
    selected: bool,
    focused: bool,
) -> gpui::Stateful<gpui::Div> {
    let colors = &theme.colors;
    let (icon, tint) = glyph_style(glyph, theme);
    div()
        .id(("activity-kind", ix))
        .role(gpui::Role::ListItem)
        .aria_label(label)
        .aria_selected(selected)
        .h(px(48.0))
        .mx(px(space::S4))
        .px(px(space::S8))
        .flex()
        .gap(px(space::S8))
        .items_center()
        .rounded(px(radius::SM))
        .when(selected, |d| {
            d.bg(if focused {
                colors.bg_selected
            } else {
                colors.bg_selected_muted
            })
        })
        .when(!selected, |d| d.hover(|d| d.bg(colors.bg_hover)))
        .child(Icon::new(icon).size(size::ICON_SM).color(tint))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(space::S8))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .type_style(typography::BODY_STRONG, theme)
                                .text_color(colors.text_primary)
                                .child(title),
                        )
                        .when_some(cadence, |d, cadence| {
                            d.child(
                                div()
                                    .flex_none()
                                    .type_style(typography::SMALL, theme)
                                    .text_color(colors.text_tertiary)
                                    .child(cadence),
                            )
                        }),
                )
                .child(
                    div()
                        .truncate()
                        .type_style(typography::SMALL, theme)
                        .text_color(colors.text_secondary)
                        .child(line.clone()),
                ),
        )
        .tooltip(Tooltip::text(line))
}
