//! Demo inbox for gallery stories: the fake Contoso store at `DEMO_NOW`,
//! first-run baseline + the 48 h backlog, then a stretch of the live demo
//! stream on a fake clock. Deterministic; fake data only.

use std::sync::{Arc, LazyLock};

use adoken_demo::DEMO_NOW;
use adoken_model::Millis;
use adoken_model::time::MINUTE;
use adoken_notify::source::{self, Enricher};
use adoken_notify::{
    DEFAULT_MORNING, DeliveryEnv, Effects, Engine, EngineState, FakeClock, Incoming,
    NotifySettings, SnoozeChoice,
};

/// The stories' "now": 25 minutes into the live stream.
pub const STORY_NOW: Millis = DEMO_NOW + 25 * MINUTE;

/// Built once per process (seeding the store is the slow part).
static STATE: LazyLock<EngineState> = LazyLock::new(build);

pub fn state() -> EngineState {
    STATE.clone()
}

pub fn clock() -> FakeClock {
    FakeClock::new(STORY_NOW, 0)
}

fn build() -> EngineState {
    let dir = std::env::temp_dir().join(format!("adoken-inbox-demo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = load(&dir).unwrap_or_else(|error| {
        eprintln!("inbox demo: {error}");
        EngineState::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    state
}

fn load(dir: &std::path::Path) -> adoken_store::Result<EngineState> {
    let store = adoken_demo::open_store_at(dir, DEMO_NOW)?;
    adoken_notify::persist::migrate(&store)?;
    let clock = FakeClock::new(DEMO_NOW, 0);
    let mut settings = NotifySettings::default();
    // Show the quiet-hours pill and a delivery window in the stories.
    settings.quiet_hours.enabled = true;
    settings.windows.insert(
        adoken_notify::Source::Pipelines,
        crate::settings::notify_rules::work_hours(),
    );
    let mut engine = Engine::new(Arc::new(clock.clone()), settings);

    let enricher = Enricher::load(&store)?;
    for incoming in source::baseline(&store, &enricher, DEMO_NOW)? {
        engine.ingest_silently(incoming);
    }
    let mut cursor = 0;
    loop {
        let batch = source::fetch(&store, cursor)?;
        for incoming in batch.incoming {
            engine.ingest_silently(incoming);
        }
        cursor = batch.progress.cursor.unwrap_or(cursor);
        if !batch.more {
            break;
        }
    }

    // Live stream after DEMO_NOW, enriched from the same store.
    let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
    let env = DeliveryEnv::default();
    for event in adoken_demo::events(&ds).take_while(|e| e.at <= STORY_NOW) {
        clock.set(event.at);
        let ctx = enricher.context(&store, &event)?;
        engine.ingest(
            Incoming::from_sync(&event, ctx),
            &env,
            &mut Effects::default(),
        );
    }
    clock.set(STORY_NOW);

    // A little triage history so every tab has content.
    let rows: Vec<_> = engine
        .list(&adoken_notify::Filter::default())
        .iter()
        .map(|n| n.id)
        .collect();
    for id in rows.iter().skip(3).step_by(4).take(4) {
        engine.open(*id);
    }
    for id in rows.iter().rev().take(3) {
        engine.mark_done(*id);
    }
    if let Some(id) = rows.get(6) {
        engine.snooze(*id, SnoozeChoice::TomorrowMorning, DEFAULT_MORNING);
    }
    if let Some(id) = rows.get(7) {
        engine.snooze(*id, SnoozeChoice::ThreeHours, DEFAULT_MORNING);
    }
    Ok(engine.state())
}
