//! The sidebar (principles.md §2): sections with their counts, then the
//! settings gear and the org. ⌘B (alias ⌘\) collapses it to an icon rail
//! (tokens.md `size.rail`) where labels become tooltips and counts become
//! badges on the icons. The width animates over `motion.base` (reduce
//! motion snaps it); the state persists as `settings.toml [sidebar]`.

use adoken_theme::{motion, radius, size, space, typography};
use adoken_ui::motion::{EASE_IN_OUT, animation, lerp};
use adoken_ui::{Icon, IconName, Kbd, Theme, Tooltip, TypeStyleExt};
use gpui::{AnimationExt, AnyElement, Context, Hsla, SharedString, div, prelude::*, px};

use super::{Nav, Shell};
use crate::avatar::Avatar;

/// `size.sidebar` (tokens.md §5.2).
pub const SIDEBAR_WIDTH: f32 = 232.0;
/// `size.rail`: collapsed sidebar.
pub const RAIL_WIDTH: f32 = 56.0;
/// Icons sit this far from the row's left edge in both modes, so they
/// don't move while the width animates: (rail − 2 × padding − icon) / 2.
const ICON_INSET: f32 = (RAIL_WIDTH - 2.0 * space::S8 - size::ICON_MD) / 2.0;
/// Rail badge pill height (`type.label` line height).
const BADGE_HEIGHT: f32 = 14.0;
/// Counts above this read "9+" on the rail (the expanded row shows the number).
const RAIL_BADGE_MAX: usize = 9;

/// `settings.toml` table.
pub const SETTINGS_TABLE: &str = "sidebar";

/// `[sidebar]` in `settings.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SidebarSettings {
    pub collapsed: bool,
    pub pinned_collapsed: bool,
}

impl SidebarSettings {
    /// Reads `[sidebar]` from the whole file's text; anything missing or
    /// malformed means the default (expanded).
    pub fn from_toml(text: &str) -> Self {
        let root = toml::from_str::<toml::Table>(text).unwrap_or_default();
        let table = root.get(SETTINGS_TABLE).and_then(toml::Value::as_table);
        let flag = |key| {
            table
                .and_then(|table| table.get(key))
                .and_then(toml::Value::as_bool)
                .unwrap_or(false)
        };
        Self {
            collapsed: flag("collapsed"),
            pinned_collapsed: flag("pinned_collapsed"),
        }
    }

    /// The `[sidebar]` table to write.
    pub fn to_table(self) -> toml::Table {
        let mut table = toml::Table::new();
        table.insert("collapsed".into(), toml::Value::Boolean(self.collapsed));
        table.insert(
            "pinned_collapsed".into(),
            toml::Value::Boolean(self.pinned_collapsed),
        );
        table
    }
}

pub fn width(collapsed: bool) -> f32 {
    if collapsed { RAIL_WIDTH } else { SIDEBAR_WIDTH }
}

/// A rail badge's text: the count up to 9, then "9+"; none for zero.
pub fn rail_badge(count: usize) -> Option<String> {
    match count {
        0 => None,
        n if n <= RAIL_BADGE_MAX => Some(n.to_string()),
        _ => Some(format!("{RAIL_BADGE_MAX}+")),
    }
}

/// "Inbox ⌘2": the rail tooltip names the section and its shortcut.
fn tooltip_text(label: &str, keys: Option<&str>) -> SharedString {
    match keys.and_then(|keys| Kbd::parse(keys).ok()) {
        Some(kbd) => format!("{label}  {}", kbd.label()).into(),
        None => label.to_owned().into(),
    }
}

impl Shell {
    pub(super) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        self.sidebar_toggles += 1;
        self.persist_sidebar(cx);
        cx.notify();
    }

    fn sidebar_row(
        &self,
        id: impl Into<gpui::ElementId>,
        theme: &Theme,
        tooltip: SharedString,
    ) -> gpui::Stateful<gpui::Div> {
        let colors = &theme.colors;
        div()
            .id(id)
            .relative()
            .flex_none()
            .h(px(size::CONTROL_MD))
            .pl(px(ICON_INSET))
            .pr(px(space::S8))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .overflow_hidden()
            .rounded(px(radius::SM))
            .cursor_pointer()
            .when(self.sidebar_collapsed, |row| {
                row.tooltip(Tooltip::text(tooltip))
            })
            .hover(|row| row.bg(colors.bg_hover))
    }

    pub(super) fn rail_icon(
        icon: IconName,
        color: Hsla,
        badge: Option<String>,
        theme: &Theme,
    ) -> gpui::Div {
        let colors = &theme.colors;
        div()
            .relative()
            .flex_none()
            .size(px(size::ICON_MD))
            .child(Icon::new(icon).size(size::ICON_MD).color(color))
            .children(badge.map(|badge| {
                div()
                    .absolute()
                    // Straddles the icon's top-right corner, inside the row.
                    .top(px(space::S2 - BADGE_HEIGHT / 2.0))
                    .left(px(size::ICON_MD - BADGE_HEIGHT / 2.0 - space::S2))
                    .h(px(BADGE_HEIGHT))
                    .min_w(px(BADGE_HEIGHT))
                    .px(px(space::S2))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius::FULL))
                    .bg(colors.accent_solid)
                    .text_color(colors.accent_on)
                    .type_style(typography::LABEL, theme)
                    .child(badge)
            }))
    }

    pub(super) fn render_sidebar(
        &self,
        theme: &Theme,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = &theme.colors;
        let collapsed = self.sidebar_collapsed;
        let items = Nav::ALL.into_iter().enumerate().map(|(index, nav)| {
            let selected = nav == self.nav;
            let count = self.nav_count(nav);
            let icon_color = if selected {
                colors.accent_fg
            } else {
                colors.text_tertiary
            };
            self.sidebar_row(nav.id(), theme, tooltip_text(nav.label(), Some(nav.keys())))
                .text_color(if selected {
                    colors.text_primary
                } else {
                    colors.text_secondary
                })
                .when(selected, |row| row.bg(colors.bg_selected))
                .when(
                    self.pin_state.focused && self.pin_state.cursor == index,
                    |row| row.child(adoken_ui::focus_ring(theme)),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.pin_state.cursor =
                        Nav::ALL.iter().position(|item| *item == nav).unwrap_or(0);
                    this.show(nav, window, cx);
                    if nav == Nav::Search {
                        this.toggle_palette(window, cx);
                    }
                }))
                .child(Self::rail_icon(
                    nav.icon(),
                    icon_color,
                    count.filter(|_| collapsed).and_then(rail_badge),
                    theme,
                ))
                .when(!collapsed, |row| {
                    row.child(div().flex_1().min_w_0().truncate().child(nav.label()))
                        .children(count.map(|n| {
                            div()
                                .flex_none()
                                .text_color(colors.text_secondary)
                                .type_style(typography::LABEL, theme)
                                .child(n.to_string())
                        }))
                })
        });

        let toggle_label = if collapsed {
            "Expand sidebar"
        } else {
            "Collapse sidebar"
        };
        let toggle = self
            .sidebar_row(
                "sidebar-toggle",
                theme,
                tooltip_text(toggle_label, Some(super::TOGGLE_SIDEBAR_KEYS)),
            )
            .text_color(colors.text_secondary)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)))
            .child(Self::rail_icon(
                if collapsed {
                    IconName::PanelLeftOpen
                } else {
                    IconName::PanelLeftClose
                },
                colors.text_tertiary,
                None,
                theme,
            ))
            .when(!collapsed, |row| {
                row.child(div().flex_1().min_w_0().truncate().child(toggle_label))
                    .child(Kbd::parse(super::TOGGLE_SIDEBAR_KEYS).expect("static binding"))
            });

        let settings = self
            .sidebar_row(
                "sidebar-settings",
                theme,
                tooltip_text("Settings", Some(crate::settings::OPEN_KEYS)),
            )
            .text_color(colors.text_secondary)
            .on_click(|_, window, cx| {
                window.dispatch_action(Box::new(crate::settings::OpenSettings), cx)
            })
            .child(Self::rail_icon(
                IconName::Settings,
                colors.text_tertiary,
                None,
                theme,
            ))
            .when(!collapsed, |row| {
                row.child(div().flex_1().min_w_0().truncate().child("Settings"))
                    .child(Kbd::parse(crate::settings::OPEN_KEYS).expect("static binding"))
            });

        let org_name = self.snapshot.org.clone();
        let me = self.snapshot.me.as_ref();
        let org = org_name.map(|name| {
            let caption = if self.source.is_demo() {
                format!("{name} · demo org")
            } else {
                name.clone()
            };
            // The signed-in user's picture (spec 0012 Q-2): orgs have none.
            let (avatar, tooltip) = match me {
                Some(me) => (
                    Avatar::person(me),
                    format!("{} · {caption}", me.display_name),
                ),
                None => (Avatar::new("", name.clone()), caption.clone()),
            };
            div()
                .id("sidebar-org")
                .flex_none()
                .h(px(size::CONTROL_LG))
                // The 20 px avatar centers on the same axis as the 16 px icons.
                .pl(px(ICON_INSET - (size::AVATAR_MD - size::ICON_MD) / 2.0))
                .pr(px(space::S8))
                .flex()
                .items_center()
                .gap(px(space::S8))
                .overflow_hidden()
                .when(collapsed, |row| row.tooltip(Tooltip::text(tooltip)))
                .child(avatar)
                .when(!collapsed, |row| {
                    row.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text_secondary)
                            .type_style(typography::SMALL, theme)
                            .child(caption),
                    )
                })
        });

        let target = width(collapsed);
        let panel = div()
            .id("sidebar")
            .key_context(crate::pins::SIDEBAR_CONTEXT)
            .track_focus(&self.pin_state.focus)
            .on_key_down(
                cx.listener(|shell, event, window, cx| shell.sidebar_key(event, window, cx)),
            )
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .gap(px(space::S2))
            .p(px(space::S8))
            .overflow_hidden()
            .bg(colors.bg_sidebar)
            .border_r_1()
            .border_color(colors.border_subtle)
            .type_style(typography::BODY, theme)
            .children(items)
            .child(self.render_live_pins(window, cx))
            .child(div().flex_1())
            .child(toggle)
            .child(settings)
            .children(org);
        // Only a user toggle animates; the persisted state paints at rest.
        if self.sidebar_toggles == 0 {
            return panel.w(px(target)).into_any_element();
        }
        let from = width(!collapsed);
        panel
            .with_animation(
                ("sidebar-width", self.sidebar_toggles),
                animation(motion::BASE, EASE_IN_OUT),
                move |panel, t| panel.w(px(lerp(from, target, t))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes `[sidebar]` the way `settings::write_table` does.
    fn persist(path: &std::path::Path, settings: SidebarSettings) {
        adoken_settings::update(path, |doc| {
            adoken_settings::set_table(doc, SETTINGS_TABLE, &settings.to_table())
        })
        .expect("writes settings.toml");
    }

    fn reload(path: &std::path::Path) -> SidebarSettings {
        SidebarSettings::from_toml(&std::fs::read_to_string(path).expect("reads settings.toml"))
    }

    #[test]
    fn collapsed_state_survives_a_restart_without_touching_other_tables() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "# my notes\n[sound]\nenabled = true\n").expect("seed");
        assert_eq!(reload(&path), SidebarSettings::default());

        persist(
            &path,
            SidebarSettings {
                collapsed: true,
                pinned_collapsed: true,
            },
        );
        assert!(reload(&path).collapsed);
        let text = std::fs::read_to_string(&path).expect("reads");
        assert!(text.contains("# my notes") && text.contains("enabled = true"));

        persist(
            &path,
            SidebarSettings {
                collapsed: false,
                pinned_collapsed: false,
            },
        );
        assert!(!reload(&path).collapsed);
    }

    #[test]
    fn missing_or_broken_settings_mean_expanded() {
        for text in [
            "",
            "[sound]\nenabled = true\n",
            "[sidebar]\ncollapsed = \"yes\"\n",
            "sidebar = 3\n",
            "[sidebar\ncollapsed = true",
        ] {
            assert!(!SidebarSettings::from_toml(text).collapsed, "{text:?}");
        }
        assert!(SidebarSettings::from_toml("[sidebar]\ncollapsed = true\n").collapsed);
    }

    #[test]
    fn rail_badges_cap_at_nine() {
        assert_eq!(rail_badge(0), None);
        assert_eq!(rail_badge(1).as_deref(), Some("1"));
        assert_eq!(rail_badge(9).as_deref(), Some("9"));
        assert_eq!(rail_badge(10).as_deref(), Some("9+"));
        assert_eq!(rail_badge(1200).as_deref(), Some("9+"));
    }

    #[test]
    fn rail_keeps_icons_where_the_expanded_sidebar_has_them() {
        // Row inset is shared, so the icon's center is the rail's center.
        let icon_center = space::S8 + ICON_INSET + size::ICON_MD / 2.0;
        assert_eq!(icon_center, RAIL_WIDTH / 2.0);
        assert!(width(true) < width(false));
    }
}
