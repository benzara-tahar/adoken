//! `cargo bench -p adoken --bench celebrate`: one frame of celebration
//! simulation at the 64-particle cap (spec 0022 AC-17, FR-12): a calm
//! Milestone burst, an arcade Step and an arcade Milestone's seal sparks
//! overlapping, trimmed to the shared cap. Painting is measured by the
//! frame recorder in `crates/gallery/tests/celebrate_smoke.rs`.

use std::hint::black_box;
use std::time::{Duration, Instant};

use adoken_app::celebrate::{Anchor, Emission, Fx};
use adoken_ken::celebrate::{Style, Tier};
use adoken_ken::confetti::MAX_LIVE;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use gpui::{Bounds, point, px, size};

const WINDOW: (f32, f32) = (1440.0, 900.0);
const FRAME_MS: f32 = 16.0;

/// Three overlapping effects, stepped past the seal's sparks (120 ms):
/// the cap is full.
fn at_cap() -> Fx {
    let mut fx = Fx::new(0x0022);
    for (n, (tier, style)) in [
        (Tier::Milestone, Style::Calm),
        (Tier::Step, Style::Arcade),
        (Tier::Milestone, Style::Arcade),
    ]
    .into_iter()
    .enumerate()
    {
        let chip = Bounds::new(
            point(px(400.0 + 200.0 * n as f32), px(500.0)),
            size(px(90.0), px(24.0)),
        );
        fx.start(
            &Emission {
                anchor: Anchor::WorkItem(n as i64),
                tier,
                style,
                fallback: None,
            },
            chip,
        );
    }
    for _ in 0..8 {
        fx.step(FRAME_MS, WINDOW.0, WINDOW.1);
    }
    assert_eq!(fx.live_particles(), MAX_LIVE, "the shared cap is full");
    fx
}

fn bench(c: &mut Criterion) {
    let mut samples = Vec::with_capacity(500);
    for _ in 0..500 {
        let mut fx = at_cap();
        let start = Instant::now();
        fx.step(FRAME_MS, WINDOW.0, WINDOW.1);
        black_box(&fx);
        samples.push(start.elapsed());
    }
    samples.sort_unstable();
    let p50 = samples[250];
    let p99 = samples[495];
    let budget = Duration::from_millis(2);
    eprintln!(
        "celebrate step at {MAX_LIVE} particles: p50={p50:?}, p99={p99:?}, budget={budget:?}"
    );
    assert!(
        p99 <= budget,
        "celebration simulation exceeds 2 ms at the cap"
    );

    c.bench_function("celebrate/step_at_cap", |b| {
        b.iter_batched(
            at_cap,
            |mut fx| {
                fx.step(FRAME_MS, WINDOW.0, WINDOW.1);
                fx
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
