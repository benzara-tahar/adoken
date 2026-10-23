//! Ken arcade layer stories (docs/design/ken.md): combo HUD frames and the
//! statusbar chip, K.O. overlay frames (and the reduce-motion toast), the
//! fight card dialog and its exported PNG, and Settings › Ken. Fixed
//! numbers and fixed animation times, so snapshots never drift.

use std::sync::Arc;

use adoken_app::ken::frames::{ko_frame, ko_toast_text};
use adoken_app::ken::{
    ComboHud, FightCardDialog, KenSettingsSection, KenToast, KoOverlay, StatusChip, preview_png,
    toast_stack,
};
use adoken_ken::card::Palette;
use adoken_ken::week::CardKo;
use adoken_ken::{FightCard, HitKind, KenSettings, Naming, Rank, Week};
use adoken_notify::FakeClock;
use adoken_theme::{radius, space, typography};
use adoken_ui::{ActiveTheme, TypeStyleExt};
use gpui::{AnyElement, App, Image, Window, div, img, prelude::*, px};

/// Fri 2026-09-25 09:30 UTC, the demo org's "now".
const NOW: i64 = adoken_demo::DEMO_NOW;
const WINDOW: i64 = 10 * 60_000;

fn caption(text: &str, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .text_color(theme.colors.text_tertiary)
        .type_style(typography::LABEL, theme)
        .child(text.to_owned())
}

fn hud(count: u32, rank: Rank, naming: Naming) -> ComboHud {
    ComboHud {
        count,
        rank,
        naming,
        remaining_ms: 6 * 60_000,
        window_ms: WINDOW,
        motion: true,
        appear_ms: f32::MAX,
        step_ms: f32::MAX,
        rank_up_ms: None,
        last_move: None,
        finish: None,
    }
}

fn frame(label: &str, child: impl IntoElement, cx: &App) -> impl IntoElement {
    let colors = &cx.theme().colors;
    div()
        .flex()
        .flex_col()
        .gap(px(space::S6))
        .child(caption(label, cx))
        .child(
            div()
                .w(px(180.0))
                .h(px(112.0))
                .p(px(space::S12))
                .rounded(px(radius::MD))
                .border_1()
                .border_color(colors.border_subtle)
                .bg(colors.bg_surface)
                .child(child),
        )
}

/// Combo HUD frames: appear, mid-pop with the move flash, rank up, the
/// hot last minute, reduce motion ("6 min" text), finish flourish; then the
/// statusbar chip states and badge toasts.
pub fn combo_hud_story(_: &mut Window, cx: &mut App) -> AnyElement {
    let hot = ComboHud {
        remaining_ms: 40_000,
        ..hud(9, Rank::Surge, Naming::EnglishKanji)
    };
    let reduced = ComboHud {
        motion: false,
        ..hud(12, Rank::Storm, Naming::RomajiKanji)
    };
    let row1 = div()
        .flex()
        .gap(px(space::S16))
        .child(frame(
            "×2 appears",
            ComboHud {
                appear_ms: 70.0,
                step_ms: 70.0,
                ..hud(2, Rank::Flurry, Naming::EnglishKanji)
            },
            cx,
        ))
        .child(frame(
            "×7 · move flash",
            ComboHud {
                step_ms: 120.0,
                last_move: Some((HitKind::Approve, 300.0)),
                ..hud(7, Rank::Gale, Naming::EnglishKanji)
            },
            cx,
        ))
        .child(frame(
            "rank up ×8",
            ComboHud {
                rank_up_ms: Some(70.0),
                ..hud(8, Rank::Surge, Naming::English)
            },
            cx,
        ))
        .child(frame("last minute", hot, cx))
        .child(frame("reduce motion", reduced, cx))
        .child(frame(
            "finish ×7",
            ComboHud {
                finish: Some((160.0, true)),
                ..hud(7, Rank::Gale, Naming::EnglishKanji)
            },
            cx,
        ));
    let colors = &cx.theme().colors;
    let chips = [
        (
            "combo + streak",
            StatusChip {
                combo: Some((7, Rank::Gale, 6 * 60_000)),
                naming: Naming::EnglishKanji,
                streak_days: 12,
                shields: 1,
                ..StatusChip::default()
            },
        ),
        (
            "streak only",
            StatusChip {
                streak_days: 4,
                ..StatusChip::default()
            },
        ),
        (
            "paused",
            StatusChip {
                streak_days: 12,
                paused: true,
                ..StatusChip::default()
            },
        ),
        (
            "quiet mode",
            StatusChip {
                quiet: Some("Quiet mode until 15:40".into()),
                ..StatusChip::default()
            },
        ),
    ];
    let row2 = div()
        .flex()
        .gap(px(space::S16))
        .children(chips.map(|(label, chip)| {
            div()
                .flex()
                .flex_col()
                .gap(px(space::S6))
                .child(caption(label, cx))
                .child(
                    div()
                        .h(px(24.0))
                        .px(px(space::S12))
                        .flex()
                        .items_center()
                        .border_1()
                        .border_color(colors.border_subtle)
                        .bg(colors.bg_sidebar)
                        .child(chip),
                )
        }));
    let toasts = toast_stack([
        KenToast {
            text: "Combo ×7 · Gale 疾風 — 4 PRs reviewed in 38 min".into(),
            ko_badge: false,
            hint: None,
        },
        KenToast {
            text: "First strike · !4830 Add retry budget".into(),
            ko_badge: false,
            hint: None,
        },
        KenToast {
            text: "Your week in review is ready".into(),
            ko_badge: false,
            hint: Some("> Ken: Show fight card".into()),
        },
    ]);
    div()
        .flex()
        .flex_col()
        .gap(px(space::S24))
        .child(row1)
        .child(row2)
        .child(div().flex().child(toasts))
        .into_any_element()
}

fn ko_stage(overlay: KoOverlay, cx: &App) -> AnyElement {
    let colors = &cx.theme().colors;
    // A stand-in for app content under the pointer-transparent overlay.
    div()
        .relative()
        .size_full()
        .rounded(px(radius::MD))
        .overflow_hidden()
        .bg(colors.bg_surface)
        .child(
            div()
                .p(px(space::S24))
                .flex()
                .flex_col()
                .gap(px(space::S8))
                .children((0..6).map(|i| {
                    div()
                        .h(px(18.0))
                        .w(px(220.0 + 60.0 * (i % 3) as f32))
                        .rounded(px(radius::XS))
                        .bg(colors.bg_raised)
                })),
        )
        .child(overlay)
        .into_any_element()
}

/// K.O. at t = 700 ms: letters landed, caption in.
pub fn ko_story(_: &mut Window, cx: &mut App) -> AnyElement {
    ko_stage(
        KoOverlay {
            prs: vec![(4821, "Retry idempotency keys".into())],
            double: false,
            frame: ko_frame(700.0, None),
            shake: true,
            subtitle: "merged into main · 3 reviewers".into(),
        },
        cx,
    )
}

/// DOUBLE K.O. at t = 260 ms: mid-impact (letters landing, flash, shake).
pub fn ko_double_story(_: &mut Window, cx: &mut App) -> AnyElement {
    ko_stage(
        KoOverlay {
            prs: vec![
                (4821, "Retry idempotency keys".into()),
                (4822, "Drop legacy retry flag".into()),
            ],
            double: true,
            frame: ko_frame(420.0, None),
            shake: true,
            subtitle: "merged into main".into(),
        },
        cx,
    )
}

/// Reduce motion (and subtle intensity, quiet hours): no overlay, a K.O.
/// toast with the glyph badge.
pub fn ko_reduced_story(_: &mut Window, cx: &mut App) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(space::S8))
        .child(caption("Reduce motion · the K.O. is a toast", cx))
        .child(toast_stack([
            KenToast {
                text: ko_toast_text(&[(4821, String::new())], false).into(),
                ko_badge: true,
                hint: None,
            },
            KenToast {
                text: ko_toast_text(&[(4821, String::new()), (4822, String::new())], true).into(),
                ko_badge: true,
                hint: None,
            },
        ]))
        .into_any_element()
}

fn card() -> FightCard {
    let clock = FakeClock::new(NOW, 0);
    FightCard {
        week: Week::of(NOW, &clock).previous(&clock),
        hits: 34,
        best_combo: Some((9, Rank::Surge)),
        kos: vec![
            CardKo {
                pr: 4821,
                title: "Retry idempotency keys".into(),
                repo: Some("payments-api".into()),
                when: "Tue 14:02".into(),
                double: false,
            },
            CardKo {
                pr: 4830,
                title: "Add retry budget".into(),
                repo: Some("checkout-web".into()),
                when: "Thu 10:47".into(),
                double: true,
            },
        ],
        streak_days: 12,
        shields: 1,
        first_strikes: 2,
        comebacks: 1,
        assists: 5,
        prs_reviewed: 14,
    }
}

/// The weekly fight card dialog (numbers only; export options off).
pub fn fight_card_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let dialog = window.use_keyed_state("ken-fight-card", cx, |_, cx| {
        FightCardDialog::with_card(card(), Naming::EnglishKanji, cx)
    });
    div()
        .relative()
        .size_full()
        .child(dialog)
        .into_any_element()
}

/// The exported 1200 × 675 share image, rendered by the export path
/// (resvg) and shown at half size.
pub fn fight_card_png_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let theme_id = cx.theme().id.clone();
    let palette = Palette::from_tokens(&cx.theme().raw);
    let image = window.use_keyed_state(
        gpui::ElementId::Name(format!("ken-png-{theme_id}").into()),
        cx,
        move |_, _| preview_png(&card(), &palette, Naming::EnglishKanji),
    );
    let image: Option<Arc<Image>> = image.read(cx).clone();
    div()
        .flex()
        .flex_col()
        .gap(px(space::S8))
        .child(caption("Export image… · 1200 × 675 PNG, numbers only", cx))
        .children(image.map(|image| img(image).w(px(600.0)).h(px(337.5))))
        .into_any_element()
}

/// Settings › Ken with arcade mode on.
pub fn settings_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let section = window.use_keyed_state("ken-settings", cx, |_, _| {
        KenSettingsSection::with_settings(KenSettings {
            enabled: true,
            ..KenSettings::default()
        })
    });
    div().w(px(720.0)).child(section).into_any_element()
}
