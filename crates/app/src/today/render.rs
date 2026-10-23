//! Today's visuals (`docs/design/today.md`, spec 0017): header, the day
//! summary card with its mood illustration, Your PRs, and the right column
//! (Up next, Waiting on you, Loose ends), in two columns or one, all from
//! the precomputed [`adoken_nudge::Day`]. Theme tokens only; the only motion
//! is the illustration crossfade.

use std::time::{Duration, Instant};

use adoken_model::{Millis, TypeFamily, Vote, WorkItemType};
use adoken_nudge::{
    CountSlot, DayMood, Focus, FocusState, Heat, LooseEnd, Mood, NudgeKind, PrState, Quiet,
    Summary, Track, TrackReviewer, WaitCard, WorkingHours, copy,
};
use adoken_theme::{ColorTokens, motion, opacity, radius, size, space, typography};
use adoken_ui::motion::{EASE_IN_OUT, animation};
use adoken_ui::{
    ActiveTheme, Button, ButtonVariant, ChipTone, EmptyState, Icon, IconName, Kbd, Notice,
    Skeleton, StageBar, Tooltip, TypeStyleExt,
};
use gpui::{
    AnimationExt, AnyElement, App, Bounds, Context, Hsla, ObjectFit, Pixels, Window, canvas, div,
    img, point, prelude::*, px,
};

use super::model::{self, CONTENT_MAX, Layout, Loaded, RIGHT_COLUMN, Row};
use super::{CROSSFADE_WINDOW, DISMISS_FOCUS_KEYS, DONE_KEYS, SNOOZE_KEYS, State, TodayView};
use crate::avatar::{Avatar, AvatarSize};

/// Faces shown per PR row before `+N` (FR-18).
const MAX_FACES: usize = 3;
/// Design §3.1: the card's height at `font_scale` 1.0, the art box beside it.
const CARD_HEIGHT: f32 = 176.0;
const ART: (f32, f32) = (264.0, 176.0);
/// Below this card width the art box shrinks and centers vertically.
const CARD_COMPACT_BELOW: f32 = 640.0;
const ART_COMPACT: (f32, f32) = (198.0, 132.0);
/// Page padding (design §2).
const PAGE_X: f32 = space::S24;
const PAGE_Y: f32 = space::S16;
/// The vote badge on a face (design §4.4).
const BADGE: f32 = 12.0;
/// Skeleton rows per column (§3 Loading).
const SKELETON_ROWS: usize = 3;

/// Which illustration paints a mood (C2; the variants reuse one, FR-7).
pub fn art_path(mood: Mood) -> &'static str {
    use adoken_ui::images::*;
    match mood {
        Mood::FreshStart => TODAY_FRESH_START,
        Mood::FindingFlow => TODAY_FINDING_FLOW,
        Mood::HelpingGrow => TODAY_HELPING_GROW,
        Mood::Waiting => TODAY_WAITING,
        Mood::Blocked => TODAY_BLOCKED,
        Mood::Shipped => TODAY_SHIPPED,
        Mood::Clear => TODAY_CLEAR,
        Mood::Stalled => TODAY_STALLED,
    }
}

/// FR-31: drop the decoded `mood` illustration once it no longer paints
/// (gpui keeps a decoded image until it's evicted). Deferred past the
/// current update so the window being drawn frees its tiles too.
pub(super) fn evict_art(mood: Mood, cx: &mut App) {
    cx.defer(move |cx| gpui::ImageSource::from(art_path(mood)).evict(None, cx));
}

fn heat_color(heat: Heat, colors: &ColorTokens<Hsla>) -> Hsla {
    match heat {
        Heat::Cool => colors.text_tertiary,
        Heat::Warm => colors.status_warning_fg,
        Heat::Hot => colors.status_danger_fg,
    }
}

/// Design §4.2: the state's glyph and tone. A blocker's tone overrides the
/// stage's; waiting on reviewers stays neutral.
pub fn state_visual(state: PrState, colors: &ColorTokens<Hsla>) -> (IconName, Hsla) {
    match state {
        PrState::MergedToday => (IconName::Sparkles, colors.status_success_fg),
        PrState::Conflict => (IconName::GitMerge, colors.status_danger_fg),
        PrState::BuildFailed => (IconName::CircleX, colors.pipeline_failed),
        PrState::ChangesRequested => (IconName::Clock, colors.vote_waiting),
        PrState::NoReviews => (IconName::Users, colors.text_secondary),
        PrState::Approved => (IconName::BadgeCheck, colors.vote_approved),
        PrState::InReview => (IconName::GitPullRequest, colors.accent_fg),
        PrState::Draft => (IconName::GitPullRequestDraft, colors.pr_draft),
    }
}

fn kind_visual(kind: NudgeKind, colors: &ColorTokens<Hsla>) -> (IconName, Hsla) {
    match kind {
        NudgeKind::MergeConflicts => (IconName::GitMerge, colors.status_danger_fg),
        NudgeKind::BuildFailing => (IconName::CircleX, colors.pipeline_failed),
        NudgeKind::WaitingForAuthor => (IconName::Clock, colors.vote_waiting),
        NudgeKind::ReviewWaiting => (IconName::GitPullRequest, colors.accent_fg),
        NudgeKind::MergedStillActive => (IconName::GitMerge, colors.pr_completed),
        NudgeKind::NoReviewerActivity => (IconName::Users, colors.text_secondary),
        NudgeKind::QuietActiveItem => (IconName::CircleDashed, colors.text_tertiary),
    }
}

fn wi_visual(kind: &WorkItemType, colors: &ColorTokens<Hsla>) -> (IconName, Hsla) {
    match kind.family() {
        TypeFamily::Bug => (IconName::Bug, colors.wi_bug),
        TypeFamily::Requirement => (IconName::BookOpen, colors.wi_story),
        TypeFamily::Task => (IconName::SquareCheck, colors.wi_task),
        TypeFamily::Feature => (IconName::Trophy, colors.wi_feature),
        TypeFamily::Epic => (IconName::Crown, colors.wi_epic),
        TypeFamily::Issue => (IconName::CircleDot, colors.wi_issue),
        TypeFamily::Other => (IconName::Square, colors.text_secondary),
    }
}

fn quiet_visual(quiet: Quiet) -> (IconName, &'static str) {
    match quiet {
        Quiet::Snoozed => (IconName::AlarmClock, "snoozed"),
        Quiet::Done => (IconName::Check, "marked done"),
        Quiet::Muted => (IconName::BellOff, "muted"),
    }
}

fn keycap(keys: &str) -> Kbd {
    Kbd::parse(keys).expect("static binding")
}

/// `3 minutes`, `2 hours`, `4 days`: spoken spans.
fn spoken_span(ms: Millis) -> String {
    use adoken_model::time::{DAY, HOUR, MINUTE};
    let ms = ms.max(0);
    if ms < HOUR {
        copy::count((ms / MINUTE).max(1) as usize, "minute", "minutes")
    } else if ms < DAY {
        copy::count((ms / HOUR) as usize, "hour", "hours")
    } else {
        copy::count((ms / DAY) as usize, "day", "days")
    }
}

/// The faces a row draws (FR-18): reviewers other than me who haven't
/// declined, required first; groups only when shown, after the people.
fn faces(track: &Track, show_groups: bool) -> Vec<&TrackReviewer> {
    let people = track.reviewers.iter().filter(|r| !r.is_group);
    let groups = track.reviewers.iter().filter(|r| r.is_group && show_groups);
    people.chain(groups).collect()
}

fn vote_word(vote: Vote) -> &'static str {
    match vote {
        Vote::NoVote => "no vote",
        Vote::Approved => "approved",
        Vote::ApprovedWithSuggestions => "approved with suggestions",
        Vote::WaitingForAuthor => "waiting for author",
        Vote::Rejected => "rejected",
    }
}

/// FR-28 / design §7: a PR row's spoken label.
pub fn pr_label(track: &Track, now: Millis, show_groups: bool) -> String {
    let mut label = format!("Pull request !{} {}. ", track.pr_id, track.title);
    if track.state == PrState::MergedToday {
        label.push_str(&format!("{}.", track.line));
    } else {
        label.push_str(&format!(
            "{}, stage {} of 4. {}. Updated {} ago.",
            track.state_label.replace(" · ", ", "),
            track.state.filled(track.stage),
            track.line.replace(" · ", ", "),
            spoken_span(now - track.activity.last)
        ));
    }
    let faces = faces(track, show_groups);
    if !faces.is_empty() {
        let names: Vec<String> = faces
            .iter()
            .map(|r| format!("{} {}", r.name, vote_word(r.vote)))
            .collect();
        label.push_str(&format!(" Reviewers: {}.", names.join(", ")));
    }
    if let Some(q) = track.quiet {
        label.push_str(&format!(" {}.", quiet_visual(q).1));
    }
    label
}

/// Design §7: `Review !4347 Fix Terraform state locking from Kim Becker,
/// waiting 18 days, required reviewer.`
pub fn waiting_label(card: &WaitCard, now: Millis) -> String {
    let mut label = format!(
        "Review !{} {} from {}, waiting {}",
        card.pr_id,
        card.title,
        card.author_name,
        spoken_span(now - card.age.since)
    );
    if card.required {
        label.push_str(", required reviewer");
    }
    if let Some(q) = card.quiet {
        label.push_str(&format!(", {}", quiet_visual(q).1));
    }
    label.push('.');
    label
}

/// The loose end pill (owner decision on spec 0017 Q-2): `Open 3d`.
pub fn open_pill(item: &LooseEnd, now: Millis) -> String {
    format!("Open {}", copy::compact(now - item.open_since))
}

/// Design §7: `Task #48213 Fix SSO redirect, Active for 3 days. Its pull
/// request !901 merged 3 days ago.`
pub fn loose_label(item: &LooseEnd, now: Millis, hours: &WorkingHours) -> String {
    let mut label = format!(
        "{} #{} {}, {} for {}.",
        item.kind.as_str(),
        item.id,
        item.title,
        item.state,
        spoken_span(now - item.open_since)
    );
    if let Some(pr) = item.merged_pr {
        label.push_str(&format!(
            " Its pull request !{pr} merged {}.",
            copy::ago(item.age.since, now, hours)
        ));
    }
    if let Some(q) = item.quiet {
        label.push_str(&format!(" {}.", quiet_visual(q).1));
    }
    label
}

/// Design §7: `Up next: Mara is blocked on you. !4821 Fix login throttle,
/// 3 files, waiting 2 days. Return: Review now.`
pub fn up_next_label(focus: &Focus, now: Millis) -> String {
    let mut label = format!("Up next: {}. {}", focus.headline, focus.label);
    if let Some(files) = focus.files {
        label.push_str(&format!(
            ", {}",
            copy::count(files as usize, "file", "files")
        ));
    }
    label.push_str(&format!(
        ", waiting {}. Return: {}.",
        spoken_span(now - focus.age.since),
        focus.nudge.action.label()
    ));
    label
}

/// The loose end tooltip: what the age measures.
fn loose_tooltip(item: &LooseEnd, now: Millis, hours: &WorkingHours) -> String {
    let open = format!(
        "#{} has been {} for {}",
        item.id,
        item.state,
        spoken_span(now - item.open_since)
    );
    match item.merged_pr {
        Some(pr) => format!(
            "{open}; PR !{pr} merged {}",
            copy::ago(item.age.since, now, hours)
        ),
        None => open,
    }
}

impl TodayView {
    /// Bring `row` into the page's scroll viewport (rows as last painted).
    pub(super) fn scroll_into_view(&self, row: Row) {
        let Some(bounds) = self.rows.borrow().get(&row).copied() else {
            return;
        };
        let viewport = self.scroll.bounds();
        let offset = self.scroll.offset();
        let margin = px(space::S16);
        let dy = if bounds.top() < viewport.top() {
            viewport.top() - bounds.top() + margin
        } else if bounds.bottom() > viewport.bottom() {
            -(bounds.bottom() - viewport.bottom() + margin)
        } else {
            return;
        };
        self.scroll.set_offset(point(offset.x, offset.y + dy));
    }

    /// A selectable row: selection ring, click = select + open, right-click
    /// = row menu, painted bounds recorded for `h`/`l` and scrolling.
    fn row_shell(
        &self,
        id: impl Into<gpui::ElementId>,
        row: Row,
        selected: bool,
        label: String,
        quiet: Option<Quiet>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let colors = &cx.theme().colors;
        let hover = colors.bg_hover;
        let rows = self.rows.clone();
        let entity = cx.entity().downgrade();
        div()
            .id(id)
            .relative()
            .role(gpui::Role::ListItem)
            .aria_label(label)
            .aria_selected(selected)
            .when(selected, |el| el.aria_active_descendant())
            .child(
                canvas(
                    move |bounds, _, _| {
                        rows.borrow_mut().insert(row, bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .children(
                selected.then(|| adoken_ui::action_menu::track_bounds(self.row_bounds.clone())),
            )
            .when(quiet.is_some(), |el| el.opacity(opacity::DIMMED))
            .when(!selected, |el| el.hover(move |el| el.bg(hover)))
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    if let Some(key) = this.loaded().and_then(|l| model::key(l, row)) {
                        this.selected = Some(key);
                    }
                    this.open_row_menu(
                        adoken_ui::action_menu::MenuPosition::at_pointer(event.position),
                        window,
                        cx,
                    );
                }),
            )
            .on_click(move |_, window, cx| {
                entity
                    .update(cx, |this, cx| {
                        if let Some(key) = this.loaded().and_then(|l| model::key(l, row)) {
                            this.selected = Some(key);
                        }
                        this.activate(row, cx);
                        window.focus(&this.focus, cx);
                        cx.notify();
                    })
                    .ok();
            })
    }

    pub(super) fn render_body(
        &mut self,
        wide: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loaded = match &self.state {
            State::Ready(loaded) => Some(loaded.clone()),
            _ => None,
        };
        let theme = cx.theme().clone();
        let width_slot = self.width.clone();
        let painted_wide = wide;
        // FR-24: Today's own width decides the columns; a crossing repaints
        // with the new layout on the next frame.
        let measure = canvas(
            move |bounds: Bounds<Pixels>, window, _| {
                let width = f32::from(bounds.size.width);
                width_slot.set(Some(width));
                if Layout::is_wide(width) != painted_wide {
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();
        let width = self
            .width
            .get()
            .unwrap_or_else(|| f32::from(window.viewport_size().width));
        let content_width = width.min(CONTENT_MAX) - 2.0 * PAGE_X;

        let mut page = div()
            .w_full()
            .max_w(px(CONTENT_MAX))
            .mx_auto()
            .px(px(PAGE_X))
            .py(px(PAGE_Y))
            .flex()
            .flex_col()
            .gap(px(space::S16));
        page = page.child(self.render_header(loaded.as_deref(), cx));
        if let Some(error) = &self.error {
            page = page.child(
                Notice::new(ChipTone::Danger, format!("Couldn't load Today. {error}."))
                    .icon(IconName::CircleX)
                    .child(Button::new("today-retry", "Retry").on_press({
                        let this = cx.entity().downgrade();
                        move |_, cx| {
                            this.update(cx, |this, cx| this.retry(cx)).ok();
                        }
                    })),
            );
        }
        match (&self.state, loaded) {
            (State::NoAccount, _) => {
                page = page.child(
                    div().pt(px(space::S32)).child(
                        EmptyState::new(IconName::Sun, "Today fills in after the first sync")
                            .body("Your reviews, pull requests and work items appear here."),
                    ),
                );
            }
            (_, Some(loaded)) => {
                self.rows.borrow_mut().clear();
                let layout = Layout::of(&loaded, wide);
                page = page
                    .child(self.render_summary(&loaded.day.summary, content_width, cx))
                    .child(self.render_columns(&loaded, &layout, cx));
            }
            (_, None) if self.skeleton => {
                page = page.child(self.render_skeleton(wide, content_width, cx));
            }
            (_, None) => {}
        }
        div()
            .id("today-scroll")
            .relative()
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(measure)
            .child(page)
            .child(self.render_scroll_restore())
            .text_color(theme.colors.text_primary)
            .into_any_element()
    }

    fn render_header(&self, loaded: Option<&Loaded>, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let now = loaded.map_or_else(|| self.clock.now(), |l| l.now);
        let stale = loaded.and_then(|l| l.snapshot.health.stale);
        let label = stale.map(|stale| {
            let age = stale
                .since
                .map(|since| format!("Updated {} ago · ", copy::compact(now - since)))
                .unwrap_or_default();
            div()
                .id("today-stale")
                .flex()
                .items_center()
                .gap(px(space::S4))
                .text_color(colors.text_tertiary)
                .type_style(typography::SMALL, theme)
                .child(format!("{age}Refresh"))
                .child(keycap(crate::sync::SYNC_NOW_KEYS))
        });
        div()
            .flex()
            .items_start()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().type_style(typography::TITLE, theme).child("Today"))
                    .child(
                        div()
                            .text_color(colors.text_secondary)
                            .type_style(typography::SMALL, theme)
                            .child(copy::long_date(now, &self.settings.hours)),
                    ),
            )
            .children(label)
            .into_any_element()
    }

    fn render_summary(
        &mut self,
        summary: &Summary,
        card_width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let compact = card_width < CARD_COMPACT_BELOW;
        let (art_w, art_h) = if compact { ART_COMPACT } else { ART };
        let slot = |s: &CountSlot, first: bool| {
            let tone = if s.quiet() {
                colors.text_tertiary
            } else {
                colors.text_primary
            };
            let label_tone = if s.quiet() {
                colors.text_tertiary
            } else {
                colors.text_secondary
            };
            div()
                .id(("today-count", s.slot as usize))
                .tooltip(Tooltip::text(s.tooltip()))
                .flex()
                .items_center()
                .when(!first, |el| {
                    el.child(
                        div()
                            .mx(px(space::S24))
                            .w(px(size::BORDER))
                            .h(px(
                                typography::TITLE.line_height + typography::SMALL.line_height
                            ))
                            .bg(colors.border_subtle),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_color(tone)
                                .type_style(typography::TITLE, &theme)
                                .child(s.number()),
                        )
                        .child(
                            div()
                                .text_color(label_tone)
                                .type_style(typography::SMALL, &theme)
                                .child(s.label()),
                        ),
                )
        };
        let text = div()
            .flex_1()
            .min_w_0()
            .pl(px(space::S24))
            .pr(px(space::S16))
            .py(px(space::S20))
            .flex()
            .flex_col()
            .child(
                div()
                    .truncate()
                    .type_style(typography::DISPLAY, &theme)
                    .child(summary.headline()),
            )
            .child(
                div()
                    .id("today-sentence")
                    .mt(px(space::S4))
                    .truncate()
                    .text_color(colors.text_secondary)
                    .tooltip(Tooltip::text(summary.sentence.clone()))
                    .child(summary.sentence.clone()),
            )
            .child(
                div()
                    .mt(px(space::S16))
                    .h(px(
                        typography::TITLE.line_height + typography::SMALL.line_height
                    ))
                    .flex()
                    .items_center()
                    .children(
                        summary
                            .slots
                            .iter()
                            .enumerate()
                            .map(|(i, s)| slot(s, i == 0)),
                    ),
            );
        div()
            .id("today.summary")
            .role(gpui::Role::Region)
            .aria_label(summary.spoken())
            .aria_description("Day summary")
            .w_full()
            .min_h(px(CARD_HEIGHT))
            .flex()
            .rounded(px(radius::MD))
            .border_1()
            .border_color(colors.border_default)
            .bg(colors.bg_raised)
            .overflow_hidden()
            .children(adoken_ui::tour::anchor(super::anchors::SUMMARY, cx))
            .child(text)
            .child(
                div()
                    .flex_none()
                    .w(px(art_w))
                    .h(px(art_h))
                    .when(compact, |el| el.my_auto())
                    .relative()
                    .child(self.render_art(summary.mood, cx)),
            )
            .into_any_element()
    }

    /// FR-30: decode `art` on the background executor (the same asset load
    /// `img` uses, so the paint finds it ready); the current art stays on
    /// screen until then, and the card's text updates right away. A fade
    /// still running ends now so at most two illustrations stay decoded.
    pub(super) fn decode_art(&mut self, art: Mood, cx: &mut Context<Self>) {
        if let Some(stale) = self.art.pending.replace(art) {
            evict_art(stale, cx);
        }
        if let Some(gone) = self.art.previous.take() {
            evict_art(gone, cx);
        }
        self.art.fading = false;
        self.art._fade = None;
        let resource = gpui::Resource::Embedded(art_path(art).into());
        let (decoded, _) = cx.fetch_asset::<gpui::ImgResourceLoader>(&resource);
        self.art._decode = Some(cx.spawn(async move |this, cx| {
            // A failed decode paints nothing, as `img` would.
            let _ = decoded.await;
            this.update(cx, |this, cx| {
                if this.art.pending == Some(art) {
                    this.art.pending = None;
                    this.swap_art(art, cx);
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// The decoded `art` replaces the current one: it fades in over it at
    /// the next paint, or under Reduce Motion simply replaces it.
    fn swap_art(&mut self, art: Mood, cx: &mut Context<Self>) {
        let old = self.art.current.replace(art).filter(|old| *old != art);
        if cx.reduce_motion() {
            if let Some(old) = old {
                evict_art(old, cx);
            }
        } else {
            self.art.previous = old;
        }
        self.art.changed_at = Some(Instant::now());
        self.art.generation += 1;
    }

    /// The mood illustration (FR-29–FR-32): painted unmodified from the art
    /// decoded off the UI thread; the card surface shows until the first
    /// one is ready. On a mood change painted right away (Today on screen)
    /// the new art fades in over the old at `motion.base`; Reduce Motion
    /// swaps it in one frame.
    fn render_art(&mut self, mood: DayMood, cx: &mut Context<Self>) -> AnyElement {
        let current = self.art.current.unwrap_or(mood.art());
        if self.art.previous.is_some() && !self.art.fading {
            let fresh = self
                .art
                .changed_at
                .is_some_and(|at| at.elapsed() <= CROSSFADE_WINDOW);
            if fresh && !cx.reduce_motion() {
                self.art.fading = true;
                self.art._fade = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(u64::from(motion::BASE)))
                        .await;
                    this.update(cx, |this, cx| {
                        if let Some(gone) = this.art.previous.take() {
                            evict_art(gone, cx);
                        }
                        this.art.fading = false;
                        cx.notify();
                    })
                    .ok();
                }));
            } else if let Some(gone) = self.art.previous.take() {
                evict_art(gone, cx);
            }
        }
        let picture = |mood: Mood| {
            img(art_path(mood))
                .absolute()
                .inset_0()
                .size_full()
                .object_fit(ObjectFit::Contain)
        };
        let mut art = div().absolute().inset_0();
        if let Some(previous) = self.art.previous {
            art = art.child(picture(previous));
        }
        if self.art.fading {
            art = art.child(
                picture(current)
                    .with_animation(
                        ("today-art", self.art.generation),
                        animation(motion::BASE, EASE_IN_OUT),
                        |image, t| image.opacity(t),
                    )
                    .into_any_element(),
            );
        } else {
            art = art.child(picture(current));
        }
        art.into_any_element()
    }

    fn render_columns(
        &mut self,
        loaded: &Loaded,
        layout: &Layout,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.selected_row(layout);
        let show_groups = self
            .groups_override
            .unwrap_or_else(|| crate::review::groups::shown(cx));
        let up_next = loaded
            .up_next_visible
            .then(|| self.render_up_next(loaded, selected, cx));
        let prs = self.render_prs(loaded, selected, show_groups, cx);
        let waiting = self.render_waiting(loaded, selected, cx);
        let loose = loaded
            .snapshot
            .health
            .work_scope
            .then(|| self.render_loose(loaded, selected, cx));
        if layout.wide {
            div()
                .flex()
                .items_start()
                .gap(px(space::S16))
                .child(div().flex_1().min_w(px(model::PRS_MIN)).child(prs))
                .child(
                    div()
                        .flex_none()
                        .w(px(RIGHT_COLUMN))
                        .flex()
                        .flex_col()
                        .gap(px(space::S16))
                        .children(up_next)
                        .child(waiting)
                        .children(loose),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(space::S16))
                .children(up_next)
                .child(prs)
                .child(waiting)
                .children(loose)
                .into_any_element()
        }
    }

    /// A titled panel (design §2): `bg.raised`, `radius.md`, a count pill.
    fn panel(
        &self,
        id: &'static str,
        title: &'static str,
        count: Option<usize>,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = cx.theme();
        let colors = &theme.colors;
        div()
            .id(id)
            .p(px(space::S16))
            .rounded(px(radius::MD))
            .border_1()
            .border_color(colors.border_default)
            .bg(colors.bg_raised)
            .flex()
            .flex_col()
            // Panels are the 0017/0020 tour anchors (`today.prs`, …): the id
            // is the anchor id (`super::anchors`).
            .children(adoken_ui::tour::anchor(id, cx))
            .child(
                div()
                    .pb(px(space::S12))
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .child(div().type_style(typography::HEADING, theme).child(title))
                    .children(count.map(|n| {
                        div()
                            .px(px(space::S8))
                            .rounded(px(radius::FULL))
                            .bg(colors.bg_hover)
                            .text_color(colors.text_secondary)
                            .type_style(typography::LABEL, theme)
                            .child(n.to_string())
                    })),
            )
    }

    fn quiet_line(&self, text: &'static str, cx: &App) -> AnyElement {
        let theme = cx.theme();
        div()
            .text_color(theme.colors.text_tertiary)
            .type_style(typography::SMALL, theme)
            .child(text)
            .into_any_element()
    }

    fn render_prs(
        &self,
        loaded: &Loaded,
        selected: Option<Row>,
        show_groups: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tracks = &loaded.day.tracks;
        let list = div()
            .id("today-pr-list")
            .role(gpui::Role::List)
            .aria_label(format!("Your pull requests, {}", tracks.len()))
            .flex()
            .flex_col()
            .gap(px(space::S8));
        let body = if tracks.is_empty() {
            let theme = cx.theme();
            div()
                .flex()
                .flex_col()
                .gap(px(space::S4))
                .child(
                    div()
                        .text_color(theme.colors.text_secondary)
                        .child("No open pull requests"),
                )
                .child(self.quiet_line(
                    "Your pull requests show here while they're open and on the day they merge.",
                    cx,
                ))
                .into_any_element()
        } else {
            list.children(tracks.iter().enumerate().map(|(i, track)| {
                let row = Row::Pr(i);
                self.render_pr_row(row, track, loaded, selected == Some(row), show_groups, cx)
            }))
            .into_any_element()
        };
        self.panel("today.prs", "Your PRs", Some(tracks.len()), cx)
            .child(body)
            .into_any_element()
    }

    fn render_pr_row(
        &self,
        row: Row,
        track: &Track,
        loaded: &Loaded,
        selected: bool,
        show_groups: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let merged = track.state == PrState::MergedToday;
        let (glyph, tone) = state_visual(track.state, colors);
        let label = pr_label(track, loaded.now, show_groups);
        let first = matches!(row, Row::Pr(0));
        let updated = format!(
            "Updated {}",
            copy::ago(track.activity.last, loaded.now, &self.settings.hours)
        );
        let title = div()
            .id(("today-pr-title", track.pr_id as u64))
            .flex_1()
            .min_w_0()
            .line_clamp(3)
            .text_ellipsis()
            .type_style(typography::BODY_STRONG, &theme)
            .tooltip(Tooltip::text(track.title.clone()))
            .child(track.title.clone());
        let line1 = div()
            .flex()
            .items_start()
            .gap(px(space::S12))
            .child(title)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .whitespace_nowrap()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, &theme)
                    .children(track.quiet.map(|q| {
                        Icon::new(quiet_visual(q).0)
                            .size(size::ICON_XS)
                            .color(colors.text_tertiary)
                    }))
                    .child(updated),
            );
        let line2 = div()
            .mt(px(space::S8))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .child(
                div()
                    // 0020 tour anchor on the first row's bar (FR-27).
                    .id(if first {
                        gpui::ElementId::from("today.pr-stage")
                    } else {
                        ("today-stage", track.pr_id as u64).into()
                    })
                    .child(StageBar::new(track.state.filled(track.stage), tone)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .min_w_0()
                    .text_color(tone)
                    .type_style(typography::SMALL, &theme)
                    .font_weight(gpui::FontWeight(560.0))
                    .child(Icon::new(glyph).size(size::ICON_SM).color(tone))
                    .child(div().truncate().child(track.state_label.clone())),
            )
            .child(div().flex_1())
            .child(self.render_faces(track, show_groups, cx));
        let line3 = div()
            .mt(px(space::S6))
            .flex()
            .items_center()
            .gap(px(space::S4))
            .min_w_0()
            .child(
                div()
                    .flex_none()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::CODE_INLINE, &theme)
                    .child(format!("!{}", track.pr_id)),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(colors.text_secondary)
                    .type_style(typography::SMALL, &theme)
                    .child(format!("· {}", track.line)),
            );
        self.row_shell(
            ("today-pr", track.pr_id as u64),
            row,
            selected,
            label,
            track.quiet,
            cx,
        )
        .p(px(space::S12))
        .rounded(px(radius::SM))
        .border_1()
        .border_color(if selected {
            colors.border_focus
        } else {
            colors.border_subtle
        })
        .bg(if selected {
            colors.bg_selected
        } else if merged {
            colors.status_success_wash
        } else {
            colors.bg_surface
        })
        .child(line1)
        .child(line2)
        .child(line3)
        .into_any_element()
    }

    /// FR-18: up to three faces, required first, `+N`; a vote badge on
    /// non-zero votes, dimmed faces without one; groups as initials after
    /// the people when shown (C1).
    fn render_faces(&self, track: &Track, show_groups: bool, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let faces = faces(track, show_groups);
        let extra = faces.len().saturating_sub(MAX_FACES);
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(space::S4))
            .children(faces.into_iter().take(MAX_FACES).map(|r| {
                let badge = (r.vote != Vote::NoVote).then(|| {
                    let (icon, color) = crate::review::pr::vote_icon(r.vote, colors);
                    div()
                        .absolute()
                        .right(px(-space::S4))
                        .bottom(px(-space::S2))
                        .size(px(BADGE))
                        .rounded(px(radius::FULL))
                        .bg(colors.bg_raised)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(icon).size(BADGE - space::S2).color(color))
                });
                let tooltip = match (r.required, r.vote) {
                    (true, Vote::NoVote) => format!("{} · Required · No vote yet", r.name),
                    (_, vote) => format!("{} · {}", r.name, vote.label()),
                };
                div()
                    .id(gpui::ElementId::Name(
                        format!("today-face-{}-{}", track.pr_id, r.person).into(),
                    ))
                    .relative()
                    .tooltip(Tooltip::text(tooltip))
                    .when(r.vote == Vote::NoVote, |el| el.opacity(opacity::DIMMED))
                    .child(
                        Avatar::new(r.person.clone(), r.name.clone())
                            .group(r.is_group)
                            .size(AvatarSize::Md),
                    )
                    .children(badge)
            }))
            .when(extra > 0, |el| {
                el.child(
                    div()
                        .text_color(colors.text_tertiary)
                        .type_style(typography::LABEL, theme)
                        .child(format!("+{extra}")),
                )
            })
            .into_any_element()
    }

    fn render_up_next(
        &self,
        loaded: &Loaded,
        selected: Option<Row>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let panel = self.panel("today.up-next", "Up next", None, cx);
        let body = match &loaded.day.focus {
            FocusState::Item(focus) => {
                let face: AnyElement = match (&focus.who, &focus.who_name) {
                    (Some(id), Some(name)) => Avatar::new(id.clone(), name.clone())
                        .size(AvatarSize::Lg)
                        .into_any_element(),
                    _ => {
                        let (icon, color) = kind_visual(focus.nudge.kind, colors);
                        Icon::new(icon)
                            .size(size::ICON_MD)
                            .color(color)
                            .into_any_element()
                    }
                };
                let waited = loaded.now - focus.age.since;
                let mut meta = div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(space::S8))
                    .text_color(colors.text_secondary)
                    .type_style(typography::SMALL, &theme);
                if let Some(files) = focus.files {
                    meta = meta.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(space::S2))
                            .child(
                                Icon::new(IconName::FileDiff)
                                    .size(size::ICON_XS)
                                    .color(colors.text_tertiary),
                            )
                            .child(files.to_string()),
                    );
                }
                if let Some(activity) = &focus.activity {
                    meta = meta.child(activity.clone());
                }
                meta = meta.child(
                    div()
                        .text_color(heat_color(focus.age.heat(), colors))
                        .child(copy::compact(waited)),
                );
                let this = cx.entity().downgrade();
                let go = Button::new("today-up-next-go", focus.nudge.action.label())
                    .variant(if focus.due {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Secondary
                    })
                    .shortcut(keycap("enter"))
                    .on_press(move |_, cx| {
                        this.update(cx, |this, cx| this.activate(Row::UpNext, cx))
                            .ok();
                    });
                let key = |keys: &str, what: &'static str| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(space::S4))
                        .child(keycap(keys))
                        .child(what)
                };
                let is_selected = selected == Some(Row::UpNext);
                self.row_shell(
                    "today-up-next-row",
                    Row::UpNext,
                    is_selected,
                    up_next_label(focus, loaded.now),
                    None,
                    cx,
                )
                .p(px(space::S8))
                .m(px(-space::S8))
                .rounded(px(radius::SM))
                .border_1()
                .border_color(if is_selected {
                    colors.border_focus
                } else {
                    adoken_ui::theme::hsla(adoken_theme::Color::TRANSPARENT)
                })
                .when(is_selected, |el| el.bg(colors.bg_selected))
                .flex()
                .flex_col()
                .gap(px(space::S8))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(space::S8))
                        .child(face)
                        .child(
                            div()
                                .min_w_0()
                                .type_style(typography::HEADING, &theme)
                                .child(focus.headline.clone()),
                        ),
                )
                .child(
                    div()
                        .line_clamp(2)
                        .text_ellipsis()
                        .type_style(typography::BODY_STRONG, &theme)
                        .child(focus.label.clone()),
                )
                .child(meta)
                .child(go)
                .child(
                    div()
                        .flex()
                        .gap(px(space::S12))
                        .text_color(colors.text_tertiary)
                        .type_style(typography::SMALL, &theme)
                        .child(key(SNOOZE_KEYS, "Snooze"))
                        .child(key(DONE_KEYS, "Done"))
                        .child(key(DISMISS_FOCUS_KEYS, "Hide")),
                )
                .into_any_element()
            }
            FocusState::Clear => div()
                .flex()
                .items_center()
                .gap(px(space::S8))
                .child(
                    Icon::new(IconName::Check)
                        .size(size::ICON_MD)
                        .color(colors.status_success_fg),
                )
                .child(
                    div()
                        .text_color(colors.text_secondary)
                        .child("Nothing waits on you"),
                )
                .into_any_element(),
            FocusState::Syncing => self.quiet_line("Up next appears after the first full sync", cx),
        };
        panel.child(body).into_any_element()
    }

    fn render_waiting(
        &self,
        loaded: &Loaded,
        selected: Option<Row>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let waiting = &loaded.day.waiting;
        let panel = self.panel("today.waiting", "Waiting on you", Some(waiting.len()), cx);
        if waiting.is_empty() {
            return panel
                .child(self.quiet_line("No reviews waiting on you", cx))
                .into_any_element();
        }
        let rows: Vec<AnyElement> = waiting
            .iter()
            .enumerate()
            .map(|(i, card)| {
                let row = Row::Waiting(i);
                self.render_wait_row(row, card, loaded, selected == Some(row), cx)
            })
            .collect();
        panel
            .child(
                div()
                    .id("today-waiting-list")
                    .role(gpui::Role::List)
                    .aria_label(format!("Waiting on you, {}", waiting.len()))
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_wait_row(
        &self,
        row: Row,
        card: &WaitCard,
        loaded: &Loaded,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let waited = loaded.now - card.age.since;
        self.row_shell(
            ("today-waiting", card.pr_id as u64),
            row,
            selected,
            waiting_label(card, loaded.now),
            card.quiet,
            cx,
        )
        .h(px(size::ROW_COMPACT))
        .px(px(space::S8))
        .mx(px(-space::S8))
        .rounded(px(radius::SM))
        .border_1()
        .border_color(if selected {
            colors.border_focus
        } else {
            adoken_ui::theme::hsla(adoken_theme::Color::TRANSPARENT)
        })
        .when(selected, |el| el.bg(colors.bg_selected))
        .flex()
        .items_center()
        .gap(px(space::S8))
        .child(Avatar::new(card.author.clone(), card.author_name.clone()).size(AvatarSize::Md))
        .child(
            div()
                .id(("today-waiting-title", card.pr_id as u64))
                .flex_1()
                .min_w_0()
                .truncate()
                .tooltip(Tooltip::text(card.title.clone()))
                .child(card.title.clone()),
        )
        .children(card.quiet.map(|q| {
            Icon::new(quiet_visual(q).0)
                .size(size::ICON_XS)
                .color(colors.text_tertiary)
        }))
        .when(card.required, |el| {
            el.child(
                Icon::new(IconName::Shield)
                    .size(size::ICON_XS)
                    .color(colors.text_tertiary),
            )
        })
        .child(
            div()
                .flex_none()
                .text_color(heat_color(card.age.heat(), colors))
                .type_style(typography::SMALL, &theme)
                .child(copy::compact(waited)),
        )
        .into_any_element()
    }

    fn render_loose(
        &self,
        loaded: &Loaded,
        selected: Option<Row>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loose = &loaded.day.loose;
        let panel = self.panel("today.loose-ends", "Loose ends", Some(loose.len()), cx);
        if loose.is_empty() {
            return panel
                .child(self.quiet_line("No loose ends", cx))
                .into_any_element();
        }
        let rows: Vec<AnyElement> = loose
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let row = Row::Loose(i);
                self.render_loose_row(row, item, loaded, selected == Some(row), cx)
            })
            .collect();
        panel
            .child(
                div()
                    .id("today-loose-list")
                    .role(gpui::Role::List)
                    .aria_label(format!("Loose ends, {}", loose.len()))
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_loose_row(
        &self,
        row: Row,
        item: &LooseEnd,
        loaded: &Loaded,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let hours = &self.settings.hours;
        let (icon, color) = wi_visual(&item.kind, colors);
        let (pill_icon, pill_color) = match item.merged_pr {
            Some(_) => (IconName::GitMerge, colors.pr_completed),
            None => (IconName::CircleDashed, colors.text_tertiary),
        };
        self.row_shell(
            ("today-loose", item.id as u64),
            row,
            selected,
            loose_label(item, loaded.now, hours),
            item.quiet,
            cx,
        )
        .h(px(size::ROW_COMPACT))
        .px(px(space::S8))
        .mx(px(-space::S8))
        .rounded(px(radius::SM))
        .border_1()
        .border_color(if selected {
            colors.border_focus
        } else {
            adoken_ui::theme::hsla(adoken_theme::Color::TRANSPARENT)
        })
        .when(selected, |el| el.bg(colors.bg_selected))
        .flex()
        .items_center()
        .gap(px(space::S8))
        .child(Icon::new(icon).size(size::ICON_SM).color(color))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(item.title.clone()),
        )
        .children(item.quiet.map(|q| {
            Icon::new(quiet_visual(q).0)
                .size(size::ICON_XS)
                .color(colors.text_tertiary)
        }))
        .child(
            div()
                .id(("today-loose-age", item.id as u64))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(space::S4))
                .px(px(space::S8))
                .py(px(space::S2))
                .rounded(px(radius::FULL))
                .bg(colors.bg_hover)
                .text_color(colors.text_secondary)
                .type_style(typography::LABEL, &theme)
                .tooltip(Tooltip::text(loose_tooltip(item, loaded.now, hours)))
                .child(Icon::new(pill_icon).size(size::ICON_XS).color(pill_color))
                .child(open_pill(item, loaded.now)),
        )
        .into_any_element()
    }

    /// §3 Loading: the card frame with two text bars and an empty art box,
    /// three PR row bars, three right-column bars. Flat under Reduce Motion.
    fn render_skeleton(&self, wide: bool, card_width: f32, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let (art_w, _) = if card_width < CARD_COMPACT_BELOW {
            ART_COMPACT
        } else {
            ART
        };
        let frame = |id: &'static str| {
            div()
                .id(id)
                .p(px(space::S16))
                .rounded(px(radius::MD))
                .border_1()
                .border_color(colors.border_default)
                .bg(colors.bg_raised)
                .flex()
                .flex_col()
                .gap(px(space::S12))
        };
        let bars = |prefix: &'static str| {
            (0..SKELETON_ROWS).map(move |i| {
                Skeleton::new((prefix, i))
                    .height(space::S48)
                    .into_any_element()
            })
        };
        let card = div()
            .h(px(CARD_HEIGHT))
            .rounded(px(radius::MD))
            .border_1()
            .border_color(colors.border_default)
            .bg(colors.bg_raised)
            .flex()
            .child(
                div()
                    .flex_1()
                    .p(px(space::S24))
                    .flex()
                    .flex_col()
                    .gap(px(space::S12))
                    .child(Skeleton::new("today-sk-headline").width(240.0).height(28.0))
                    .child(Skeleton::new("today-sk-sentence").width(320.0)),
            )
            .child(div().flex_none().w(px(art_w)));
        let prs = frame("today-sk-prs").children(bars("today-sk-pr"));
        let right = frame("today-sk-right").children(bars("today-sk-right"));
        let columns = if wide {
            div()
                .flex()
                .items_start()
                .gap(px(space::S16))
                .child(div().flex_1().child(prs))
                .child(div().flex_none().w(px(RIGHT_COLUMN)).child(right))
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(space::S16))
                .child(prs)
                .child(right)
        };
        div()
            .flex()
            .flex_col()
            .gap(px(space::S16))
            .child(card)
            .child(columns)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_model::time::{DAY, HOUR, MINUTE};
    use adoken_nudge::{Age, Commits, PrActivity, Stage};
    use gpui::AssetSource as _;

    const NOW: Millis = 1_790_000_000_000;

    fn track() -> Track {
        Track {
            pr_id: 66231,
            title: "Fix session timeout on reconnect".into(),
            stage: Stage::Review,
            blocker: None,
            blocked: None,
            state: PrState::InReview,
            state_label: "In review".into(),
            reviewers: vec![
                TrackReviewer {
                    person: "jd".into(),
                    name: "Jane Doe".into(),
                    vote: Vote::Approved,
                    required: true,
                    is_group: false,
                },
                TrackReviewer {
                    person: "grp".into(),
                    name: "React PR reviewers".into(),
                    vote: Vote::NoVote,
                    required: true,
                    is_group: true,
                },
                TrackReviewer {
                    person: "ms".into(),
                    name: "Mike Smith".into(),
                    vote: Vote::NoVote,
                    required: false,
                    is_group: false,
                },
            ],
            nudge: None,
            quiet: None,
            activity: PrActivity {
                commits: Commits::Known {
                    n: 2,
                    lower_bound: false,
                },
                comments: 1,
                opened: false,
                merged_at: None,
                last: NOW - 3 * MINUTE,
            },
            line: "2 commits today · 1 new comment".into(),
        }
    }

    #[test]
    fn ac27_pr_rows_read_as_one_sentence() {
        assert_eq!(
            pr_label(&track(), NOW, false),
            "Pull request !66231 Fix session timeout on reconnect. In review, stage 2 of 4. \
             2 commits today, 1 new comment. Updated 3 minutes ago. \
             Reviewers: Jane Doe approved, Mike Smith no vote."
        );
        let mut merged = track();
        merged.state = PrState::MergedToday;
        merged.stage = Stage::Merged;
        merged.line = "Merged today at 14:32".into();
        assert!(pr_label(&merged, NOW, false).starts_with(
            "Pull request !66231 Fix session timeout on reconnect. Merged today at 14:32."
        ));
    }

    #[test]
    fn ac20_group_reviewers_hide_unless_shown_and_come_after_people() {
        let t = track();
        let names = |show| {
            faces(&t, show)
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(false), ["Jane Doe", "Mike Smith"]);
        assert_eq!(
            names(true),
            ["Jane Doe", "Mike Smith", "React PR reviewers"]
        );
    }

    #[test]
    fn ac22_loose_ends_say_how_long_they_have_been_open() {
        let nudge = adoken_nudge::Nudge {
            kind: NudgeKind::MergedStillActive,
            subject: adoken_model::Subject::WorkItem(48213),
            since: NOW - 3 * DAY,
            waited: 0,
            label: String::new(),
            message: String::new(),
            detail: None,
            action: adoken_nudge::NudgeAction::OpenInAdo,
            ping: Vec::new(),
            related: None,
        };
        let item = LooseEnd {
            id: 48213,
            title: "Fix SSO redirect".into(),
            kind: WorkItemType::Task,
            state: "Active".into(),
            merged_pr: Some(901),
            age: Age {
                since: NOW - 3 * DAY,
                waited: 27 * HOUR,
                threshold: 24 * HOUR,
            },
            open_since: NOW - 8 * DAY - HOUR,
            quiet: None,
            nudge,
        };
        let hours = WorkingHours::default();
        assert_eq!(open_pill(&item, NOW), "Open 8d");
        assert_eq!(
            loose_label(&item, NOW, &hours),
            "Task #48213 Fix SSO redirect, Active for 8 days. Its pull request !901 merged 3 days ago."
        );
    }

    #[test]
    fn ac27_waiting_rows_and_up_next_read_aloud() {
        let nudge = adoken_nudge::Nudge {
            kind: NudgeKind::ReviewWaiting,
            subject: adoken_model::Subject::Pr(4347),
            since: NOW - 18 * DAY,
            waited: 0,
            label: "!4347 Fix Terraform state locking".into(),
            message: String::new(),
            detail: None,
            action: adoken_nudge::NudgeAction::ReviewNow,
            ping: Vec::new(),
            related: None,
        };
        let age = Age {
            since: NOW - 18 * DAY,
            waited: 0,
            threshold: 1,
        };
        let card = WaitCard {
            pr_id: 4347,
            title: "Fix Terraform state locking".into(),
            author: "kb".into(),
            author_name: "Kim Becker".into(),
            age,
            files: Some(3),
            threads: 0,
            required: true,
            quiet: None,
            nudge: nudge.clone(),
        };
        assert_eq!(
            waiting_label(&card, NOW),
            "Review !4347 Fix Terraform state locking from Kim Becker, waiting 18 days, required reviewer."
        );
        let focus = Focus {
            nudge,
            due: true,
            who: Some("kb".into()),
            who_name: Some("Kim Becker".into()),
            headline: "Kim is blocked on you".into(),
            label: "!4347 Fix Terraform state locking".into(),
            files: Some(3),
            age,
            activity: None,
        };
        assert_eq!(
            up_next_label(&focus, NOW),
            "Up next: Kim is blocked on you. !4347 Fix Terraform state locking, 3 files, \
             waiting 18 days. Return: Review now."
        );
    }

    #[test]
    fn ac29_every_mood_paints_its_own_shipped_illustration() {
        let paths: std::collections::HashSet<&str> =
            Mood::ALL.iter().map(|m| art_path(*m)).collect();
        assert_eq!(paths.len(), 8, "one illustration per mood");
        let mut total = 0;
        for mood in Mood::ALL {
            let bytes = adoken_ui::Assets
                .load(art_path(mood))
                .expect("asset source")
                .unwrap_or_else(|| panic!("{} is embedded", mood.id()));
            assert!(bytes.len() <= 350 * 1024, "{} ≤ 350 KB", mood.id());
            total += bytes.len();
            let art = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
                .unwrap_or_else(|e| panic!("{} decodes: {e}", mood.id()));
            assert_eq!(
                (art.width(), art.height()),
                (528, 352),
                "{} is 528 × 352",
                mood.id()
            );
            assert!(art.color().has_alpha(), "{} is RGBA", mood.id());
            let rgba = art.to_rgba8();
            assert!(
                rgba.pixels().any(|p| p[3] == 0),
                "{} has transparent pixels (a cutout)",
                mood.id()
            );
            assert!(
                rgba.pixels().any(|p| p[3] == 255),
                "{} has opaque pixels",
                mood.id()
            );
        }
        assert!(total <= 2_500 * 1024, "all eight ≤ 2.5 MB, got {total}");
        let manifest: toml::Table = toml::from_str(
            &std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/manifest.toml"),
            )
            .expect("assets/manifest.toml"),
        )
        .expect("manifest parses");
        let shipped: std::collections::HashSet<&str> = manifest["asset"]
            .as_array()
            .expect("[[asset]] entries")
            .iter()
            .filter(|a| a.get("ship").and_then(toml::Value::as_bool) == Some(true))
            .filter_map(|a| a.get("path").and_then(toml::Value::as_str))
            .collect();
        for path in &paths {
            assert!(
                shipped.contains(path),
                "{path} is listed in assets/manifest.toml with ship = true"
            );
        }
        assert_eq!(
            art_path(DayMood::FirstSync.art()),
            adoken_ui::images::TODAY_FRESH_START
        );
        assert_eq!(
            art_path(DayMood::Stale.art()),
            adoken_ui::images::TODAY_WAITING
        );
    }
}
