//! Pure layout and navigation rules of the work item detail
//! (docs/design/work-item-detail.md §2, §4.3, §6.1, §7.1, §8, §10.3): which
//! arrangement a pane width gets, how paths become breadcrumbs, how links
//! group, which rows of the discussion and history show, and the keyboard
//! cursor's stops in visual order. No GPUI types: everything here is unit
//! tested.

use adoken_model::LinkKind;
use adoken_theme::size;

/// Pane-width arrangement (design §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrangement {
    /// Header, property grid, meta line, then sections.
    Single,
    /// Main column + the properties rail on the right.
    Wide,
}

pub fn arrangement(pane_width: f32) -> Arrangement {
    if pane_width >= size::DETAIL_WIDE_MIN {
        Arrangement::Wide
    } else {
        Arrangement::Single
    }
}

/// Narrowest property cell before the grid drops to one column (§2.4).
pub const MIN_CELL: f32 = 240.0;
/// Pane padding left + right (§2.1: `space.24` each side).
pub const PANE_PADDING_X: f32 = 48.0;
/// Column gap of the property grid (§4.1: `space.8`).
pub const GRID_GAP: f32 = 8.0;

/// Property grid columns for a single-column pane (§2.4): two while each
/// cell keeps [`MIN_CELL`] at the current font scale, else one.
pub fn grid_columns(pane_width: f32, font_scale: f32) -> usize {
    let content = pane_width.min(size::DETAIL_PROSE_MAX + PANE_PADDING_X) - PANE_PADDING_X;
    let cell = (content - GRID_GAP) / 2.0;
    if cell >= MIN_CELL * font_scale.max(1.0) {
        2
    } else {
        1
    }
}

/// One piece of a breadcrumb path (§4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Crumb {
    /// An ancestor segment (`text.tertiary`).
    Ancestor(String),
    /// The collapsed middle ("…").
    Ellipsis,
    /// The leaf (`text.primary`).
    Leaf(String),
}

/// `Storefront\Infra\Payments` → crumbs joined by ` › `. When the full path
/// exceeds `max_chars`, keeps the first segment and the leaf and collapses
/// the middle (`Storefront › … › Payments`); the leaf itself ellipsizes in
/// the element if still too long. Never truncates from the start.
pub fn breadcrumb(path: &str, max_chars: usize) -> Vec<Crumb> {
    let segments: Vec<&str> = path.split('\\').filter(|s| !s.is_empty()).collect();
    let Some((leaf, ancestors)) = segments.split_last() else {
        return Vec::new();
    };
    let full_len = segments.iter().map(|s| s.chars().count()).sum::<usize>()
        + SEPARATOR_CHARS * (segments.len() - 1);
    let mut out = Vec::with_capacity(segments.len());
    if full_len <= max_chars || ancestors.len() <= 1 {
        out.extend(ancestors.iter().map(|s| Crumb::Ancestor((*s).to_owned())));
    } else {
        out.push(Crumb::Ancestor(ancestors[0].to_owned()));
        out.push(Crumb::Ellipsis);
    }
    out.push(Crumb::Leaf((*leaf).to_owned()));
    out
}

/// ` › ` is three characters wide.
const SEPARATOR_CHARS: usize = 3;
pub const SEPARATOR: &str = " › ";

/// The path's leaf (`Sprint 41` of `Storefront\Sprint 41`).
pub fn leaf(path: &str) -> &str {
    path.rsplit('\\').next().unwrap_or(path)
}

/// Spoken form (§4.3): "Payments, in Storefront, Infra".
pub fn spoken_path(path: &str) -> String {
    let segments: Vec<&str> = path.split('\\').filter(|s| !s.is_empty()).collect();
    match segments.split_last() {
        None => String::new(),
        Some((leaf, [])) => (*leaf).to_owned(),
        Some((leaf, ancestors)) => format!("{leaf}, in {}", ancestors.join(", ")),
    }
}

/// Title limits (design §3.2, spec FR-12).
pub const TITLE_MAX: usize = 255;
/// The counter shows from this many characters.
pub const TITLE_COUNTER_FROM: usize = 230;
pub const TITLE_EMPTY: &str = "Title can't be empty";

/// Pasted newlines become spaces (§3.2).
pub fn sanitize_title(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// The title to send, or why nothing is sent: `Ok(None)` when unchanged,
/// `Err` when empty (spec FR-12, AC-11).
pub fn title_edit(current: &str, typed: &str) -> Result<Option<String>, &'static str> {
    let typed = sanitize_title(typed);
    let trimmed = typed.trim();
    if trimmed.is_empty() {
        return Err(TITLE_EMPTY);
    }
    if trimmed == current.trim() {
        return Ok(None);
    }
    Ok(Some(trimmed.to_owned()))
}

/// "240/255" once the title reaches [`TITLE_COUNTER_FROM`] characters.
pub fn title_counter(text: &str) -> Option<String> {
    let n = text.chars().count();
    (n >= TITLE_COUNTER_FROM).then(|| format!("{n}/{TITLE_MAX}"))
}

/// Comments shown before "Show {n} earlier comments" (design §7.1, Q-3).
pub const COMMENTS_SHOWN: usize = 5;
/// History rows shown before "Show all {n}" (§8).
pub const HISTORY_SHOWN: usize = 5;
/// Rows a link group shows before "Show {n} more" (§6.1).
pub const LINK_GROUP_SHOWN: usize = 5;

/// First visible comment index (oldest first): the last five unless
/// expanded.
pub fn first_visible_comment(total: usize, expanded: bool) -> usize {
    if expanded {
        0
    } else {
        total.saturating_sub(COMMENTS_SHOWN)
    }
}

/// History rows to show (newest first).
pub fn visible_history(total: usize, expanded: bool) -> usize {
    if expanded {
        total
    } else {
        total.min(HISTORY_SHOWN)
    }
}

/// Link groups in display order (spec FR-23, design §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkGroup {
    Parent,
    Children,
    Related,
    Predecessors,
    Successors,
    Duplicates,
    DuplicateOf,
    PullRequests,
    Commits,
    Other,
}

impl LinkGroup {
    pub fn label(self) -> &'static str {
        match self {
            LinkGroup::Parent => "Parent",
            LinkGroup::Children => "Children",
            LinkGroup::Related => "Related",
            LinkGroup::Predecessors => "Predecessors",
            LinkGroup::Successors => "Successors",
            LinkGroup::Duplicates => "Duplicates",
            LinkGroup::DuplicateOf => "Duplicate of",
            LinkGroup::PullRequests => "Pull requests",
            LinkGroup::Commits => "Commits",
            LinkGroup::Other => "Other links",
        }
    }

    /// Singular relation name for a row's spoken label ("Child: …").
    pub fn singular(self) -> &'static str {
        match self {
            LinkGroup::Parent => "Parent",
            LinkGroup::Children => "Child",
            LinkGroup::Related => "Related",
            LinkGroup::Predecessors => "Predecessor",
            LinkGroup::Successors => "Successor",
            LinkGroup::Duplicates => "Duplicate",
            LinkGroup::DuplicateOf => "Duplicate of",
            LinkGroup::PullRequests => "Pull request",
            LinkGroup::Commits => "Commit",
            LinkGroup::Other => "Link",
        }
    }

    /// Work item relations are editable (spec FR-22); PRs, commits and
    /// other artifacts are read-only.
    pub fn is_work_item(self) -> bool {
        self < LinkGroup::PullRequests
    }

    /// Group of a stored relation.
    pub fn of_kind(kind: LinkKind) -> LinkGroup {
        match kind {
            LinkKind::Parent => LinkGroup::Parent,
            LinkKind::Child => LinkGroup::Children,
            LinkKind::Related => LinkGroup::Related,
            LinkKind::Predecessor => LinkGroup::Predecessors,
            LinkKind::Successor => LinkGroup::Successors,
            LinkKind::Duplicate => LinkGroup::Duplicates,
            LinkKind::DuplicateOf => LinkGroup::DuplicateOf,
        }
    }

    /// Group of a read-only relation (`ArtifactLink`, `Hyperlink`, …):
    /// pull requests, commits, then everything else.
    pub fn of_artifact(is_pr: bool, url: &str) -> LinkGroup {
        if is_pr {
            LinkGroup::PullRequests
        } else if url.to_ascii_lowercase().contains("/git/commit/") {
            LinkGroup::Commits
        } else {
            LinkGroup::Other
        }
    }
}

/// The Add link picker's title (design §6.2).
pub fn picker_title(kind: LinkKind, id: i64, has_parent: bool) -> String {
    match kind {
        LinkKind::Parent if has_parent => format!("Change the parent of #{id}"),
        LinkKind::Parent => format!("Set the parent of #{id}"),
        LinkKind::Child => format!("Add a child to #{id}"),
        LinkKind::Related => format!("Link a related item to #{id}"),
        LinkKind::Predecessor => format!("Add a predecessor of #{id}"),
        LinkKind::Successor => format!("Add a successor of #{id}"),
        LinkKind::Duplicate => format!("Mark an item as a duplicate of #{id}"),
        LinkKind::DuplicateOf => format!("Mark #{id} as a duplicate of…"),
    }
}

/// Why `target` can't be picked for a `kind` link from `item`, shown
/// right-aligned and disabled in the picker (design §6.2): the item
/// itself, already linked this way, or a child offered as parent.
pub fn refusal(
    kind: LinkKind,
    item: i64,
    target: i64,
    existing: &[(LinkGroup, i64)],
) -> Option<String> {
    if target == item {
        return Some("This item".into());
    }
    let group = LinkGroup::of_kind(kind);
    if existing.iter().any(|&(g, id)| g == group && id == target) {
        return Some(match kind {
            LinkKind::Parent => "Already the parent".into(),
            LinkKind::Child => "Already a child".into(),
            _ => format!("Already {}", group.singular().to_lowercase()),
        });
    }
    if kind == LinkKind::Parent
        && existing
            .iter()
            .any(|&(g, id)| g == LinkGroup::Children && id == target)
    {
        return Some("A child of this item".into());
    }
    None
}

/// A keyboard cursor stop in visual order (design §10.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stop {
    NotAvailable,
    Title,
    TitleNotice,
    State,
    StateNotice,
    Field(Field),
    FieldNotice(Field),
    Description,
    DescriptionNotice,
    DescriptionShowAll,
    /// A link row, by index into the detail's link rows.
    Link(usize),
    /// "Show {n} more" in a link group.
    LinkShowMore(LinkGroup),
    AddLink,
    EarlierComments,
    /// A comment, by index into the thread (oldest first).
    Comment(usize),
    /// The notice under a failed pending comment.
    CommentNotice(usize),
    Composer,
    HistoryShowAll,
}

/// The four editable properties (design §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    AssignedTo,
    Project,
    Area,
    Iteration,
}

impl Field {
    pub const GRID: [Field; 4] = [
        Field::AssignedTo,
        Field::Project,
        Field::Area,
        Field::Iteration,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::AssignedTo => "Assigned to",
            Field::Project => "Project",
            Field::Area => "Area",
            Field::Iteration => "Iteration",
        }
    }
}

/// FR-36: a property cell's accessible label, "Assigned to, Ada
/// Lindqvist. Change, A"; read-only cells drop the action.
pub fn cell_label(field: Field, value: &str, editable: bool) -> String {
    let keys = match field {
        Field::AssignedTo => Some("A"),
        Field::Area => Some("Shift A"),
        Field::Iteration => Some("Shift I"),
        // Moving to another project is slice C (v0.2).
        Field::Project => None,
    };
    match keys.filter(|_| editable) {
        Some(keys) => format!("{}, {value}. Change, {keys}", field.label()),
        None => format!("{}, {value}", field.label()),
    }
}

/// The cell's tooltip (design §4.1).
pub fn cell_tooltip(field: Field) -> &'static str {
    match field {
        Field::AssignedTo => "Change assignee  a",
        Field::Area => "Change area  ⇧A",
        Field::Iteration => "Change iteration  ⇧I",
        Field::Project => "Project",
    }
}

/// FR-36: the state chip's label, "State: Doing, in progress. Change
/// state, S"; read-only it's just "State: Doing".
pub fn state_label(state: &str, category: &str, editable: bool) -> String {
    if editable {
        format!("State: {state}, {category}. Change state, S")
    } else {
        format!("State: {state}")
    }
}

/// What the stop walk needs to know about the pane.
#[derive(Debug, Clone, Default)]
pub struct StopInput {
    pub not_available: bool,
    pub title_notice: bool,
    pub state_notice: bool,
    pub read_only: bool,
    pub field_notices: Vec<Field>,
    pub description_notice: bool,
    pub description_show_all: bool,
    /// Work item, PR, commit and other link rows shown (in order).
    pub link_rows: usize,
    /// Groups with a "Show more" row, and the row index it follows.
    pub link_show_more: Vec<(usize, LinkGroup)>,
    pub can_add_link: bool,
    pub earlier_comments: bool,
    /// Indices of the shown comments (oldest first) and whether each has
    /// a notice (a failed post).
    pub comments: Vec<(usize, bool)>,
    pub composer: bool,
    pub history_show_all: bool,
}

/// Every actionable stop in visual order (design §10.3). Project is a
/// stop too: it shows its tooltip and value, and slice C will edit it.
pub fn stops(input: &StopInput) -> Vec<Stop> {
    let mut out = Vec::new();
    if input.not_available {
        out.push(Stop::NotAvailable);
    }
    out.push(Stop::Title);
    if input.title_notice {
        out.push(Stop::TitleNotice);
    }
    out.push(Stop::State);
    if input.state_notice {
        out.push(Stop::StateNotice);
    }
    for field in Field::GRID {
        out.push(Stop::Field(field));
        if input.field_notices.contains(&field) {
            out.push(Stop::FieldNotice(field));
        }
    }
    out.push(Stop::Description);
    if input.description_notice {
        out.push(Stop::DescriptionNotice);
    }
    if input.description_show_all {
        out.push(Stop::DescriptionShowAll);
    }
    for row in 0..input.link_rows {
        out.push(Stop::Link(row));
        out.extend(
            input
                .link_show_more
                .iter()
                .filter(|(after, _)| *after == row)
                .map(|(_, group)| Stop::LinkShowMore(*group)),
        );
    }
    if input.can_add_link {
        out.push(Stop::AddLink);
    }
    if input.earlier_comments {
        out.push(Stop::EarlierComments);
    }
    for &(ix, notice) in &input.comments {
        out.push(Stop::Comment(ix));
        if notice {
            out.push(Stop::CommentNotice(ix));
        }
    }
    if input.composer {
        out.push(Stop::Composer);
    }
    if input.history_show_all {
        out.push(Stop::HistoryShowAll);
    }
    out
}

/// The cursor after moving `delta` stops from `current` (clamped, no
/// wrap). A cursor on a stop that vanished restarts at the title.
pub fn step_stop(stops: &[Stop], current: Option<Stop>, delta: isize) -> Option<Stop> {
    if stops.is_empty() {
        return None;
    }
    let ix = current.and_then(|c| stops.iter().position(|s| *s == c));
    let next = match ix {
        Some(ix) => (ix as isize + delta).clamp(0, stops.len() as isize - 1) as usize,
        None => stops.iter().position(|s| *s == Stop::Title).unwrap_or(0),
    };
    Some(stops[next])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rail_appears_at_the_wide_min_pane_width() {
        assert_eq!(arrangement(999.0), Arrangement::Single);
        assert_eq!(arrangement(size::DETAIL_WIDE_MIN), Arrangement::Wide);
        assert_eq!(arrangement(1208.0), Arrangement::Wide);
    }

    #[test]
    fn grid_keeps_two_columns_down_to_240_px_cells() {
        // 900 px window with sidebar: pane 668 → cells (620 - 8) / 2 = 306.
        assert_eq!(grid_columns(668.0, 1.0), 2);
        // Exactly 240 px cells: 48 + 8 + 480.
        assert_eq!(grid_columns(536.0, 1.0), 2);
        assert_eq!(grid_columns(535.0, 1.0), 1);
        // Text scale 1.3 at 668: 306 < 312.
        assert_eq!(grid_columns(668.0, 1.3), 1);
    }

    #[test]
    fn long_paths_keep_root_and_leaf_and_collapse_the_middle() {
        assert_eq!(
            breadcrumb("Storefront\\Infra", 40),
            vec![
                Crumb::Ancestor("Storefront".into()),
                Crumb::Leaf("Infra".into())
            ]
        );
        assert_eq!(
            breadcrumb("Storefront\\Infra\\Platform\\Payments", 30),
            vec![
                Crumb::Ancestor("Storefront".into()),
                Crumb::Ellipsis,
                Crumb::Leaf("Payments".into())
            ]
        );
        // Two segments never collapse: the leaf ellipsizes instead.
        assert_eq!(breadcrumb("A very long project\\Leaf", 5).len(), 2);
        assert_eq!(breadcrumb("", 10), Vec::new());
    }

    #[test]
    fn paths_are_spoken_with_in() {
        assert_eq!(
            spoken_path("Storefront\\Infra\\Payments"),
            "Payments, in Storefront, Infra"
        );
        assert_eq!(spoken_path("Storefront"), "Storefront");
    }

    #[test]
    fn empty_titles_are_refused_and_unchanged_ones_send_nothing() {
        assert_eq!(title_edit("Fix it", "   "), Err(TITLE_EMPTY));
        assert_eq!(title_edit("Fix it", "Fix it "), Ok(None));
        assert_eq!(
            title_edit("Fix it", "Fix\nit now"),
            Ok(Some("Fix it now".into()))
        );
    }

    #[test]
    fn title_counter_starts_at_230_characters() {
        assert_eq!(title_counter(&"x".repeat(229)), None);
        assert_eq!(title_counter(&"é".repeat(240)), Some("240/255".into()));
    }

    #[test]
    fn discussion_shows_the_last_five_unless_expanded() {
        assert_eq!(first_visible_comment(3, false), 0);
        assert_eq!(first_visible_comment(7, false), 2);
        assert_eq!(first_visible_comment(7, true), 0);
        assert_eq!(visible_history(16, false), 5);
        assert_eq!(visible_history(16, true), 16);
    }

    #[test]
    fn relations_group_in_spec_order() {
        let mut groups: Vec<LinkGroup> = [
            LinkKind::Related,
            LinkKind::Child,
            LinkKind::DuplicateOf,
            LinkKind::Parent,
        ]
        .into_iter()
        .map(LinkGroup::of_kind)
        .collect();
        groups.push(LinkGroup::of_artifact(
            false,
            "vstfs:///Git/Commit/abc%2Fdef%2F3f9c",
        ));
        groups.push(LinkGroup::of_artifact(true, "vstfs:///Git/PullRequestId/x"));
        groups.push(LinkGroup::of_artifact(false, "https://example.com"));
        groups.sort();
        assert_eq!(
            groups,
            vec![
                LinkGroup::Parent,
                LinkGroup::Children,
                LinkGroup::Related,
                LinkGroup::DuplicateOf,
                LinkGroup::PullRequests,
                LinkGroup::Commits,
                LinkGroup::Other,
            ]
        );
        assert!(LinkGroup::DuplicateOf.is_work_item());
        assert!(!LinkGroup::Commits.is_work_item());
        for kind in LinkKind::ALL {
            assert!(LinkGroup::of_kind(kind).is_work_item());
        }
    }

    #[test]
    fn link_picker_refuses_self_existing_and_child_as_parent() {
        let existing = [(LinkGroup::Children, 7), (LinkGroup::Related, 9)];
        let why = |kind, target| refusal(kind, 42, target, &existing);
        assert_eq!(why(LinkKind::Related, 42).as_deref(), Some("This item"));
        assert_eq!(why(LinkKind::Child, 7).as_deref(), Some("Already a child"));
        assert_eq!(
            why(LinkKind::Related, 9).as_deref(),
            Some("Already related")
        );
        assert_eq!(
            why(LinkKind::Parent, 7).as_deref(),
            Some("A child of this item")
        );
        assert_eq!(why(LinkKind::Related, 7), None);
    }

    #[test]
    fn change_parent_picker_says_so() {
        assert_eq!(
            picker_title(LinkKind::Parent, 20101, false),
            "Set the parent of #20101"
        );
        assert_eq!(
            picker_title(LinkKind::Parent, 20101, true),
            "Change the parent of #20101"
        );
    }

    #[test]
    fn stops_follow_visual_order_with_notices_after_their_element() {
        let input = StopInput {
            state_notice: true,
            field_notices: vec![Field::Area],
            link_rows: 2,
            link_show_more: vec![(1, LinkGroup::Children)],
            can_add_link: true,
            earlier_comments: true,
            comments: vec![(3, false), (4, true)],
            composer: true,
            ..StopInput::default()
        };
        assert_eq!(
            stops(&input),
            vec![
                Stop::Title,
                Stop::State,
                Stop::StateNotice,
                Stop::Field(Field::AssignedTo),
                Stop::Field(Field::Project),
                Stop::Field(Field::Area),
                Stop::FieldNotice(Field::Area),
                Stop::Field(Field::Iteration),
                Stop::Description,
                Stop::Link(0),
                Stop::Link(1),
                Stop::LinkShowMore(LinkGroup::Children),
                Stop::AddLink,
                Stop::EarlierComments,
                Stop::Comment(3),
                Stop::Comment(4),
                Stop::CommentNotice(4),
                Stop::Composer,
            ]
        );
    }

    /// Spec 0001 AC-19: the read-only hint is not a keyboard stop.
    #[test]
    fn read_only_pane_has_no_token_stop_and_no_editors() {
        let walk = stops(&StopInput {
            read_only: true,
            not_available: true,
            ..StopInput::default()
        });
        assert_eq!(walk[0], Stop::NotAvailable);
        assert!(!walk.contains(&Stop::AddLink));
        assert!(!walk.contains(&Stop::Composer));
    }

    #[test]
    fn cursor_clamps_and_restarts_at_the_title_when_its_stop_vanished() {
        let walk = stops(&StopInput::default());
        assert_eq!(step_stop(&walk, Some(Stop::Title), -1), Some(Stop::Title));
        assert_eq!(step_stop(&walk, Some(Stop::Title), 1), Some(Stop::State));
        assert_eq!(
            step_stop(&walk, Some(Stop::Composer), 1),
            Some(Stop::Title),
            "a stop that's gone restarts at the title"
        );
        let last = *walk.last().unwrap();
        assert_eq!(step_stop(&walk, Some(last), 5), Some(last));
    }

    /// AC-26 / FR-36: field, value and shortcut in each label; read-only
    /// values read as values, and Project (slice C) offers no action.
    #[test]
    fn ac26_labels_name_field_value_and_shortcut() {
        assert_eq!(
            cell_label(Field::AssignedTo, "Ada Lindqvist", true),
            "Assigned to, Ada Lindqvist. Change, A"
        );
        assert_eq!(
            cell_label(Field::Area, &spoken_path("Storefront\\Infra"), true),
            "Area, Infra, in Storefront. Change, Shift A"
        );
        assert_eq!(
            cell_label(Field::Iteration, "Sprint 41", false),
            "Iteration, Sprint 41"
        );
        assert_eq!(
            cell_label(Field::Project, "Storefront", true),
            "Project, Storefront"
        );
        assert_eq!(
            state_label("Doing", "in progress", true),
            "State: Doing, in progress. Change state, S"
        );
        assert_eq!(state_label("Doing", "", false), "State: Doing");
    }
}
