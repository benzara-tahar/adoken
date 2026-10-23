//! Onboarding screens (docs/design/onboarding.md §2–§4, motion §6–§7).

use adoken_ado::{AccessLevel, AzStatus, ConnectPath, MIN_AZ_VERSION, ProbeOutcome, Scope, scopes};
use adoken_theme::{elevation, motion as durations, radius, size, space, typography};
use adoken_ui::motion::{EASE_OUT, animation, delayed, lerp};
use adoken_ui::{
    ActiveTheme, Button, ButtonSize, ButtonVariant, Checkbox, Chip, ChipTone, ContainScroll, Icon,
    IconName, Kbd, ListRow, Notice, OptionCard, Segmented, Skeleton, Spinner, Status, StatusIcon,
    StepIndicator, Theme, TypeStyleExt,
};
use gpui::{
    AnimationExt, AnyElement, Context, Div, ElementId, Entity, IntoElement, MouseButton, Render,
    SharedString, Window, div, prelude::*, px,
};
use time::OffsetDateTime;

use super::flow::{
    self, Direction, Failure, LIFETIMES, Load, Minting, Screen, SignIn, Verifying, summarize,
};
use super::*;

/// Design §2 card geometry.
const CARD_WIDTH: f32 = 560.0;
const CARD_MIN_HEIGHT: f32 = 420.0;
/// Below this window size the card goes full-bleed (design §2).
const FULL_BLEED_BELOW: (f32, f32) = (720.0, 560.0);
/// `bg.overlay` at 0.96 over the onboarding background (design §2).
const CARD_ALPHA: f32 = 0.96;
/// The welcome illustration's box (design §3 screen 0): laid out from the
/// first frame so nothing moves when the decode lands (FR-19, AC-26).
const WELCOME_IMAGE: (f32, f32) = (360.0, 240.0);
/// Step slide distance (design §6).
const SLIDE_X: f32 = 24.0;
/// Rows visible in the org/project lists before they scroll.
const LIST_ROWS: f32 = 5.5;

const INSTALL_AZ_URL: &str = "https://learn.microsoft.com/cli/azure/install-azure-cli";
const UPDATE_AZ_URL: &str = "https://learn.microsoft.com/cli/azure/update-azure-cli";
const TENANT_HINT: &str = "az login --tenant <tenant-id>";

/// One screen's content, laid into the shared card.
struct Page {
    headline: SharedString,
    subline: Option<SharedString>,
    body: Vec<AnyElement>,
    secondary: Vec<AnyElement>,
    primary: Option<AnyElement>,
}

impl Page {
    fn new(headline: impl Into<SharedString>) -> Self {
        Self {
            headline: headline.into(),
            subline: None,
            body: Vec::new(),
            secondary: Vec::new(),
            primary: None,
        }
    }

    fn subline(mut self, subline: impl Into<SharedString>) -> Self {
        self.subline = Some(subline.into());
        self
    }

    fn body(mut self, element: impl IntoElement) -> Self {
        self.body.push(element.into_any_element());
        self
    }

    fn secondary(mut self, element: impl IntoElement) -> Self {
        self.secondary.push(element.into_any_element());
        self
    }

    fn primary(mut self, element: impl IntoElement) -> Self {
        self.primary = Some(element.into_any_element());
        self
    }
}

fn primary_button(id: &'static str, label: impl Into<SharedString>) -> Button {
    Button::new(id, label)
        .variant(ButtonVariant::Primary)
        .size(ButtonSize::Lg)
        .shortcut(Kbd::parse("enter").expect("static binding"))
}

fn confirm_button(id: &'static str, label: impl Into<SharedString>) -> Button {
    primary_button(id, label).on_press(|window, cx| window.dispatch_action(Box::new(Confirm), cx))
}

fn link(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id, label).variant(ButtonVariant::Ghost)
}

/// Design 2b: `30 days · 90 days · 180 days · 1 year`.
fn lifetime_label(days: u32) -> String {
    match days {
        365 => "1 year".to_owned(),
        days => format!("{days} days"),
    }
}

fn small(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_color(theme.colors.text_secondary)
        .type_style(typography::SMALL, theme)
        .child(text.into())
}

fn tertiary(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_color(theme.colors.text_tertiary)
        .type_style(typography::SMALL, theme)
        .child(text.into())
}

fn row() -> Div {
    div().flex().items_center().gap(px(space::S8))
}

fn column() -> Div {
    div().flex().flex_col().gap(px(space::S8))
}

/// `fade-in`: opacity 0→1 and translate Y `dy`→0 after `delay_ms`. Static
/// when `animate` is false (reduce motion is handled by gpui itself).
fn fade_in(
    element: Div,
    id: impl Into<ElementId>,
    delay_ms: u32,
    duration_ms: u32,
    dy: f32,
    animate: bool,
) -> AnyElement {
    if !animate {
        return element.into_any_element();
    }
    element
        .relative()
        .with_animations(
            id,
            delayed(delay_ms, animation(duration_ms, EASE_OUT)),
            move |element, ix, t| {
                let t = if ix == 0 { 0.0 } else { t };
                element.opacity(t).top(px(lerp(dy, 0.0, t)))
            },
        )
        .into_any_element()
}

/// Error shake: X 0→−4→4→−4→0 over 240 ms (design §6). gpui snaps it to
/// rest under reduce motion.
fn shake_offset(t: f32) -> f32 {
    const KEYS: [f32; 5] = [0.0, -4.0, 4.0, -4.0, 0.0];
    let pos = t.clamp(0.0, 1.0) * (KEYS.len() - 1) as f32;
    let ix = (pos.floor() as usize).min(KEYS.len() - 2);
    lerp(KEYS[ix], KEYS[ix + 1], pos - ix as f32)
}

/// "5m ago" style age for project rows.
pub(super) fn relative_age(then: OffsetDateTime, now: OffsetDateTime) -> String {
    let secs = (now - then).whole_seconds().max(0);
    let (value, unit) = match secs {
        s if s < 60 => return "just now".into(),
        s if s < 3_600 => (s / 60, "m"),
        s if s < 86_400 => (s / 3_600, "h"),
        s if s < 30 * 86_400 => (s / 86_400, "d"),
        s if s < 365 * 86_400 => (s / (30 * 86_400), "mo"),
        s => (s / (365 * 86_400), "y"),
    };
    format!("{value}{unit} ago")
}

fn scope_icon(scope: &Scope) -> IconName {
    match scope.id {
        "vso.build" => IconName::Rocket,
        "vso.code" | "vso.code_write" => IconName::GitPullRequest,
        "vso.identity" => IconName::Users,
        "vso.project" => IconName::FolderKanban,
        _ => IconName::SquareCheck,
    }
}

/// One design §4 scope row (spec 0001 FR-13): icon (or `status`), ADO
/// label, access chip, `vso.*` id, what it allows, and `note` (2q's result)
/// under it. Shared by 2b, 2p, 2q and Settings › Account.
pub(crate) fn scope_row(
    scope: &'static Scope,
    status: Option<AnyElement>,
    note: Option<AnyElement>,
    highlighted: bool,
    theme: &Theme,
) -> Div {
    let colors = &theme.colors;
    let icon = status.unwrap_or_else(|| {
        Icon::new(scope_icon(scope))
            .color(colors.text_secondary)
            .into_any_element()
    });
    row()
        .items_start()
        .gap(px(space::S12))
        .px(px(space::S8))
        .py(px(space::S6))
        .rounded(px(radius::SM))
        .when(highlighted, |row| row.bg(colors.status_warning_wash))
        .child(div().flex_none().pt(px(space::S2)).child(icon))
        .child(
            column()
                .flex_1()
                .min_w_0()
                .gap(px(space::S2))
                .child(
                    row()
                        .gap(px(space::S6))
                        .child(
                            div()
                                .type_style(typography::BODY_STRONG, theme)
                                .child(scope.label),
                        )
                        .child(Chip::new(scope.access))
                        .child(
                            div()
                                .text_color(colors.text_tertiary)
                                .type_style(typography::CODE_INLINE, theme)
                                .child(scope.id),
                        ),
                )
                .child(small(scope.why, theme))
                .children(note),
        )
}

fn retry_label(failure: &Failure) -> SharedString {
    match failure.retry_in(Instant::now()) {
        Some(secs) => format!("Try again in {secs}s").into(),
        None => "Try again".into(),
    }
}

impl Onboarding {
    fn listener_button<F>(&self, button: Button, cx: &mut Context<Self>, f: F) -> Button
    where
        F: Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    {
        let this = cx.entity().downgrade();
        button.on_press(move |window, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| f(this, window, cx)).ok();
        })
    }

    /// A failure notice with a Retry button (countdown when throttled).
    fn failure_notice(
        &self,
        failure: &Failure,
        id: &'static str,
        retry: fn(&mut Flow, Instant) -> Vec<Effect>,
        cx: &mut Context<Self>,
    ) -> Notice {
        let offline = matches!(failure.error, OnboardError::Offline);
        let waiting = failure.retry_in(Instant::now()).is_some();
        let notice = Notice::new(
            if offline {
                ChipTone::Warning
            } else {
                ChipTone::Danger
            },
            failure.error.message(),
        );
        let notice = if offline {
            notice.icon(IconName::WifiOff)
        } else {
            notice
        };
        notice.child(
            self.listener_button(
                Button::new(id, retry_label(failure))
                    .icon(IconName::RefreshCw)
                    .disabled(waiting),
                cx,
                move |this, window, cx| this.apply(window, cx, retry),
            ),
        )
    }

    fn paste_instead(&self, id: &'static str, cx: &mut Context<Self>) -> Button {
        self.listener_button(link(id, "Paste a token instead"), cx, |this, window, cx| {
            this.apply(window, cx, |flow, _| flow.use_paste_path())
        })
    }

    // Screen 0 ---------------------------------------------------------------

    fn welcome(&self, animate: bool, cx: &mut Context<Self>) -> Page {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        // Design §3: the illustration carries the fist and the name, so no
        // 拳 text emblem. Decorative for assistive tech (§9); gpui decodes
        // it off the UI thread and the fixed box never resizes.
        let image = div()
            .id("welcome-image")
            .flex_none()
            .w(px(WELCOME_IMAGE.0))
            .h(px(WELCOME_IMAGE.1))
            .child(
                gpui::img(adoken_ui::images::ONBOARDING_WELCOME)
                    .size_full()
                    .object_fit(gpui::ObjectFit::Contain),
            );
        let image = fade_in(
            div().child(image),
            "welcome-image-in",
            0,
            durations::SLOW,
            6.0,
            animate,
        );
        let headline = div()
            .type_style(typography::DISPLAY, &theme)
            .child("Welcome to adoken");
        let subline = div()
            .max_w(px(420.0))
            .text_color(colors.text_secondary)
            .child("Review pull requests, track work items, and stay on top of your day.");
        let button = row().child(confirm_button("get-started", "Get started"));
        let note = tertiary("Works with Azure DevOps Services (dev.azure.com).", &theme);
        let stack = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(space::S16))
            .text_center()
            .child(image)
            .child(fade_in(
                headline,
                "welcome-headline",
                360,
                durations::BASE,
                4.0,
                animate,
            ))
            .child(fade_in(
                subline,
                "welcome-subline",
                440,
                durations::BASE,
                4.0,
                animate,
            ))
            .child(fade_in(
                button,
                "welcome-button",
                520,
                durations::BASE,
                4.0,
                animate,
            ))
            .child(fade_in(
                note,
                "welcome-note",
                520,
                durations::BASE,
                0.0,
                animate,
            ));
        Page {
            headline: SharedString::default(),
            subline: None,
            body: vec![stack.into_any_element()],
            secondary: Vec::new(),
            primary: None,
        }
    }

    // Screen 1 ---------------------------------------------------------------

    fn connect(&self, cx: &mut Context<Self>) -> Page {
        let theme = cx.theme().clone();
        let detection = &self.flow.detection;
        let az_ready = matches!(detection.status(), Some(AzStatus::Ready { .. }));
        let usable = detection.az_usable();

        let az_status: AnyElement =
            match detection {
                flow::Detection::Checking { since } => {
                    if since.elapsed() < flow::DETECTION_QUIET {
                        div().into_any_element()
                    } else {
                        row()
                            .child(Spinner::new())
                            .child("Looking for az…")
                            .into_any_element()
                    }
                }
                flow::Detection::Done(status) => match status {
                    AzStatus::Ready { version, user, .. } => row()
                        .child(StatusIcon::new("az-ready", Status::Done))
                        .child(format!("az {version} · signed in as {user}"))
                        .into_any_element(),
                    AzStatus::NotLoggedIn { version, .. } => row()
                        .child(StatusIcon::new("az-signed-out", Status::Attention))
                        .child(format!("az {version} · not signed in"))
                        .into_any_element(),
                    AzStatus::TooOld { found, .. } => column()
                        .gap(px(space::S2))
                        .child(
                            row()
                                .child(StatusIcon::new("az-old", Status::Attention))
                                .child(format!("az {found} · needs {MIN_AZ_VERSION} or newer")),
                        )
                        .child(
                            row()
                                .child(self.listener_button(
                                    link("az-update", "How to update").icon(IconName::ExternalLink),
                                    cx,
                                    |_, _, cx| cx.open_url(UPDATE_AZ_URL),
                                ))
                                .child(self.check_again_button(cx)),
                        )
                        .into_any_element(),
                    AzStatus::NotInstalled => column()
                        .gap(px(space::S2))
                        .child(
                            row()
                                .child(StatusIcon::new("az-missing", Status::Unavailable))
                                .child("Azure CLI not found"),
                        )
                        .child(
                            row()
                                .child(
                                    self.listener_button(
                                        link("az-install", "Install Azure CLI")
                                            .icon(IconName::ExternalLink),
                                        cx,
                                        |_, _, cx| cx.open_url(INSTALL_AZ_URL),
                                    ),
                                )
                                .child(self.check_again_button(cx)),
                        )
                        .into_any_element(),
                    AzStatus::Failed { .. } => column()
                        .gap(px(space::S2))
                        .child(
                            row()
                                .child(StatusIcon::new("az-failed", Status::Attention))
                                .child("az didn't respond"),
                        )
                        .child(row().child(self.check_again_button(cx)).child(
                            self.listener_button(
                                link("az-details", "Details"),
                                cx,
                                |this, _, cx| {
                                    this.details_open = !this.details_open;
                                    cx.notify();
                                },
                            ),
                        ))
                        .into_any_element(),
                },
            };

        let failure_detail = match detection.status() {
            Some(AzStatus::Failed { reason }) if self.details_open => {
                Some(Notice::new(ChipTone::Neutral, reason.clone()).icon(IconName::Terminal))
            }
            _ => None,
        };

        let this = cx.entity().downgrade();
        let az_card = OptionCard::new("choice-az", "Use Azure CLI")
            .description("adoken creates a token for you. Nothing to copy.")
            .status(az_status)
            .shortcut(Kbd::parse("1").expect("static binding"))
            .selected(self.flow.choice == ConnectPath::Az)
            .disabled(!usable && detection.status().is_some())
            .when(az_ready, |card| {
                card.badge(Chip::new("Recommended").tone(ChipTone::Accent))
            })
            .on_click({
                let this = this.clone();
                move |window, cx| {
                    this.update(cx, |this, cx| {
                        this.apply(window, cx, |flow, _| {
                            flow.select_choice(ConnectPath::Az);
                            Vec::new()
                        })
                    })
                    .ok();
                }
            });
        let paste_card = OptionCard::new("choice-paste", "Paste a token")
            .description("Create one in the browser, paste it here.")
            .shortcut(Kbd::parse("2").expect("static binding"))
            .selected(self.flow.choice == ConnectPath::Paste)
            .on_click(move |window, cx| {
                this.update(cx, |this, cx| {
                    this.apply(window, cx, |flow, _| {
                        flow.select_choice(ConnectPath::Paste);
                        Vec::new()
                    })
                })
                .ok();
            });

        let can_continue = match self.flow.choice {
            ConnectPath::Paste => true,
            ConnectPath::Az => usable,
        };
        Page::new("Connect Azure DevOps")
            .subline("adoken needs a personal access token. It can make one for you with the Azure CLI, or you can paste your own.")
            .body(
                column()
                    .gap(px(space::S12))
                    .child(az_card)
                    .children(failure_detail)
                    .child(paste_card),
            )
            .secondary(tertiary("↑↓ or 1 / 2 to choose", &theme))
            .primary(confirm_button("connect-continue", "Continue").disabled(!can_continue))
    }

    fn check_again_button(&self, cx: &mut Context<Self>) -> Button {
        self.listener_button(
            link("az-check-again", "Check again").icon(IconName::RefreshCw),
            cx,
            |this, window, cx| this.apply(window, cx, |flow, now| flow.detect_again(now)),
        )
    }

    // Screen 1a --------------------------------------------------------------

    fn sign_in_page(&self, cx: &mut Context<Self>) -> Page {
        let theme = cx.theme().clone();
        let page = Page::new("Sign in with Azure CLI")
            .subline("This opens your browser. adoken never sees your password.");
        match &self.flow.sign_in {
            SignIn::Idle => page
                .secondary(self.listener_button(
                    link("device-code", "Use a device code instead"),
                    cx,
                    |this, window, cx| this.apply(window, cx, |flow, _| flow.start_sign_in(true)),
                ))
                .primary(confirm_button("open-sign-in", "Open sign-in")),
            SignIn::Waiting {
                device_code,
                prompt,
            } => {
                let waiting =
                    row()
                        .child(Spinner::new().size(size::ICON_MD))
                        .child(if *device_code {
                            "Waiting for you to enter the code in the browser…"
                        } else {
                            "Waiting for you to finish in the browser…"
                        });
                let code = prompt.as_ref().map(|prompt| {
                    let code = prompt.code.clone();
                    let url = prompt.url.clone();
                    column().child(small(prompt.message.clone(), &theme)).child(
                        row()
                            .child(
                                div()
                                    .px(px(space::S8))
                                    .py(px(space::S4))
                                    .rounded(px(radius::SM))
                                    .bg(theme.colors.bg_code)
                                    .type_style(typography::CODE, &theme)
                                    .child(prompt.code.clone()),
                            )
                            .child(self.listener_button(
                                link("copy-code", "Copy").icon(IconName::Copy),
                                cx,
                                move |this, window, cx| {
                                    let text = code.clone();
                                    this.apply(window, cx, move |flow, _| flow.copy_scope(&text))
                                },
                            ))
                            .child(self.listener_button(
                                link("open-device-page", "Open page").icon(IconName::ExternalLink),
                                cx,
                                move |_, _, cx| cx.open_url(&url),
                            )),
                    )
                });
                page.body(column().child(waiting).children(code))
                    .secondary(
                        self.listener_button(
                            Button::new("cancel-sign-in", "Cancel")
                                .shortcut(Kbd::parse("escape").expect("static binding")),
                            cx,
                            |this, window, cx| {
                                this.apply(window, cx, |flow, _| flow.cancel_sign_in())
                            },
                        ),
                    )
                    .primary(primary_button("open-sign-in", "Open sign-in").loading(true))
            }
            SignIn::Succeeded => {
                let user = match self.flow.detection.status() {
                    Some(AzStatus::Ready { user, .. }) => format!("Signed in as {user}"),
                    _ => "Signed in".into(),
                };
                page.body(
                    row()
                        .child(StatusIcon::new("signed-in", Status::Done))
                        .child(user),
                )
                .primary(confirm_button("sign-in-continue", "Continue"))
            }
            SignIn::Failed(error) => page
                .body(
                    Notice::new(ChipTone::Danger, error.message())
                        .child(self.paste_instead("sign-in-paste", cx)),
                )
                .primary(confirm_button("sign-in-retry", "Try again")),
            SignIn::TimedOut => page
                .body(
                    Notice::new(ChipTone::Warning, "Sign-in timed out.")
                        .child(self.paste_instead("sign-in-paste", cx)),
                )
                .primary(confirm_button("sign-in-retry", "Try again")),
        }
    }

    // Screen 2a --------------------------------------------------------------

    fn pick_org(&self, cx: &mut Context<Self>) -> Page {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let page = Page::new("Choose your organization")
            .subline("adoken works with one organization. You can pick projects next.");

        let manual = self.flow.manual_org.then(|| {
            column()
                .child(self.org_field(&self.manual_org, &theme))
                .children(self.flow.org_error.clone().map(|error| {
                    div()
                        .text_color(colors.status_danger_fg)
                        .type_style(typography::SMALL, &theme)
                        .child(error)
                }))
                .child(tertiary(
                    format!("Org in another directory? Run `{TENANT_HINT}`, then come back."),
                    &theme,
                ))
        });
        let toggle_manual = self.listener_button(
            link(
                "toggle-manual",
                if self.flow.manual_org {
                    "Choose from the list"
                } else {
                    "Not listed? Enter it manually"
                },
            ),
            cx,
            |this, window, cx| {
                if this.flow.manual_org {
                    this.flow.manual_org = false;
                    window.focus(&this.focus, cx);
                } else {
                    this.flow.open_manual_org();
                    window.focus(&this.manual_org.focus_handle(cx), cx);
                }
                cx.notify();
            },
        );

        let body: AnyElement = match &self.flow.orgs {
            Load::Idle | Load::Loading => column()
                .children((0..3u32).map(|ix| {
                    row()
                        .h(px(size::PALETTE_ROW_DEFAULT))
                        .px(px(space::S12))
                        .child(Skeleton::new(("org-skeleton-icon", ix)).width(size::ICON_MD))
                        .child(
                            column()
                                .flex_1()
                                .gap(px(space::S4))
                                .child(Skeleton::new(("org-skeleton-name", ix)).width(160.0))
                                .child(
                                    Skeleton::new(("org-skeleton-url", ix))
                                        .width(220.0)
                                        .height(typography::SMALL.line_height - space::S4),
                                ),
                        )
                }))
                .into_any_element(),
            Load::Failed(failure) => column()
                .child(self.failure_notice(failure, "retry-orgs", Flow::retry_orgs, cx))
                .children(
                    matches!(
                        failure.error,
                        OnboardError::NotInTenant | OnboardError::AzUnavailable { .. }
                    )
                    .then(|| row().child(self.paste_instead("orgs-paste", cx))),
                )
                .into_any_element(),
            Load::Loaded(list) if list.orgs.is_empty() => {
                let who = list
                    .me
                    .email
                    .clone()
                    .unwrap_or_else(|| list.me.display_name.clone());
                column()
                    .child(small(format!("No organizations found for {who}."), &theme))
                    .child(
                        row()
                            .child(tertiary("Signed into the wrong directory?", &theme))
                            .child(
                                div()
                                    .type_style(typography::CODE_INLINE, &theme)
                                    .text_color(colors.text_secondary)
                                    .child(TENANT_HINT),
                            )
                            .child(self.listener_button(
                                link("copy-tenant", "Copy command").icon(IconName::Copy),
                                cx,
                                |this, window, cx| {
                                    this.apply(window, cx, |flow, _| flow.copy_scope(TENANT_HINT))
                                },
                            )),
                    )
                    .into_any_element()
            }
            Load::Loaded(list) => {
                let visible = self.flow.visible_orgs();
                let filter = (list.orgs.len() > flow::ORG_FILTER_THRESHOLD)
                    .then(|| self.field(&self.org_filter, &theme));
                let default = self.flow.default_org.clone();
                let this = cx.entity().downgrade();
                let rows = visible.iter().enumerate().map(|(ix, org)| {
                    let selected = !self.flow.manual_org && ix == self.flow.org_cursor;
                    let is_default = default
                        .as_ref()
                        .is_some_and(|d| d.name().eq_ignore_ascii_case(&org.name));
                    let this = this.clone();
                    ListRow::new(("org", ix), org.name.clone())
                        .subtitle(format!("dev.azure.com/{}", org.name))
                        .leading(radio(selected, &theme))
                        .when(is_default, |row| {
                            row.trailing(Chip::new("az default").tone(ChipTone::Accent))
                        })
                        .selected(selected)
                        .on_click(move |event, window, cx| {
                            let double = event.click_count() > 1;
                            this.update(cx, |this, cx| {
                                this.flow.select_org(ix);
                                if double {
                                    this.apply(window, cx, |flow, now| flow.confirm(now));
                                }
                                cx.notify();
                            })
                            .ok();
                        })
                });
                column()
                    .children(filter)
                    .child(
                        div()
                            .id("org-list")
                            .max_h(px(size::PALETTE_ROW_DEFAULT * LIST_ROWS))
                            .overflow_y_scroll()
                            .track_scroll(&self.org_scroll)
                            .contain_scroll()
                            .children(rows),
                    )
                    .when(visible.is_empty(), |list| {
                        list.child(tertiary("No organization matches that filter.", &theme))
                    })
                    .into_any_element()
            }
        };
        let can_continue = self.flow.manual_org
            || (matches!(self.flow.orgs, Load::Loaded(_)) && !self.flow.visible_orgs().is_empty());
        page.body(column().gap(px(space::S12)).child(body).children(manual))
            .secondary(toggle_manual)
            .primary(confirm_button("org-continue", "Continue").disabled(!can_continue))
    }

    /// `dev.azure.com/ [ org ]` field (design 2a manual entry, 2p).
    fn org_field(&self, input: &Entity<TextInput>, theme: &Theme) -> Div {
        let colors = &theme.colors;
        div()
            .flex()
            .items_center()
            .h(px(size::CONTROL_LG))
            .px(px(space::S12))
            .rounded(px(radius::SM))
            .border_1()
            .border_color(colors.border_control)
            .bg(colors.bg_input)
            .type_style(typography::BODY, theme)
            .child(
                div()
                    .flex_none()
                    .text_color(colors.text_tertiary)
                    .child("dev.azure.com/"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(colors.text_primary)
                    .child(input.clone()),
            )
    }

    fn field(&self, input: &Entity<TextInput>, theme: &Theme) -> Div {
        let colors = &theme.colors;
        div()
            .flex()
            .items_center()
            .h(px(size::CONTROL_MD))
            .px(px(space::S8))
            .rounded(px(radius::SM))
            .border_1()
            .border_color(colors.border_control)
            .bg(colors.bg_input)
            .text_color(colors.text_primary)
            .type_style(typography::BODY, theme)
            .child(input.clone())
    }

    // Screen 2b --------------------------------------------------------------

    /// What the chosen level does (design 2b: the subline above the list).
    fn level_summary(&self, theme: &Theme) -> Div {
        tertiary(self.flow.level.summary(), theme)
    }

    /// Read-only demoted to a link under the scope list (spec 0001 Q-5,
    /// accepted): it switches to the other level.
    fn access_level_link(&self, cx: &mut Context<Self>) -> Div {
        let (label, other) = match self.flow.level {
            AccessLevel::Review => ("Need a read-only token instead?", AccessLevel::ReadOnly),
            AccessLevel::ReadOnly => ("Use the Review token instead", AccessLevel::Review),
        };
        let locked = matches!(self.flow.minting, Minting::Creating | Minting::Created(_));
        row().child(
            self.listener_button(link("access-level", label), cx, move |this, _, cx| {
                this.flow.set_level(other);
                cx.notify();
            })
            .disabled(locked),
        )
    }

    fn scope_rows(&self, level: AccessLevel, theme: &Theme) -> Div {
        column()
            .gap(px(space::S4))
            .children(scopes(level).iter().map(|scope| {
                let highlighted = self.flow.highlight.contains(&scope.id);
                scope_row(scope, None, None, highlighted, theme)
            }))
    }

    fn review_access(&self, cx: &mut Context<Self>) -> Page {
        let theme = cx.theme().clone();
        let colors = theme.colors;
        let this = cx.entity().downgrade();
        let lifetime_ix = LIFETIMES
            .iter()
            .position(|d| *d == self.flow.lifetime_days)
            .unwrap_or(1);
        // Design 2b: no note under it. adoken doesn't remind about expiry;
        // the date shows in Settings › Account (spec 0001 FR-18).
        let lifetime = column()
            .gap(px(space::S4))
            .child(
                row()
                    .child(
                        div()
                            .type_style(typography::BODY_STRONG, &theme)
                            .child("Expires in"),
                    )
                    .child(
                        Segmented::new("lifetime", LIFETIMES.map(lifetime_label), lifetime_ix)
                            .on_select(move |ix, _, cx| {
                                this.update(cx, |this, cx| {
                                    this.flow.set_lifetime(LIFETIMES[ix]);
                                    cx.notify();
                                })
                                .ok();
                            }),
                    ),
            )
            .children(match &self.flow.minting {
                Minting::Created(meta) if meta.lifetime_capped => Some(
                    div()
                        .text_color(colors.status_warning_fg)
                        .type_style(typography::SMALL, &theme)
                        .child("Your organization limits token lifetime, so adoken used 30 days."),
                ),
                _ => None,
            });

        let org = self
            .flow
            .org
            .as_ref()
            .map(|o| o.name().to_owned())
            .unwrap_or_default();
        let details = column()
            .gap(px(space::S12))
            .child(
                row()
                    .child(
                        div()
                            .type_style(typography::BODY_STRONG, &theme)
                            .child("Token name"),
                    )
                    .child(small(format!("{} · {org} only", self.token_name), &theme)),
            )
            .child(lifetime)
            .child(
                row()
                    .child(
                        Icon::new(IconName::Lock)
                            .size(size::ICON_SM)
                            .color(colors.text_tertiary),
                    )
                    .child(tertiary(
                        "Stored in your macOS Keychain. Never written to disk or logs.",
                        &theme,
                    )),
            );

        let why = self.why_disclosure(cx);
        let status: Option<AnyElement> = match &self.flow.minting {
            Minting::Failed(failure) => {
                let mut notice = Notice::new(ChipTone::Danger, failure.error.message());
                if matches!(failure.error, OnboardError::NotInTenant) {
                    notice = notice.detail(format!("Hint: {TENANT_HINT}"));
                }
                Some(
                    notice
                        .child(self.paste_instead("create-paste", cx))
                        .into_any_element(),
                )
            }
            _ => None,
        };

        let primary: AnyElement = match &self.flow.minting {
            Minting::Idle => confirm_button("create-token", "Create token").into_any_element(),
            Minting::Creating => primary_button("create-token", "Creating…")
                .loading(true)
                .into_any_element(),
            Minting::Created(_) => row()
                .child(StatusIcon::new("token-created", Status::Done))
                .child(
                    div()
                        .type_style(typography::BODY_STRONG, &theme)
                        .child("Token created"),
                )
                .into_any_element(),
            Minting::Failed(failure) => confirm_button("create-token", retry_label(failure))
                .disabled(failure.retry_in(Instant::now()).is_some())
                .into_any_element(),
        };

        Page::new("Create a token for adoken")
            .subline("adoken will create a personal access token with exactly these permissions. It won't ask for more later.")
            .body(
                column()
                    .gap(px(space::S16))
                    .child(
                        column()
                            .gap(px(space::S8))
                            .child(self.level_summary(&theme))
                            .child(self.scope_rows(self.flow.level, &theme)),
                    )
                    .child(
                        column()
                            .gap(px(space::S4))
                            .child(self.access_level_link(cx))
                            .child(why),
                    )
                    .child(details)
                    .children(status),
            )
            .secondary(self.paste_instead("review-paste", cx))
            .primary(primary)
    }

    fn why_disclosure(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let text = self.flow.level.why_these();
        column()
            .gap(px(space::S4))
            .child(row().child(self.listener_button(
                link(
                    "why-these",
                    if self.why_open { "Hide" } else { "Why these?" },
                ),
                cx,
                |this, _, cx| {
                    this.why_open = !this.why_open;
                    cx.notify();
                },
            )))
            .when(self.why_open, |col| col.child(small(text, &theme)))
    }

    // Screen 2p --------------------------------------------------------------

    fn create_in_browser(&self, cx: &mut Context<Self>) -> Page {
        let theme = cx.theme().clone();
        let colors = theme.colors;
        let reason = self
            .flow
            .paste_reason
            .clone()
            .map(|reason| Notice::new(ChipTone::Warning, reason));
        let org_error = self.flow.paste_org_error.clone().map(|error| {
            div()
                .text_color(colors.status_danger_fg)
                .type_style(typography::SMALL, &theme)
                .child(error)
        });
        let step = |n: usize, content: AnyElement| {
            row()
                .items_start()
                .gap(px(space::S12))
                .child(
                    div()
                        .flex_none()
                        .w(px(size::ICON_LG))
                        .text_color(colors.text_tertiary)
                        .type_style(typography::SMALL, &theme)
                        .child(format!("{n}.")),
                )
                .child(div().flex_1().min_w_0().child(content))
        };
        let scope_list =
            column()
                .gap(px(space::S2))
                .children(
                    scopes(self.flow.level)
                        .iter()
                        .enumerate()
                        .map(|(ix, scope)| {
                            // Design 2p step 3: copies the name ADO's dialog shows.
                            let label = scope.label;
                            let highlighted = self.flow.highlight.contains(&scope.id);
                            row()
                                .items_start()
                                .child(div().flex_1().min_w_0().child(scope_row(
                                    scope,
                                    None,
                                    None,
                                    highlighted,
                                    &theme,
                                )))
                                .child(
                                    div().flex_none().pt(px(space::S2)).child(
                                        self.listener_button(
                                            link(("copy-scope", ix), "Copy")
                                                .icon(IconName::Copy)
                                                .tooltip(format!("Copy \"{}\"", scope.label))
                                                .aria_label(format!("Copy {}", scope.label)),
                                            cx,
                                            move |this, window, cx| {
                                                this.apply(window, cx, move |flow, _| {
                                                    flow.copy_scope(label)
                                                })
                                            },
                                        ),
                                    ),
                                )
                        }),
                );
        let checklist = column()
            .gap(px(space::S12))
            .child(step(
                1,
                small(
                    "Click New Token. Name it \"adoken\". Set expiry (90 days suggested; up to 1 year is fine).",
                    &theme,
                )
                .into_any_element(),
            ))
            .child(step(
                2,
                small(
                    "Under Organization, keep this organization only (not \"All accessible organizations\"). Under Scopes, choose Custom defined, then Show all scopes.",
                    &theme,
                )
                .into_any_element(),
            ))
            .child(step(
                3,
                column()
                    .child(small(
                        "Tick exactly these, in the order Azure DevOps lists them:",
                        &theme,
                    ))
                    .child(scope_list)
                    .into_any_element(),
            ))
            .child(step(
                4,
                small("Click Create, then copy the token.", &theme).into_any_element(),
            ));

        Page::new("Create a token")
            .subline("Takes about a minute. Keep this window open.")
            .body(
                column()
                    .gap(px(space::S16))
                    .children(reason)
                    .child(if self.flow.sheet {
                        // Design §10.2: the sheet replaces this org's token.
                        column().child(small(
                            format!("dev.azure.com/{}", self.flow.paste_org_text),
                            &theme,
                        ))
                    } else {
                        column()
                            .gap(px(space::S4))
                            .child(self.org_field(&self.paste_org, &theme))
                            .children(org_error)
                    })
                    .child(
                        column()
                            .gap(px(space::S2))
                            .child(self.level_summary(&theme))
                            .child(self.access_level_link(cx)),
                    )
                    .child(
                        row().child(
                            self.listener_button(
                                Button::new("open-token-page", "Open token page")
                                    .icon(IconName::ExternalLink),
                                cx,
                                |this, window, cx| {
                                    this.apply(window, cx, |flow, _| flow.open_token_page())
                                },
                            ),
                        ),
                    )
                    .child(checklist),
            )
            .secondary(tertiary("Paste anywhere with ⌘V", &theme))
            .primary(confirm_button("copied-it", "I've copied it"))
    }

    // Screen 2q --------------------------------------------------------------

    fn paste_verify(&self, cx: &mut Context<Self>) -> Page {
        let theme = cx.theme().clone();
        let colors = theme.colors;
        let page = Page::new("Paste your token");
        let saved = self.token_saved();

        let field: AnyElement = if saved {
            row()
                .child(StatusIcon::new("token-saved", Status::Done))
                .child("Token saved to Keychain")
                .into_any_element()
        } else {
            let revealed = self.token_revealed;
            let field = div()
                .id("token-field")
                .flex()
                .items_center()
                .gap(px(space::S8))
                .h(px(size::CONTROL_LG))
                .pl(px(space::S12))
                .pr(px(space::S4))
                .rounded(px(radius::SM))
                .border_1()
                .border_color(match &self.flow.verifying {
                    Verifying::Failed(_) | Verifying::Malformed(_) => colors.status_danger_fg,
                    _ => colors.border_control,
                })
                .bg(colors.bg_input)
                .text_color(colors.text_primary)
                .type_style(typography::CODE, &theme)
                .child(div().flex_1().min_w_0().child(self.token.clone()))
                .child(
                    self.listener_button(
                        link(
                            "reveal-token",
                            if revealed { "Hide token" } else { "Show token" },
                        )
                        .icon(if revealed {
                            IconName::EyeOff
                        } else {
                            IconName::Eye
                        }),
                        cx,
                        |this, _, cx| {
                            this.token_revealed = !this.token_revealed;
                            let masked = !this.token_revealed;
                            this.token
                                .update(cx, |input, cx| input.set_masked(masked, cx));
                            cx.notify();
                        },
                    ),
                );
            let shakes = self.shakes;
            if shakes > 0 {
                field
                    .relative()
                    .with_animation(
                        ("token-shake", shakes),
                        animation(240, adoken_ui::motion::EASE_IN_OUT),
                        |field, t| field.left(px(shake_offset(t))),
                    )
                    .into_any_element()
            } else {
                field.into_any_element()
            }
        };

        let (rows, result): (Vec<AnyElement>, Option<AnyElement>) = match &self.flow.verifying {
            Verifying::Done(verification) => {
                let summary = summarize(verification);
                let rows = verification
                    .probes
                    .iter()
                    .enumerate()
                    .map(|(ix, probe)| {
                        let shown = ix < self.revealed;
                        let status = if !shown {
                            Status::Running
                        } else {
                            match &probe.outcome {
                                ProbeOutcome::Granted | ProbeOutcome::GrantedWriteUntested => {
                                    Status::Done
                                }
                                ProbeOutcome::Missing | ProbeOutcome::Failed(_) => {
                                    if probe.scope.required {
                                        Status::Failed
                                    } else {
                                        Status::Missing
                                    }
                                }
                                ProbeOutcome::Untested => Status::Pending,
                            }
                        };
                        let icon = StatusIcon::new(("probe", ix), status).into_any_element();
                        let note = match &probe.outcome {
                            _ if !shown => None,
                            ProbeOutcome::GrantedWriteUntested => Some(
                                row()
                                    .flex_wrap()
                                    .pt(px(space::S2))
                                    .child(Chip::new("Write untested"))
                                    .child(tertiary(
                                        if probe.scope.id == "vso.work_write" {
                                            "Checked the first time you edit or link a work item."
                                        } else {
                                            "Checked the first time you vote or comment."
                                        },
                                        &theme,
                                    ))
                                    .into_any_element(),
                            ),
                            ProbeOutcome::Missing if !probe.scope.required => {
                                probe.scope.if_missing.map(|text| {
                                    div()
                                        .text_color(colors.status_warning_fg)
                                        .type_style(typography::SMALL, &theme)
                                        .child(text)
                                        .into_any_element()
                                })
                            }
                            ProbeOutcome::Untested => Some(
                                tertiary("Couldn't test: no project visible yet.", &theme)
                                    .into_any_element(),
                            ),
                            ProbeOutcome::Failed(reason) => {
                                Some(tertiary(reason.clone(), &theme).into_any_element())
                            }
                            _ => None,
                        };
                        scope_row(probe.scope, Some(icon), note, false, &theme).into_any_element()
                    })
                    .collect();
                let all_shown = self.revealed >= verification.probes.len();
                let message = all_shown.then(|| {
                    if !summary.missing_required.is_empty() {
                        let names: Vec<_> =
                            summary.missing_required.iter().map(|s| s.title).collect();
                        Notice::new(
                            ChipTone::Danger,
                            format!("This token is missing {}.", names.join(", ")),
                        )
                        .into_any_element()
                    } else if !summary.missing_optional.is_empty() {
                        let names: Vec<_> =
                            summary.missing_optional.iter().map(|s| s.title).collect();
                        Notice::new(
                            ChipTone::Warning,
                            format!(
                                "Token works. Signed in as {}. Missing: {}.",
                                summary.who,
                                names.join(", ")
                            ),
                        )
                        .into_any_element()
                    } else {
                        Notice::new(
                            ChipTone::Success,
                            format!("Token works. Signed in as {}.", summary.who),
                        )
                        .into_any_element()
                    }
                });
                (rows, message)
            }
            other => {
                let status = if matches!(other, Verifying::Running) {
                    Status::Running
                } else {
                    Status::Pending
                };
                let rows = scopes(self.flow.level)
                    .iter()
                    .enumerate()
                    .map(|(ix, scope)| {
                        let icon = StatusIcon::new(("probe", ix), status).into_any_element();
                        scope_row(scope, Some(icon), None, false, &theme).into_any_element()
                    })
                    .collect();
                let message = match other {
                    Verifying::Malformed(message) => {
                        Some(Notice::new(ChipTone::Danger, message.clone()).into_any_element())
                    }
                    Verifying::Failed(failure) => Some(
                        Notice::new(ChipTone::Danger, failure.error.message()).into_any_element(),
                    ),
                    _ => None,
                };
                (rows, message)
            }
        };

        let primary: AnyElement = match &self.flow.verifying {
            Verifying::Done(verification) if verification.saved.is_some() => {
                let summary = summarize(verification);
                let label = if summary.missing_optional.iter().any(|s| s.id == "vso.build")
                    && summary.missing_optional.len() == 1
                {
                    "Continue without pipelines".to_owned()
                } else {
                    "Continue".to_owned()
                };
                confirm_button("paste-continue", label).into_any_element()
            }
            Verifying::Done(_) => primary_button("paste-continue", "Continue")
                .disabled(true)
                .into_any_element(),
            Verifying::Running => primary_button("verify", "Verifying…")
                .loading(true)
                .into_any_element(),
            Verifying::Failed(failure) => confirm_button("verify", retry_label(failure))
                .disabled(failure.retry_in(Instant::now()).is_some())
                .into_any_element(),
            Verifying::Idle | Verifying::Malformed(_) => {
                confirm_button("verify", "Verify").into_any_element()
            }
        };
        let fix = matches!(&self.flow.verifying, Verifying::Done(v) if !summarize(v).missing_optional.is_empty() || !summarize(v).missing_required.is_empty());
        // Path B step 5: adoken can't read a pasted token's expiry without
        // `vso.tokens`, so it asks; shown in Settings › Account only.
        let expires = self.token_saved().then(|| {
            column()
                .gap(px(space::S4))
                .child(
                    div()
                        .type_style(typography::BODY_STRONG, &theme)
                        .child("Expires on (optional)"),
                )
                .child(self.field(&self.expires, &theme))
                .child(match &self.flow.expires_error {
                    Some(error) => div()
                        .text_color(colors.status_danger_fg)
                        .type_style(typography::SMALL, &theme)
                        .child(error.clone()),
                    None => tertiary(
                        "adoken can't read this from Azure DevOps. It's shown in Settings › Account.",
                        &theme,
                    ),
                })
        });

        page.body(
            column()
                .gap(px(space::S16))
                .child(field)
                .child(column().gap(px(space::S2)).children(rows))
                .children(result)
                .children(expires),
        )
        .when_secondary(fix, || {
            self.listener_button(link("fix-token", "Fix token"), cx, |this, window, cx| {
                this.apply(window, cx, |flow, _| flow.fix_token())
            })
            .into_any_element()
        })
        .primary(primary)
    }

    // Screen 3 ---------------------------------------------------------------

    fn projects_page(&self, cx: &mut Context<Self>) -> Page {
        let theme = cx.theme().clone();
        let page = Page::new("Pick your projects")
            .subline("adoken syncs these. Change it anytime in Settings › Projects.");
        match &self.flow.projects {
            Load::Idle | Load::Loading => page
                .body(column().children((0..3u32).map(|ix| {
                    row()
                        .h(px(size::PALETTE_ROW_DEFAULT))
                        .px(px(space::S12))
                        .child(Skeleton::new(("project-skeleton-box", ix)).width(size::ICON_MD))
                        .child(Skeleton::new(("project-skeleton-name", ix)).width(200.0))
                })))
                .primary(primary_button("start", "Continue").disabled(true)),
            Load::Failed(failure) => page
                .body(self.failure_notice(failure, "retry-projects", Flow::retry_projects, cx))
                .primary(primary_button("start", "Continue").disabled(true)),
            Load::Loaded(picker) => {
                let now = OffsetDateTime::now_utc();
                let visible = picker.visible();
                let this = cx.entity().downgrade();
                let rows = visible.iter().enumerate().map(|(vix, &ix)| {
                    let project = &picker.projects[ix];
                    let checked = picker.selected.contains(&project.id);
                    let this = this.clone();
                    ListRow::new(("project", ix), project.name.clone())
                        .leading(Checkbox::new(checked))
                        .subtitle(project.description.clone().unwrap_or_default())
                        .when_some(project.last_update_time, |row, t| {
                            row.trailing(relative_age(t, now))
                        })
                        .selected(vix == picker.cursor)
                        .on_click(move |_, _, cx| {
                            this.update(cx, |this, cx| {
                                if let Some(picker) = this.flow.picker_mut() {
                                    picker.toggle(Some(vix));
                                }
                                cx.notify();
                            })
                            .ok();
                        })
                });
                let count = picker.selected.len();
                let many = picker.projects.len() > flow::PROJECT_PRESELECT_MAX;
                let footer = row()
                    .child(small(
                        match count {
                            1 => "1 project selected".to_owned(),
                            n => format!("{n} projects selected"),
                        },
                        &theme,
                    ))
                    .when(many, |row| {
                        row.child(Kbd::parse("secondary-a").expect("static binding"))
                            .child(tertiary("Select all", &theme))
                    });
                page.body(
                    column()
                        .gap(px(space::S12))
                        .child(self.field(&self.project_filter, &theme))
                        .child(
                            div()
                                .id("project-list")
                                .max_h(px(size::PALETTE_ROW_DEFAULT * LIST_ROWS))
                                .overflow_y_scroll()
                                .track_scroll(&self.project_scroll)
                                .contain_scroll()
                                .children(rows),
                        )
                        .when(picker.projects.is_empty(), |col| {
                            col.child(small(
                                "This organization has no projects you can see.",
                                &theme,
                            ))
                        })
                        .when(count == 0 && !picker.projects.is_empty(), |col| {
                            col.child(tertiary("Pick at least one project.", &theme))
                        }),
                )
                .secondary(footer)
                .primary(confirm_button("start", "Continue").disabled(count == 0))
            }
        }
    }

    // Done -------------------------------------------------------------------

    fn done(&self, cx: &mut Context<Self>) -> Page {
        let theme = cx.theme().clone();
        let org = self
            .flow
            .org
            .as_ref()
            .map(|o| o.name().to_owned())
            .unwrap_or_default();
        let projects = self.flow.state().projects.len();
        Page::new("You're connected")
            .subline(format!(
                "adoken is set up for {org} with {projects} project{}.",
                if projects == 1 { "" } else { "s" }
            ))
            .body(
                column()
                    .child(
                        row()
                            .child(StatusIcon::new("done-token", Status::Done))
                            .child("Token saved to your Keychain"),
                    )
                    .child(
                        row()
                            .child(Kbd::parse(crate::palette::TOGGLE_KEYS).expect("static binding"))
                            .child(small("Search everything, any time.", &theme)),
                    ),
            )
            .primary(confirm_button("finish", "Start using adoken"))
    }

    fn page(&self, animate_welcome: bool, cx: &mut Context<Self>) -> Page {
        match self.flow.screen {
            Screen::Welcome => self.welcome(animate_welcome, cx),
            Screen::Connect => self.connect(cx),
            Screen::SignIn => self.sign_in_page(cx),
            Screen::PickOrg => self.pick_org(cx),
            Screen::ReviewAccess => self.review_access(cx),
            Screen::CreateInBrowser => self.create_in_browser(cx),
            Screen::PasteVerify => self.paste_verify(cx),
            Screen::Projects => self.projects_page(cx),
            Screen::Done => self.done(cx),
        }
    }
}

impl Page {
    fn when_secondary(self, condition: bool, element: impl FnOnce() -> AnyElement) -> Self {
        if condition {
            let mut this = self;
            this.secondary.push(element());
            this
        } else {
            self
        }
    }
}

fn radio(selected: bool, theme: &Theme) -> Div {
    let colors = &theme.colors;
    div()
        .size(px(size::ICON_SM))
        .rounded(px(radius::FULL))
        .border_1()
        .border_color(if selected {
            colors.accent_solid
        } else {
            colors.border_strong
        })
        .flex()
        .items_center()
        .justify_center()
        .when(selected, |radio| {
            radio.child(
                div()
                    .size(px(space::S6))
                    .rounded(px(radius::FULL))
                    .bg(colors.accent_solid),
            )
        })
}

impl Render for Onboarding {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tick_while_counting(window, cx);
        let theme = cx.theme().clone();
        let colors = theme.colors;
        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
        let full_bleed = vw < FULL_BLEED_BELOW.0 || vh < FULL_BLEED_BELOW.1;
        let screen = self.flow.screen;
        let animate_welcome = !self.welcome_settled;
        let page = self.page(animate_welcome, cx);

        let header = screen.indicator().map(|active| {
            let previous = self.flow.previous.and_then(Screen::indicator);
            let back = self.flow.can_go_back().then(|| {
                self.listener_button(
                    link("back", "‹ Back").shortcut(Kbd::parse(BACK_KEYS).expect("static binding")),
                    cx,
                    |this, window, cx| this.apply(window, cx, |flow, _| flow.back()),
                )
            });
            // The sheet closes from its header too (and with Esc).
            let close = self.flow.sheet.then(|| {
                self.listener_button(
                    link("close-sheet", "Close")
                        .shortcut(Kbd::parse("escape").expect("static binding")),
                    cx,
                    |this, _, cx| this.close_sheet(cx),
                )
            });
            let steps = self.flow.indicator().len();
            row()
                .justify_between()
                .h(px(size::CONTROL_MD))
                .child(row().children(back))
                .child(
                    row()
                        .gap(px(space::S12))
                        .child(
                            StepIndicator::new("steps", steps, active.min(steps))
                                .previous(previous.filter(|p| *p != active)),
                        )
                        .children(close),
                )
        });

        let text = (!page.headline.is_empty()).then(|| {
            column()
                .gap(px(space::S6))
                .child(
                    div()
                        .type_style(typography::DISPLAY, &theme)
                        .child(page.headline.clone()),
                )
                .children(page.subline.clone().map(|subline| {
                    div()
                        .max_w(px(CARD_WIDTH - 2.0 * space::S40))
                        .text_color(colors.text_secondary)
                        .child(subline)
                }))
        });

        let content = div()
            .flex()
            .flex_col()
            .gap(px(space::S24))
            .children(text)
            .children(page.body);
        // Design §6: incoming screen slides 24 px and fades in (mirrored on
        // Back); instant under reduce motion.
        let dx = match self.flow.direction {
            Direction::Forward => SLIDE_X,
            Direction::Back => -SLIDE_X,
        };
        let content = if self.flow.transition == 0 {
            content.into_any_element()
        } else {
            content
                .relative()
                .with_animation(
                    ("screen", self.flow.transition),
                    animation(durations::SLOW, EASE_OUT),
                    move |content, t| content.opacity(t).left(px(lerp(dx, 0.0, t))),
                )
                .into_any_element()
        };

        let footer = (!page.secondary.is_empty() || page.primary.is_some()).then(|| {
            row()
                .justify_between()
                .pt(px(space::S8))
                .child(row().children(page.secondary))
                .children(page.primary)
        });

        let card = div()
            .id("onboarding-card")
            .occlude()
            .flex()
            .flex_col()
            .gap(px(space::S24))
            .p(px(space::S40))
            .bg(colors.bg_overlay.opacity(CARD_ALPHA))
            .text_color(colors.text_primary)
            .type_style(typography::BODY, &theme)
            .map(|card| {
                if full_bleed {
                    card.size_full()
                } else {
                    card.w(px(CARD_WIDTH))
                        .min_h(px(CARD_MIN_HEIGHT))
                        .max_h(px(vh - size::TITLEBAR - 2.0 * space::S24))
                        .rounded(px(radius::XL))
                        .border_1()
                        .border_color(colors.border_default)
                        .shadow(theme.shadows(elevation::LEVEL_3))
                }
            })
            .when(screen == Screen::Welcome, |card| card.justify_center())
            .children(header)
            .child(
                div()
                    .id("onboarding-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(content),
            )
            .children(footer);

        div()
            .id("onboarding")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .when(!full_bleed, |root| {
                // Centered, shifted up 4 % of the window height (design §2).
                root.pt(px(size::TITLEBAR)).pb(px(vh * 0.08))
            })
            // The sheet sits over the app on the scrim (design §10.2).
            .map(|root| {
                if self.flow.sheet {
                    root.bg(colors.bg_scrim).occlude()
                } else {
                    root.bg(colors.bg_app)
                }
            })
            .text_color(colors.text_primary)
            .on_action(cx.listener(Self::on_confirm))
            .on_action(cx.listener(Self::on_back))
            .on_action(cx.listener(|this, _: &SelectNext, window, cx| this.on_move(1, window, cx)))
            .on_action(cx.listener(|this, _: &SelectPrev, window, cx| this.on_move(-1, window, cx)))
            .on_action(
                cx.listener(|this, _: &ChooseFirst, _, cx| this.on_choose(ConnectPath::Az, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ChooseSecond, _, cx| this.on_choose(ConnectPath::Paste, cx)),
            )
            .on_action(cx.listener(Self::on_toggle))
            .on_action(cx.listener(Self::on_select_all))
            .on_action(cx.listener(Self::on_clear))
            .on_action(cx.listener(Self::on_paste))
            .on_action(cx.listener(Self::on_cancel))
            .on_action(|_: &crate::FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &crate::FocusPrevious, window, cx| window.focus_prev(cx))
            // Design §6: any key or click jumps the welcome intro to its end.
            .on_key_down(cx.listener(|this, _, _, cx| {
                if !this.welcome_settled {
                    this.welcome_settled = true;
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if !this.welcome_settled {
                        this.welcome_settled = true;
                        cx.notify();
                    }
                }),
            )
            .child(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_age_picks_the_largest_whole_unit() {
        let now = OffsetDateTime::UNIX_EPOCH + time::Duration::days(800);
        let ago = |d: time::Duration| relative_age(now - d, now);
        assert_eq!(ago(time::Duration::seconds(5)), "just now");
        assert_eq!(ago(time::Duration::minutes(5)), "5m ago");
        assert_eq!(ago(time::Duration::hours(3)), "3h ago");
        assert_eq!(ago(time::Duration::days(2)), "2d ago");
        assert_eq!(ago(time::Duration::days(65)), "2mo ago");
        assert_eq!(ago(time::Duration::days(400)), "1y ago");
        // Clock skew (a future timestamp) never shows a negative age.
        assert_eq!(
            relative_age(now + time::Duration::hours(1), now),
            "just now"
        );
    }

    #[test]
    fn shake_returns_to_rest_and_peaks_at_four_pixels() {
        assert_eq!(shake_offset(0.0), 0.0);
        assert_eq!(shake_offset(1.0), 0.0);
        assert_eq!(shake_offset(0.25), -4.0);
        assert_eq!(shake_offset(0.5), 4.0);
        let max = (0..=100)
            .map(|i| shake_offset(i as f32 / 100.0).abs())
            .fold(0.0, f32::max);
        assert_eq!(max, 4.0);
    }
}
