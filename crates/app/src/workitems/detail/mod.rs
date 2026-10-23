//! The work item detail (spec 0015 slices A + B; design
//! docs/design/work-item-detail.md §2–§14): header with the editable
//! title, state chip-menu and freshness / Sync this item; the property
//! grid (or the properties rail on wide panes); the rich description and
//! its editor; grouped links with Add link and Remove; the discussion with
//! its composer and @mentions; history.
//!
//! Everything renders from the loaded [`DetailData`] plus the write
//! controller's state ([`WiEdits`]): edits show their pending value on the
//! next frame and Azure DevOps' answer replaces it (spec FR-6, FR-7).
//! Single-letter keys work from the list selection and the detail, never
//! while a text field, picker, menu, overlay or editor has the keyboard
//! ([`KEYS_CONTEXT`], spec §3.3, AC-25).

pub mod model;

mod description;
mod discussion;
mod fields;
mod header;
pub use header::{Badge, badge, freshness_button};
mod history;
mod links;
mod status;

/// Guided tour anchors (spec 0020 FR-10).
pub mod anchors {
    pub const STATE: &str = "workitem.state";
    pub const FIELDS: &str = "workitem.fields";
    pub const DESCRIPTION: &str = "workitem.description";
    pub const DISCUSSION: &str = "workitem.discussion";
    pub const FRESHNESS: &str = "workitem.freshness";
}

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adoken_model::{LinkKind, Person as StoredPerson, WorkItem};
use adoken_store::wi::DraftKind;
use adoken_sync::wi::WiChange;
use adoken_ui::action_menu::BoundsSlot;
use adoken_ui::picker::{
    Directory, LinkType, LinkTypeEvent, LinkTypeMenu, PeopleChoice, PeopleEvent, PeopleMode,
    PeoplePicker, Person, PickerSlot, Placement, StateCategory as MenuCategory, StateMenu,
    StateMenuEvent, StateOption, TreeEvent, TreeKind, TreePicker,
};
use adoken_ui::text_area::Atom;
use adoken_ui::{
    IconName, TextArea, TextAreaEvent, TextInput, TextInputEvent, Toast, ToastStack, ToastTone,
};
use gpui::{
    App, Bounds, ClipboardItem, Context, Entity, FocusHandle, Focusable, KeyBinding, Pixels,
    ScrollHandle, SharedString, Subscription, Task, UniformListScrollHandle, Window, actions,
    prelude::*, px,
};

use self::model::{Field, LinkGroup, Stop, StopInput};
use super::data::{DetailData, Editable, LinkRow, LinkTarget};
use super::pick::{PickRow, load_catalog, picker_rows};
use super::writes::{self, Field as EditField, Gate, WiEdits};

/// What the next paint should scroll into view (design §10.3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Reveal {
    #[default]
    Nothing,
    /// The keyboard cursor's stop.
    Cursor,
    Composer,
}

/// The scroll offset that brings `target` inside `view` with `margin` to
/// spare, moving as little as possible (offsets are ≤ 0).
pub fn reveal_offset(
    offset: Pixels,
    max: Pixels,
    view: Bounds<Pixels>,
    target: Bounds<Pixels>,
    margin: Pixels,
) -> Pixels {
    let mut next = offset;
    if target.bottom() + margin > view.bottom() {
        next -= target.bottom() + margin - view.bottom();
    }
    if target.top() - margin < view.top() {
        next += view.top() - (target.top() - margin);
    }
    next.clamp(-max, px(0.0))
}

/// Run the pending reveal: after layout, the cursor (or composer) bounds
/// and the scroll viewport are known.
pub(super) fn reveal_pass(
    reveal: std::rc::Rc<Cell<Reveal>>,
    composer: BoundsSlot,
    scroll: ScrollHandle,
) -> impl IntoElement {
    gpui::canvas(
        move |_, window, _| {
            let target = match reveal.get() {
                Reveal::Nothing => return,
                Reveal::Cursor => status::CURSOR.with(|slot| slot.get()),
                Reveal::Composer => composer.get(),
            };
            reveal.set(Reveal::Nothing);
            let Some(target) = target else {
                return;
            };
            let offset = scroll.offset();
            let y = reveal_offset(
                offset.y,
                scroll.max_offset().y,
                scroll.bounds(),
                target,
                px(adoken_theme::space::S24),
            );
            if y != offset.y {
                scroll.set_offset(gpui::point(offset.x, y));
                window.refresh();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
}
use super::{Detail, WorkItemsEvent, WorkItemsView};
use crate::palette::PaletteCommand;
use crate::palette::catalog::Catalog;

actions!(
    work_items,
    [
        /// Edit the work item's title.
        EditWorkItemTitle,
        /// Change the work item's state…
        ChangeWorkItemState,
        /// Assign the work item…
        AssignWorkItem,
        /// Assign the work item to me.
        AssignWorkItemToMe,
        /// Unassign the work item.
        UnassignWorkItem,
        /// Change the work item's area…
        ChangeWorkItemArea,
        /// Change the work item's iteration…
        ChangeWorkItemIteration,
        /// Edit the work item's description.
        EditWorkItemDescription,
        /// Add a link to the work item…
        AddWorkItemLink,
        /// Set or change the work item's parent…
        AddWorkItemParent,
        /// Add a child to the work item…
        AddWorkItemChild,
        /// Link a related work item…
        AddWorkItemRelated,
        /// Remove a link from the work item…
        RemoveWorkItemLink,
        /// Comment on the work item.
        CommentOnWorkItem,
        /// Sync this work item from Azure DevOps.
        SyncWorkItem,
        /// Copy the work item's web link.
        CopyWorkItemLink,
        /// Copy the work item's id.
        CopyWorkItemId,
        /// Next stop in the work item detail.
        NextWorkItemStop,
        /// Previous stop in the work item detail.
        PreviousWorkItemStop,
        /// Edit or open the focused detail stop.
        ActivateWorkItemStop,
        /// Save the title being edited.
        SaveWorkItemTitle,
        /// Discard the title being edited.
        CancelWorkItemTitle,
        /// Save the description being edited.
        SaveWorkItemDescription,
        /// Leave the description editor (the draft is kept).
        CloseWorkItemDescription,
        /// Switch the description editor between Write and Preview.
        ToggleWorkItemPreview,
        /// Post the comment being written.
        PostWorkItemComment,
        /// Leave the comment composer (the draft is kept).
        LeaveWorkItemComposer,
        /// Mention list: next person.
        MentionNext,
        /// Mention list: previous person.
        MentionPrevious,
        /// Mention list: insert the selected person.
        MentionInsert,
        /// Mention list: close.
        MentionClose,
        /// Work item overlay: next row.
        WorkItemOverlayNext,
        /// Work item overlay: previous row.
        WorkItemOverlayPrevious,
        /// Work item overlay: choose or confirm.
        WorkItemOverlayConfirm,
        /// Work item overlay: close.
        WorkItemOverlayClose,
        /// Send the refused change with the field Azure DevOps asked for.
        ApplyWorkItemRule,
        /// Drop the change Azure DevOps refused for a rule.
        CancelWorkItemRule,
    ]
);

pub const TITLE_KEYS: &str = "t";
pub const STATE_KEYS: &str = "s";
pub const STATE_ALIAS_KEYS: &str = "shift-s";
pub const ASSIGN_KEYS: &str = "a";
pub const ASSIGN_ME_KEYS: &str = "i";
pub const AREA_KEYS: &str = "shift-a";
pub const ITERATION_KEYS: &str = "shift-i";
pub const DESCRIPTION_KEYS: &str = "d";
pub const LINK_KEYS: &str = "l";
pub const REMOVE_LINK_KEYS: &str = "backspace";
pub const COMMENT_KEYS: &str = "c";
pub const COMMENT_ALIAS_KEYS: &str = "shift-c";
pub const SYNC_KEYS: &str = "secondary-shift-r";
pub const OPEN_KEYS: &str = "o";
pub const COPY_LINK_KEYS: &str = "y";
pub const COPY_ID_KEYS: &str = "shift-y";
pub const SAVE_KEYS: &str = "secondary-enter";
pub const PREVIEW_KEYS: &str = "secondary-e";

/// The whole Work items view (⌘⇧R works even while typing).
pub const VIEW_CONTEXT: &str = "WorkItems";
pub const LIST_CONTEXT: &str = "WorkItemList";
pub const DETAIL_CONTEXT: &str = "WorkItemDetail";
/// Editors and overlays declare these so letters type instead of acting.
const OVERLAY: &str = "WorkItemOverlay";
const TITLE_CONTEXT: &str = "WorkItemTitleEditor";
const DESCRIPTION_CONTEXT: &str = "WorkItemDescriptionEditor";
const COMPOSER_CONTEXT: &str = "WorkItemComposer";
const MENTION_CONTEXT: &str = "WorkItemMention";
const RULE_CONTEXT: &str = "WorkItemRuleNotice";
/// The wrappers' full key contexts: each editor also declares
/// `WorkItemEditor` so [`KEYS_CONTEXT`] letters type instead of acting.
pub(super) const TITLE_EDITOR_CONTEXT: &str = "WorkItemEditor WorkItemTitleEditor";
pub(super) const DESCRIPTION_EDITOR_CONTEXT: &str = "WorkItemEditor WorkItemDescriptionEditor";
pub(super) const COMPOSER_CONTEXT_EDITING: &str = "WorkItemEditor WorkItemComposer";
pub(super) const COMPOSER_MENTION_CONTEXT: &str = "WorkItemEditor WorkItemComposer WorkItemMention";
pub(super) const OVERLAY_CONTEXT: &str = OVERLAY;
pub(super) const RULE_NOTICE_CONTEXT: &str = "WorkItemEditor WorkItemRuleNotice";
/// Single-letter keys: the list or the detail, never while a text field,
/// picker, menu, overlay or editor has the keyboard (spec §3.3, AC-25).
pub const KEYS_CONTEXT: &str = "(WorkItemList || WorkItemDetail) && !TextInput && !Picker && !ActionMenu && !WorkItemOverlay && !WorkItemEditor";
/// Detail-cursor keys: as [`KEYS_CONTEXT`], in the detail only.
const CURSOR_CONTEXT: &str =
    "WorkItemDetail && !TextInput && !Picker && !ActionMenu && !WorkItemOverlay && !WorkItemEditor";

/// FR-31: the selection rests this long before a partly synced item
/// fetches itself.
const REST: Duration = Duration::from_millis(400);
/// FR-13: the directory search waits for typing to pause.
const DIRECTORY_DEBOUNCE: Duration = Duration::from_millis(300);
/// Pending lines re-check "Waiting for Azure DevOps…" (3 s) and the
/// freshness label its minute.
const TICK: Duration = Duration::from_secs(1);
/// Rows the Add link picker shows before it scrolls.
pub(super) const PICKER_VISIBLE_ROWS: f32 = 8.5;
/// How long the accessible "saved" announcement stays (FR-36).
const ANNOUNCE_FOR: adoken_model::Millis = 4_000;

/// "Assigned to: Dana Ruiz, saved" for the first write in `before` (field,
/// value while sending) that no longer has any status: Azure DevOps
/// confirmed it. Failures, conflicts and rule notices keep a status, so
/// they never read as saved.
pub fn saved_announcement(before: &[(EditField, String)], now: &[EditField]) -> Option<String> {
    let (field, value) = before.iter().find(|(field, _)| !now.contains(field))?;
    let label = match field {
        EditField::Title => "Title",
        EditField::State => "State",
        EditField::AssignedTo => "Assigned to",
        EditField::Area => "Area",
        EditField::Iteration => "Iteration",
        EditField::Description => return Some("Description saved".into()),
        EditField::Link { .. } => return Some("Link saved".into()),
    };
    Some(if value.is_empty() {
        format!("{label} saved")
    } else {
        format!("{label}: {value}, saved")
    })
}

#[cfg(test)]
mod announce_tests {
    use super::*;

    /// FR-36: pending → confirmed announces "saved" once; a failure (it
    /// keeps a status) and an idle pane announce nothing.
    #[test]
    fn fr36_saved_is_announced_once_when_a_sending_write_confirms() {
        let sending = vec![(EditField::AssignedTo, "Dana Ruiz".to_owned())];
        assert_eq!(
            saved_announcement(&sending, &[EditField::AssignedTo]),
            None,
            "still sending, or failed with its status kept"
        );
        assert_eq!(
            saved_announcement(&sending, &[]).as_deref(),
            Some("Assigned to: Dana Ruiz, saved")
        );
        // The next change sees nothing sending: no second announcement.
        assert_eq!(saved_announcement(&[], &[]), None);
        assert_eq!(
            saved_announcement(&[(EditField::Description, "# x".into())], &[]).as_deref(),
            Some("Description saved")
        );
    }
}

pub fn bindings() -> Vec<KeyBinding> {
    let keys = Some(KEYS_CONTEXT);
    let cursor = Some(CURSOR_CONTEXT);
    let view = Some(VIEW_CONTEXT);
    let title = Some(TITLE_CONTEXT);
    let description = Some(DESCRIPTION_CONTEXT);
    let composer = Some(COMPOSER_CONTEXT);
    let mention = Some(MENTION_CONTEXT);
    let overlay = Some(OVERLAY);
    let rule = Some(RULE_CONTEXT);
    vec![
        KeyBinding::new(TITLE_KEYS, EditWorkItemTitle, keys),
        KeyBinding::new(STATE_KEYS, ChangeWorkItemState, keys),
        KeyBinding::new(STATE_ALIAS_KEYS, ChangeWorkItemState, keys),
        KeyBinding::new(ASSIGN_KEYS, AssignWorkItem, keys),
        KeyBinding::new(ASSIGN_ME_KEYS, AssignWorkItemToMe, keys),
        KeyBinding::new(AREA_KEYS, ChangeWorkItemArea, keys),
        KeyBinding::new(ITERATION_KEYS, ChangeWorkItemIteration, keys),
        KeyBinding::new(DESCRIPTION_KEYS, EditWorkItemDescription, keys),
        KeyBinding::new(LINK_KEYS, AddWorkItemLink, keys),
        KeyBinding::new(COMMENT_KEYS, CommentOnWorkItem, keys),
        KeyBinding::new(COMMENT_ALIAS_KEYS, CommentOnWorkItem, keys),
        KeyBinding::new(OPEN_KEYS, super::OpenWorkItemInAdo, keys),
        KeyBinding::new(COPY_LINK_KEYS, CopyWorkItemLink, keys),
        KeyBinding::new(COPY_ID_KEYS, CopyWorkItemId, keys),
        KeyBinding::new(SYNC_KEYS, SyncWorkItem, view),
        KeyBinding::new(REMOVE_LINK_KEYS, RemoveWorkItemLink, cursor),
        KeyBinding::new("down", NextWorkItemStop, cursor),
        KeyBinding::new("j", NextWorkItemStop, cursor),
        KeyBinding::new("up", PreviousWorkItemStop, cursor),
        KeyBinding::new("k", PreviousWorkItemStop, cursor),
        KeyBinding::new("enter", ActivateWorkItemStop, cursor),
        KeyBinding::new("escape", super::FocusWorkItemList, cursor),
        KeyBinding::new("enter", SaveWorkItemTitle, title),
        KeyBinding::new("escape", CancelWorkItemTitle, title),
        KeyBinding::new(SAVE_KEYS, SaveWorkItemDescription, description),
        KeyBinding::new("escape", CloseWorkItemDescription, description),
        KeyBinding::new(PREVIEW_KEYS, ToggleWorkItemPreview, description),
        KeyBinding::new(SAVE_KEYS, PostWorkItemComment, composer),
        KeyBinding::new("escape", LeaveWorkItemComposer, composer),
        KeyBinding::new("down", MentionNext, mention),
        KeyBinding::new("up", MentionPrevious, mention),
        KeyBinding::new("enter", MentionInsert, mention),
        KeyBinding::new("tab", MentionInsert, mention),
        KeyBinding::new("escape", MentionClose, mention),
        KeyBinding::new("down", WorkItemOverlayNext, overlay),
        KeyBinding::new("up", WorkItemOverlayPrevious, overlay),
        KeyBinding::new("enter", WorkItemOverlayConfirm, overlay),
        KeyBinding::new(SAVE_KEYS, WorkItemOverlayConfirm, overlay),
        KeyBinding::new("escape", WorkItemOverlayClose, overlay),
        KeyBinding::new("enter", ApplyWorkItemRule, rule),
        KeyBinding::new("escape", CancelWorkItemRule, rule),
    ]
}

/// Spec FR-28: with a token that can't write, the edit commands are listed
/// in `⌘P` disabled with the read-only hint (design §9.4) as the reason.
pub fn register_gates(cx: &mut App) {
    fn read_only(cx: &App) -> Option<SharedString> {
        (!writes::access(cx).can_write()).then(|| writes::READ_ONLY_HINT.into())
    }
    let edits: [Box<dyn gpui::Action>; 14] = [
        Box::new(EditWorkItemTitle),
        Box::new(ChangeWorkItemState),
        Box::new(AssignWorkItem),
        Box::new(AssignWorkItemToMe),
        Box::new(UnassignWorkItem),
        Box::new(ChangeWorkItemArea),
        Box::new(ChangeWorkItemIteration),
        Box::new(EditWorkItemDescription),
        Box::new(AddWorkItemLink),
        Box::new(AddWorkItemParent),
        Box::new(AddWorkItemChild),
        Box::new(AddWorkItemRelated),
        Box::new(RemoveWorkItemLink),
        Box::new(CommentOnWorkItem),
    ];
    for action in &edits {
        crate::palette::gates::register(action.as_ref(), read_only, cx);
    }
}

/// Spec FR-35: every work item action as a `⌘P` command.
pub fn palette_commands() -> Vec<PaletteCommand> {
    let command = |title, keys, action: Box<dyn gpui::Action>| PaletteCommand {
        title,
        keys,
        action,
    };
    vec![
        command(
            "Work item: Edit title",
            Some(TITLE_KEYS),
            Box::new(EditWorkItemTitle),
        ),
        command(
            "Work item: Change state…",
            Some(STATE_KEYS),
            Box::new(ChangeWorkItemState),
        ),
        command(
            "Work item: Assign…",
            Some(ASSIGN_KEYS),
            Box::new(AssignWorkItem),
        ),
        command(
            "Work item: Assign to me",
            Some(ASSIGN_ME_KEYS),
            Box::new(AssignWorkItemToMe),
        ),
        command("Work item: Unassign", None, Box::new(UnassignWorkItem)),
        command(
            "Work item: Change area…",
            Some(AREA_KEYS),
            Box::new(ChangeWorkItemArea),
        ),
        command(
            "Work item: Change iteration…",
            Some(ITERATION_KEYS),
            Box::new(ChangeWorkItemIteration),
        ),
        command(
            "Work item: Edit description",
            Some(DESCRIPTION_KEYS),
            Box::new(EditWorkItemDescription),
        ),
        command(
            "Work item: Add link…",
            Some(LINK_KEYS),
            Box::new(AddWorkItemLink),
        ),
        command("Work item: Add parent…", None, Box::new(AddWorkItemParent)),
        command("Work item: Add child…", None, Box::new(AddWorkItemChild)),
        command(
            "Work item: Add related link…",
            None,
            Box::new(AddWorkItemRelated),
        ),
        command(
            "Work item: Remove link…",
            None,
            Box::new(RemoveWorkItemLink),
        ),
        command(
            "Work item: Comment",
            Some(COMMENT_KEYS),
            Box::new(CommentOnWorkItem),
        ),
        command(
            "Work item: Sync this work item",
            Some(SYNC_KEYS),
            Box::new(SyncWorkItem),
        ),
        command(
            "Work item: Open in Azure DevOps",
            Some(OPEN_KEYS),
            Box::new(super::OpenWorkItemInAdo),
        ),
        command(
            "Work item: Copy link",
            Some(COPY_LINK_KEYS),
            Box::new(CopyWorkItemLink),
        ),
        command(
            "Work item: Copy id",
            Some(COPY_ID_KEYS),
            Box::new(CopyWorkItemId),
        ),
    ]
}

/// What a key, command or `⌘K` row asked the detail to do. Held until the
/// item's detail has loaded when it arrives first (palette `⌘K`, list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Title,
    State,
    Assign,
    AssignToMe,
    Unassign,
    Area,
    Iteration,
    Description,
    AddLink(Option<LinkKind>),
    RemoveLink,
    Comment,
    Sync,
}

impl Intent {
    /// Whether the intent writes (and so needs edit access).
    fn writes(self) -> bool {
        !matches!(self, Intent::Sync)
    }
}

/// `⌘K` intents for a work item opened from the palette (spec FR-35): the
/// palette queues one, then opens the item; the view runs it once the
/// detail has loaded.
#[derive(Default)]
pub struct QueuedIntent(pub Option<(i64, Intent)>);

impl gpui::Global for QueuedIntent {}

pub fn queue_intent(id: i64, intent: Intent, cx: &mut App) {
    cx.set_global(QueuedIntent(Some((id, intent))));
}

fn take_queued(id: i64, cx: &mut App) -> Option<Intent> {
    let queued = cx.try_global::<QueuedIntent>()?.0?;
    if queued.0 != id {
        return None;
    }
    cx.set_global(QueuedIntent(None));
    Some(queued.1)
}

pub(super) struct TitleEditor {
    pub input: Entity<TextInput>,
    pub error: Option<&'static str>,
    _events: Subscription,
}

pub(super) struct DescriptionEditor {
    pub area: Entity<TextArea>,
    /// Markdown the editor opened on (Save is off while unchanged).
    pub base: String,
    pub markdown_field: bool,
    pub preview: Option<Arc<adoken_richtext::Doc>>,
    pub previewing: bool,
    pub draft_saved: bool,
    /// Keeps `⌘E` / `Esc` / `⌘↩` reachable while Preview hides the field.
    pub focus: FocusHandle,
    _events: Subscription,
    _preview: Option<Task<()>>,
}

pub(super) struct Composer {
    pub item: i64,
    pub area: Entity<TextArea>,
    /// Byte offset of the `@` that opened the mention list.
    pub mention_at: Option<usize>,
    _events: Subscription,
}

/// Add link step 2: the work item picker (spec 0004 FR-17 engine).
pub(super) struct LinkPicker {
    pub kind: LinkKind,
    pub input: Entity<TextInput>,
    pub catalog: Option<Arc<Catalog>>,
    pub rows: Vec<PickRow>,
    pub selected: usize,
    pub scroll: UniformListScrollHandle,
    generation: u64,
    _search: Option<Task<()>>,
    _catalog: Option<Task<()>>,
    _input: Subscription,
}

/// Remove link: which row and, for the palette, the pick list first.
pub(super) enum Removal {
    /// "Remove link to #4821?".
    Confirm { row: usize },
    /// `⌘P` › Remove link…: choose among the work item links first.
    Choose { selected: usize },
}

pub(super) struct RemoveOverlay {
    pub what: Removal,
    pub focus: FocusHandle,
}

/// Anchors for pickers and menus, recorded as the detail paints.
#[derive(Default, Clone)]
pub(super) struct Anchors {
    pub title: BoundsSlot,
    pub state: BoundsSlot,
    pub cells: [BoundsSlot; 4],
    pub add_link: BoundsSlot,
    pub composer: BoundsSlot,
    pub pane: Rc<Cell<f32>>,
}

impl Anchors {
    pub fn cell(&self, field: Field) -> &BoundsSlot {
        &self.cells[Field::GRID.iter().position(|f| *f == field).unwrap_or(0)]
    }
}

/// The detail's interaction state for the item on screen.
#[derive(Default)]
pub(super) struct DetailUi {
    /// The keyboard cursor (design §10.3).
    pub cursor: Option<Stop>,
    /// The editor was opened from the list: focus goes back there.
    pub from_list: bool,
    pub title: Option<TitleEditor>,
    pub description: Option<DescriptionEditor>,
    pub compare: bool,
    pub composer: Option<Composer>,
    pub mention: PickerSlot<PeoplePicker>,
    pub state_menu: PickerSlot<StateMenu>,
    pub people: PickerSlot<PeoplePicker>,
    pub tree: PickerSlot<TreePicker>,
    pub tree_field: Option<Field>,
    /// The open tree picker shows the project's real tree (not the
    /// loaded-paths fallback, FR-14).
    pub tree_ready: bool,
    pub link_menu: PickerSlot<LinkTypeMenu>,
    pub link_picker: Option<LinkPicker>,
    pub remove: Option<RemoveOverlay>,
    pub rule_input: Option<Entity<TextInput>>,
    pub show_all_description: bool,
    pub show_all_comments: bool,
    pub show_all_history: bool,
    pub expanded_groups: HashSet<LinkGroup>,
    pub anchors: Anchors,
    /// Set by cursor moves; consumed by the pane's [`reveal_pass`].
    pub reveal: std::rc::Rc<Cell<Reveal>>,
    pub intent: Option<Intent>,
    /// FR-36: what was sending at the last controller change (field,
    /// value) and which comments, to announce "saved" once confirmed.
    pub sending: Vec<(EditField, String)>,
    pub sending_comments: Vec<u64>,
    /// The polite, accessible-only announcement and when it was made.
    pub announcement: Option<(String, adoken_model::Millis)>,
    _rest: Option<Task<()>>,
    _tick: Option<Task<()>>,
    _directory: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// `Dana Ruiz` for the pickers, from a stored person.
pub(super) fn picker_person(person: &StoredPerson) -> Person {
    Person::new(
        person.id.clone(),
        person.display_name.clone(),
        person.unique_name.clone().unwrap_or_default(),
    )
    .group(person.is_group)
}

fn menu_category(category: adoken_model::StateCategory) -> MenuCategory {
    use adoken_model::StateCategory as C;
    match category {
        C::Proposed => MenuCategory::Proposed,
        C::InProgress => MenuCategory::InProgress,
        C::Resolved => MenuCategory::Resolved,
        C::Completed => MenuCategory::Completed,
        C::Removed => MenuCategory::Removed,
    }
}

fn link_kind(ty: LinkType) -> LinkKind {
    match ty {
        LinkType::Parent => LinkKind::Parent,
        LinkType::Child => LinkKind::Child,
        LinkType::Related => LinkKind::Related,
        LinkType::Predecessor => LinkKind::Predecessor,
        LinkType::Successor => LinkKind::Successor,
        LinkType::Duplicate => LinkKind::Duplicate,
        LinkType::DuplicateOf => LinkKind::DuplicateOf,
    }
}

/// `@<id>` tokens of a stored draft → display text and chips (FR-26).
pub(super) fn atoms_of(text: &str, name: impl Fn(&str) -> String) -> (String, Vec<Atom>) {
    let mut out = String::with_capacity(text.len());
    let mut atoms = Vec::new();
    let mut at = 0;
    for (range, id) in adoken_richtext::mention_ranges(text) {
        out.push_str(&text[at..range.start]);
        let display = format!("@{}", name(id));
        atoms.push(Atom {
            range: out.len()..out.len() + display.len(),
            id: SharedString::from(id.to_owned()),
        });
        out.push_str(&display);
        at = range.end;
    }
    out.push_str(&text[at..]);
    (out, atoms)
}

/// The `@query` the caret is finishing, if any: its `@` offset and the
/// text after it (FR-26: `@` at the start or after whitespace, no
/// whitespace in the query).
pub fn mention_query(line: &str, line_start: usize) -> Option<(usize, &str)> {
    let at = line.rfind('@')?;
    let before = line[..at].chars().next_back();
    let query = &line[at + 1..];
    let opens = before.is_none_or(char::is_whitespace);
    (opens && !query.contains(char::is_whitespace) && !query.starts_with('<'))
        .then_some((line_start + at, query))
}

impl WorkItemsView {
    // ── access ───────────────────────────────────────────────────────────

    pub(super) fn detail(&self) -> Option<&DetailData> {
        match &self.detail {
            Detail::Ready(detail) => Some(detail),
            _ => None,
        }
    }

    /// What edits may do on the item on screen (spec FR-28, §3.4).
    pub(super) fn gate(&self, cx: &App) -> Gate {
        match self.detail() {
            Some(detail) => writes::gate(cx, detail.item.id),
            None => Gate::ReadOnly,
        }
    }

    pub(super) fn edits(cx: &App) -> Option<Entity<WiEdits>> {
        WiEdits::global(cx)
    }

    /// Explain why an edit can't start; `false` when it can.
    fn refused(&self, intent: Intent, cx: &mut App) -> bool {
        if !intent.writes() {
            return false;
        }
        match self.gate(cx) {
            Gate::Editable => false,
            Gate::Offline => {
                // Title, description and composer still open offline
                // (drafts save locally, design §9.6).
                if matches!(
                    intent,
                    Intent::Title | Intent::Description | Intent::Comment
                ) {
                    return false;
                }
                toast(
                    "wi-offline",
                    IconName::WifiOff,
                    "Offline — reconnect to send",
                    cx,
                );
                true
            }
            Gate::ReadOnly => {
                // Spec 0001 FR-9 / AC-19: the toast has no action.
                ToastStack::show(
                    Toast::new(
                        "wi-read-only",
                        IconName::Lock,
                        "Editing work items needs Work Items (Read & write).",
                    )
                    .tone(ToastTone::Warning),
                    cx,
                );
                true
            }
            Gate::Gone => {
                toast(
                    "wi-gone",
                    IconName::CircleAlert,
                    "This work item was deleted or you lost access to it",
                    cx,
                );
                true
            }
        }
    }

    // ── intents ──────────────────────────────────────────────────────────

    /// Run `intent` on the item on screen, or hold it until the detail of
    /// the selection loads.
    pub(super) fn intend(&mut self, intent: Intent, window: &mut Window, cx: &mut Context<Self>) {
        let ready = self
            .detail()
            .is_some_and(|d| Some(d.item.id) == self.detail_id());
        if !ready {
            if self.detail_id().is_some() {
                self.ui.intent = Some(intent);
            }
            return;
        }
        if self.refused(intent, cx) {
            return;
        }
        let from_list = self.list_focus.is_focused(window);
        self.ui.from_list = from_list;
        match intent {
            Intent::Title => self.open_title(window, cx),
            Intent::State => self.open_state_menu(from_list, window, cx),
            Intent::Assign => self.open_people(window, cx),
            Intent::AssignToMe => self.assign(Some(PeopleChoice::AssignToMe), cx),
            Intent::Unassign => self.assign(Some(PeopleChoice::Unassigned), cx),
            Intent::Area => self.open_tree(Field::Area, window, cx),
            Intent::Iteration => self.open_tree(Field::Iteration, window, cx),
            Intent::Description => self.open_description(window, cx),
            Intent::AddLink(None) => self.open_link_menu(window, cx),
            Intent::AddLink(Some(kind)) => self.open_link_picker(kind, window, cx),
            Intent::RemoveLink => self.request_removal(window, cx),
            Intent::Comment => self.focus_composer(window, cx),
            Intent::Sync => self.sync_item(cx),
        }
    }

    /// A held intent once its detail is on screen.
    pub(super) fn run_held_intent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.detail().map(|d| d.item.id) else {
            return;
        };
        let intent = self.ui.intent.take().or_else(|| take_queued(id, cx));
        if let Some(intent) = intent {
            self.intend(intent, window, cx);
        }
    }

    /// A different item came on screen: editors and overlays of the last
    /// one close (their drafts are saved), the cursor restarts.
    pub(super) fn reset_detail_ui(&mut self) {
        let anchors = self.ui.anchors.clone();
        let intent = self.ui.intent.take();
        self.ui = DetailUi {
            anchors,
            intent,
            ..DetailUi::default()
        };
    }

    /// After a load: the composer for this item (restoring its draft), the
    /// FR-31 rest timer, and the pending/freshness tick.
    pub(super) fn detail_loaded(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(detail) = self.detail() else {
            return;
        };
        let id = detail.item.id;
        if self.ui.composer.as_ref().is_none_or(|c| c.item != id) {
            let draft = detail
                .draft(DraftKind::Comment)
                .map(|d| d.body.clone())
                .unwrap_or_default();
            let composer = self.new_composer(id, &draft, window, cx);
            self.ui.composer = Some(composer);
        }
        if self.ui._rest.is_none() {
            self.ui._rest = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(REST).await;
                this.update(cx, |this, cx| {
                    if let Some(detail) = this.detail().filter(|d| d.item.id == id)
                        && let Some(edits) = Self::edits(cx)
                    {
                        let (item, extras) = (detail.item.clone(), detail.extras.clone());
                        edits.update(cx, |edits, cx| edits.rested_on(&item, extras.as_ref(), cx));
                    }
                })
                .ok();
            }));
        }
        self.ensure_tick(cx);
        self.run_held_intent(window, cx);
    }

    /// Re-render every second while something on screen depends on time
    /// (pending lines turning into "Waiting for Azure DevOps…", the
    /// freshness label's minute). Ends with the detail.
    fn ensure_tick(&mut self, cx: &mut Context<Self>) {
        if self.ui._tick.is_some() || self.clock.is_some() {
            return;
        }
        self.ui._tick = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let alive = this
                    .update(cx, |this, cx| {
                        if this.detail().is_some() {
                            cx.notify();
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        }));
    }

    pub(super) fn observe_edits(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Subscription> {
        let edits = WiEdits::install(self.store.clone(), cx);
        Some(cx.observe_in(&edits, window, |this, _, window, cx| {
            this.edits_changed(window, cx)
        }))
    }

    /// The controller changed (an answer, metadata, a sync): late data
    /// reaches open pickers, a rule asking for a typed value gets its
    /// field (focused, design §9.5), and the view repaints.
    fn edits_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.ui.state_menu.picker().cloned()
            && menu.read(cx).model().is_none()
            && let Some(states) = self.state_options(cx)
        {
            menu.update(cx, |menu, cx| menu.set_states(Some(states), cx));
        }
        if let (Some(tree), Some(field), false) = (
            self.ui.tree.picker().cloned(),
            self.ui.tree_field,
            self.ui.tree_ready,
        ) && let Some((nodes, true)) = self.tree_nodes(field, cx)
        {
            self.ui.tree_ready = true;
            tree.update(cx, |tree, cx| tree.set_nodes(Some(nodes), cx));
        }
        self.track_confirmations(cx);
        // The confirmed answer replaces the shown item on the frame the
        // pending clock clears (FR-7), ahead of the store reload.
        if let Some(edits) = Self::edits(cx)
            && let Detail::Ready(detail) = &mut self.detail
            && let Some(applied) = edits.read(cx).applied(detail.item.id)
            && applied.rev > detail.item.rev
        {
            detail.item = applied.clone();
        }
        let wants_input = self.rule(cx).is_some_and(|rule| {
            matches!(
                rule.kind,
                adoken_model::FieldKind::Text | adoken_model::FieldKind::Person
            )
        });
        match (wants_input, self.ui.rule_input.is_some()) {
            (true, false) => {
                let input = cx.new(|cx| TextInput::new("", cx));
                window.focus(&input.focus_handle(cx), cx);
                self.ui.rule_input = Some(input);
            }
            (false, true) => self.ui.rule_input = None,
            _ => {}
        }
        cx.notify();
    }

    /// FR-36: a write that was sending and now has no status was confirmed
    /// (failures, conflicts and rules keep a status): announce it once,
    /// politely and to assistive technology only. Same for a posted comment.
    fn track_confirmations(&mut self, cx: &mut Context<Self>) {
        let (Some(id), Some(edits)) = (self.detail_id(), Self::edits(cx)) else {
            return;
        };
        let edits = edits.read(cx);
        let statuses: Vec<(EditField, bool, String)> = edits
            .statuses(id)
            .map(|(field, status)| (*field, status.is_sending(), status.value()))
            .collect();
        let comments: Vec<u64> = edits
            .pending_comments(id)
            .iter()
            .filter(|c| matches!(c.status, writes::CommentStatus::Sending { .. }))
            .map(|c| c.local)
            .collect();
        let present: Vec<EditField> = statuses.iter().map(|(f, _, _)| *f).collect();
        let still_listed: Vec<u64> = edits.pending_comments(id).iter().map(|c| c.local).collect();
        let saved = saved_announcement(&self.ui.sending, &present).or_else(|| {
            self.ui
                .sending_comments
                .iter()
                .any(|local| !still_listed.contains(local))
                .then(|| "Comment posted".to_owned())
        });
        if let Some(text) = saved {
            self.ui.announcement = Some((text, self.now()));
        }
        self.ui.sending = statuses
            .into_iter()
            .filter(|(_, sending, _)| *sending)
            .map(|(field, _, value)| (field, value))
            .collect();
        self.ui.sending_comments = comments;
    }

    /// The announcement while it's fresh (it clears after a few seconds).
    pub(super) fn announcement(&self) -> Option<&str> {
        let (text, at) = self.ui.announcement.as_ref()?;
        (self.now() - at < ANNOUNCE_FOR).then_some(text.as_str())
    }

    // ── cursor ───────────────────────────────────────────────────────────

    pub(super) fn stop_input(&self, cx: &App) -> StopInput {
        let Some(detail) = self.detail() else {
            return StopInput::default();
        };
        let id = detail.item.id;
        let gate = self.gate(cx);
        let edits = Self::edits(cx);
        let edits = edits.as_ref().map(|e| e.read(cx));
        let has = |field: EditField| {
            edits.is_some_and(|e| e.status(id, field).is_some_and(|s| !s.is_sending()))
        };
        let shown = self.shown_link_rows(detail);
        let comments = &detail.comments;
        let first = model::first_visible_comment(comments.len(), self.ui.show_all_comments);
        let editable = gate == Gate::Editable || gate == Gate::Offline;
        StopInput {
            not_available: gate == Gate::Gone,
            title_notice: has(EditField::Title),
            state_notice: has(EditField::State),
            read_only: gate == Gate::ReadOnly,
            field_notices: [
                (Field::AssignedTo, EditField::AssignedTo),
                (Field::Area, EditField::Area),
                (Field::Iteration, EditField::Iteration),
            ]
            .into_iter()
            .filter(|(_, f)| has(*f))
            .map(|(f, _)| f)
            .collect(),
            description_notice: has(EditField::Description)
                || matches!(detail.editable, Editable::Blocked(_)),
            description_show_all: detail
                .description
                .as_ref()
                .is_some_and(|d| d.lines > description::CLIP_LINES)
                && !self.ui.show_all_description,
            link_rows: shown.rows.len(),
            link_show_more: shown.more.clone(),
            can_add_link: editable,
            earlier_comments: first > 0,
            comments: (first..comments.len()).map(|ix| (ix, false)).collect(),
            composer: editable,
            history_show_all: detail.history.len() > model::HISTORY_SHOWN
                && !self.ui.show_all_history,
        }
    }

    fn step_cursor(&mut self, delta: isize, cx: &mut Context<Self>) {
        let stops = model::stops(&self.stop_input(cx));
        self.ui.cursor = model::step_stop(&stops, self.ui.cursor, delta);
        if let Some(stop) = self.ui.cursor {
            self.scroll_to_stop(stop);
        }
        cx.notify();
    }

    /// Scroll the stop into view on the next paint (instant, design
    /// §10.3): the pane's reveal pass reads where the cursor painted.
    fn scroll_to_stop(&mut self, _stop: Stop) {
        self.ui.reveal.set(Reveal::Cursor);
    }

    fn activate_stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(stop) = self.ui.cursor else {
            self.ui.cursor = Some(Stop::Title);
            cx.notify();
            return;
        };
        match stop {
            Stop::NotAvailable => self.open_in_ado(cx),
            Stop::Title => self.intend(Intent::Title, window, cx),
            Stop::State => self.intend(Intent::State, window, cx),
            Stop::Field(Field::AssignedTo) => self.intend(Intent::Assign, window, cx),
            Stop::Field(Field::Project) => {}
            Stop::Field(Field::Area) => self.intend(Intent::Area, window, cx),
            Stop::Field(Field::Iteration) => self.intend(Intent::Iteration, window, cx),
            Stop::Description => self.intend(Intent::Description, window, cx),
            Stop::DescriptionNotice => {
                if matches!(
                    self.detail().map(|d| &d.editable),
                    Some(Editable::Blocked(_))
                ) {
                    self.open_in_ado(cx);
                } else {
                    self.retry_edit(EditField::Description, cx);
                }
            }
            Stop::DescriptionShowAll => {
                self.ui.show_all_description = true;
                cx.notify();
            }
            Stop::Link(row) => self.open_link_row(row, window, cx),
            Stop::LinkShowMore(group) => {
                self.ui.expanded_groups.insert(group);
                cx.notify();
            }
            Stop::AddLink => self.intend(Intent::AddLink(None), window, cx),
            Stop::EarlierComments => {
                self.ui.show_all_comments = true;
                cx.notify();
            }
            Stop::Comment(_) | Stop::CommentNotice(_) => {}
            Stop::Composer => self.intend(Intent::Comment, window, cx),
            Stop::HistoryShowAll => {
                self.ui.show_all_history = true;
                cx.notify();
            }
            Stop::TitleNotice => self.retry_edit(EditField::Title, cx),
            Stop::StateNotice => self.retry_edit(EditField::State, cx),
            Stop::FieldNotice(Field::AssignedTo) => self.retry_edit(EditField::AssignedTo, cx),
            Stop::FieldNotice(Field::Area) => self.retry_edit(EditField::Area, cx),
            Stop::FieldNotice(Field::Iteration) => self.retry_edit(EditField::Iteration, cx),
            Stop::FieldNotice(Field::Project) => {}
        }
    }

    /// Focus returns where the editor was opened from (design §10.1).
    fn return_focus(&mut self, stop: Stop, window: &mut Window, cx: &mut Context<Self>) {
        self.ui.cursor = Some(stop);
        if self.ui.from_list {
            window.focus(&self.list_focus, cx);
        } else {
            window.focus(&self.detail_focus, cx);
        }
        cx.notify();
    }

    // ── title (FR-12) ────────────────────────────────────────────────────

    fn open_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.detail().map(|d| d.item.title.clone()) else {
            return;
        };
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Title", cx);
            input.set_text(&title, cx);
            input
        });
        let events = cx.subscribe(&input, |this, _, event, cx| {
            if *event == TextInputEvent::Changed
                && let Some(editor) = &mut this.ui.title
            {
                editor.error = None;
                cx.notify();
            }
        });
        // All text is selected (design §3.2).
        input.update(cx, |input, cx| input.select_all_text(cx));
        window.focus(&input.focus_handle(cx), cx);
        self.ui.cursor = Some(Stop::Title);
        self.ui.title = Some(TitleEditor {
            input,
            error: None,
            _events: events,
        });
        cx.notify();
    }

    fn save_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(editor), Some(detail)) = (&mut self.ui.title, self.detail.as_ready()) else {
            return;
        };
        let typed = editor.input.read(cx).text().to_owned();
        match model::title_edit(&detail.item.title, &typed) {
            Err(why) => {
                editor.error = Some(why);
                cx.notify();
            }
            Ok(None) => self.close_title(window, cx),
            Ok(Some(title)) => {
                let item = detail.item.clone();
                let change = WiChange::Title {
                    base: item.title.clone(),
                    to: title,
                };
                self.close_title(window, cx);
                self.send(&item, change, cx);
            }
        }
    }

    fn close_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ui.title = None;
        self.return_focus(Stop::Title, window, cx);
    }

    // ── sending ──────────────────────────────────────────────────────────

    /// Hand `change` to the controller: it shows as pending on the next
    /// frame; a local refusal (link guards) explains itself.
    pub(super) fn send(&mut self, item: &WorkItem, change: WiChange, cx: &mut Context<Self>) {
        let Some(edits) = Self::edits(cx) else {
            return;
        };
        let refused = edits
            .update(cx, |edits, cx| edits.edit(item, change, cx))
            .err();
        if let Some(why) = refused {
            toast("wi-refused", IconName::CircleAlert, why, cx);
        }
        cx.notify();
    }

    pub(super) fn retry_edit(&mut self, field: EditField, cx: &mut Context<Self>) {
        if let (Some(edits), Some(id)) = (Self::edits(cx), self.detail_id()) {
            edits.update(cx, |edits, cx| edits.retry(id, field, cx));
        }
    }

    // ── state (FR-11) ────────────────────────────────────────────────────

    /// The menu's states, `None` while the type's metadata loads.
    fn state_options(&self, cx: &mut App) -> Option<Vec<StateOption>> {
        let item = self.detail()?.item.clone();
        let edits = Self::edits(cx)?;
        let states = edits.update(cx, |edits, cx| edits.states(&item, cx))?;
        Some(
            states
                .into_iter()
                .map(|s| StateOption::new(s.name, menu_category(s.category)))
                .collect(),
        )
    }

    fn open_state_menu(&mut self, keyboard: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.detail().map(|d| d.item.state.clone()) else {
            return;
        };
        let states = self.state_options(cx);
        let placement = self.placement(&self.ui.anchors.state.clone(), 0.0, keyboard, window);
        let menu = cx.new(|cx| {
            StateMenu::new(
                "Change state",
                current.clone(),
                states,
                placement,
                window,
                cx,
            )
        });
        let chosen = cx.subscribe(&menu, move |this, _, event: &StateMenuEvent, cx| {
            let StateMenuEvent::Chosen(name) = event;
            let Some(item) = this.detail().map(|d| d.item.clone()) else {
                return;
            };
            if !name.eq_ignore_ascii_case(&item.state) {
                let change = WiChange::State {
                    base: item.state.clone(),
                    to: name.to_string(),
                };
                this.send(&item, change, cx);
            }
            this.ui.cursor = Some(Stop::State);
        });
        self.ui._subscriptions.push(chosen);
        self.ui.cursor = Some(Stop::State);
        self.ui
            .state_menu
            .open(menu, |this: &mut Self| &mut this.ui.state_menu, cx);
    }

    /// Under `anchor` (its last painted bounds), else near the pane top.
    fn placement(
        &self,
        anchor: &BoundsSlot,
        min_width: f32,
        keyboard: bool,
        window: &Window,
    ) -> Placement {
        let min_width = if min_width > 0.0 {
            min_width
        } else {
            adoken_theme::size::ACTION_MENU_WIDTH
        };
        let placement = match anchor.get() {
            Some(bounds) => Placement::below(bounds, min_width),
            None => {
                let size = window.viewport_size();
                Placement::at(
                    gpui::point(size.width / 2.0, size.height / 4.0),
                    gpui::Anchor::TopLeft,
                    min_width,
                )
            }
        };
        if keyboard || !window.is_window_active() {
            placement.keyboard()
        } else {
            placement
        }
    }

    // ── assignee (FR-13) ─────────────────────────────────────────────────

    fn me(&self) -> Option<&StoredPerson> {
        self.list.people.iter().find(|p| p.id == self.list.me)
    }

    fn open_people(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(detail) = self.detail() else {
            return;
        };
        let me = self
            .me()
            .map(picker_person)
            .unwrap_or_else(|| Person::new(self.list.me.clone(), "Me", ""));
        let current = detail.item.assigned_to.clone().map(SharedString::from);
        let people: Vec<Person> = self
            .list
            .people
            .iter()
            .filter(|p| !p.is_group && p.unique_name.as_deref().is_some_and(|u| !u.is_empty()))
            .map(picker_person)
            .collect();
        let recent: Vec<Person> = Self::edits(cx)
            .map(|e| {
                e.read(cx)
                    .recent_assignees()
                    .iter()
                    .map(picker_person)
                    .collect()
            })
            .unwrap_or_default();
        let keyboard = self.ui.from_list || self.detail_focus.is_focused(window);
        let placement = self.placement(
            &self.ui.anchors.cell(Field::AssignedTo).clone(),
            adoken_theme::size::PICKER_POPOVER,
            keyboard,
            window,
        );
        let picker = cx.new(|cx| {
            let mut picker = PeoplePicker::new(PeopleMode::Assign { me }, placement, window, cx);
            picker.set_title("Change assignee");
            picker.set_avatar(avatar_fn());
            picker.set_people(people, cx);
            picker.set_recent(recent, cx);
            picker.set_current(current, cx);
            picker
        });
        let events = cx.subscribe(
            &picker,
            |this, picker, event: &PeopleEvent, cx| match event {
                PeopleEvent::Chosen(choice) => {
                    this.assign(Some(choice.clone()), cx);
                    this.ui.cursor = Some(Stop::Field(Field::AssignedTo));
                }
                PeopleEvent::QueryChanged(query) => {
                    this.search_directory(picker, query.clone(), cx)
                }
                PeopleEvent::RetryDirectory => {
                    let query = picker.read(cx).query().to_owned();
                    this.search_directory(picker, query.into(), cx);
                }
            },
        );
        self.ui._subscriptions.push(events);
        self.ui.cursor = Some(Stop::Field(Field::AssignedTo));
        self.ui
            .people
            .open(picker, |this: &mut Self| &mut this.ui.people, cx);
    }

    /// FR-13: one directory search once typing pauses 300 ms on a query of
    /// 3+ characters with fewer than 5 local matches; a newer keystroke
    /// cancels it (dropping the task drops the request).
    fn search_directory(
        &mut self,
        picker: Entity<PeoplePicker>,
        query: SharedString,
        cx: &mut Context<Self>,
    ) {
        self.ui._directory = None;
        let local = picker.read(cx).local_match_count();
        if !adoken_ui::picker::wants_directory(&query, local) || self.gate(cx) == Gate::Offline {
            return;
        }
        self.ui._directory = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DIRECTORY_DEBOUNCE).await;
            let Ok(Some(search)) = this.update(cx, |_, cx| {
                picker.update(cx, |picker, cx| {
                    picker.set_directory(Directory::Searching, cx)
                });
                let edits = Self::edits(cx)?;
                Some(edits.update(cx, |edits, cx| edits.search_people(query.to_string(), cx)))
            }) else {
                return;
            };
            let result = search.await;
            picker.update(cx, |picker, cx| {
                if picker.query() != query.as_ref() {
                    return;
                }
                let directory = match result {
                    Ok(people) => Directory::Results(
                        people
                            .iter()
                            .filter(|p| !p.is_group)
                            .map(picker_person)
                            .collect(),
                    ),
                    Err(_) => Directory::Failed,
                };
                picker.set_directory(directory, cx);
            });
        }));
    }

    /// Assign to `choice` (spec FR-13): me, nobody, or a person.
    fn assign(&mut self, choice: Option<PeopleChoice>, cx: &mut Context<Self>) {
        let (Some(choice), Some(item)) = (choice, self.detail().map(|d| d.item.clone())) else {
            return;
        };
        let person = |id: &str| self.list.people.iter().find(|p| p.id == id).cloned();
        let to = match choice {
            PeopleChoice::AssignToMe => match person(&self.list.me) {
                Some(me) => Some(me),
                None => return,
            },
            PeopleChoice::Unassigned => None,
            PeopleChoice::Person(p) => Some(person(&p.id).unwrap_or_else(|| StoredPerson {
                id: p.id.to_string(),
                display_name: p.name.to_string(),
                unique_name: Some(p.sign_in.to_string()).filter(|s| !s.is_empty()),
                is_group: p.group,
                image_url: None,
            })),
        };
        if to.as_ref().map(|p| &p.id) == item.assigned_to.as_ref() {
            return;
        }
        let change = WiChange::Assign {
            base: item.assigned_to.clone(),
            to,
        };
        self.send(&item, change, cx);
    }

    // ── area / iteration (FR-14) ─────────────────────────────────────────

    /// The picker's nodes and whether they're the project's real tree: the
    /// cached tree, else (while it loads) the paths seen in loaded items
    /// (FR-14).
    fn tree_nodes(
        &self,
        field: Field,
        cx: &mut App,
    ) -> Option<(Vec<adoken_ui::picker::TreeNode>, bool)> {
        let detail = self.detail()?;
        let project_id = detail.item.project_id.clone();
        let edits = Self::edits(cx)?;
        let tree = edits.update(cx, |edits, cx| edits.tree(&project_id, cx));
        let detail = self.detail()?;
        match tree {
            Some(tree) => {
                let nodes = match field {
                    Field::Iteration => &tree.iterations,
                    _ => &tree.areas,
                };
                Some((fields::tree_nodes(nodes), true))
            }
            None => {
                let mut paths: Vec<&str> = self
                    .list
                    .items
                    .iter()
                    .filter(|i| i.project_id == detail.item.project_id)
                    .map(|i| match field {
                        Field::Iteration => i.iteration_path.as_str(),
                        _ => i.area_path.as_str(),
                    })
                    .filter(|p| !p.is_empty())
                    .collect();
                if paths.is_empty() {
                    paths.push(&detail.project);
                }
                Some((adoken_ui::picker::nodes_from_paths(&paths), false))
            }
        }
    }

    fn open_tree(&mut self, field: Field, window: &mut Window, cx: &mut Context<Self>) {
        let Some(detail) = self.detail() else {
            return;
        };
        let current = match field {
            Field::Iteration => detail.item.iteration_path.clone(),
            _ => detail.item.area_path.clone(),
        };
        let (kind, title) = match field {
            Field::Iteration => (
                TreeKind::Iteration {
                    today: self.now().div_euclid(adoken_model::time::DAY),
                },
                "Change iteration",
            ),
            _ => (TreeKind::Area, "Change area"),
        };
        let (nodes, ready) = self
            .tree_nodes(field, cx)
            .map_or((None, false), |(nodes, ready)| (Some(nodes), ready));
        self.ui.tree_ready = ready;
        let keyboard = self.ui.from_list || self.detail_focus.is_focused(window);
        let placement = self.placement(
            &self.ui.anchors.cell(field).clone(),
            adoken_theme::size::PICKER_POPOVER,
            keyboard,
            window,
        );
        let picker = cx.new(|cx| {
            TreePicker::new(
                kind,
                title,
                nodes,
                Some(current.into()),
                placement,
                window,
                cx,
            )
        });
        let events = cx.subscribe(&picker, move |this, _, event: &TreeEvent, cx| {
            let TreeEvent::Chosen(path) = event;
            let Some(item) = this.detail().map(|d| d.item.clone()) else {
                return;
            };
            let change = match field {
                Field::Iteration if path.as_ref() != item.iteration_path => WiChange::Iteration {
                    base: item.iteration_path.clone(),
                    to: path.to_string(),
                },
                Field::Area if path.as_ref() != item.area_path => WiChange::Area {
                    base: item.area_path.clone(),
                    to: path.to_string(),
                },
                _ => return,
            };
            this.send(&item, change, cx);
            this.ui.cursor = Some(Stop::Field(field));
        });
        self.ui._subscriptions.push(events);
        self.ui.tree_field = Some(field);
        self.ui.cursor = Some(Stop::Field(field));
        self.ui
            .tree
            .open(picker, |this: &mut Self| &mut this.ui.tree, cx);
    }

    // ── description (FR-17..FR-20) ───────────────────────────────────────

    fn open_description(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(detail) = self.detail() else {
            return;
        };
        let (base, markdown_field) = match &detail.editable {
            Editable::Markdown(source) => (source.clone(), true),
            Editable::Html(markdown) => (markdown.clone(), false),
            Editable::Blocked(_) => {
                // The notice explains; `d` focuses its Open button.
                self.ui.cursor = Some(Stop::DescriptionNotice);
                window.focus(&self.detail_focus, cx);
                cx.notify();
                return;
            }
            Editable::NotFetched => {
                self.ui.cursor = Some(Stop::Description);
                toast(
                    "wi-description-fetch",
                    IconName::RefreshCw,
                    "Sync this work item first to edit its description",
                    cx,
                );
                self.sync_item(cx);
                return;
            }
        };
        let text = detail
            .draft(DraftKind::Description)
            .map_or_else(|| base.clone(), |d| d.body.clone());
        let label = if markdown_field {
            "Description, Markdown"
        } else {
            "Description, saved as HTML"
        };
        let area = cx.new(|cx| {
            let mut area = TextArea::new("Write a description in Markdown", cx);
            area.set_lines(8, 24, cx);
            area.set_accessible_label(label);
            area.set_text(&text, Vec::new(), cx);
            area
        });
        let events = cx.subscribe(&area, |this, area, _: &TextAreaEvent, cx| {
            let text = area.read(cx).text().to_owned();
            this.save_draft(DraftKind::Description, text, cx);
            if let Some(editor) = &mut this.ui.description {
                editor.draft_saved = true;
            }
            cx.notify();
        });
        window.focus(&area.focus_handle(cx), cx);
        self.ui.cursor = Some(Stop::Description);
        self.ui.description = Some(DescriptionEditor {
            area,
            base,
            markdown_field,
            preview: None,
            previewing: false,
            draft_saved: false,
            focus: cx.focus_handle(),
            _events: events,
            _preview: None,
        });
        cx.notify();
    }

    /// `⌘E`: Preview renders the current text with FR-3's renderer, parsed
    /// off the UI thread.
    pub(super) fn toggle_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.ui.description else {
            return;
        };
        editor.previewing = !editor.previewing;
        if editor.previewing {
            let text = editor.area.read(cx).text().to_owned();
            let parse = cx.background_spawn(async move {
                Arc::new(adoken_richtext::parse(
                    &text,
                    adoken_richtext::Format::Markdown,
                ))
            });
            editor._preview = Some(cx.spawn(async move |this, cx| {
                let doc = parse.await;
                this.update(cx, |this, cx| {
                    if let Some(editor) = &mut this.ui.description {
                        editor.preview = Some(doc);
                        cx.notify();
                    }
                })
                .ok();
            }));
            window.focus(&editor.focus, cx);
        } else {
            window.focus(&editor.area.focus_handle(cx), cx);
        }
        cx.notify();
    }

    fn save_description(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(editor), Some(detail)) = (&self.ui.description, self.detail()) else {
            return;
        };
        let markdown = editor.area.read(cx).text().to_owned();
        if markdown == editor.base {
            return;
        }
        let item = detail.item.clone();
        let base = detail.description_source.as_ref().map(|s| s.source.clone());
        let change = if editor.markdown_field {
            WiChange::Description {
                to: markdown,
                format: adoken_model::TextFormat::Markdown,
                base,
            }
        } else {
            let names = &self.list.names;
            let original = base.clone().unwrap_or_default();
            let known = adoken_richtext::mention_names(&original);
            let lookup = |id: &str| {
                names.get(id).cloned().or_else(|| {
                    known
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(id))
                        .map(|(_, n)| n.clone())
                })
            };
            WiChange::Description {
                to: adoken_richtext::markdown_to_html(&markdown, &lookup),
                format: adoken_model::TextFormat::Html,
                base,
            }
        };
        self.ui.description = None;
        self.return_focus(Stop::Description, window, cx);
        self.send(&item, change, cx);
    }

    fn close_description(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ui.description.take().is_some() {
            self.return_focus(Stop::Description, window, cx);
        }
    }

    pub(super) fn save_draft(&mut self, kind: DraftKind, body: String, cx: &mut Context<Self>) {
        let (Some(edits), Some(item)) = (Self::edits(cx), self.detail().map(|d| d.item.clone()))
        else {
            return;
        };
        edits.update(cx, |edits, cx| edits.save_draft(&item, kind, body, cx));
    }

    /// Discard mine (FR-19) or the draft chip's discard: the draft goes.
    pub(super) fn discard_draft(&mut self, kind: DraftKind, cx: &mut Context<Self>) {
        self.save_draft(kind, String::new(), cx);
        if let Detail::Ready(detail) = &mut self.detail {
            detail.drafts.retain(|d| d.kind != kind);
        }
        cx.notify();
    }

    // ── links (FR-21..FR-23) ─────────────────────────────────────────────

    fn open_link_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(has_parent) = self.detail().map(|d| d.parent().is_some()) else {
            return;
        };
        let keyboard = self.ui.from_list || self.detail_focus.is_focused(window);
        let placement = self.placement(&self.ui.anchors.add_link.clone(), 0.0, keyboard, window);
        let menu = cx.new(|cx| LinkTypeMenu::new(has_parent, placement, window, cx));
        let events = cx.subscribe_in(
            &menu,
            window,
            |this, _, event: &LinkTypeEvent, window, cx| {
                let LinkTypeEvent::Chosen(ty) = event;
                this.open_link_picker(link_kind(*ty), window, cx);
            },
        );
        self.ui._subscriptions.push(events);
        self.ui.cursor = Some(Stop::AddLink);
        self.ui
            .link_menu
            .open(menu, |this: &mut Self| &mut this.ui.link_menu, cx);
    }

    fn open_link_picker(&mut self, kind: LinkKind, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("Search work items, #1234, or paste a link", cx));
        let events = cx.subscribe(&input, |this, input, event, cx| {
            if *event == TextInputEvent::Changed {
                let text = input.read(cx).text().to_owned();
                this.search_link_picker(text, cx);
            }
        });
        window.focus(&input.focus_handle(cx), cx);
        self.ui.link_picker = Some(LinkPicker {
            kind,
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
        let store = self.store.clone();
        let job =
            cx.background_spawn(async move { load_catalog(&store).map_err(|e| e.to_string()) });
        let task = cx.spawn(async move |this, cx| {
            let catalog = job.await;
            this.update(cx, |this, cx| {
                let Some(picker) = this.ui.link_picker.as_mut() else {
                    return;
                };
                match catalog {
                    Ok(catalog) => {
                        picker.catalog = Some(Arc::new(catalog));
                        let text = picker.input.read(cx).text().to_owned();
                        this.search_link_picker(text, cx);
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
        if let Some(picker) = self.ui.link_picker.as_mut() {
            picker._catalog = Some(task);
        }
        cx.notify();
    }

    fn search_link_picker(&mut self, text: String, cx: &mut Context<Self>) {
        let now = self.now();
        let Some(picker) = self.ui.link_picker.as_mut() else {
            return;
        };
        let Some(catalog) = picker.catalog.clone() else {
            return;
        };
        picker.generation += 1;
        let generation = picker.generation;
        let job = cx.background_spawn(async move { picker_rows(&catalog, &text, now) });
        picker._search = Some(cx.spawn(async move |this, cx| {
            let rows = job.await;
            this.update(cx, |this, cx| {
                if let Some(picker) = this.ui.link_picker.as_mut()
                    && picker.generation == generation
                {
                    picker.rows = rows;
                    picker.selected = 0;
                    picker.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Why the picker row can't be chosen (design §6.2).
    pub(super) fn pick_refusal(&self, kind: LinkKind, row: &PickRow) -> Option<String> {
        let detail = self.detail()?;
        let target = row.id()?;
        model::refusal(kind, detail.item.id, target, &detail.linked())
    }

    fn choose_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = &self.ui.link_picker else {
            return;
        };
        let kind = picker.kind;
        let Some(row) = picker.rows.get(picker.selected).cloned() else {
            return;
        };
        let Some(target) = row.id() else {
            return;
        };
        if self.pick_refusal(kind, &row).is_some() {
            return;
        }
        let Some(detail) = self.detail() else {
            return;
        };
        let item = detail.item.clone();
        let replace_parent = (kind == LinkKind::Parent)
            .then(|| detail.parent().and_then(LinkRow::work_item_id))
            .flatten();
        self.ui.link_picker = None;
        self.return_focus(Stop::AddLink, window, cx);
        self.send(
            &item,
            WiChange::AddLink {
                kind,
                target,
                replace_parent,
            },
            cx,
        );
    }

    /// `⌫` on a link row asks first; elsewhere (`⌘P`) choose the link.
    fn request_removal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(detail) = self.detail() else {
            return;
        };
        let what = match self.ui.cursor {
            Some(Stop::Link(row)) if self.detail_focus.is_focused(window) => {
                let shown = self.shown_link_rows(detail);
                let Some(&ix) = shown.rows.get(row) else {
                    return;
                };
                if !detail.links[ix].group.is_work_item() {
                    // PR, commit and other rows are read-only (AC-19).
                    return;
                }
                Removal::Confirm { row: ix }
            }
            _ => {
                if !detail.links.iter().any(|r| r.group.is_work_item()) {
                    toast("wi-no-links", IconName::Link, "No work item is linked", cx);
                    return;
                }
                Removal::Choose { selected: 0 }
            }
        };
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.ui.remove = Some(RemoveOverlay { what, focus });
        cx.notify();
    }

    pub(super) fn request_remove_row(
        &mut self,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.refused(Intent::RemoveLink, cx) {
            return;
        }
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.ui.remove = Some(RemoveOverlay {
            what: Removal::Confirm { row },
            focus,
        });
        cx.notify();
    }

    /// The work item link rows (by index into `links`), for the Remove
    /// link chooser.
    pub(super) fn removable_rows(&self) -> Vec<usize> {
        self.detail().map_or_else(Vec::new, |d| {
            d.links
                .iter()
                .enumerate()
                .filter(|(_, r)| r.group.is_work_item())
                .map(|(ix, _)| ix)
                .collect()
        })
    }

    fn confirm_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ui.link_picker.is_some() {
            self.choose_link(window, cx);
            return;
        }
        let removable = self.removable_rows();
        let Some(overlay) = self.ui.remove.as_mut() else {
            return;
        };
        match overlay.what {
            Removal::Choose { selected } => {
                if let Some(&row) = removable.get(selected) {
                    overlay.what = Removal::Confirm { row };
                    cx.notify();
                }
            }
            Removal::Confirm { row } => {
                self.ui.remove = None;
                let Some(detail) = self.detail() else {
                    return;
                };
                let Some(link) = detail.links.get(row) else {
                    return;
                };
                let (Some(kind), Some(target)) = (link.kind, link.work_item_id()) else {
                    return;
                };
                let item = detail.item.clone();
                self.return_focus(self.ui.cursor.unwrap_or(Stop::Title), window, cx);
                self.send(&item, WiChange::RemoveLink { kind, target }, cx);
            }
        }
    }

    fn step_overlay(&mut self, delta: isize, cx: &mut Context<Self>) {
        let removable = self.removable_rows().len();
        if let Some(picker) = self.ui.link_picker.as_mut() {
            let n = picker.rows.len();
            if n > 0 {
                picker.selected =
                    (picker.selected as isize + delta).rem_euclid(n as isize) as usize;
                picker
                    .scroll
                    .scroll_to_item(picker.selected, gpui::ScrollStrategy::Nearest);
            }
        } else if let Some(RemoveOverlay {
            what: Removal::Choose { selected },
            ..
        }) = self.ui.remove.as_mut()
            && removable > 0
        {
            *selected = (*selected as isize + delta).rem_euclid(removable as isize) as usize;
        }
        cx.notify();
    }

    pub(super) fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let had = self.ui.link_picker.take().is_some() | self.ui.remove.take().is_some();
        if had {
            let stop = self.ui.cursor.unwrap_or(Stop::AddLink);
            self.return_focus(stop, window, cx);
        }
    }

    /// `↩` on a link row: work items open in Work items, PRs in their
    /// review, the rest in the browser (FR-23).
    pub(super) fn open_link_row(
        &mut self,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(detail) = self.detail() else {
            return;
        };
        let shown = self.shown_link_rows(detail);
        let Some(link) = shown.rows.get(row).and_then(|&ix| detail.links.get(ix)) else {
            return;
        };
        match &link.target {
            LinkTarget::WorkItem { id, .. } => {
                let id = *id;
                self.open_work_item(id, window, cx);
            }
            LinkTarget::PullRequest { id, .. } => cx.emit(WorkItemsEvent::OpenPr(*id)),
            LinkTarget::Commit { web, .. } | LinkTarget::Other { web, .. } => {
                if let Some(url) = web.clone() {
                    cx.open_url(&url);
                }
            }
        }
    }

    // ── discussion (FR-25, FR-26) ────────────────────────────────────────

    fn new_composer(
        &mut self,
        item: i64,
        draft: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Composer {
        let names = self.list.names.clone();
        let (text, atoms) = atoms_of(draft, |id| {
            names
                .get(id)
                .cloned()
                .unwrap_or_else(|| "Unknown user".into())
        });
        let area = cx.new(|cx| {
            let mut area = TextArea::new("Add a comment…", cx);
            area.set_lines(1, 12, cx);
            area.set_accessible_label("Add a comment, Markdown, at to mention");
            area.set_text(&text, atoms, cx);
            area
        });
        let events = cx.subscribe_in(
            &area,
            window,
            |this, area, _: &TextAreaEvent, window, cx| {
                let body = area.read(cx).serialize(adoken_richtext::mention_token);
                this.save_draft(DraftKind::Comment, body, cx);
                this.update_mention(window, cx);
            },
        );
        Composer {
            item,
            area,
            mention_at: None,
            _events: events,
        }
    }

    fn focus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = &self.ui.composer else {
            return;
        };
        // Focus expands the composer to three lines (design §7.3).
        composer
            .area
            .update(cx, |area, cx| area.set_lines(3, 12, cx));
        window.focus(&composer.area.focus_handle(cx), cx);
        self.ui.cursor = Some(Stop::Composer);
        self.ui.reveal.set(Reveal::Composer);
        cx.notify();
    }

    /// Leaving collapses it back to one line (the draft stays).
    fn collapse_composer(&mut self, cx: &mut Context<Self>) {
        if let Some(composer) = &self.ui.composer {
            composer
                .area
                .update(cx, |area, cx| area.set_lines(1, 12, cx));
        }
    }

    fn post_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(composer), Some(item)) =
            (&self.ui.composer, self.detail().map(|d| d.item.clone()))
        else {
            return;
        };
        if self.gate(cx) != Gate::Editable {
            return;
        }
        let markdown = composer
            .area
            .read(cx)
            .serialize(adoken_richtext::mention_token);
        if markdown.trim().is_empty() {
            return;
        }
        let area = composer.area.clone();
        area.update(cx, |area, cx| area.set_text("", Vec::new(), cx));
        self.end_mention(cx);
        if let Some(edits) = Self::edits(cx)
            && let Err(why) = edits.update(cx, |edits, cx| edits.comment(&item, markdown, cx))
        {
            toast("wi-comment-refused", IconName::CircleAlert, why, cx);
        }
        self.collapse_composer(cx);
        self.return_focus(Stop::Composer, window, cx);
    }

    fn leave_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_mention(cx);
        self.collapse_composer(cx);
        self.return_focus(Stop::Composer, window, cx);
    }

    /// The mention list closed: `↩`/`↑`/`↓` edit the text again.
    pub(super) fn end_mention(&mut self, cx: &mut Context<Self>) {
        self.ui.mention.close();
        if let Some(composer) = self.ui.composer.as_mut() {
            composer.mention_at = None;
            composer
                .area
                .update(cx, |area, cx| area.set_navigation(true, cx));
        }
        cx.notify();
    }

    /// Keep the mention list in step with the `@query` before the caret.
    fn update_mention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = self.ui.composer.as_mut() else {
            return;
        };
        let state = composer.area.read(cx).state();
        let (start, line) = state.before_caret_on_line();
        let query = mention_query(line, start)
            .filter(|(at, _)| !state.atoms().iter().any(|a| a.range.start == *at))
            .map(|(at, q)| (at, q.to_owned()));
        match query {
            None => {
                if composer.mention_at.is_some() {
                    self.end_mention(cx);
                }
            }
            Some((at, query)) => {
                composer.mention_at = Some(at);
                if let Some(picker) = self.ui.mention.picker().cloned() {
                    picker.update(cx, |picker, cx| picker.set_query(&query, cx));
                } else {
                    composer
                        .area
                        .update(cx, |area, cx| area.set_navigation(false, cx));
                    self.open_mention(query, window, cx);
                }
            }
        }
    }

    fn open_mention(&mut self, query: String, window: &mut Window, cx: &mut Context<Self>) {
        let width = adoken_theme::size::ACTION_MENU_WIDTH;
        let placement = match self.ui.anchors.composer.get() {
            Some(anchor) => Placement::above(anchor, width),
            // Not painted yet (opened before the first frame): mid-window.
            None => {
                let size = window.viewport_size();
                Placement::at(
                    gpui::point(size.width / 3.0, size.height / 2.0),
                    gpui::Anchor::BottomLeft,
                    width,
                )
            }
        }
        .keyboard();
        let people: Vec<Person> = self.list.people.iter().map(picker_person).collect();
        let picker = cx.new(|cx| {
            let mut picker = PeoplePicker::new(PeopleMode::Mention, placement, window, cx);
            picker.set_avatar(avatar_fn());
            picker.set_people(people, cx);
            picker.set_query(&query, cx);
            picker
        });
        let events = cx.subscribe(&picker, |this, _, event: &PeopleEvent, cx| {
            if let PeopleEvent::Chosen(PeopleChoice::Person(person)) = event {
                this.insert_mention(person.clone(), cx);
            }
        });
        let dismissed = cx.subscribe(&picker, |this, _, _: &gpui::DismissEvent, cx| {
            this.end_mention(cx);
        });
        self.ui._subscriptions.extend([events, dismissed]);
        self.ui
            .mention
            .open(picker, |this: &mut Self| &mut this.ui.mention, cx);
    }

    fn insert_mention(&mut self, person: Person, cx: &mut Context<Self>) {
        let Some(composer) = self.ui.composer.as_mut() else {
            return;
        };
        let Some(at) = composer.mention_at.take() else {
            return;
        };
        let caret = composer.area.read(cx).state().edit.cursor();
        let display = format!("@{}", person.name);
        composer.area.update(cx, |area, cx| {
            area.insert_atom(at..caret, &display, person.id.clone(), cx)
        });
        self.end_mention(cx);
    }

    fn mention_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(picker) = self.ui.mention.picker().cloned() {
            picker.update(cx, |picker, cx| {
                if delta > 0 {
                    picker.select_next(cx)
                } else {
                    picker.select_previous(cx)
                }
            });
        }
    }

    fn mention_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.ui.mention.picker().cloned() else {
            return;
        };
        let choice = picker.update(cx, |picker, cx| picker.confirm(window, cx));
        if let Some(PeopleChoice::Person(person)) = choice {
            self.insert_mention(person, cx);
        }
    }

    // ── sync and web ─────────────────────────────────────────────────────

    pub(super) fn sync_item(&mut self, cx: &mut Context<Self>) {
        let (Some(edits), Some(item)) = (Self::edits(cx), self.detail().map(|d| d.item.clone()))
        else {
            return;
        };
        let press = edits.update(cx, |edits, cx| edits.sync_item(&item, cx));
        if let writes::SyncPress::Disabled(why) = press {
            toast("wi-sync", IconName::RefreshCw, why, cx);
        }
        cx.notify();
    }

    fn copy(&mut self, link: bool, cx: &mut Context<Self>) {
        let Some(id) = self.detail_id().or_else(|| self.selected_id()) else {
            return;
        };
        let (text, message) = if link {
            match self.web_url() {
                Some(url) => (url, "Copied link".to_owned()),
                None => return,
            }
        } else {
            (format!("#{id}"), format!("Copied #{id}"))
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        toast("wi-copied", IconName::Check, message, cx);
    }

    // ── action handlers ──────────────────────────────────────────────────

    /// The detail's action listeners on the view's root (they run from the
    /// list too: keys act on the selection, spec §3.3).
    pub(super) fn on_detail_actions(
        &self,
        root: gpui::Stateful<gpui::Div>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        use gpui::InteractiveElement as _;
        root.on_action(
            cx.listener(|this, _: &EditWorkItemTitle, w, cx| this.intend(Intent::Title, w, cx)),
        )
        .on_action(
            cx.listener(|this, _: &ChangeWorkItemState, w, cx| this.intend(Intent::State, w, cx)),
        )
        .on_action(
            cx.listener(|this, _: &AssignWorkItem, w, cx| this.intend(Intent::Assign, w, cx)),
        )
        .on_action(
            cx.listener(|this, _: &AssignWorkItemToMe, w, cx| {
                this.intend(Intent::AssignToMe, w, cx)
            }),
        )
        .on_action(
            cx.listener(|this, _: &UnassignWorkItem, w, cx| this.intend(Intent::Unassign, w, cx)),
        )
        .on_action(
            cx.listener(|this, _: &ChangeWorkItemArea, w, cx| this.intend(Intent::Area, w, cx)),
        )
        .on_action(cx.listener(|this, _: &ChangeWorkItemIteration, w, cx| {
            this.intend(Intent::Iteration, w, cx)
        }))
        .on_action(cx.listener(|this, _: &EditWorkItemDescription, w, cx| {
            this.intend(Intent::Description, w, cx)
        }))
        .on_action(
            cx.listener(|this, _: &AddWorkItemLink, w, cx| {
                this.intend(Intent::AddLink(None), w, cx)
            }),
        )
        .on_action(cx.listener(|this, _: &AddWorkItemParent, w, cx| {
            this.intend(Intent::AddLink(Some(LinkKind::Parent)), w, cx)
        }))
        .on_action(cx.listener(|this, _: &AddWorkItemChild, w, cx| {
            this.intend(Intent::AddLink(Some(LinkKind::Child)), w, cx)
        }))
        .on_action(cx.listener(|this, _: &AddWorkItemRelated, w, cx| {
            this.intend(Intent::AddLink(Some(LinkKind::Related)), w, cx)
        }))
        .on_action(
            cx.listener(|this, _: &RemoveWorkItemLink, w, cx| {
                this.intend(Intent::RemoveLink, w, cx)
            }),
        )
        .on_action(
            cx.listener(|this, _: &CommentOnWorkItem, w, cx| this.intend(Intent::Comment, w, cx)),
        )
        .on_action(cx.listener(|this, _: &SyncWorkItem, w, cx| this.intend(Intent::Sync, w, cx)))
        .on_action(cx.listener(|this, _: &CopyWorkItemLink, _, cx| this.copy(true, cx)))
        .on_action(cx.listener(|this, _: &CopyWorkItemId, _, cx| this.copy(false, cx)))
        .on_action(cx.listener(|this, _: &NextWorkItemStop, _, cx| this.step_cursor(1, cx)))
        .on_action(cx.listener(|this, _: &PreviousWorkItemStop, _, cx| this.step_cursor(-1, cx)))
        .on_action(cx.listener(|this, _: &ActivateWorkItemStop, w, cx| this.activate_stop(w, cx)))
        .on_action(cx.listener(|this, _: &SaveWorkItemTitle, w, cx| this.save_title(w, cx)))
        .on_action(cx.listener(|this, _: &CancelWorkItemTitle, w, cx| this.close_title(w, cx)))
        .on_action(
            cx.listener(|this, _: &SaveWorkItemDescription, w, cx| this.save_description(w, cx)),
        )
        .on_action(
            cx.listener(|this, _: &CloseWorkItemDescription, w, cx| this.close_description(w, cx)),
        )
        .on_action(cx.listener(|this, _: &ToggleWorkItemPreview, w, cx| this.toggle_preview(w, cx)))
        .on_action(cx.listener(|this, _: &PostWorkItemComment, w, cx| this.post_comment(w, cx)))
        .on_action(cx.listener(|this, _: &LeaveWorkItemComposer, w, cx| this.leave_composer(w, cx)))
        .on_action(cx.listener(|this, _: &MentionNext, _, cx| this.mention_step(1, cx)))
        .on_action(cx.listener(|this, _: &MentionPrevious, _, cx| this.mention_step(-1, cx)))
        .on_action(cx.listener(|this, _: &MentionInsert, w, cx| this.mention_confirm(w, cx)))
        .on_action(cx.listener(|this, _: &MentionClose, _, cx| this.end_mention(cx)))
        .on_action(cx.listener(|this, _: &WorkItemOverlayNext, _, cx| this.step_overlay(1, cx)))
        .on_action(
            cx.listener(|this, _: &WorkItemOverlayPrevious, _, cx| this.step_overlay(-1, cx)),
        )
        .on_action(
            cx.listener(|this, _: &WorkItemOverlayConfirm, w, cx| this.confirm_overlay(w, cx)),
        )
        .on_action(cx.listener(|this, _: &WorkItemOverlayClose, w, cx| this.close_overlay(w, cx)))
        .on_action(cx.listener(|this, _: &ApplyWorkItemRule, w, cx| this.apply_rule(w, cx)))
        .on_action(cx.listener(|this, _: &CancelWorkItemRule, w, cx| this.cancel_rule(w, cx)))
    }

    // ── rule violations (FR-9) ───────────────────────────────────────────

    /// The field and violation of the open rule notice, if any.
    pub(super) fn rule_status(&self, cx: &App) -> Option<(EditField, writes::RuleViolation)> {
        let id = self.detail_id()?;
        let edits = Self::edits(cx)?;
        let edits = edits.read(cx);
        edits
            .statuses(id)
            .find_map(|(field, status)| Some((*field, status.rule()?.clone())))
    }

    fn rule(&self, cx: &App) -> Option<writes::RuleViolation> {
        self.rule_status(cx).map(|(_, rule)| rule)
    }

    fn rule_field(&self, cx: &App) -> Option<EditField> {
        self.rule_status(cx).map(|(field, _)| field)
    }

    pub(super) fn apply_rule_value(&mut self, value: String, cx: &mut Context<Self>) {
        let (Some(edits), Some(id), Some(field)) =
            (Self::edits(cx), self.detail_id(), self.rule_field(cx))
        else {
            return;
        };
        self.ui.rule_input = None;
        edits.update(cx, |edits, cx| edits.resend_with_rule(id, field, value, cx));
    }

    fn apply_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.ui.rule_input.clone() else {
            return;
        };
        let value = input.read(cx).text().trim().to_owned();
        if value.is_empty() {
            return;
        }
        self.apply_rule_value(value, cx);
        window.focus(&self.detail_focus, cx);
    }

    fn cancel_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(edits), Some(id), Some(field)) =
            (Self::edits(cx), self.detail_id(), self.rule_field(cx))
        else {
            return;
        };
        self.ui.rule_input = None;
        edits.update(cx, |edits, cx| edits.dismiss(id, field, cx));
        window.focus(&self.detail_focus, cx);
    }
}

/// A short, stable id per stop, for scrolling it into view.
pub(super) fn stop_key(stop: Stop) -> String {
    match stop {
        Stop::Link(row) => format!("link-{row}"),
        Stop::Comment(ix) => format!("comment-{ix}"),
        Stop::CommentNotice(ix) => format!("comment-notice-{ix}"),
        Stop::Field(field) => format!("field-{field:?}"),
        Stop::FieldNotice(field) => format!("field-notice-{field:?}"),
        Stop::LinkShowMore(group) => format!("more-{group:?}"),
        other => format!("{other:?}"),
    }
}

/// A toast with no action (copies, refusals, offline).
pub(super) fn toast(
    key: &'static str,
    icon: IconName,
    message: impl Into<SharedString>,
    cx: &mut App,
) {
    ToastStack::show(Toast::new(key, icon, message), cx);
}

/// Picker avatars: the people's real avatars (spec 0012).
pub(super) fn avatar_fn() -> adoken_ui::picker::AvatarFn {
    Rc::new(|person, _, _| {
        use gpui::IntoElement as _;
        crate::avatar::Avatar::new(person.id.clone(), person.name.clone())
            .group(person.group)
            .into_any_element()
    })
}

trait AsReady {
    fn as_ready(&self) -> Option<&DetailData>;
}

impl AsReady for Detail {
    fn as_ready(&self) -> Option<&DetailData> {
        match self {
            Detail::Ready(detail) => Some(detail),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_queries_open_after_space_or_at_line_start() {
        assert_eq!(mention_query("@da", 10), Some((10, "da")));
        assert_eq!(mention_query("hi @da", 0), Some((3, "da")));
        assert_eq!(mention_query("hi @", 0), Some((3, "")));
        assert_eq!(mention_query("mail a@b", 0), None, "inside a word");
        assert_eq!(mention_query("@dana ruiz", 0), None, "a space ends it");
        assert_eq!(mention_query("no at", 0), None);
    }

    #[test]
    fn draft_mention_tokens_become_chips() {
        let id = "d2910000-0000-0000-0000-000000000001";
        let (text, atoms) = atoms_of(&format!("@<{id}> please verify"), |_| "Dana Ruiz".into());
        assert_eq!(text, "@Dana Ruiz please verify");
        assert_eq!(atoms.len(), 1);
        assert_eq!(&text[atoms[0].range.clone()], "@Dana Ruiz");
        assert_eq!(atoms[0].id.as_ref(), id);
    }

    fn stack(contexts: &[&str]) -> Vec<gpui::KeyContext> {
        contexts
            .iter()
            .map(|c| gpui::KeyContext::parse(c).expect("context"))
            .collect()
    }

    /// AC-25: letters act from the list and the detail, never while a text
    /// field, picker, menu, overlay or editor has the keyboard.
    #[test]
    fn ac25_letter_keys_act_in_list_and_detail_but_never_while_typing() {
        let keys = gpui::KeyBindingContextPredicate::parse(KEYS_CONTEXT).expect("predicate");
        let base = ["Workspace", VIEW_CONTEXT];
        for pane in [LIST_CONTEXT, DETAIL_CONTEXT] {
            assert!(keys.eval(&stack(&[base[0], base[1], pane])), "{pane}");
        }
        for inner in [
            "TextInput",
            "TextInput TextArea",
            "Picker",
            "ActionMenu",
            OVERLAY_CONTEXT,
            TITLE_EDITOR_CONTEXT,
            DESCRIPTION_EDITOR_CONTEXT,
            COMPOSER_CONTEXT_EDITING,
            COMPOSER_MENTION_CONTEXT,
            RULE_NOTICE_CONTEXT,
        ] {
            assert!(
                !keys.eval(&stack(&[base[0], base[1], DETAIL_CONTEXT, inner])),
                "letters stay inert under {inner}"
            );
        }
        // The description editor's Preview keeps ⌘↩ / ⌘E / Esc, not letters.
        let save = gpui::KeyBindingContextPredicate::parse(DESCRIPTION_CONTEXT).expect("predicate");
        assert!(save.eval(&stack(&[DETAIL_CONTEXT, DESCRIPTION_EDITOR_CONTEXT])));
    }

    /// AC-25 / FR-35: every single-letter key has its `⌘P` command with the
    /// same keys, every FR-35 title exists (minus slice C's Move), and the
    /// cheat sheet lists each binding.
    #[test]
    fn ac25_every_key_is_a_palette_command_and_in_the_cheat_sheet() {
        let commands = palette_commands();
        let all = crate::key_bindings();
        let sheet = crate::help::binding_infos(&all);
        let ours = bindings();
        for info in crate::help::binding_infos(&ours)
            .into_iter()
            .filter(|b| b.context.as_deref() == Some(KEYS_CONTEXT))
        {
            let name = info.action;
            let command = commands
                .iter()
                .find(|c| c.action.name() == name)
                .unwrap_or_else(|| panic!("{name} in ⌘P"));
            assert!(command.keys.is_some(), "{} shows its keys", command.title);
            assert!(
                sheet.iter().any(|b| b.action == name),
                "{name} in the cheat sheet"
            );
        }
        let titles: Vec<&str> = commands.iter().map(|c| c.title).collect();
        for title in [
            "Work item: Edit title",
            "Work item: Change state…",
            "Work item: Assign…",
            "Work item: Assign to me",
            "Work item: Unassign",
            "Work item: Change area…",
            "Work item: Change iteration…",
            "Work item: Edit description",
            "Work item: Add link…",
            "Work item: Remove link…",
            "Work item: Comment",
            "Work item: Sync this work item",
            "Work item: Open in Azure DevOps",
            "Work item: Copy link",
            "Work item: Copy id",
        ] {
            assert!(titles.contains(&title), "{title}");
        }
        assert!(
            !titles.iter().any(|t| t.contains("Move to project")),
            "slice C is v0.2"
        );
    }

    #[test]
    fn reveal_scrolls_as_little_as_possible() {
        use gpui::{Bounds, point, px, size};
        let view = Bounds::new(point(px(0.0), px(100.0)), size(px(500.0), px(400.0)));
        let row = |top: f32| Bounds::new(point(px(0.0), px(top)), size(px(500.0), px(36.0)));
        let margin = px(24.0);
        // Visible: no change.
        assert_eq!(
            reveal_offset(px(0.0), px(1000.0), view, row(200.0), margin),
            px(0.0)
        );
        // Below: scrolls down just enough (offsets are negative).
        assert_eq!(
            reveal_offset(px(0.0), px(1000.0), view, row(600.0), margin),
            px(-160.0)
        );
        // Above: scrolls back up, never past the top.
        assert_eq!(
            reveal_offset(px(-50.0), px(1000.0), view, row(90.0), margin),
            px(-16.0)
        );
        assert_eq!(
            reveal_offset(px(-5.0), px(1000.0), view, row(98.0), margin),
            px(0.0)
        );
    }
}
