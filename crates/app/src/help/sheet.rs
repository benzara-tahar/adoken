//! The cheat sheet's model, generated (never hand-maintained): every
//! registered keybinding, grouped by the screen it acts on (spec 0025 FR-7),
//! titled by the palette command for the same action, else the action's doc
//! comment, else its humanized name. Palette commands without a shown
//! binding are listed too ("Command palette"): every action is reachable
//! (§7).
//!
//! A binding's screen comes from its key context through [`CONTEXTS`], a
//! fixed map that also hides contexts that only drive a widget's internals
//! (text editing, pickers, menus, dialogs). A context missing from the map
//! fails a unit test, so a new view can't silently add a raw-context group.
//!
//! Pure: callers snapshot gpui's keymap into [`BindingInfo`]s.

use std::collections::{HashMap, HashSet};

use gpui::KeyBindingContextPredicate;

/// Group for palette commands that have no shown key binding.
pub const PALETTE_GROUP: &str = "Command palette";

/// The sheet's groups, in display order (FR-7); [`PALETTE_GROUP`] follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Everywhere,
    Today,
    Inbox,
    PullRequests,
    PullRequestReview,
    WorkItems,
    Pipelines,
    Search,
    Pins,
    Activity,
    Settings,
}

impl Screen {
    /// Display order.
    pub const ALL: [Screen; 11] = [
        Screen::Everywhere,
        Screen::Today,
        Screen::Inbox,
        Screen::PullRequests,
        Screen::PullRequestReview,
        Screen::WorkItems,
        Screen::Pipelines,
        Screen::Search,
        Screen::Pins,
        Screen::Activity,
        Screen::Settings,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Screen::Everywhere => "Everywhere",
            Screen::Today => "Today",
            Screen::Inbox => "Inbox",
            Screen::PullRequests => "Pull requests",
            Screen::PullRequestReview => "Pull request review",
            Screen::WorkItems => "Work items",
            Screen::Pipelines => "Pipelines",
            Screen::Search => "Search",
            Screen::Pins => "Pins",
            Screen::Activity => "Activity",
            Screen::Settings => "Settings",
        }
    }
}

/// Where a key context's bindings show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Shown(Screen),
    /// Widget internals: never listed.
    Hidden,
}

/// Every key context adoken declares (`key_context`) or binds in, and where
/// its bindings show (FR-7). Add new contexts here.
pub const CONTEXTS: &[(&str, Placement)] = &[
    // Everywhere: the shell.
    ("Workspace", Placement::Shown(Screen::Everywhere)),
    ("Today", Placement::Shown(Screen::Today)),
    ("Inbox", Placement::Shown(Screen::Inbox)),
    // Pull requests: the list.
    ("PrList", Placement::Shown(Screen::PullRequests)),
    // Pull request review: the pane, the review, its complete sheet, the
    // file tree filter, thread composers.
    ("PullRequest", Placement::Shown(Screen::PullRequestReview)),
    ("PrReview", Placement::Shown(Screen::PullRequestReview)),
    ("ReviewSheet", Placement::Shown(Screen::PullRequestReview)),
    (
        "ReviewTreeFilter",
        Placement::Shown(Screen::PullRequestReview),
    ),
    (
        "ReviewComposer",
        Placement::Shown(Screen::PullRequestReview),
    ),
    // Work items: the view, list, detail and its editors.
    ("WorkItems", Placement::Shown(Screen::WorkItems)),
    ("WorkItemList", Placement::Shown(Screen::WorkItems)),
    ("WorkItemDetail", Placement::Shown(Screen::WorkItems)),
    ("WorkItemEditor", Placement::Shown(Screen::WorkItems)),
    ("WorkItemTitleEditor", Placement::Shown(Screen::WorkItems)),
    (
        "WorkItemDescriptionEditor",
        Placement::Shown(Screen::WorkItems),
    ),
    ("WorkItemComposer", Placement::Shown(Screen::WorkItems)),
    ("PipelineList", Placement::Shown(Screen::Pipelines)),
    ("PipelineDetail", Placement::Shown(Screen::Pipelines)),
    // Search: ⌘P.
    ("Palette", Placement::Shown(Screen::Search)),
    // Pins: the sidebar.
    ("Sidebar", Placement::Shown(Screen::Pins)),
    ("ActivityPanel", Placement::Shown(Screen::Activity)),
    ("Settings", Placement::Shown(Screen::Settings)),
    // Text editing.
    ("TextInput", Placement::Hidden),
    ("TextArea", Placement::Hidden),
    // Pickers.
    ("Picker", Placement::Hidden),
    ("RepoPicker", Placement::Hidden),
    ("CodeFontPicker", Placement::Hidden),
    ("ThemePicker", Placement::Hidden),
    ("ReviewWorkItemPicker", Placement::Hidden),
    ("WorkItemOverlay", Placement::Hidden),
    ("WorkItemMention", Placement::Hidden),
    // Menus.
    ("ActionMenu", Placement::Hidden),
    ("InboxMenu", Placement::Hidden),
    ("ReviewVoteMenu", Placement::Hidden),
    ("ReviewStatusMenu", Placement::Hidden),
    // Dialogs, overlays and handles.
    ("ReviewConfirm", Placement::Hidden),
    ("PrReviewEditing", Placement::Hidden),
    ("WorkItemRuleNotice", Placement::Hidden),
    ("ActivitySplitter", Placement::Hidden),
    ("KenFightCard", Placement::Hidden),
    ("CheatSheet", Placement::Hidden),
    ("Onboarding", Placement::Hidden),
];

/// One registered binding, flattened from a gpui `KeyBinding`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingInfo {
    /// `Action::name()`, e.g. `shell::ToggleSidebar`.
    pub action: &'static str,
    /// Display label, e.g. `⌘B` (see `adoken_ui::format_keystroke`).
    pub keys: String,
    /// The binding's context predicate as written (`Workspace && !TextInput`).
    pub context: Option<String>,
}

/// One palette command: its title names the action everywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandInfo {
    pub title: &'static str,
    pub action: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub title: String,
    pub action: &'static str,
    /// Alternatives (`↓`, `J`), in registration order; empty for
    /// palette-only commands.
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub name: &'static str,
    pub entries: Vec<Entry>,
}

/// Builds the sheet: groups in [`Screen::ALL`] order, then
/// [`PALETTE_GROUP`]; rows by first registration; one row per action per
/// group with every key that triggers it. Hidden bindings are dropped.
pub fn build(
    bindings: &[BindingInfo],
    commands: &[CommandInfo],
    docs: impl Fn(&'static str) -> Option<&'static str>,
) -> Vec<Group> {
    let titles: HashMap<&str, &str> = commands
        .iter()
        .rev()
        .map(|command| (command.action, command.title))
        .collect();
    let title = |action: &'static str| -> String {
        titles
            .get(action)
            .map(|title| (*title).to_owned())
            .or_else(|| docs(action).and_then(doc_title))
            .unwrap_or_else(|| humanize_action(action))
    };

    let mut groups: Vec<Group> = Screen::ALL
        .iter()
        .map(|screen| Group {
            name: screen.name(),
            entries: Vec::new(),
        })
        .collect();
    let mut shown: HashSet<&'static str> = HashSet::new();
    for binding in bindings {
        if is_placeholder(binding.action) || binding.keys.is_empty() {
            continue;
        }
        let Placement::Shown(screen) = placement(binding.context.as_deref()) else {
            continue;
        };
        shown.insert(binding.action);
        // `Screen::ALL` is declaration order.
        let group = &mut groups[screen as usize];
        match group
            .entries
            .iter_mut()
            .find(|e| e.action == binding.action)
        {
            Some(entry) => {
                if !entry.keys.contains(&binding.keys) {
                    entry.keys.push(binding.keys.clone());
                }
            }
            None => group.entries.push(Entry {
                title: title(binding.action),
                action: binding.action,
                keys: vec![binding.keys.clone()],
            }),
        }
    }

    let mut palette_only: Vec<Entry> = Vec::new();
    for command in commands {
        if !shown.contains(command.action)
            && !palette_only.iter().any(|e| e.action == command.action)
        {
            palette_only.push(Entry {
                title: command.title.to_owned(),
                action: command.action,
                keys: Vec::new(),
            });
        }
    }
    groups.push(Group {
        name: PALETTE_GROUP,
        entries: palette_only,
    });
    groups.retain(|group| !group.entries.is_empty());
    groups
}

/// Keeps rows where every whitespace-separated term appears (case-folded)
/// in the title, the group name, a key label, or the action name. Groups
/// whose name matches keep all their rows.
pub fn filter(groups: &[Group], query: &str) -> Vec<Group> {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if terms.is_empty() {
        return groups.to_vec();
    }
    groups
        .iter()
        .filter_map(|group| {
            let group_name = group.name.to_lowercase();
            let entries: Vec<Entry> = group
                .entries
                .iter()
                .filter(|entry| {
                    let haystack = format!(
                        "{} {} {} {}",
                        entry.title.to_lowercase(),
                        group_name,
                        entry.keys.join(" ").to_lowercase(),
                        entry.action.to_lowercase(),
                    );
                    terms.iter().all(|term| haystack.contains(term.as_str()))
                })
                .cloned()
                .collect();
            (!entries.is_empty()).then_some(Group {
                name: group.name,
                entries,
            })
        })
        .collect()
}

/// gpui's disabling actions never show.
fn is_placeholder(action: &str) -> bool {
    action.ends_with("::NoAction") || action.ends_with("::Unbind")
}

/// A key context's entry in [`CONTEXTS`]; `None` when unlisted.
pub fn context_placement(context: &str) -> Option<Placement> {
    CONTEXTS
        .iter()
        .find(|(name, _)| *name == context)
        .map(|(_, placement)| *placement)
}

/// Where a binding shows: no predicate → [`Screen::Everywhere`]; otherwise
/// by the contexts it requires (negated contexts and `key == value` tests
/// don't place it). Any hidden or unlisted required context hides it; else
/// the first required context that isn't Everywhere picks the screen.
pub fn placement(predicate: Option<&str>) -> Placement {
    let Some(predicate) = predicate else {
        return Placement::Shown(Screen::Everywhere);
    };
    match KeyBindingContextPredicate::parse(predicate) {
        Ok(predicate) => resolve(&predicate),
        Err(_) => Placement::Hidden,
    }
}

fn resolve(predicate: &KeyBindingContextPredicate) -> Placement {
    match predicate {
        KeyBindingContextPredicate::Identifier(context) => {
            context_placement(context).unwrap_or(Placement::Hidden)
        }
        KeyBindingContextPredicate::Equal(..)
        | KeyBindingContextPredicate::NotEqual(..)
        | KeyBindingContextPredicate::Not(_) => Placement::Shown(Screen::Everywhere),
        KeyBindingContextPredicate::Descendant(a, b)
        | KeyBindingContextPredicate::And(a, b)
        | KeyBindingContextPredicate::Or(a, b) => match (resolve(a), resolve(b)) {
            (Placement::Hidden, _) | (_, Placement::Hidden) => Placement::Hidden,
            (Placement::Shown(Screen::Everywhere), other) => other,
            (first, _) => first,
        },
    }
}

/// `shell::ToggleSidebar` → `Toggle sidebar`.
pub fn humanize_action(action: &str) -> String {
    humanize(action.rsplit("::").next().unwrap_or(action))
}

/// Splits CamelCase into a sentence-case phrase; two-letter words are
/// acronyms (`Pr` → `PR`).
fn humanize(ident: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for ch in ident.chars() {
        if ch == '_' {
            words.push(String::new());
        } else if ch.is_uppercase() || words.is_empty() {
            words.push(ch.to_string());
        } else if let Some(word) = words.last_mut() {
            word.push(ch);
        }
    }
    words
        .into_iter()
        .filter(|w| !w.is_empty())
        .enumerate()
        .map(|(ix, word)| {
            if word.chars().count() == 2 && word.chars().all(char::is_alphabetic) {
                word.to_uppercase()
            } else if ix == 0 {
                word
            } else {
                word.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// First sentence of an action's doc comment, without the final period.
fn doc_title(doc: &str) -> Option<String> {
    let line = doc.lines().map(str::trim).find(|line| !line.is_empty())?;
    let sentence = line.split(". ").next().unwrap_or(line);
    Some(sentence.trim_end_matches('.').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::KeyBinding;

    fn no_docs(_: &str) -> Option<&'static str> {
        None
    }

    fn binding(action: &'static str, keys: &str, context: Option<&str>) -> BindingInfo {
        BindingInfo {
            action,
            keys: keys.to_owned(),
            context: context.map(str::to_owned),
        }
    }

    fn keys(groups: &[Group], group: &str, action: &str) -> Vec<String> {
        groups
            .iter()
            .find(|g| g.name == group)
            .and_then(|g| g.entries.iter().find(|e| e.action == action))
            .map(|e| e.keys.clone())
            .unwrap_or_default()
    }

    fn names(groups: &[Group]) -> Vec<&'static str> {
        groups.iter().map(|g| g.name).collect()
    }

    /// Every context identifier a predicate mentions, negated ones too.
    fn identifiers(predicate: &KeyBindingContextPredicate, out: &mut Vec<String>) {
        match predicate {
            KeyBindingContextPredicate::Identifier(context) => out.push(context.to_string()),
            KeyBindingContextPredicate::Equal(..) | KeyBindingContextPredicate::NotEqual(..) => {}
            KeyBindingContextPredicate::Not(inner) => identifiers(inner, out),
            KeyBindingContextPredicate::Descendant(a, b)
            | KeyBindingContextPredicate::And(a, b)
            | KeyBindingContextPredicate::Or(a, b) => {
                identifiers(a, out);
                identifiers(b, out);
            }
        }
    }

    /// Contexts the bindings mention that [`CONTEXTS`] doesn't list.
    fn unlisted_contexts(bindings: &[BindingInfo]) -> Vec<String> {
        let mut unlisted: Vec<String> = Vec::new();
        for context in bindings.iter().filter_map(|b| b.context.as_deref()) {
            let predicate = KeyBindingContextPredicate::parse(context)
                .unwrap_or_else(|error| panic!("{context}: {error}"));
            let mut found = Vec::new();
            identifiers(&predicate, &mut found);
            for name in found {
                if context_placement(&name).is_none() && !unlisted.contains(&name) {
                    unlisted.push(name);
                }
            }
        }
        unlisted
    }

    fn live_bindings() -> Vec<BindingInfo> {
        crate::help::binding_infos(&crate::key_bindings())
    }

    /// AC-6: every context in the live keymap is mapped or hidden; a new,
    /// unlisted context fails here.
    #[test]
    fn ac6_every_live_context_is_mapped_or_hidden() {
        assert_eq!(unlisted_contexts(&live_bindings()), Vec::<String>::new());
        let with_new_view = [
            binding("shell::ToggleSidebar", "⌘B", Some("Workspace")),
            binding("x::Next", "J", Some("BrandNewView && !TextInput")),
        ];
        assert_eq!(unlisted_contexts(&with_new_view), ["BrandNewView"]);
    }

    #[test]
    fn contexts_are_listed_once() {
        for (ix, (name, _)) in CONTEXTS.iter().enumerate() {
            assert!(
                CONTEXTS[ix + 1..].iter().all(|(other, _)| other != name),
                "{name} listed twice"
            );
        }
    }

    /// AC-6: groups follow FR-7 whatever the registration order.
    #[test]
    fn ac6_groups_appear_in_fr7_order() {
        let bindings = [
            binding("settings::NextTab", "⌃⇥", Some("Settings")),
            binding("activity::Dismiss", "Esc", Some("ActivityPanel")),
            binding("pins::MovePinUp", "⌥⌘↑", Some("Sidebar")),
            binding("palette::Complete", "⇥", Some("Palette")),
            binding("pipelines::Open", "↩", Some("PipelineDetail")),
            binding(
                "workitems::Assign",
                "A",
                Some("(WorkItemList || WorkItemDetail) && !TextInput && !Picker"),
            ),
            binding("pr::NextFile", "J", Some("PrReview")),
            binding("review::SelectNextPr", "↓", Some("PrList")),
            binding("inbox::MarkDone", "E", Some("Inbox")),
            binding("today::SelectNext", "→", Some("Today")),
            binding(
                "help::ToggleShortcuts",
                "?",
                Some("Workspace && !TextInput"),
            ),
        ];
        let commands = [CommandInfo {
            title: "Theme: Choose…",
            action: "theme_picker::OpenThemePicker",
        }];
        let groups = build(&bindings, &commands, no_docs);
        assert_eq!(
            names(&groups),
            [
                "Everywhere",
                "Today",
                "Inbox",
                "Pull requests",
                "Pull request review",
                "Work items",
                "Pipelines",
                "Search",
                "Pins",
                "Activity",
                "Settings",
                "Command palette",
            ]
        );
        let palette = groups.last().expect("palette group");
        assert_eq!(palette.entries[0].title, "Theme: Choose…");
        assert!(palette.entries[0].keys.is_empty());
    }

    /// AC-6 over the real keymap: only screen groups, in FR-7 order, and
    /// never a raw context ("Text input", "Picker", "Workspace").
    #[test]
    fn ac6_live_sheet_has_only_screen_groups() {
        let groups = build(&live_bindings(), &[], no_docs);
        let order: Vec<usize> = groups
            .iter()
            .map(|group| {
                Screen::ALL
                    .iter()
                    .position(|screen| screen.name() == group.name)
                    .unwrap_or_else(|| panic!("{} is not a screen", group.name))
            })
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
        for raw in [
            "Text input",
            "Picker",
            "Workspace",
            "Action menu",
            "Onboarding",
        ] {
            assert!(groups.iter().all(|g| g.name != raw), "{raw} group");
        }
    }

    /// AC-6: a key rebound in `keymap.toml` shows its new key, not the old.
    #[test]
    fn ac6_a_rebound_key_shows_its_new_key() {
        let toml = "[[binding]]\nkeys = \"alt-/\"\naction = \"help::ToggleShortcuts\"\n";
        let rebound = crate::keymap::compile(toml, crate::key_bindings(), |name, _| {
            Err(format!("unknown action {name}"))
        })
        .expect("keymap.toml");
        let groups = build(&crate::help::binding_infos(&rebound), &[], no_docs);
        let label = |keys| {
            crate::help::binding_infos(&[KeyBinding::new(keys, crate::help::ToggleShortcuts, None)])
                .remove(0)
                .keys
        };
        let shown = keys(&groups, Screen::Everywhere.name(), "help::ToggleShortcuts");
        assert!(shown.contains(&label("alt-/")), "{shown:?}");
        assert!(
            !shown.contains(&label(crate::help::TOGGLE_KEYS)),
            "{shown:?}"
        );
    }

    #[test]
    fn placement_follows_required_contexts() {
        let shown = Placement::Shown;
        assert_eq!(placement(None), shown(Screen::Everywhere));
        assert_eq!(placement(Some("Workspace")), shown(Screen::Everywhere));
        assert_eq!(placement(Some("!TextInput")), shown(Screen::Everywhere));
        assert_eq!(
            placement(Some("Workspace && !TextInput")),
            shown(Screen::Everywhere)
        );
        assert_eq!(placement(Some("Workspace && Inbox")), shown(Screen::Inbox));
        assert_eq!(
            placement(Some("PrReview > ReviewTreeFilter")),
            shown(Screen::PullRequestReview)
        );
        assert_eq!(
            placement(Some(
                "(WorkItemList || WorkItemDetail) && !TextInput && !WorkItemEditor"
            )),
            shown(Screen::WorkItems)
        );
        assert_eq!(
            placement(Some("mode == insert && Palette")),
            shown(Screen::Search)
        );
        assert_eq!(placement(Some("TextInput")), Placement::Hidden);
        assert_eq!(
            placement(Some("Onboarding && !TextInput")),
            Placement::Hidden
        );
        assert_eq!(placement(Some("Workspace && Picker")), Placement::Hidden);
        // Unlisted: hidden at runtime (and caught by the test above).
        assert_eq!(placement(Some("BrandNewView")), Placement::Hidden);
    }

    #[test]
    fn alternatives_merge_into_one_row_per_screen() {
        let bindings = [
            binding("today::SelectNext", "↓", Some("Today")),
            binding("today::SelectNext", "J", Some("Today")),
            binding("today::SelectNext", "J", Some("Today")),
            binding("palette::SelectNext", "↓", Some("Palette")),
        ];
        let groups = build(&bindings, &[], no_docs);
        assert_eq!(keys(&groups, "Today", "today::SelectNext"), ["↓", "J"]);
        assert_eq!(keys(&groups, "Search", "palette::SelectNext"), ["↓"]);
        // A bound palette command isn't repeated as palette-only.
        let commands = [CommandInfo {
            title: "Next",
            action: "today::SelectNext",
        }];
        let groups = build(&bindings, &commands, no_docs);
        assert!(groups.iter().all(|g| g.name != PALETTE_GROUP));
    }

    #[test]
    fn hidden_bindings_drop_and_their_commands_stay_reachable() {
        let bindings = [
            binding("text_input::Backspace", "⌫", Some("TextInput")),
            binding("pr::Approve", "1", Some("ReviewVoteMenu")),
            binding("pr::OpenVoteMenu", "⇧V", Some("PrReview")),
        ];
        let commands = [CommandInfo {
            title: "Approve",
            action: "pr::Approve",
        }];
        let groups = build(&bindings, &commands, no_docs);
        assert_eq!(names(&groups), ["Pull request review", PALETTE_GROUP]);
        assert!(
            groups
                .iter()
                .flat_map(|g| &g.entries)
                .all(|e| e.action != "text_input::Backspace")
        );
        assert_eq!(groups[1].entries[0].action, "pr::Approve");
        assert!(groups[1].entries[0].keys.is_empty());
    }

    #[test]
    fn titles_prefer_palette_then_docs_then_name() {
        let bindings = [
            binding("shell::ToggleSidebar", "⌘B", None),
            binding("shell::ToggleSidebar", "⌃⌘S", None),
            binding("inbox::Snooze", "S", Some("Inbox")),
            binding("pr::NextFile", "J", Some("PrReview")),
        ];
        let commands = [CommandInfo {
            title: "Toggle sidebar",
            action: "shell::ToggleSidebar",
        }];
        let docs = HashMap::from([
            (
                "inbox::Snooze",
                "Snooze the selected notification. Pick a time.",
            ),
            ("shell::ToggleSidebar", "Collapse or expand the sidebar."),
        ]);
        let groups = build(&bindings, &commands, |action| docs.get(action).copied());
        let title = |action| {
            groups
                .iter()
                .flat_map(|g| &g.entries)
                .find(|e| e.action == action)
                .map(|e| e.title.clone())
                .expect("row")
        };
        assert_eq!(title("shell::ToggleSidebar"), "Toggle sidebar");
        assert_eq!(title("inbox::Snooze"), "Snooze the selected notification");
        assert_eq!(title("pr::NextFile"), "Next file");
        assert_eq!(
            keys(&groups, "Everywhere", "shell::ToggleSidebar"),
            ["⌘B", "⌃⌘S"]
        );
    }

    #[test]
    fn disabling_actions_and_empty_groups_are_dropped() {
        let bindings = [
            binding("zed::NoAction", "⌘W", None),
            binding("zed::Unbind", "⌘E", Some("Inbox")),
        ];
        assert!(build(&bindings, &[], no_docs).is_empty());
    }

    #[test]
    fn filter_matches_all_terms_across_fields() {
        let bindings = [
            binding("shell::ToggleSidebar", "⌘B", None),
            binding("inbox::MarkDone", "E", Some("Inbox")),
            binding("inbox::Snooze", "S", Some("Inbox")),
        ];
        let groups = build(&bindings, &[], no_docs);
        let rows = |query| -> Vec<&'static str> {
            filter(&groups, query)
                .iter()
                .flat_map(|g| g.entries.iter().map(|e| e.action))
                .collect()
        };
        assert_eq!(rows("sidebar"), ["shell::ToggleSidebar"]);
        assert_eq!(rows("⌘b"), ["shell::ToggleSidebar"]);
        assert_eq!(rows("everywhere"), ["shell::ToggleSidebar"]);
        assert_eq!(rows("inbox"), ["inbox::MarkDone", "inbox::Snooze"]);
        assert_eq!(rows("INBOX snooze"), ["inbox::Snooze"]);
        assert!(rows("inbox sidebar").is_empty());
        assert_eq!(rows("  ").len(), 3);
    }
}
