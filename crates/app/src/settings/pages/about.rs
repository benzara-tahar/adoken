//! Keyboard (read-only bindings), Ken (embedded), Account and About.

use adoken_theme::{radius, space, typography};
use adoken_ui::{
    Button, ButtonVariant, ChipTone, ContainScroll, IconName, Kbd, Notice, Theme, TypeStyleExt,
};
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};

use super::{compact_row, row, section, wide};
use crate::palette::PaletteCommand;
use crate::settings::view::{Account, SettingsView};

/// Bundled with the binary so it reads offline.
const THIRD_PARTY_NOTICES: &str = include_str!("../../../../../THIRD_PARTY_NOTICES.md");
const LICENSES_MAX_HEIGHT: f32 = 320.0;

pub type KeyGroup = (&'static str, Vec<(SharedString, &'static str)>);

/// Every command with a shortcut, grouped by area; a command listed by an
/// earlier group isn't repeated.
pub fn key_groups(demo: bool) -> Vec<KeyGroup> {
    let settings_window = vec![
        ("Next tab".into(), crate::settings::NEXT_TAB_KEYS),
        ("Previous tab".into(), crate::settings::PREVIOUS_TAB_KEYS),
        ("Search settings".into(), crate::settings::SEARCH_KEYS),
        ("Close Settings".into(), crate::settings::CLOSE_KEYS),
    ];
    let sources: [(&'static str, Vec<PaletteCommand>); 5] = [
        ("Today", crate::today::commands()),
        ("Inbox", crate::inbox::commands()),
        ("Pull requests", crate::review::palette_commands()),
        ("Settings", crate::settings::commands()),
        ("General", crate::palette_commands(demo)),
    ];
    let mut seen = std::collections::HashSet::new();
    let mut groups: Vec<KeyGroup> = sources
        .into_iter()
        .map(|(name, commands)| {
            let rows = commands
                .into_iter()
                .filter_map(|command| Some((command.title, command.keys?)))
                .filter(|(title, _)| seen.insert(*title))
                .map(|(title, keys)| (SharedString::from(title), keys))
                .collect::<Vec<_>>();
            (name, rows)
        })
        .collect();
    if let Some((_, rows)) = groups.iter_mut().find(|(name, _)| *name == "Settings") {
        rows.extend(settings_window);
    }
    groups.retain(|(_, rows)| !rows.is_empty());
    groups
}

impl SettingsView {
    pub(super) fn keyboard(&self, theme: &Theme) -> Vec<Option<AnyElement>> {
        key_groups(self.demo)
            .into_iter()
            .map(|(name, keys)| {
                let rows = keys
                    .into_iter()
                    .filter(|(title, _)| {
                        self.visible(title, &format!("{name} keyboard shortcuts keys bindings"))
                    })
                    .filter_map(|(title, keys)| {
                        Kbd::parse(keys)
                            .ok()
                            .map(|kbd| compact_row(theme, title, kbd))
                    })
                    .collect();
                section(theme, name, None, rows)
            })
            .collect()
    }

    pub(super) fn ken(&self, theme: &Theme) -> Vec<Option<AnyElement>> {
        if !self.visible("Arcade mode", "ken combo ko streak playful") {
            return Vec::new();
        }
        let embed = self.embeds.ken.clone().map(|ken| wide(div().child(ken)));
        vec![section(
            theme,
            "Arcade mode",
            Some(
                "Playful touches: review combos, merge K.O.s. Off by default; respects Reduce Motion.",
            ),
            embed.into_iter().collect(),
        )]
    }

    pub(super) fn account(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<Option<AnyElement>> {
        if !self.visible(
            "Account",
            "account token pat scope permission access expiry expires created replace replaced automatically sign out organization demo",
        ) {
            return Vec::new();
        }
        let mut rows = Vec::new();
        match self.account.clone() {
            Account::Demo => {
                rows.push(row(
                    theme,
                    "Demo org · fake data",
                    Some("Contoso: fake data generated on this Mac. Your changes stay local; nothing is sent to Azure DevOps and there is no account to sign out of.".into()),
                    div(),
                ));
            }
            Account::Live => match self.account_snapshot(cx) {
                None => rows.push(row(
                    theme,
                    "Not connected",
                    Some("Connect Azure DevOps from ⌘P › Connect Azure DevOps…".into()),
                    div(),
                )),
                Some(snapshot) => self.account_rows(snapshot, theme, cx, &mut rows),
            },
            Account::ConfirmSignOut { org } => rows.push(row(
                theme,
                format!("Sign out of {org}?"),
                Some("adoken starts at Welcome next time. Nothing changes in Azure DevOps.".into()),
                div()
                    .flex()
                    .gap(px(space::S8))
                    .child(
                        Button::new("account-cancel", "Cancel")
                            .variant(ButtonVariant::Ghost)
                            .on_press(self.press(cx, |this, _, cx| this.load_account_again(cx))),
                    )
                    .child(
                        Button::new("account-confirm", "Sign out")
                            .variant(ButtonVariant::Primary)
                            .on_press(self.press(cx, |this, _, cx| this.sign_out(cx))),
                    ),
            )),
            Account::SigningOut => rows.push(row(theme, "Signing out…", None, div())),
            Account::SignedOut => rows.push(row(
                theme,
                "Signed out",
                Some("Quit adoken to finish; it starts at Welcome next time.".into()),
                Button::new("account-quit", "Quit adoken").on_press(|_, cx| cx.quit()),
            )),
            Account::Failed(error) => rows.push(wide(
                Notice::new(ChipTone::Danger, "Couldn't sign out")
                    .icon(IconName::CircleAlert)
                    .detail(error),
            )),
        }
        vec![section(theme, "Account", None, rows)]
    }

    /// Spec 0009 FR-8/FR-9, design §10.1: organization, Token, Access,
    /// Permissions, Sign out. Read from memory only.
    fn account_rows(
        &mut self,
        snapshot: crate::account::Snapshot,
        theme: &Theme,
        cx: &mut Context<Self>,
        rows: &mut Vec<AnyElement>,
    ) {
        use crate::account::ReplaceState;
        let colors = &theme.colors;
        let meta = snapshot.meta;
        let org = meta.org.clone();
        let avatar = meta.me.as_ref().map(|me| {
            crate::avatar::Avatar::new(me.id.clone(), me.display_name.clone())
                .size(crate::avatar::AvatarSize::Lg)
        });
        rows.push(row(
            theme,
            org.clone(),
            meta.me
                .as_ref()
                .map(|me| format!("Signed in as {}", me.display_name).into()),
            div().children(avatar),
        ));

        // Token: how it was made, created and expiry, or the FR-15 state.
        let replacing = snapshot.replace == ReplaceState::Replacing;
        let mut replace = Button::new(
            "account-replace",
            if replacing {
                "Replacing…"
            } else {
                "Replace token…"
            },
        )
        .loading(replacing)
        .disabled(replacing)
        .on_press(self.press(cx, |_, _, cx| crate::account::replace_token(cx)));
        if snapshot.problem.is_some() {
            replace = replace.variant(ButtonVariant::Primary);
        }
        let token_line = crate::account::token_line(&meta);
        rows.push(row(theme, "Token", Some(token_line.into()), replace));
        if let Some(problem) = snapshot.problem {
            rows.push(wide(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(space::S2))
                    .child(
                        div()
                            .text_color(colors.status_danger_fg)
                            .type_style(typography::SMALL, theme)
                            .child(problem.account_line()),
                    )
                    .when(problem == crate::account::TokenProblem::Rejected, |col| {
                        col.child(
                            div()
                                .text_color(colors.text_tertiary)
                                .type_style(typography::SMALL, theme)
                                .child("Signed out of Azure DevOps for a while? Sign in in your browser, then replace the token."),
                        )
                    }),
            ));
        }
        match snapshot.replace {
            ReplaceState::Done { note, token_page } => rows.push(wide(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .text_color(colors.text_secondary)
                    .type_style(typography::SMALL, theme)
                    .child(note)
                    .children(token_page.map(|url| {
                        Button::new("account-token-page", "Open token page")
                            .variant(ButtonVariant::Ghost)
                            .icon(IconName::ExternalLink)
                            .on_press(move |_, cx| cx.open_url(&url))
                    })),
            )),
            ReplaceState::Failed(cause) => rows.push(wide(
                Notice::new(ChipTone::Danger, "Couldn't replace the token")
                    .icon(IconName::CircleAlert)
                    .detail(cause),
            )),
            ReplaceState::Idle | ReplaceState::Replacing => {}
        }

        rows.push(row(
            theme,
            "Access",
            Some(format!("{} · {}", meta.level.label(), meta.level.summary()).into()),
            Button::new("account-change-access", "Change access…")
                .on_press(self.press(cx, |_, _, cx| crate::account::change_access(cx))),
        ));

        // Permissions (design §10.1): one block under the title, the missing
        // line above the §4 rows, each with its status at the right.
        let scope_rows = adoken_ado::scopes(meta.level).iter().map(|scope| {
            let status = meta.scope_status(scope.id);
            let (icon, color) = match status {
                adoken_ado::ScopeStatus::Granted => {
                    (IconName::CircleCheck, colors.status_success_fg)
                }
                adoken_ado::ScopeStatus::NotCheckedYet => {
                    (IconName::CircleDashed, colors.text_tertiary)
                }
                adoken_ado::ScopeStatus::Missing => (IconName::CircleX, colors.status_warning_fg),
            };
            let missing = (status == adoken_ado::ScopeStatus::Missing)
                .then_some(scope.if_missing)
                .flatten()
                .map(|text| {
                    div()
                        .text_color(colors.status_warning_fg)
                        .type_style(typography::SMALL, theme)
                        .child(text)
                        .into_any_element()
                });
            div()
                .flex()
                .items_start()
                .gap(px(space::S16))
                .child(div().flex_1().min_w_0().child(crate::onboarding::scope_row(
                    scope, None, missing, false, theme,
                )))
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(space::S4))
                        .pt(px(space::S6))
                        .text_color(color)
                        .type_style(typography::SMALL, theme)
                        .child(adoken_ui::Icon::new(icon).color(color))
                        .child(status.label()),
                )
        });
        rows.push(wide(
            div()
                .flex()
                .flex_col()
                .gap(px(space::S8))
                .child(
                    div()
                        .text_color(colors.text_primary)
                        .type_style(typography::BODY, theme)
                        .child("Permissions"),
                )
                .children(crate::account::missing_line(&meta).map(|line| {
                    div()
                        .text_color(colors.text_secondary)
                        .type_style(typography::SMALL, theme)
                        .child(line)
                }))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(space::S2))
                        .children(scope_rows),
                ),
        ));

        rows.push(row(
            theme,
            "Sign out",
            Some(
                "Removes the token from the Keychain and your setup progress. Synced data stays."
                    .into(),
            ),
            Button::new("account-sign-out", "Sign out…")
                .icon(IconName::LogOut)
                .on_press(self.press(cx, move |this, _, cx| {
                    this.account = Account::ConfirmSignOut { org: org.clone() };
                    cx.notify();
                })),
        ));
    }

    pub(super) fn about(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<Option<AnyElement>> {
        if !self.visible("About", "version licenses third party credits") {
            return Vec::new();
        }
        let colors = &theme.colors;
        let mut rows = vec![
            row(
                theme,
                "adoken",
                Some("Local-first Azure DevOps client. 拳".into()),
                div()
                    .text_color(colors.text_secondary)
                    .type_style(typography::CODE_INLINE, theme)
                    .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
            ),
            row(
                theme,
                "Third-party licenses",
                Some("Code and assets adoken builds on, with their licenses.".into()),
                Button::new(
                    "about-licenses",
                    if self.licenses_open {
                        "Hide"
                    } else {
                        "Show licenses"
                    },
                )
                .variant(ButtonVariant::Secondary)
                .on_press(self.press(cx, |this, _, cx| {
                    this.licenses_open = !this.licenses_open;
                    cx.notify();
                })),
            ),
        ];
        if self.licenses_open {
            rows.push(wide(
                div()
                    .id("about-licenses-text")
                    .max_h(px(LICENSES_MAX_HEIGHT))
                    .overflow_y_scroll()
                    .contain_scroll()
                    .p(px(space::S12))
                    .rounded(px(radius::SM))
                    .bg(colors.bg_code)
                    .text_color(colors.text_secondary)
                    .type_style(typography::CODE, theme)
                    .child(THIRD_PARTY_NOTICES),
            ));
        }
        vec![section(theme, "About", None, rows)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_groups_list_each_command_once_and_parse() {
        let groups = key_groups(false);
        let mut titles = std::collections::HashSet::new();
        for (name, rows) in &groups {
            assert!(!rows.is_empty(), "{name}");
            for (title, keys) in rows {
                assert!(titles.insert(title.clone()), "{title} listed twice");
                assert!(Kbd::parse(keys).is_ok(), "{title}: {keys}");
            }
        }
        let settings = groups
            .iter()
            .find(|(name, _)| *name == "Settings")
            .expect("a Settings group");
        assert!(
            settings
                .1
                .iter()
                .any(|(_, keys)| *keys == crate::settings::OPEN_KEYS)
        );
    }
}
