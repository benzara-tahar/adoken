//! Avatar stories (spec 0012 FR-13): generated pictures for demo people with
//! an image URL, initials for the rest, groups always initials. No photos.

use std::sync::LazyLock;

use adoken_app::avatar::{self, Avatar, AvatarSize};
use adoken_model::Person;
use adoken_theme::{space, typography};
use adoken_ui::{ActiveTheme, TypeStyleExt};
use gpui::{AnyElement, App, Window, div, prelude::*, px};

static PEOPLE: LazyLock<Vec<Person>> =
    LazyLock::new(|| adoken_demo::dataset(adoken_demo::DEFAULT_SEED).people);

/// Every story sees the demo org's pictures, whatever ran before it (the
/// snapshot harness shares one app across stories).
pub fn init(cx: &mut App) {
    avatar::init(cx);
    avatar::use_demo(avatar::demo_ids(&PEOPLE), cx);
}

fn pick(picture: bool, group: bool) -> &'static Person {
    PEOPLE
        .iter()
        .find(|p| p.is_group == group && (group || p.image_url.is_some() == picture))
        .expect("demo org has people with and without pictures, and groups")
}

pub fn avatar_story(_window: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    let loaded = pick(true, false);
    let initials = pick(false, false);
    let group = pick(false, true);
    let caption = |text: &'static str| {
        div()
            .w(px(96.0))
            .text_color(theme.colors.text_tertiary)
            .type_style(typography::LABEL, theme)
            .child(text)
    };
    let cell = |avatar: Avatar| div().w(px(96.0)).child(avatar);
    let sizes = [
        ("sm · 16", AvatarSize::Sm),
        ("md · 20", AvatarSize::Md),
        ("lg · 28", AvatarSize::Lg),
    ];
    let grid = sizes.into_iter().map(|(label, size)| {
        div()
            .flex()
            .items_center()
            .gap(px(space::S16))
            .child(caption(label))
            .child(cell(Avatar::person(loaded).size(size)))
            .child(cell(Avatar::person(initials).size(size)))
            .child(cell(Avatar::person(group).size(size)))
    });
    // The same row with a picture and with initials: the name starts at the
    // same x and the row keeps its height (no layout shift, AC-13).
    let pair = [loaded, initials].map(|person| {
        div()
            .flex()
            .items_center()
            .gap(px(space::S8))
            .h(px(28.0))
            .child(Avatar::person(person))
            .child(
                div()
                    .text_color(theme.colors.text_primary)
                    .type_style(typography::BODY, theme)
                    .child(person.display_name.clone()),
            )
    });
    div()
        .flex()
        .flex_col()
        .gap(px(space::S12))
        .child(
            div()
                .flex()
                .gap(px(space::S16))
                .child(caption(""))
                .child(caption("picture"))
                .child(caption("initials"))
                .child(caption("group")),
        )
        .children(grid)
        .child(div().flex().flex_col().gap(px(space::S4)).children(pair))
        .into_any_element()
}
