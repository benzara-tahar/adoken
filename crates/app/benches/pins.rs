//! `cargo bench -p adoken --bench pins`: off-thread indexed snapshot and
//! per-frame cached presentation/jump overhead, over the full demo cache.

use std::hint::black_box;
use std::time::{Duration, Instant};

use adoken_app::pins::{render, snapshot};
use adoken_model::{Pin, PinSubject};
use adoken_store::write;
use criterion::{Criterion, criterion_group, criterion_main};

fn bench(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
    let store = adoken_demo::open_store_at(dir.path(), ds.now).unwrap();
    let initial_pins = adoken_demo::pins(&ds);
    store
        .write(|tx| {
            for wi in ds
                .work_items
                .iter()
                .filter(|wi| {
                    !initial_pins
                        .iter()
                        .any(|p| p.subject == PinSubject::WorkItem(wi.id))
                })
                .take(16)
            {
                write::add_pin(
                    tx,
                    &Pin {
                        subject: PinSubject::WorkItem(wi.id),
                        position: 0,
                        label: None,
                        title: wi.title.clone(),
                        pinned_at: ds.now,
                        opened_at: Some(ds.now),
                        seen_result: None,
                        unavailable: None,
                    },
                )?;
            }
            Ok(())
        })
        .unwrap();
    let options = snapshot::Options {
        now: ds.now,
        auto_unpin: false,
        stale_after: None,
    };
    let (cached, _) = snapshot::read(&store, options).unwrap();
    assert_eq!(cached.pins.len(), 20);

    let mut samples = Vec::with_capacity(200);
    for _ in 0..200 {
        let start = Instant::now();
        black_box(snapshot::read(&store, options).unwrap());
        samples.push(start.elapsed());
    }
    samples.sort_unstable();
    let p50 = samples[100];
    let p99 = samples[198];
    let budget = Duration::from_millis(5);
    eprintln!("pins snapshot: p50={p50:?}, p99={p99:?}, budget={budget:?}");
    assert!(p99 <= budget, "indexed 20-pin snapshot exceeds 5 ms budget");

    c.bench_function("pins/snapshot_20_full_cache", |b| {
        b.iter(|| black_box(snapshot::read(black_box(&store), options).unwrap()))
    });
    c.bench_function("pins/cached_frame_20", |b| {
        b.iter(|| {
            black_box(cached.summary());
            for pin in &cached.pins {
                black_box(render::look(black_box(pin), ds.now));
            }
        })
    });
    c.bench_function("pins/jump_cached_subject", |b| {
        b.iter(|| {
            for index in 0..9 {
                black_box(adoken_app::shell::pinned::selected_subject(
                    black_box(&cached),
                    black_box(index),
                ));
            }
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
