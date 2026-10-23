//! Work item editing pickers (spec 0015 FR-11/13/14/21; design §3.3, §4.5,
//! §6.2, §7.3), each in its meaningful states side by side. Pickers paint on
//! their own anchored layer, so every frame is placed at window coordinates.

use std::rc::Rc;
use std::sync::LazyLock;

use adoken_app::avatar::Avatar;
use adoken_theme::{size, space, typography};
use adoken_ui::picker::{
    DayRange, Directory, LinkTypeMenu, PeopleMode, PeoplePicker, Person, Placement, StateCategory,
    StateMenu, StateOption, TreeKind, TreeNode, TreePicker,
};
use adoken_ui::{ActiveTheme, TypeStyleExt};
use gpui::{AnyElement, App, Entity, Window, div, point, prelude::*, px};

static PEOPLE: LazyLock<Vec<adoken_model::Person>> =
    LazyLock::new(|| adoken_demo::dataset(adoken_demo::DEFAULT_SEED).people);

/// Columns: one frame each, captioned.
const TOP: f32 = 40.0;
const GAP: f32 = 24.0;

fn left(column: usize, width: f32) -> f32 {
    space::S24 + column as f32 * (width + GAP)
}

fn at(column: usize, width: f32) -> Placement {
    Placement::at(
        point(px(left(column, width)), px(TOP)),
        gpui::Anchor::TopLeft,
        width,
    )
    .keyboard()
}

/// The story backdrop with a caption over each column.
fn stage(captions: &[&'static str], width: f32, pickers: Vec<AnyElement>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .relative()
        .size_full()
        .bg(theme.colors.bg_surface)
        .children(captions.iter().enumerate().map(|(column, caption)| {
            div()
                .absolute()
                .left(px(left(column, width)))
                .top(px(space::S12))
                .text_color(theme.colors.text_tertiary)
                .type_style(typography::LABEL, theme)
                .child(*caption)
        }))
        .children(pickers)
        .into_any_element()
}

fn agile_bug() -> Vec<StateOption> {
    use StateCategory::*;
    vec![
        StateOption::new("New", Proposed),
        StateOption::new("Doing", InProgress),
        StateOption::new("Code review", InProgress),
        StateOption::new("Resolved", Resolved),
        StateOption::new("Closed", Completed),
        StateOption::new("Removed", Removed),
    ]
}

pub fn state_menu(window: &mut Window, cx: &mut App) -> AnyElement {
    let width = size::ACTION_MENU_WIDTH;
    let menu = |key: &'static str,
                column: usize,
                query: &'static str,
                loading: bool,
                window: &mut Window,
                cx: &mut App|
     -> Entity<StateMenu> {
        window.use_keyed_state(key, cx, move |window, cx| {
            let states = (!loading).then(agile_bug);
            let mut menu = StateMenu::new(
                "Change state",
                "Doing",
                states,
                at(column, width),
                window,
                cx,
            );
            if loading {
                menu.show_loading_now(cx);
            }
            menu.set_query(query, cx);
            menu
        })
    };
    let menus = vec![
        menu("state-menu-open", 0, "", false, window, cx).into_any_element(),
        menu("state-menu-filtered", 1, "res", false, window, cx).into_any_element(),
        menu("state-menu-empty", 2, "zz", false, window, cx).into_any_element(),
        menu("state-menu-loading", 3, "", true, window, cx).into_any_element(),
    ];
    stage(
        &[
            "Open (current: Doing)",
            "Typed \u{201c}res\u{201d}",
            "No match",
            "Loading",
        ],
        width,
        menus,
        cx,
    )
}

pub fn link_type_menu(window: &mut Window, cx: &mut App) -> AnyElement {
    let width = size::ACTION_MENU_WIDTH;
    let menus = [false, true]
        .into_iter()
        .enumerate()
        .map(|(column, has_parent)| {
            let key = if has_parent {
                "link-type-has-parent"
            } else {
                "link-type-no-parent"
            };
            window
                .use_keyed_state(key, cx, move |window, cx| {
                    LinkTypeMenu::new(has_parent, at(column, width), window, cx)
                })
                .into_any_element()
        })
        .collect();
    stage(&["No parent yet", "Has a parent"], width, menus, cx)
}

fn person(p: &adoken_model::Person) -> Person {
    Person::new(
        p.id.clone(),
        p.display_name.clone(),
        p.unique_name.clone().unwrap_or_default(),
    )
    .group(p.is_group)
}

fn named(name: &str) -> Person {
    PEOPLE
        .iter()
        .find(|p| p.display_name == name)
        .map(person)
        .unwrap_or_else(|| Person::new("", name, ""))
}

fn avatar() -> adoken_ui::picker::AvatarFn {
    Rc::new(|p: &Person, _: &mut Window, _: &mut App| {
        Avatar::new(p.id.clone(), p.name.clone())
            .group(p.group)
            .into_any_element()
    })
}

pub fn people_picker(window: &mut Window, cx: &mut App) -> AnyElement {
    let width = size::PICKER_POPOVER;
    let picker = |key: &'static str,
                  column: usize,
                  mode: PeopleMode,
                  query: &'static str,
                  directory: Directory,
                  window: &mut Window,
                  cx: &mut App|
     -> Entity<PeoplePicker> {
        window.use_keyed_state(key, cx, move |window, cx| {
            let mut picker = PeoplePicker::new(mode, at(column, width), window, cx);
            picker.set_avatar(avatar());
            picker.set_people(PEOPLE.iter().map(person).collect(), cx);
            picker.set_recent(vec![named("Yara Haddad"), named("Mei Tanaka")], cx);
            picker.set_current(Some(named("Mei Tanaka").id), cx);
            picker.set_query(query, cx);
            picker.set_directory(directory, cx);
            picker
        })
    };
    let me = named("Ada Lindqvist");
    let found = vec![
        Person::new("dir-1", "Omar Haddad", "omar.haddad@contoso.com"),
        Person::new("dir-2", "Haddie Moss", "haddie.moss@contoso.com"),
    ];
    let pickers = vec![
        picker(
            "people-open",
            0,
            PeopleMode::Assign { me: me.clone() },
            "",
            Directory::Off,
            window,
            cx,
        )
        .into_any_element(),
        picker(
            "people-directory",
            1,
            PeopleMode::Assign { me: me.clone() },
            "hadd",
            Directory::Results(found),
            window,
            cx,
        )
        .into_any_element(),
        picker(
            "people-searching",
            2,
            PeopleMode::Assign { me: me.clone() },
            "zzz",
            Directory::Searching,
            window,
            cx,
        )
        .into_any_element(),
        picker(
            "people-failed",
            3,
            PeopleMode::Assign { me },
            "qqq",
            Directory::Failed,
            window,
            cx,
        )
        .into_any_element(),
        picker(
            "people-mention",
            4,
            PeopleMode::Mention,
            "an",
            Directory::Off,
            window,
            cx,
        )
        .into_any_element(),
    ];
    stage(
        &[
            "Open",
            "Typed \u{201c}hadd\u{201d}, directory results",
            "Searching your directory",
            "Directory failed",
            "@mention (groups included)",
        ],
        width,
        pickers,
        cx,
    )
}

/// Areas of the demo project, with the item in Infra\Payments.
fn area_nodes() -> Vec<TreeNode> {
    adoken_ui::picker::nodes_from_paths(&[
        "Storefront",
        "Storefront\\Infra",
        "Storefront\\Infra\\Payments",
        "Storefront\\Infra\\Search",
        "Storefront\\Infra\\Settlement",
        "Storefront\\Web",
        "Storefront\\Web\\Checkout",
        "Storefront\\Web\\Catalog",
        "Storefront\\Mobile",
        "Storefront\\Payments Gateway",
    ])
}

pub fn area_picker(window: &mut Window, cx: &mut App) -> AnyElement {
    let width = size::PICKER_POPOVER;
    let picker = |key: &'static str,
                  column: usize,
                  query: &'static str,
                  loading: bool,
                  window: &mut Window,
                  cx: &mut App|
     -> Entity<TreePicker> {
        window.use_keyed_state(key, cx, move |window, cx| {
            let nodes = (!loading).then(area_nodes);
            let mut picker = TreePicker::new(
                TreeKind::Area,
                "Change area",
                nodes,
                Some("Storefront\\Infra\\Payments".into()),
                at(column, width),
                window,
                cx,
            );
            if loading {
                picker.show_loading_now(cx);
            }
            picker.set_query(query, cx);
            picker
        })
    };
    let pickers = vec![
        picker("area-open", 0, "", false, window, cx).into_any_element(),
        picker("area-filtered", 1, "pay", false, window, cx).into_any_element(),
        picker("area-empty", 2, "zzz", false, window, cx).into_any_element(),
        picker("area-loading", 3, "", true, window, cx).into_any_element(),
    ];
    stage(
        &[
            "Expanded to current",
            "Typed \u{201c}pay\u{201d}",
            "No match",
            "Loading",
        ],
        width,
        pickers,
        cx,
    )
}

/// 2026-10-01, the demo's today, as days since 1970-01-01.
const TODAY: i64 = 20_727;

fn iteration_nodes() -> Vec<TreeNode> {
    let mut nodes = vec![
        TreeNode::new("Storefront", None),
        TreeNode::new("2026 H2", Some(0)),
    ];
    // Two-week sprints; Sprint 44 holds today.
    for (n, start) in [
        (41, 20_682),
        (42, 20_696),
        (43, 20_710),
        (44, 20_724),
        (45, 20_738),
        (46, 20_752),
    ] {
        nodes.push(
            TreeNode::new(format!("Sprint {n}"), Some(1)).dates(DayRange {
                start,
                finish: start + 11,
            }),
        );
    }
    nodes.push(TreeNode::new("Backlog", Some(0)));
    nodes
}

pub fn iteration_picker(window: &mut Window, cx: &mut App) -> AnyElement {
    let width = size::PICKER_POPOVER;
    let picker = window.use_keyed_state("iteration-open", cx, move |window, cx| {
        TreePicker::new(
            TreeKind::Iteration { today: TODAY },
            "Change iteration",
            Some(iteration_nodes()),
            Some("Storefront\\2026 H2\\Sprint 44".into()),
            at(0, width),
            window,
            cx,
        )
    });
    stage(
        &["Current / Next / Recent above the tree"],
        width,
        vec![picker.into_any_element()],
        cx,
    )
}
