//! Pure view-model for Today (spec 0017, `docs/design/today.md`): loading
//! from the store, the responsive layout (two columns or one), the
//! selection the keys move over, and what each row opens. No gpui.

use std::collections::HashMap;
use std::sync::Arc;

use adoken_model::{Millis, Subject};
use adoken_nudge::{
    Clock, DataHealth, Day, FocusState, Ledger, Nudge, NudgeAction, Pass, PrRecords, Settings,
    Snapshot, SyncHealth, evaluate, today,
};
use adoken_store::Store;

/// Everything the view renders, recomputed after every pass or triage action.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub snapshot: Arc<Snapshot>,
    pub ledger: Ledger,
    pub pass: Pass,
    pub day: Day,
    pub now: Millis,
    /// Up next (0006 AC-9): on workdays once working hours begin, until `e`
    /// hides it for the day.
    pub up_next_visible: bool,
}

impl Loaded {
    /// Runs one pass over `snapshot` at `now`, updating `ledger`.
    pub fn compute(
        snapshot: Arc<Snapshot>,
        mut ledger: Ledger,
        settings: &Settings,
        now: Millis,
    ) -> Self {
        let pass = evaluate(&snapshot, settings, &mut ledger, now);
        Self::present(snapshot, ledger, pass, settings, now)
    }

    fn present(
        snapshot: Arc<Snapshot>,
        ledger: Ledger,
        pass: Pass,
        settings: &Settings,
        now: Millis,
    ) -> Self {
        let day = today(&snapshot, settings, &pass, &ledger, now);
        Self {
            up_next_visible: ledger.today_visible(now, &settings.hours),
            snapshot,
            ledger,
            pass,
            day,
            now,
        }
    }

    /// Re-derive after a triage action changed the ledger, without firing:
    /// user actions never send nudges, only passes do.
    pub fn refresh(&mut self, settings: &Settings) {
        let mut scratch = self.ledger.clone();
        let mut pass = evaluate(&self.snapshot, settings, &mut scratch, self.now);
        pass.fired.clear();
        let ledger = std::mem::take(&mut self.ledger);
        *self = Self::present(self.snapshot.clone(), ledger, pass, settings, self.now);
    }
}

/// Store health for the §3 states: nudges wait for the first full sync (a
/// seeded demo store counts as synced); freshness and scopes come from the
/// running sync (spec 0017 FR-10).
fn health(store: &Store, sync: &SyncHealth, now: Millis) -> adoken_store::Result<DataHealth> {
    let initial = store.last_sync()?.is_some() || store.meta("demo")?.is_some();
    Ok(sync.data_health(initial, now))
}

/// Blocking load + pass; run on a background executor. `Ok(None)` before
/// "me" is known. Saves the ledger when the pass changed it and returns the
/// store generation after that save (so the view can ignore its own write).
pub fn load(
    store: &Store,
    settings: &Settings,
    clock: &dyn Clock,
    sync: &SyncHealth,
) -> adoken_store::Result<Option<(Loaded, u64)>> {
    adoken_nudge::store::migrate(store)?;
    let now = clock.now();
    let day_start = settings.hours.day_start(now);
    let Some(snapshot) =
        adoken_nudge::store::load_snapshot(store, health(store, sync, now)?, day_start)?
    else {
        return Ok(None);
    };
    let before = adoken_nudge::store::load_ledger(store)?;
    let loaded = Loaded::compute(Arc::new(snapshot), before.clone(), settings, now);
    if loaded.ledger != before {
        adoken_nudge::store::save_ledger(store, &loaded.ledger)?;
    }
    let mut sink = adoken_nudge::store::InboxSink::new(store);
    for (subject, kind) in &loaded.pass.resolved {
        adoken_nudge::NudgeSink::resolve(&mut sink, subject, *kind, loaded.now);
    }
    for nudge in &loaded.pass.fired {
        adoken_nudge::NudgeSink::deliver(&mut sink, nudge, loaded.now);
    }
    sink.finish()?;
    Ok(Some((loaded, store.generation())))
}

/// The same selection as the store path over an in-memory demo dataset.
pub fn demo_snapshot(ds: &adoken_demo::Dataset, settings: &Settings, now: Millis) -> Snapshot {
    let extras: HashMap<i64, PrRecords<'_>> = ds
        .pr_extras
        .iter()
        .map(|(id, extras)| {
            (
                *id,
                PrRecords {
                    iterations: &extras.iterations,
                    threads: &extras.threads,
                    files: extras.files.len() as u32,
                },
            )
        })
        .collect();
    Snapshot::from_records(
        &ds.me,
        &ds.people,
        &ds.prs,
        ds.pr_extras
            .iter()
            .map(|(id, extras)| (*id, extras.threads.clone())),
        &ds.runs,
        &ds.work_items,
    )
    .with_day_records(
        settings.hours.day_start(now),
        &ds.prs,
        &extras,
        &ds.work_items,
    )
}

/// Today over an in-memory demo dataset (gallery stories, tests): a fresh
/// ledger, one pass at `now`.
pub fn demo(ds: &adoken_demo::Dataset, settings: &Settings, now: Millis) -> Loaded {
    Loaded::compute(
        Arc::new(demo_snapshot(ds, settings, now)),
        Ledger::default(),
        settings,
        now,
    )
}

// ── layout and keyboard ─────────────────────────────────────────────────

/// FR-24: Today's own width from which the page has two columns.
pub const BREAKPOINT: f32 = 960.0;
/// The right column's width in two columns.
pub const RIGHT_COLUMN: f32 = 320.0;
/// Your PRs never narrows below this in two columns.
pub const PRS_MIN: f32 = 520.0;
/// Content is capped at this width and centered.
pub const CONTENT_MAX: f32 = 1280.0;

/// A selectable row, by position in the current [`Day`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Row {
    UpNext,
    Pr(usize),
    Waiting(usize),
    Loose(usize),
}

/// A row by what it shows: survives reorders, reloads and the breakpoint
/// (FR-24 "with the selection kept", 0018 back/forward).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RowKey {
    UpNext,
    /// One of my PRs.
    Pr(i64),
    /// A review waiting on me.
    Review(i64),
    WorkItem(i64),
}

/// Which rows exist and how they're arranged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    /// Two columns (Today's width ≥ [`BREAKPOINT`]).
    pub wide: bool,
    /// Up next is drawn and selectable.
    pub up_next: bool,
    pub prs: usize,
    pub waiting: usize,
    pub loose: usize,
}

impl Layout {
    pub fn of(loaded: &Loaded, wide: bool) -> Self {
        let day = &loaded.day;
        Layout {
            wide,
            up_next: loaded.up_next_visible && matches!(day.focus, FocusState::Item(_)),
            prs: day.tracks.len(),
            waiting: day.waiting.len(),
            loose: day.loose.len(),
        }
    }

    pub fn is_wide(width: f32) -> bool {
        width >= BREAKPOINT
    }

    fn right(&self) -> impl Iterator<Item = Row> {
        self.up_next
            .then_some(Row::UpNext)
            .into_iter()
            .chain((0..self.waiting).map(Row::Waiting))
            .chain((0..self.loose).map(Row::Loose))
    }

    /// Two columns: Your PRs, then the right column (Up next, Waiting on
    /// you, Loose ends). One column: Up next, Your PRs, Waiting, Loose ends.
    pub fn columns(&self) -> Vec<Vec<Row>> {
        let prs = (0..self.prs).map(Row::Pr);
        if self.wide {
            vec![prs.collect(), self.right().collect()]
        } else {
            let mut single: Vec<Row> = self.up_next.then_some(Row::UpNext).into_iter().collect();
            single.extend(prs);
            single.extend((0..self.waiting).map(Row::Waiting));
            single.extend((0..self.loose).map(Row::Loose));
            vec![single]
        }
    }

    /// `(column, index)` of `row`.
    pub fn locate(&self, row: Row) -> Option<(usize, usize)> {
        self.columns()
            .iter()
            .enumerate()
            .find_map(|(c, rows)| rows.iter().position(|r| *r == row).map(|i| (c, i)))
    }

    /// `Home`: the first row of the page.
    pub fn first(&self) -> Option<Row> {
        self.columns().into_iter().flatten().next()
    }

    /// `End`: the last row of the page.
    pub fn last(&self) -> Option<Row> {
        self.columns().into_iter().flatten().last()
    }

    /// `↓`/`↑`, `j`/`k`: the next or previous row in the same column,
    /// clamped at its ends.
    pub fn vertical(&self, row: Row, delta: isize) -> Row {
        let columns = self.columns();
        let Some((c, i)) = self.locate(row) else {
            return self.first().unwrap_or(row);
        };
        let rows = &columns[c];
        rows[i.saturating_add_signed(delta).min(rows.len() - 1)]
    }

    /// `→`/`l` (`right`) and `←`/`h`: in two columns, the other column's row
    /// nearest vertically (`center` gives a painted row's vertical center;
    /// unknown rows fall back to the same index); in one column, the same as
    /// `↓`/`↑`.
    pub fn across(&self, row: Row, right: bool, center: impl Fn(Row) -> Option<f32>) -> Row {
        if !self.wide {
            return self.vertical(row, if right { 1 } else { -1 });
        }
        let columns = self.columns();
        let Some((c, i)) = self.locate(row) else {
            return self.first().unwrap_or(row);
        };
        let target = if right { 1 } else { 0 };
        if c == target || columns[target].is_empty() {
            return row;
        }
        let rows = &columns[target];
        match center(row) {
            Some(y) => *rows
                .iter()
                .min_by(|a, b| {
                    let d = |r: &Row| center(*r).map_or(f32::MAX, |c| (c - y).abs());
                    d(a).total_cmp(&d(b))
                })
                .expect("non-empty column"),
            None => rows[i.min(rows.len() - 1)],
        }
    }
}

pub fn key(loaded: &Loaded, row: Row) -> Option<RowKey> {
    let day = &loaded.day;
    Some(match row {
        Row::UpNext => RowKey::UpNext,
        Row::Pr(i) => RowKey::Pr(day.tracks.get(i)?.pr_id),
        Row::Waiting(i) => RowKey::Review(day.waiting.get(i)?.pr_id),
        Row::Loose(i) => RowKey::WorkItem(day.loose.get(i)?.id),
    })
}

/// The row showing `key` in `layout`, if it still exists.
pub fn find(loaded: &Loaded, layout: &Layout, key: RowKey) -> Option<Row> {
    let day = &loaded.day;
    let row = match key {
        RowKey::UpNext => layout.up_next.then_some(Row::UpNext)?,
        RowKey::Pr(id) => Row::Pr(day.tracks.iter().position(|t| t.pr_id == id)?),
        RowKey::Review(id) => Row::Waiting(day.waiting.iter().position(|c| c.pr_id == id)?),
        RowKey::WorkItem(id) => Row::Loose(day.loose.iter().position(|l| l.id == id)?),
    };
    layout.locate(row).map(|_| row)
}

/// What activating a row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Pr(i64),
    WorkItem(i64),
    Run(i64),
}

pub fn nudge_target(nudge: &Nudge) -> Option<Target> {
    match (nudge.action, &nudge.subject) {
        (NudgeAction::OpenFailingRun(run), _) => Some(Target::Run(run)),
        (_, Subject::Pr(id)) => Some(Target::Pr(*id)),
        (_, Subject::WorkItem(id)) => Some(Target::WorkItem(*id)),
        _ => None,
    }
}

/// `↩` on a row: Up next runs its nudge's action (a failing run opens the
/// run); rows open their PR or work item.
pub fn target(loaded: &Loaded, row: Row) -> Option<Target> {
    let day = &loaded.day;
    match row {
        Row::UpNext => match &day.focus {
            FocusState::Item(focus) => nudge_target(&focus.nudge),
            _ => None,
        },
        Row::Waiting(i) => Some(Target::Pr(day.waiting.get(i)?.pr_id)),
        Row::Pr(i) => Some(Target::Pr(day.tracks.get(i)?.pr_id)),
        Row::Loose(i) => Some(Target::WorkItem(day.loose.get(i)?.id)),
    }
}

pub fn pin_subject(loaded: &Loaded, row: Row) -> Option<adoken_model::PinSubject> {
    use adoken_model::PinSubject;
    match target(loaded, row)? {
        Target::Pr(id) => Some(PinSubject::Pr(id)),
        Target::WorkItem(id) => Some(PinSubject::WorkItem(id)),
        Target::Run(id) => loaded
            .snapshot
            .runs
            .iter()
            .find(|run| run.id == id)
            .map(|run| PinSubject::Definition {
                project_id: run.project_id.clone(),
                id: run.pipeline_id,
            }),
    }
}

/// The nudge `s`/`d`/`x` act on for a row, if any (a PR row's top
/// condition, 0017 FR-23).
pub fn nudge(loaded: &Loaded, row: Row) -> Option<&Nudge> {
    let day = &loaded.day;
    match row {
        Row::UpNext => match &day.focus {
            FocusState::Item(focus) => Some(&focus.nudge),
            _ => None,
        },
        Row::Waiting(i) => day.waiting.get(i).map(|c| &c.nudge),
        Row::Pr(i) => day.tracks.get(i)?.nudge.as_ref(),
        Row::Loose(i) => day.loose.get(i).map(|l| &l.nudge),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_nudge::FocusState;

    fn demo_loaded() -> Loaded {
        let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
        demo(&ds, &Settings::default(), ds.now)
    }

    fn layout(wide: bool) -> Layout {
        Layout {
            wide,
            up_next: true,
            prs: 3,
            waiting: 2,
            loose: 1,
        }
    }

    #[test]
    fn ac23_the_breakpoint_switches_between_two_columns_and_one() {
        assert!(!Layout::is_wide(959.0));
        assert!(Layout::is_wide(960.0));
        assert_eq!(
            layout(true).columns(),
            [
                vec![Row::Pr(0), Row::Pr(1), Row::Pr(2)],
                vec![Row::UpNext, Row::Waiting(0), Row::Waiting(1), Row::Loose(0)],
            ]
        );
        assert_eq!(
            layout(false).columns(),
            [vec![
                Row::UpNext,
                Row::Pr(0),
                Row::Pr(1),
                Row::Pr(2),
                Row::Waiting(0),
                Row::Waiting(1),
                Row::Loose(0),
            ]],
            "FR-24 order"
        );
    }

    #[test]
    fn ac24_keys_move_within_and_across_columns() {
        let wide = layout(true);
        // Rows paint top to bottom: PRs 100 apart, right rows 60 apart.
        let center = |row: Row| {
            Some(match row {
                Row::Pr(i) => 50.0 + 100.0 * i as f32,
                Row::UpNext => 40.0,
                Row::Waiting(i) => 130.0 + 60.0 * i as f32,
                Row::Loose(_) => 250.0,
            })
        };
        let second = Row::Pr(1); // center 150
        let right = wide.across(second, true, center);
        assert_eq!(right, Row::Waiting(0), "nearest vertically (130 vs 190)");
        assert_eq!(
            wide.vertical(right, 1),
            Row::Waiting(1),
            "j stays in the column"
        );
        assert_eq!(wide.across(Row::Waiting(1), false, center), Row::Pr(1));
        assert_eq!(wide.across(second, false, center), second, "already left");
        assert_eq!(wide.vertical(Row::Pr(2), 1), Row::Pr(2), "clamped");
        assert_eq!(
            wide.across(second, true, |_| None),
            Row::Waiting(0),
            "same index"
        );
        assert_eq!(
            (wide.first(), wide.last()),
            (Some(Row::Pr(0)), Some(Row::Loose(0)))
        );

        let narrow = layout(false);
        assert_eq!(
            narrow.across(Row::Pr(2), true, center),
            Row::Waiting(0),
            "l acts as j"
        );
        assert_eq!(narrow.across(Row::Pr(0), false, center), Row::UpNext);
    }

    #[test]
    fn rows_are_found_again_by_what_they_show() {
        let loaded = demo_loaded();
        for wide in [true, false] {
            let layout = Layout::of(&loaded, wide);
            for row in layout.columns().into_iter().flatten() {
                let key = key(&loaded, row).expect("a key");
                assert_eq!(find(&loaded, &layout, key), Some(row));
            }
        }
    }

    #[test]
    fn ac24_enter_opens_the_row_and_up_next_runs_its_action() {
        let loaded = demo_loaded();
        let card = &loaded.day.waiting[0];
        assert_eq!(
            target(&loaded, Row::Waiting(0)),
            Some(Target::Pr(card.pr_id))
        );
        assert_eq!(
            target(&loaded, Row::Pr(0)),
            Some(Target::Pr(loaded.day.tracks[0].pr_id))
        );
        if let Some(item) = loaded.day.loose.first() {
            assert_eq!(
                target(&loaded, Row::Loose(0)),
                Some(Target::WorkItem(item.id))
            );
        }
        let FocusState::Item(focus) = &loaded.day.focus else {
            panic!("the demo org has an Up next");
        };
        assert_eq!(target(&loaded, Row::UpNext), nudge_target(&focus.nudge));
        assert_eq!(nudge(&loaded, Row::UpNext), Some(&focus.nudge));
    }

    #[test]
    fn ac21_e_hides_up_next_and_waiting_moves_to_the_top() {
        let mut loaded = demo_loaded();
        loaded.up_next_visible = true;
        assert_eq!(Layout::of(&loaded, true).columns()[1][0], Row::UpNext);
        let settings = Settings::default();
        loaded.ledger.dismiss_today(loaded.now, &settings.hours);
        loaded.refresh(&settings);
        assert!(!loaded.up_next_visible);
        let layout = Layout::of(&loaded, true);
        assert_eq!(layout.columns()[1][0], Row::Waiting(0));
    }

    #[test]
    fn triage_refresh_never_fires_and_moves_up_next_on() {
        let mut loaded = demo_loaded();
        let FocusState::Item(first) = loaded.day.focus.clone() else {
            panic!("the demo org has an Up next");
        };
        loaded
            .ledger
            .dismiss(&first.nudge.subject, first.nudge.kind);
        loaded.refresh(&Settings::default());
        assert!(loaded.pass.fired.is_empty());
        let FocusState::Item(next) = &loaded.day.focus else {
            panic!("more is due");
        };
        assert_ne!(next.nudge.subject, first.nudge.subject);
    }

    /// The store path (indexed reads, today's merges, pushes, threads, file
    /// counts) builds the same day as the in-memory demo selection.
    #[test]
    fn the_store_path_builds_the_same_day_as_the_in_memory_demo() {
        let now = adoken_demo::DEMO_NOW + 6 * adoken_model::time::HOUR;
        let dir = tempfile::tempdir().unwrap();
        let store = adoken_demo::open_store_at(dir.path(), now).unwrap();
        let settings = Settings::default();
        let clock = adoken_nudge::FixedClock::new(now);
        let (stored, _) = load(&store, &settings, &clock, &SyncHealth::default())
            .unwrap()
            .expect("me");
        let ds = adoken_demo::dataset_at(adoken_demo::DEFAULT_SEED, now);
        let memory = demo(&ds, &settings, now);

        let waiting = |l: &Loaded| {
            l.day
                .waiting
                .iter()
                .map(|c| (c.pr_id, c.files, c.threads))
                .collect::<Vec<_>>()
        };
        assert!(!stored.day.waiting.is_empty());
        assert_eq!(waiting(&stored), waiting(&memory));
        let tracks = |l: &Loaded| {
            l.day
                .tracks
                .iter()
                .map(|t| (t.pr_id, t.stage, t.blocker, t.line.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(tracks(&stored), tracks(&memory));
        assert_eq!(stored.day.summary, memory.day.summary);
        assert_eq!(stored.day.focus, memory.day.focus);
    }

    #[test]
    fn today_rows_pin_the_canonical_subject_not_the_nudge_or_run_id() {
        use adoken_model::PinSubject;
        let loaded = demo_loaded();
        for (ix, card) in loaded.day.waiting.iter().enumerate() {
            assert_eq!(
                pin_subject(&loaded, Row::Waiting(ix)),
                Some(PinSubject::Pr(card.pr_id))
            );
        }
        for (ix, card) in loaded.day.loose.iter().enumerate() {
            assert_eq!(
                pin_subject(&loaded, Row::Loose(ix)),
                Some(PinSubject::WorkItem(card.id))
            );
        }
        assert_eq!(pin_subject(&loaded, Row::Waiting(usize::MAX)), None);
        assert_eq!(pin_subject(&loaded, Row::Loose(usize::MAX)), None);
    }
}
