//! Spec 0023 AC-2 probe: the real source's CPU time is in the right units on
//! both read paths. Its own test binary, so no other test's threads add CPU
//! to the process while it measures.

#![cfg(target_os = "macos")]

use std::time::Duration;

use adoken_app::resource::source::ProcSource;

/// This thread's CPU time, from the kernel's per-thread clock.
fn thread_cpu() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid out-pointer.
    let status = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    assert_eq!(status, 0, "clock_gettime(CLOCK_THREAD_CPUTIME_ID)");
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// Spin until this thread has used `cpu` of CPU time (a wall-clock bound
/// would under-run on a loaded machine).
fn busy(cpu: Duration) {
    let start = thread_cpu();
    let mut x = 0u64;
    while thread_cpu() - start < cpu {
        for i in 0..1_000u64 {
            x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(i));
        }
    }
    std::hint::black_box(x);
}

#[test]
fn a_500_ms_busy_loop_reads_as_500_ms_of_cpu_on_both_paths() {
    let source = ProcSource::new();
    let primary = source.primary().expect("proc_pid_rusage reads");
    let fallback = source.fallback().expect("task_info + getrusage read");
    busy(Duration::from_millis(500));
    let primary = source.primary().unwrap().cpu_ns - primary.cpu_ns;
    let fallback = source.fallback().unwrap().cpu_ns - fallback.cpu_ns;
    for (path, ns) in [("primary", primary), ("fallback", fallback)] {
        let ms = ns / 1_000_000;
        assert!(
            (450..=550).contains(&ms),
            "{path} path read {ms} ms of CPU for a 500 ms busy loop"
        );
    }
}

#[test]
fn both_paths_agree_on_memory() {
    // The fallback reads the same footprint the primary does (FR-4); the
    // process barely allocates between the two calls.
    let source = ProcSource::new();
    let primary = source.primary().unwrap();
    let fallback = source.fallback().unwrap();
    let diff = primary.footprint.abs_diff(fallback.footprint);
    assert!(
        diff < 4 << 20,
        "footprint {} vs {}",
        primary.footprint,
        fallback.footprint
    );
    assert!(primary.peak.unwrap() >= primary.footprint);
}
