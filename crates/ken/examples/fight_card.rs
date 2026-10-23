//! Render a sample fight card (fake numbers) to a PNG for review:
//! `cargo run -p adoken-ken --example fight_card -- <out.png> [theme-id] [--names]`.

use adoken_ken::card::{self, ExportOptions, Palette};
use adoken_ken::week::{CardKo, FightCard, Week};
use adoken_ken::{Naming, Rank};
use adoken_notify::FakeClock;
use adoken_theme::ThemeRegistry;

/// Fri 2026-09-25 09:30 UTC, the demo org's "now".
const NOW: i64 = 1_790_328_600_000;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "fight-card.png".into());
    let theme = args.next().unwrap_or_else(|| "adoken-dark".into());
    let names = std::env::args().any(|a| a == "--names");
    let registry = ThemeRegistry::builtin();
    let variant = registry
        .variant(&theme)
        .unwrap_or_else(|| panic!("unknown theme {theme}"));
    let clock = FakeClock::new(NOW, 0);
    let card = FightCard {
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
    };
    let options = ExportOptions {
        show_repos: names,
        show_titles: names,
        background: None,
    };
    let svg = card::svg(
        &card,
        &Palette::from_tokens(&variant.colors),
        Naming::EnglishKanji,
        &options,
    );
    let png = card::png(&svg, &card::options()).expect("render");
    card::save(&png, std::path::Path::new(&out)).expect("save");
    println!("wrote {out}");
}
