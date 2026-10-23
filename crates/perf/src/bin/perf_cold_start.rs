//! Cold-start timer: launches the app N times with `ADOKEN_PERF=1` and times
//! process spawn → first presented frame (see `adoken_perf::cold_start`).
//! Optionally launches once more with `ADOKEN_PERF=frames=F` to time F forced
//! full redraws against the 16 ms frame budget.
//!
//! Usage: perf-cold-start <app-binary> [--runs N] [--warmup W] [--frames F] [--timeout S]
//!
//! Prints per-run lines, a summary, and `CHECK` rows (`adoken_perf::budget`).

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use adoken_perf::probe::{ENV, Event, unix_now_ns};
use adoken_perf::{Budget, Ms, Samples};

struct Args {
    app: PathBuf,
    runs: usize,
    warmup: usize,
    frames: u32,
    timeout: Duration,
}

const USAGE: &str =
    "usage: perf-cold-start <app-binary> [--runs N] [--warmup W] [--frames F] [--timeout SECS]";

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut app = None;
    let mut parsed = Args {
        app: PathBuf::new(),
        runs: 20,
        warmup: 1,
        frames: 120,
        timeout: Duration::from_secs(20),
    };
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| {
            args.next()
                .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))
        };
        let number = |flag: &str, raw: String| {
            raw.parse::<u64>()
                .map_err(|_| format!("{flag}: not a number: {raw}"))
        };
        match arg.as_str() {
            "--runs" => parsed.runs = number("--runs", value("--runs")?)? as usize,
            "--warmup" => parsed.warmup = number("--warmup", value("--warmup")?)? as usize,
            "--frames" => {
                parsed.frames = u32::try_from(number("--frames", value("--frames")?)?)
                    .map_err(|_| "--frames: too large".to_string())?
            }
            "--timeout" => {
                parsed.timeout = Duration::from_secs(number("--timeout", value("--timeout")?)?)
            }
            "-h" | "--help" => return Err(USAGE.into()),
            flag if flag.starts_with('-') => return Err(format!("unknown flag {flag}\n{USAGE}")),
            path => app = Some(PathBuf::from(path)),
        }
    }
    parsed.app = app.ok_or_else(|| USAGE.to_string())?;
    if parsed.runs == 0 {
        return Err("--runs must be at least 1".into());
    }
    Ok(parsed)
}

/// What one probed launch reported.
struct Launch {
    first_frame: Duration,
    frames: Vec<Duration>,
}

fn launch(app: &Path, mode: &str, timeout: Duration) -> Result<Launch, String> {
    let spawned_ns = unix_now_ns();
    let mut child = Command::new(app)
        .env(ENV, mode)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", app.display()))?;
    let deadline = Instant::now() + timeout;

    let (tx, rx) = mpsc::channel();
    let stdout = child.stdout.take().expect("piped stdout");
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if let Some(event) = Event::parse(&line)
                && tx.send(event).is_err()
            {
                break;
            }
        }
    });
    let mut stderr = child.stderr.take().expect("piped stderr");
    let stderr = thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });

    let mut first_frame_ns = None;
    let mut frames = Vec::new();
    loop {
        let wait = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(Event::FirstFrame { unix_ns }) => first_frame_ns = Some(unix_ns),
            Ok(Event::Frame { draw }) => frames.push(draw),
            Ok(Event::Done) => break,
            // stdout closed: the app exited (or crashed) without `done`.
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                kill(&mut child);
                return Err(format!(
                    "no `done` within {timeout:?} (is the cold-start probe installed in the app?)"
                ));
            }
        }
    }

    let status = wait_or_kill(&mut child, Duration::from_secs(5));
    let Some(first_frame_ns) = first_frame_ns else {
        let stderr = stderr.join().unwrap_or_default();
        let tail: Vec<&str> = stderr.lines().rev().take(15).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(format!(
            "app exited ({status}) without reporting a first frame; stderr tail:\n{}",
            tail.join("\n")
        ));
    };
    let first_frame = first_frame_ns
        .checked_sub(spawned_ns)
        .map(Duration::from_nanos)
        .ok_or("first frame timestamp precedes spawn (wall clock stepped backwards)")?;
    Ok(Launch {
        first_frame,
        frames,
    })
}

fn wait_or_kill(child: &mut Child, grace: Duration) -> String {
    let deadline = Instant::now() + grace;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.to_string(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                kill(child);
                return "killed after `done`: app did not quit".into();
            }
            Err(e) => return format!("wait failed: {e}"),
        }
    }
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn run(args: &Args) -> Result<(), String> {
    println!(
        "cold start: {} ({} runs after {} warm-up)",
        args.app.display(),
        args.runs,
        args.warmup
    );
    let mut cold = Samples::with_capacity("cold start -> first frame", args.runs);
    for i in 0..args.warmup + args.runs {
        let result = launch(&args.app, "1", args.timeout)?;
        let label = if i < args.warmup {
            "warm-up".to_string()
        } else {
            cold.record(result.first_frame);
            format!("run {:>2}", i - args.warmup + 1)
        };
        println!("  {label}: {}", Ms(result.first_frame));
    }
    let cold = cold.summary().expect("runs >= 1");
    println!("{cold}");

    let frame_check = if args.frames > 0 {
        let result = launch(&args.app, &format!("frames={}", args.frames), args.timeout)?;
        let mut draws = Samples::with_capacity("frame draw (full redraw)", result.frames.len());
        for &draw in &result.frames {
            draws.record(draw);
        }
        let draws = draws
            .summary()
            .ok_or("frames run reported no frame timings")?;
        println!("{draws}");
        Some(Budget::FRAME.check(&draws))
    } else {
        None
    };

    println!("{}", Budget::COLD_START.check(&cold).row());
    if let Some(check) = frame_check {
        println!("{}", check.row());
    }
    Ok(())
}

fn main() -> ExitCode {
    let result = parse_args().and_then(|args| run(&args));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("perf-cold-start: {message}");
            ExitCode::FAILURE
        }
    }
}
