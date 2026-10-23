//! `cargo bench -p adoken --bench resource_meter`: one resource meter sample
//! on the primary path (spec 0023 AC-12): `proc_pid_rusage`, the Mach
//! timebase conversion and the statusbar text. Budget < 5 µs per sample;
//! the bench fails above it. (In `crates/app`, not `crates/perf`: the
//! source lives in the app crate, which `crates/perf` can't depend on.)

#[cfg(target_os = "macos")]
mod mac {
    use std::hint::black_box;
    use std::time::{Duration, Instant};

    use adoken_app::resource::meter::{Memory, MeterText, cpu_over, cpu_percent};
    use adoken_app::resource::source::ProcSource;
    use criterion::Criterion;

    /// Read, convert, format: what the sampler does per wake.
    fn sample(source: &ProcSource, previous_cpu_ns: u64) -> String {
        let reading = source.primary().expect("proc_pid_rusage reads");
        let cpu = cpu_over(reading.cpu_ns.saturating_sub(previous_cpu_ns), 2_000).map(cpu_percent);
        MeterText {
            memory: Memory::from_bytes(reading.footprint),
            cpu,
        }
        .full()
    }

    pub fn bench(c: &mut Criterion) {
        let source = ProcSource::new();
        let start_cpu = source.primary().unwrap().cpu_ns;

        // The gate: the median of 100 batches of 1,000 samples, per sample.
        const BATCH: u32 = 1_000;
        let mut per_sample: Vec<Duration> = (0..100)
            .map(|_| {
                let start = Instant::now();
                for _ in 0..BATCH {
                    black_box(sample(black_box(&source), start_cpu));
                }
                start.elapsed() / BATCH
            })
            .collect();
        per_sample.sort_unstable();
        let p50 = per_sample[50];
        let budget = Duration::from_micros(5);
        eprintln!("resource meter sample: p50={p50:?}, budget={budget:?}");
        assert!(p50 < budget, "one meter sample exceeds the 5 µs budget");

        c.bench_function("resource_meter/primary_sample", |b| {
            b.iter(|| black_box(sample(black_box(&source), start_cpu)))
        });
    }
}

#[cfg(target_os = "macos")]
criterion::criterion_group!(benches, mac::bench);
#[cfg(target_os = "macos")]
criterion::criterion_main!(benches);

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("resource_meter: skipped (the meter has a source on macOS only)");
}
