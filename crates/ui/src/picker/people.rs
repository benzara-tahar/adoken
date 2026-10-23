//! People picker (spec 0015 FR-13, FR-26; design §4.5, §7.3).
//!
//! Assign mode: a search field, then **Assign to me** (`i`), **Unassigned**,
//! **Recent** (≤ 5), **People** (the store, matched on name and sign-in name
//! with the `Cmd+P` matcher) and **From your directory**, a group the host
//! fills asynchronously (it owns the debounce and the request; see
//! [`wants_directory`]). Groups and people without a sign-in name are left
//! out. Mention mode: the same sources as a bare list under the composer's
//! caret, groups included, driven by the host while the composer keeps the
//! keyboard.

use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

use adoken_search::{DocKind, Index, SearchDoc};
use adoken_theme::{motion, radius, size, space, typography};
use gpui::{
    AnyElement, App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Role,
    ScrollStrategy, SharedString, Subscription, Transformation, UniformListScrollHandle, Window,
    div, prelude::*, px, radians, uniform_list,
};

use super::{
    Choose, Close, MENTION_ROWS, Placement, ReturnFocus, SelectNext, SelectPrevious, VISIBLE_ROWS,
    card, find_ci, footer_hint, group_label, highlighted, list_height, message_row, now_ms,
    overlay, row_frame, search_field,
};
use crate::icon::{Icon, IconName};
use crate::kbd::Kbd;
use crate::list_row::match_ranges;
use crate::text_input::{TextInput, TextInputEvent};
use crate::theme::{ActiveTheme, Theme, TypeStyleExt};

/// One identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Person {
    /// Identity id (avatars, `@<id>` mentions).
    pub id: SharedString,
    pub name: SharedString,
    /// Unique (sign-in) name; empty when unknown.
    pub sign_in: SharedString,
    pub group: bool,
}

impl Person {
    pub fn new(
        id: impl Into<SharedString>,
        name: impl Into<SharedString>,
        sign_in: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            sign_in: sign_in.into(),
            group: false,
        }
    }

    pub fn group(mut self, group: bool) -> Self {
        self.group = group;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeopleMode {
    /// The Assigned to picker; `me` backs "Assign to me".
    Assign { me: Person },
    /// The composer's @mention list.
    Mention,
}

/// The directory group (FR-13). The host sets `Searching` when its
/// debounced request goes out and the outcome when it lands; the picker
/// drops it back to `Off` whenever the query changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Directory {
    /// No group (no search yet, offline, or query too short).
    #[default]
    Off,
    Searching,
    Results(Vec<Person>),
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Recent,
    People,
    Directory,
}

impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Self::Recent => "Recent",
            Self::People => "People",
            Self::Directory => "From your directory",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeopleRow {
    AssignToMe,
    Unassigned,
    Label(Section),
    /// `ix` indexes the section's list; `highlights` are byte ranges in the
    /// name.
    Person {
        section: Section,
        ix: usize,
        highlights: Vec<Range<usize>>,
    },
    /// "No people match …".
    NoMatch,
    /// "Searching your directory…".
    Searching,
    /// "Couldn't search your directory · Retry" (`↩` retries).
    DirectoryFailed,
}

impl PeopleRow {
    fn selectable(&self) -> bool {
        matches!(
            self,
            Self::AssignToMe | Self::Unassigned | Self::Person { .. } | Self::DirectoryFailed
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeopleChoice {
    AssignToMe,
    Unassigned,
    Person(Person),
}

/// Identity of a selectable row, kept across rebuilds.
#[derive(Clone, Debug, PartialEq, Eq)]
enum RowKey {
    AssignToMe,
    Unassigned,
    Person(SharedString),
    Retry,
}

/// FR-13: a directory search is worth sending once 3+ characters are typed
/// and fewer than 5 local people match.
pub fn wants_directory(query: &str, local_matches: usize) -> bool {
    query.trim().chars().count() >= 3 && local_matches < 5
}

/// One source list and its matcher.
struct Source {
    people: Vec<Person>,
    index: Index,
}

impl Source {
    fn new(people: Vec<Person>) -> Self {
        let docs = people
            .iter()
            .enumerate()
            .map(|(ix, p)| SearchDoc {
                id: ix as u64,
                kind: DocKind::Person,
                title: p.name.to_string(),
                subtitle: p.sign_in.to_string(),
            })
            .collect();
        Self {
            index: Index::new(docs),
            people,
        }
    }

    /// `(index, name highlights)`, best first; everyone in order when the
    /// query is empty.
    fn matches(&self, query: &str) -> Vec<(usize, Vec<Range<usize>>)> {
        if query.is_empty() {
            return (0..self.people.len()).map(|ix| (ix, Vec::new())).collect();
        }
        self.index
            .query(query, MATCH_LIMIT)
            .into_iter()
            .map(|hit| {
                let ix = hit.doc as usize;
                (ix, match_ranges(&self.people[ix].name, &hit.positions))
            })
            .collect()
    }
}

impl Default for Source {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

/// Rows and selection of the people picker.
pub struct PeopleModel {
    mention: bool,
    recent: Source,
    people: Source,
    directory: Directory,
    /// Directory results kept after eligibility and de-duplication.
    directory_people: Vec<Person>,
    /// Current assignee id; `None` = unassigned.
    current: Option<SharedString>,
    query: String,
    rows: Vec<PeopleRow>,
    selected: Option<usize>,
    /// The user moved the selection since the query last changed.
    moved: bool,
    local_matches: usize,
}

/// Recent assignees shown (FR-13).
pub const RECENT_MAX: usize = 5;
/// Best store matches listed for a query: the matcher rescores one batch
/// instead of thousands, and nobody scrolls past this many people.
pub const MATCH_LIMIT: usize = 100;

impl PeopleModel {
    pub fn new(mention: bool) -> Self {
        let mut model = Self {
            mention,
            recent: Source::default(),
            people: Source::default(),
            directory: Directory::Off,
            directory_people: Vec::new(),
            current: None,
            query: String::new(),
            rows: Vec::new(),
            selected: None,
            moved: false,
            local_matches: 0,
        };
        model.rebuild();
        model
    }

    /// FR-13 / FR-26: who may appear.
    fn eligible(&self, person: &Person) -> bool {
        if self.mention {
            !person.id.is_empty()
        } else {
            !person.group && !person.sign_in.is_empty()
        }
    }

    fn keep(&self, people: Vec<Person>) -> Vec<Person> {
        people.into_iter().filter(|p| self.eligible(p)).collect()
    }

    /// Most recent first; at most [`RECENT_MAX`] are kept.
    pub fn set_recent(&mut self, recent: Vec<Person>) {
        let mut recent = self.keep(recent);
        recent.truncate(RECENT_MAX);
        self.recent = Source::new(recent);
        self.rebuild();
    }

    pub fn set_people(&mut self, people: Vec<Person>) {
        self.people = Source::new(self.keep(people));
        self.rebuild();
    }

    pub fn set_current(&mut self, current: Option<SharedString>) {
        self.current = current;
    }

    pub fn current(&self) -> Option<&SharedString> {
        self.current.as_ref()
    }

    pub fn set_directory(&mut self, directory: Directory) {
        self.directory = directory;
        self.rebuild();
    }

    pub fn directory(&self) -> &Directory {
        &self.directory
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// New query: the directory group resets and the selection lands on the
    /// first person match (or the first row when the query is empty).
    pub fn set_query(&mut self, query: &str) {
        let query = query.trim();
        if query == self.query {
            return;
        }
        self.query = query.to_owned();
        self.directory = Directory::Off;
        self.moved = false;
        self.rebuild();
    }

    /// People matching the query outside the directory group.
    pub fn local_match_count(&self) -> usize {
        self.local_matches
    }

    pub fn rows(&self) -> &[PeopleRow] {
        &self.rows
    }

    pub fn selected_row(&self) -> Option<usize> {
        self.selected
    }

    pub fn person(&self, section: Section, ix: usize) -> &Person {
        match section {
            Section::Recent => &self.recent.people[ix],
            Section::People => &self.people.people[ix],
            Section::Directory => &self.directory_people[ix],
        }
    }

    fn key(&self, row: usize) -> Option<RowKey> {
        Some(match self.rows.get(row)? {
            PeopleRow::AssignToMe => RowKey::AssignToMe,
            PeopleRow::Unassigned => RowKey::Unassigned,
            PeopleRow::Person { section, ix, .. } => {
                RowKey::Person(self.person(*section, *ix).id.clone())
            }
            PeopleRow::DirectoryFailed => RowKey::Retry,
            _ => return None,
        })
    }

    fn rebuild(&mut self) {
        let kept = if self.moved {
            self.selected.and_then(|row| self.key(row))
        } else {
            None
        };
        let mut rows = Vec::new();
        if !self.mention {
            rows.extend([PeopleRow::AssignToMe, PeopleRow::Unassigned]);
        }
        let recent = self.recent.matches(&self.query);
        let recent_ids: HashSet<&SharedString> = self.recent.people.iter().map(|p| &p.id).collect();
        let people: Vec<_> = self
            .people
            .matches(&self.query)
            .into_iter()
            .filter(|(ix, _)| !recent_ids.contains(&self.people.people[*ix].id))
            .collect();
        self.local_matches = recent.len() + people.len();
        for (section, hits) in [(Section::Recent, recent), (Section::People, people)] {
            if hits.is_empty() {
                continue;
            }
            if !self.mention {
                rows.push(PeopleRow::Label(section));
            }
            rows.extend(hits.into_iter().map(|(ix, highlights)| PeopleRow::Person {
                section,
                ix,
                highlights,
            }));
        }

        let local_ids: HashSet<&SharedString> = self
            .recent
            .people
            .iter()
            .chain(&self.people.people)
            .map(|p| &p.id)
            .collect();
        self.directory_people = match &self.directory {
            Directory::Results(found) => found
                .iter()
                .filter(|p| self.eligible(p) && !local_ids.contains(&p.id))
                .cloned()
                .collect(),
            _ => Vec::new(),
        };
        if !self.query.is_empty() && self.local_matches == 0 && self.directory_people.is_empty() {
            rows.push(PeopleRow::NoMatch);
        }
        match &self.directory {
            Directory::Off => {}
            Directory::Searching => {
                rows.extend([PeopleRow::Label(Section::Directory), PeopleRow::Searching]);
            }
            Directory::Failed => {
                rows.extend([
                    PeopleRow::Label(Section::Directory),
                    PeopleRow::DirectoryFailed,
                ]);
            }
            Directory::Results(_) => {
                if !self.directory_people.is_empty() {
                    rows.push(PeopleRow::Label(Section::Directory));
                }
                rows.extend(self.directory_people.iter().enumerate().map(|(ix, p)| {
                    PeopleRow::Person {
                        section: Section::Directory,
                        ix,
                        highlights: find_ci(&p.name, &self.query).into_iter().collect(),
                    }
                }));
            }
        }
        self.rows = rows;

        let kept =
            kept.and_then(|key| (0..self.rows.len()).find(|&r| self.key(r) == Some(key.clone())));
        self.selected = kept.or_else(|| self.initial_selection());
        if kept.is_none() {
            self.moved = false;
        }
    }

    fn initial_selection(&self) -> Option<usize> {
        let first_person = || {
            self.rows
                .iter()
                .position(|r| matches!(r, PeopleRow::Person { .. }))
        };
        let first = || self.rows.iter().position(PeopleRow::selectable);
        if self.query.is_empty() {
            first()
        } else {
            first_person().or_else(first)
        }
    }

    /// `↑`/`↓` over selectable rows, wrapping.
    pub fn step(&mut self, delta: isize) {
        let len = self.rows.len() as isize;
        let Some(mut row) = self.selected else { return };
        for _ in 0..len {
            row = (row as isize + delta).rem_euclid(len) as usize;
            if self.rows[row].selectable() {
                self.selected = Some(row);
                self.moved = true;
                return;
            }
        }
    }

    /// Pointer hover.
    pub fn highlight(&mut self, row: usize) {
        if self.rows.get(row).is_some_and(PeopleRow::selectable) {
            self.selected = Some(row);
            self.moved = true;
        }
    }

    /// What `↩` picks (`None` on the retry row or with nothing selected).
    pub fn chosen(&self) -> Option<PeopleChoice> {
        Some(match self.rows.get(self.selected?)? {
            PeopleRow::AssignToMe => PeopleChoice::AssignToMe,
            PeopleRow::Unassigned => PeopleChoice::Unassigned,
            PeopleRow::Person { section, ix, .. } => {
                PeopleChoice::Person(self.person(*section, *ix).clone())
            }
            _ => return None,
        })
    }

    pub fn retry_selected(&self) -> bool {
        self.selected
            .and_then(|r| self.rows.get(r))
            .is_some_and(|r| *r == PeopleRow::DirectoryFailed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeopleEvent {
    /// The query changed (Assign mode's field): debounce, then ask
    /// [`wants_directory`] with [`PeoplePicker::local_match_count`].
    QueryChanged(SharedString),
    Chosen(PeopleChoice),
    RetryDirectory,
}

/// Draws a person's avatar (the app passes its `Avatar`).
pub type AvatarFn = Rc<dyn Fn(&Person, &mut Window, &mut App) -> AnyElement>;

/// Initials on `accent.wash`, `users` for groups: the look of the app's
/// avatar before a picture loads (spec 0012).
fn initials_avatar(person: &Person, _: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    let initials: String = person
        .name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    div()
        .size(px(size::AVATAR_MD))
        .flex_none()
        .rounded(px(radius::FULL))
        .bg(theme.colors.accent_wash)
        .text_color(theme.colors.accent_fg)
        .type_style(typography::LABEL, theme)
        .flex()
        .items_center()
        .justify_center()
        .child(initials)
        .into_any_element()
}

pub struct PeoplePicker {
    mode: PeopleMode,
    title: SharedString,
    model: PeopleModel,
    /// Assign mode only.
    input: Option<Entity<TextInput>>,
    avatar: AvatarFn,
    placement: Placement,
    scroll: UniformListScrollHandle,
    searching_since: u64,
    return_focus: ReturnFocus,
    /// Mention mode never takes focus; this backs `Focusable` only.
    focus: FocusHandle,
    _input_events: Option<Subscription>,
}

impl EventEmitter<PeopleEvent> for PeoplePicker {}
impl EventEmitter<DismissEvent> for PeoplePicker {}

impl Focusable for PeoplePicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.input {
            Some(input) => input.focus_handle(cx),
            None => self.focus.clone(),
        }
    }
}

impl PeoplePicker {
    /// Assign mode takes the keyboard into its search field; Mention mode
    /// leaves it in the composer.
    pub fn new(
        mode: PeopleMode,
        placement: Placement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mention = mode == PeopleMode::Mention;
        let (input, events) = if mention {
            (None, None)
        } else {
            let input = cx.new(|cx| TextInput::new("Search people", cx));
            let events = cx.subscribe(&input, |this, input, event, cx| {
                if *event == TextInputEvent::Changed {
                    let query = input.read(cx).text().to_owned();
                    this.query_changed(&query, cx);
                }
            });
            (Some(input), Some(events))
        };
        let return_focus = ReturnFocus::capture(window, cx);
        if let Some(input) = &input {
            window.focus(&input.focus_handle(cx), cx);
        }
        Self {
            title: if mention { "People" } else { "Change assignee" }.into(),
            mode,
            model: PeopleModel::new(mention),
            input,
            avatar: Rc::new(initials_avatar),
            placement,
            scroll: UniformListScrollHandle::new(),
            searching_since: 0,
            return_focus,
            focus: cx.focus_handle(),
            _input_events: events,
        }
    }

    pub fn set_title(&mut self, title: impl Into<SharedString>) {
        self.title = title.into();
    }

    pub fn set_avatar(&mut self, avatar: AvatarFn) {
        self.avatar = avatar;
    }

    pub fn set_recent(&mut self, recent: Vec<Person>, cx: &mut Context<Self>) {
        self.model.set_recent(recent);
        self.reveal(cx);
    }

    pub fn set_people(&mut self, people: Vec<Person>, cx: &mut Context<Self>) {
        self.model.set_people(people);
        self.reveal(cx);
    }

    /// The current assignee's id (`None`: unassigned); checked in the list.
    pub fn set_current(&mut self, current: Option<SharedString>, cx: &mut Context<Self>) {
        self.model.set_current(current);
        cx.notify();
    }

    /// The directory group; never moves a selection the user made.
    pub fn set_directory(&mut self, directory: Directory, cx: &mut Context<Self>) {
        if directory == Directory::Searching && self.model.directory() != &Directory::Searching {
            self.searching_since = now_ms();
        }
        self.model.set_directory(directory);
        cx.notify();
    }

    pub fn model(&self) -> &PeopleModel {
        &self.model
    }

    pub fn query(&self) -> &str {
        self.model.query()
    }

    pub fn local_match_count(&self) -> usize {
        self.model.local_match_count()
    }

    /// Replaces the query (Mention mode: the text after `@`; Assign mode:
    /// the search field's text).
    /// Takes effect at once, so a directory state set right after belongs to
    /// this query.
    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        if let Some(input) = &self.input {
            input.update(cx, |input, cx| input.set_text(query, cx));
        }
        self.query_changed(query, cx);
    }

    fn query_changed(&mut self, query: &str, cx: &mut Context<Self>) {
        let before = self.model.query().to_owned();
        self.model.set_query(query);
        if self.model.query() != before {
            cx.emit(PeopleEvent::QueryChanged(
                self.model.query().to_owned().into(),
            ));
        }
        self.reveal(cx);
    }

    fn reveal(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.model.selected_row() {
            self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    pub fn select_next(&mut self, cx: &mut Context<Self>) {
        self.model.step(1);
        self.reveal(cx);
    }

    pub fn select_previous(&mut self, cx: &mut Context<Self>) {
        self.model.step(-1);
        self.reveal(cx);
    }

    /// `↩` (or `Tab` in the composer): emits `Chosen` and closes, or asks
    /// the host to retry the directory. Returns the choice.
    pub fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<PeopleChoice> {
        if self.model.retry_selected() {
            cx.emit(PeopleEvent::RetryDirectory);
            return None;
        }
        let choice = self.model.chosen()?;
        self.return_focus.restore(window, cx);
        cx.emit(PeopleEvent::Chosen(choice.clone()));
        cx.emit(DismissEvent);
        Some(choice)
    }

    /// Closes without a choice and hands the keyboard back.
    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.return_focus.restore(window, cx);
        cx.emit(DismissEvent);
    }

    fn row_label(&self, row: &PeopleRow) -> SharedString {
        match row {
            PeopleRow::AssignToMe => "Assign to me".into(),
            PeopleRow::Unassigned => "Unassigned".into(),
            PeopleRow::Label(section) => section.label().into(),
            PeopleRow::Person { section, ix, .. } => {
                let person = self.model.person(*section, *ix);
                let mut label = person.name.to_string();
                if !person.sign_in.is_empty() {
                    label.push_str(", ");
                    label.push_str(&person.sign_in);
                }
                if person.group {
                    label.push_str(", group");
                }
                if !self.is_mention() && self.model.current() == Some(&person.id) {
                    label.push_str(", current");
                }
                label.into()
            }
            PeopleRow::NoMatch => {
                format!("No people match \u{201c}{}\u{201d}", self.model.query()).into()
            }
            PeopleRow::Searching => "Searching your directory…".into(),
            PeopleRow::DirectoryFailed => "Couldn't search your directory · Retry".into(),
        }
    }

    fn is_mention(&self) -> bool {
        self.mode == PeopleMode::Mention
    }

    fn render_row(&self, row: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let entry = self.model.rows()[row].clone();
        let label = self.row_label(&entry);
        let selected = self.model.selected_row() == Some(row);
        let avatar = match (&entry, &self.mode) {
            (PeopleRow::AssignToMe, PeopleMode::Assign { me }) => {
                Some((self.avatar)(me, window, cx))
            }
            (PeopleRow::Person { section, ix, .. }, _) => {
                let person = self.model.person(*section, *ix).clone();
                // Design §7.3: groups show `users` 16 in place of an avatar.
                Some(if person.group {
                    Icon::new(IconName::Users)
                        .size(size::ICON_MD)
                        .color(colors.text_secondary)
                        .into_any_element()
                } else {
                    (self.avatar)(&person, window, cx)
                })
            }
            _ => None,
        };
        let mut avatar = avatar.into_iter();
        let check = |current: bool| {
            current.then(|| {
                Icon::new(IconName::Check)
                    .size(size::ICON_SM)
                    .color(colors.text_secondary)
            })
        };
        let leading = |el: AnyElement| {
            div()
                .w(px(size::AVATAR_MD))
                .flex_none()
                .flex()
                .justify_center()
                .child(el)
        };
        let option = |theme: &Theme| {
            row_frame(("people-row", row), selected, theme)
                .role(Role::ListBoxOption)
                .aria_label(label.clone())
                .aria_selected(selected)
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.model.selected_row() != Some(row) {
                        this.model.highlight(row);
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.model.highlight(row);
                    this.confirm(window, cx);
                }))
        };
        match entry {
            PeopleRow::AssignToMe => option(&theme)
                .aria_keyshortcuts("i")
                .children(avatar.next().map(leading))
                .child(div().flex_1().min_w_0().truncate().child(label))
                .child(Kbd::from_label("I"))
                .into_any_element(),
            PeopleRow::Unassigned => option(&theme)
                .child(leading(
                    Icon::new(IconName::UserX)
                        .size(size::ICON_MD)
                        .color(colors.text_secondary)
                        .into_any_element(),
                ))
                .child(div().flex_1().min_w_0().truncate().child(label))
                .children(check(self.model.current().is_none()))
                .into_any_element(),
            PeopleRow::Person {
                section,
                ix,
                highlights,
            } => {
                let person = self.model.person(section, ix).clone();
                let current = !self.is_mention() && self.model.current() == Some(&person.id);
                option(&theme)
                    .children(avatar.next().map(leading))
                    .child(
                        div()
                            .flex_none()
                            .max_w(gpui::relative(0.6))
                            .truncate()
                            .child(highlighted(person.name.clone(), highlights, &theme)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::SMALL, &theme)
                            .child(person.sign_in.clone()),
                    )
                    .children(check(current))
                    .into_any_element()
            }
            PeopleRow::Label(section) => group_label(section.label(), &theme)
                .id(("people-row", row))
                .role(Role::Label)
                .aria_label(label)
                .into_any_element(),
            PeopleRow::NoMatch => message_row(label.clone(), &theme)
                .id(("people-row", row))
                .role(Role::Label)
                .aria_label(label)
                .into_any_element(),
            PeopleRow::Searching => {
                // Still for 150 ms, then `glyph-spin` (reduce motion: still).
                let spinning = now_ms().saturating_sub(self.searching_since)
                    >= u64::from(motion::HOVER_INTENT);
                let phase = crate::motion::pulse_phase(
                    crate::motion::ms(motion::GLYPH_SPIN),
                    cx.entity_id(),
                    cx,
                );
                let phase = if spinning { phase } else { 0.0 };
                let spinner = Icon::new(IconName::LoaderCircle)
                    .size(size::ICON_SM)
                    .color(colors.text_tertiary)
                    .transform(Transformation::rotate(radians(
                        phase * std::f32::consts::TAU,
                    )));
                div()
                    .id(("people-row", row))
                    .h(px(size::ROW_COMPACT))
                    .px(px(space::S8))
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .text_color(colors.text_tertiary)
                    .role(Role::Status)
                    .aria_label(label.clone())
                    .child(spinner)
                    .child(label)
                    .into_any_element()
            }
            PeopleRow::DirectoryFailed => option(&theme)
                .child(
                    Icon::new(IconName::CircleAlert)
                        .size(size::ICON_SM)
                        .color(colors.text_tertiary),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(space::S4))
                        .text_color(colors.text_secondary)
                        .child(div().flex_none().child("Couldn't search your directory ·"))
                        .child(
                            div()
                                .flex_none()
                                .text_color(colors.text_link)
                                .child("Retry"),
                        ),
                )
                .into_any_element(),
        }
    }
}

impl Render for PeoplePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let mention = self.is_mention();
        let count = self.model.rows().len();
        let max = if mention { MENTION_ROWS } else { VISIBLE_ROWS };
        let list = uniform_list(
            "people-rows",
            count,
            cx.processor(|this, range: Range<usize>, window, cx| {
                range
                    .map(|row| this.render_row(row, window, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .h(list_height(count, max));
        let list = div()
            .id("people-list")
            .role(Role::ListBox)
            .aria_label(if mention {
                self.title.clone()
            } else {
                "People".into()
            })
            .child(list);
        let card = card("people-picker", self.placement.width, &theme)
            .role(if mention { Role::Group } else { Role::Dialog })
            .aria_label(self.title.clone())
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.dismiss(window, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.select_next(cx)))
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.select_previous(cx)))
            .on_action(cx.listener(|this, _: &Choose, window, cx| {
                this.confirm(window, cx);
            }))
            .on_action(cx.listener(|this, _: &Close, window, cx| this.dismiss(window, cx)))
            .when_some(self.input.clone(), |card, input| {
                card.child(
                    search_field(input.into_any_element(), &theme)
                        .id("people-search")
                        .role(Role::SearchInput)
                        .aria_label("Search people"),
                )
            })
            .child(list)
            .when(!mention, |card| card.child(footer_hint(&theme)))
            .when(mention, |card| card.p(px(space::S4)));
        overlay("people-picker-in", card, self.placement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: &str, name: &str, sign_in: &str) -> Person {
        Person::new(id, name, sign_in)
    }

    fn store() -> Vec<Person> {
        vec![
            p("1", "Dana Kim", "dana@contoso.com"),
            p("2", "Dan Okafor", "dan.o@contoso.com"),
            p("3", "Ada Lindqvist", "ada@contoso.com"),
            p("4", "Kwame Mensah", "kwame@contoso.com"),
            p("5", "No Signin", ""),
            p("6", "Payments Team", "[Storefront]\\Payments").group(true),
        ]
    }

    fn assign() -> PeopleModel {
        let mut model = PeopleModel::new(false);
        model.set_people(store());
        model.set_recent(vec![p("1", "Dana Kim", "dana@contoso.com")]);
        model
    }

    fn names(model: &PeopleModel) -> Vec<String> {
        model
            .rows()
            .iter()
            .map(|row| match row {
                PeopleRow::AssignToMe => "<me>".into(),
                PeopleRow::Unassigned => "<none>".into(),
                PeopleRow::Label(s) => format!("[{}]", s.label()),
                PeopleRow::Person { section, ix, .. } => {
                    model.person(*section, *ix).name.to_string()
                }
                PeopleRow::NoMatch => "<no match>".into(),
                PeopleRow::Searching => "<searching>".into(),
                PeopleRow::DirectoryFailed => "<failed>".into(),
            })
            .collect()
    }

    #[test]
    fn assign_lists_fixed_rows_recent_then_store_people_without_groups_or_unknown_sign_ins() {
        let model = assign();
        assert_eq!(
            names(&model),
            [
                "<me>",
                "<none>",
                "[Recent]",
                "Dana Kim",
                "[People]",
                "Dan Okafor",
                "Ada Lindqvist",
                "Kwame Mensah",
            ]
        );
        assert_eq!(model.selected_row(), Some(0), "Assign to me first");
    }

    #[test]
    fn mention_includes_groups_and_drops_fixed_rows_and_labels() {
        let mut model = PeopleModel::new(true);
        model.set_people(store());
        model.set_query("pay");
        assert_eq!(names(&model), ["Payments Team"]);
        assert!(matches!(model.chosen(), Some(PeopleChoice::Person(p)) if p.group));
    }

    #[test]
    fn typing_matches_name_or_sign_in_and_selects_the_first_person() {
        let mut model = assign();
        model.set_query("dan");
        let rows = names(&model);
        assert_eq!(rows[..2], ["<me>", "<none>"], "fixed rows stay");
        assert!(rows.contains(&"Dana Kim".to_owned()));
        assert!(rows.contains(&"Dan Okafor".to_owned()));
        assert!(!rows.contains(&"Kwame Mensah".to_owned()));
        assert!(
            matches!(model.chosen(), Some(PeopleChoice::Person(_))),
            "↩ after typing picks a person, not Assign to me"
        );
        // Sign-in name only.
        model.set_query("kwame@");
        assert!(names(&model).contains(&"Kwame Mensah".to_owned()));
    }

    #[test]
    fn recent_people_are_not_repeated_under_people() {
        let model = assign();
        let rows = names(&model);
        assert_eq!(rows.iter().filter(|n| *n == "Dana Kim").count(), 1);
    }

    #[test]
    fn recent_keeps_at_most_five() {
        let mut model = PeopleModel::new(false);
        model.set_recent(
            (0..8)
                .map(|i| p(&i.to_string(), &format!("P{i}"), "x@y"))
                .collect(),
        );
        let people = model
            .rows()
            .iter()
            .filter(|r| matches!(r, PeopleRow::Person { .. }))
            .count();
        assert_eq!(people, RECENT_MAX);
    }

    #[test]
    fn nothing_local_shows_no_match_with_the_directory_group_under_it() {
        let mut model = assign();
        model.set_query("zzz");
        assert_eq!(model.local_match_count(), 0);
        assert_eq!(names(&model), ["<me>", "<none>", "<no match>"]);
        model.set_directory(Directory::Searching);
        assert_eq!(
            names(&model),
            [
                "<me>",
                "<none>",
                "<no match>",
                "[From your directory]",
                "<searching>"
            ]
        );
        // Results replace the searching row; the first result gets the
        // selection because the user hadn't moved it.
        model.set_directory(Directory::Results(vec![
            p("9", "Dana Ruiz", "dana.ruiz@contoso.com"),
            p("1", "Dana Kim", "dana@contoso.com"),
            p("10", "Ops", "ops").group(true),
        ]));
        assert_eq!(
            names(&model),
            ["<me>", "<none>", "[From your directory]", "Dana Ruiz"],
            "local duplicates and groups are dropped"
        );
        assert!(matches!(model.chosen(), Some(PeopleChoice::Person(p)) if p.id == "9"));
    }

    #[test]
    fn directory_results_never_move_a_selection_the_user_made() {
        let mut model = assign();
        model.set_query("dan");
        model.set_directory(Directory::Searching);
        model.step(-1); // onto Unassigned? walk until a fixed row
        while !matches!(model.chosen(), Some(PeopleChoice::Unassigned)) {
            model.step(-1);
        }
        model.set_directory(Directory::Results(vec![p("9", "Dana Ruiz", "dr@c")]));
        assert_eq!(model.chosen(), Some(PeopleChoice::Unassigned));
    }

    #[test]
    fn a_new_query_drops_the_directory_group() {
        let mut model = assign();
        model.set_query("zzz");
        model.set_directory(Directory::Failed);
        assert!(names(&model).contains(&"<failed>".to_owned()));
        model.set_query("zzzz");
        assert_eq!(model.directory(), &Directory::Off);
    }

    #[test]
    fn retry_row_is_selectable_but_chooses_nobody() {
        let mut model = PeopleModel::new(true);
        model.set_query("zzz");
        model.set_directory(Directory::Failed);
        assert!(model.retry_selected());
        assert_eq!(model.chosen(), None);
    }

    #[test]
    fn selection_wraps_and_skips_labels_and_messages() {
        let mut model = assign();
        model.step(-1);
        assert!(
            matches!(model.chosen(), Some(PeopleChoice::Person(p)) if p.name == "Kwame Mensah")
        );
        model.step(1);
        assert_eq!(model.chosen(), Some(PeopleChoice::AssignToMe));
        model.step(1);
        model.step(1);
        assert!(matches!(model.chosen(), Some(PeopleChoice::Person(p)) if p.name == "Dana Kim"));
    }

    #[test]
    fn directory_search_needs_three_chars_and_few_local_matches() {
        assert!(!wants_directory("da", 0));
        assert!(wants_directory("dan", 4));
        assert!(!wants_directory("dan", 5));
        assert!(!wants_directory("  d ", 0));
    }
}
