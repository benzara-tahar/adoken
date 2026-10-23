use super::*;

#[test]
fn rename_trims_resets_and_caps_unicode_characters_not_bytes() {
    assert_eq!(normalized_label(None), None);
    assert_eq!(normalized_label(Some(" \n\t ".into())), None);
    assert_eq!(
        normalized_label(Some("  Payments CI  ".into())),
        Some("Payments CI".into())
    );
    let label = normalized_label(Some("🦀".repeat(61))).unwrap();
    assert_eq!(label, "🦀".repeat(60));
}

#[test]
fn reorder_clamps_at_both_ends_without_overflow() {
    assert_eq!(move_destination(0, -1, 20), 0);
    assert_eq!(move_destination(19, 1, 20), 19);
    assert_eq!(move_destination(2, -1, 20), 1);
    assert_eq!(move_destination(2, isize::MAX, 20), 19);
    assert_eq!(move_destination(2, isize::MIN, 20), 0);
    assert_eq!(move_destination(0, 1, 1), 0);
}

#[test]
fn snapshots_cannot_overwrite_optimistic_mutations_or_newer_commits() {
    assert!(!snapshot_is_current(1, 5, 99));
    assert!(!snapshot_is_current(0, 5, 4));
    assert!(snapshot_is_current(0, 5, 5));
    assert!(snapshot_is_current(0, 5, 6));
}

#[test]
fn known_missing_optional_scopes_refuse_only_the_affected_kind() {
    let scopes = vec!["vso.code".into(), "vso.work".into()];
    assert!(scope_check(PinKind::PullRequest, &scopes).is_ok());
    assert!(scope_check(PinKind::WorkItem, &scopes).is_ok());
    assert!(scope_check(PinKind::Definition, &scopes).is_err());
    assert!(scope_check(PinKind::Definition, &[]).is_ok());
    assert!(scope_check(PinKind::WorkItem, &["vso.work_write".into()]).is_ok());
}

fn fixture_pin(id: i64, position: u32) -> Pin {
    Pin {
        subject: PinSubject::WorkItem(id),
        position,
        label: Some(format!("Local {id}")),
        title: format!("Last known {id}"),
        pinned_at: 1,
        opened_at: None,
        seen_result: None,
        unavailable: None,
    }
}

#[test]
fn missing_subjects_survive_refresh_and_restart_in_original_order_with_labels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pins.sqlite");
    let store = Store::open(&path).unwrap();
    store
        .write(|connection| {
            write::add_pin(connection, &fixture_pin(8, 0))?;
            write::add_pin(connection, &fixture_pin(3, 1))?;
            Ok(())
        })
        .unwrap();
    let options = Options {
        now: 90 * adoken_model::time::DAY,
        auto_unpin: true,
        stale_after: None,
    };
    let first = refresh_blocking(&store, options).unwrap();
    assert_eq!(
        first
            .pins
            .iter()
            .map(|view| view.subject().clone())
            .collect::<Vec<_>>(),
        vec![PinSubject::WorkItem(8), PinSubject::WorkItem(3)]
    );
    assert_eq!(first.pins[0].display_title(), "Local 8");
    assert_eq!(first.pins[0].title, "Last known 8");
    assert!(matches!(first.pins[0].headline, status::Headline::Fetching));
    drop(store);
    let reopened = Store::open(path).unwrap();
    assert_eq!(
        refresh_blocking(&reopened, options).unwrap().pins,
        first.pins
    );
}

/// Spec 0025 AC-2: `⌃1`…`⌃9` (Alt on Windows) open no pin, from anywhere
/// including the sidebar, and `⌘P` has no per-pin jump commands.
#[test]
fn ac2_digit_keys_open_no_pin_and_the_palette_has_no_jump_commands() {
    use gpui::{KeyContext, Keymap, Keystroke};
    let keymap = Keymap::new(crate::key_bindings());
    let stack: Vec<KeyContext> = ["Workspace", SIDEBAR_CONTEXT]
        .iter()
        .map(|c| KeyContext::parse(c).expect("context"))
        .collect();
    for modifier in ["ctrl", "alt"] {
        for digit in 1..=9 {
            let keys = format!("{modifier}-{digit}");
            let typed = [Keystroke::parse(&keys).expect("keystroke")];
            let (bindings, _) = keymap.bindings_for_input(&typed, &stack);
            let action = bindings.first().map(|binding| binding.action().name());
            assert!(
                action.is_none_or(|name| !name.starts_with("pins::")),
                "{keys} runs {action:?}"
            );
        }
    }
    for demo in [false, true] {
        let titles: Vec<&str> = crate::palette_commands(demo)
            .iter()
            .map(|command| command.title)
            .filter(|title| {
                let title = title.to_lowercase();
                title.contains("jump to pin") || title.starts_with("open pin ")
            })
            .collect();
        assert!(titles.is_empty(), "per-pin jump commands: {titles:?}");
    }
}

#[test]
fn rapid_unpin_then_undo_commits_in_ui_order_even_when_restore_is_polled_first() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("pins.sqlite")).unwrap();
    let pin = fixture_pin(7, 0);
    store
        .write(|connection| {
            write::add_pin(connection, &pin)?;
            write::add_pin(connection, &fixture_pin(8, 1))?;
            Ok(())
        })
        .unwrap();
    let (removed, before_restore) = tokio::sync::oneshot::channel();
    let (restored, _) = tokio::sync::oneshot::channel();
    let remove = ordered_write(store.clone(), None, removed, |connection| {
        write::unpin(connection, &PinSubject::WorkItem(7))
    });
    let restore = ordered_write(
        store.clone(),
        Some(before_restore),
        restored,
        |connection| write::restore_pin(connection, &pin),
    );
    let (restore_result, remove_result) =
        futures::executor::block_on(async { futures::join!(restore, remove) });
    remove_result.0.unwrap();
    restore_result.0.unwrap();
    let persisted = store.with_conn(adoken_store::read::pins).unwrap();
    assert_eq!(
        persisted
            .iter()
            .map(|pin| pin.subject.clone())
            .collect::<Vec<_>>(),
        vec![PinSubject::WorkItem(7), PinSubject::WorkItem(8)]
    );
    assert_eq!(persisted[0].label, pin.label);
}

#[test]
fn failed_write_releases_serial_gate_and_rolls_back_before_next_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("pins.sqlite")).unwrap();
    store
        .write(|connection| {
            write::add_pin(connection, &fixture_pin(7, 0))?;
            Ok(())
        })
        .unwrap();
    let (failed, before_next) = tokio::sync::oneshot::channel();
    let (finished, _) = tokio::sync::oneshot::channel();
    let fail = ordered_write(store.clone(), None, failed, |connection| {
        write::rename_pin(connection, &PinSubject::WorkItem(7), "Must roll back")?;
        connection.execute("INSERT INTO deliberately_absent_table VALUES (1)", [])?;
        Ok(())
    });
    let next = ordered_write(store.clone(), Some(before_next), finished, |connection| {
        write::add_pin(connection, &fixture_pin(8, 1))
    });
    let (next_result, failed_result) =
        futures::executor::block_on(async { futures::join!(next, fail) });
    assert!(failed_result.0.is_err());
    next_result.0.unwrap();
    let snapshot = refresh_blocking(
        &store,
        Options {
            now: 10,
            auto_unpin: false,
            stale_after: None,
        },
    )
    .unwrap();
    assert_eq!(snapshot.pins[0].display_title(), "Local 7");
    assert_eq!(snapshot.pins[1].subject(), &PinSubject::WorkItem(8));
}

fn demo_fixture() -> (tempfile::TempDir, Store, adoken_demo::Dataset) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("pins.sqlite")).unwrap();
    let dataset = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
    adoken_demo::seed(&store, &dataset).unwrap();
    store
        .write(|connection| {
            for pin in adoken_store::read::pins(connection)? {
                write::unpin(connection, &pin.subject)?;
            }
            Ok(())
        })
        .unwrap();
    (dir, store, dataset)
}

#[test]
fn auto_unpin_rechecks_reopening_inside_delete_transaction() {
    let (_dir, store, dataset) = demo_fixture();
    let mut pr = dataset.prs[0].clone();
    pr.status = adoken_model::PrStatus::Completed;
    pr.closed = Some(dataset.now);
    let subject = PinSubject::Pr(pr.id);
    let mut pin = fixture_pin(pr.id, 0);
    pin.subject = subject.clone();
    store
        .write(|connection| {
            write::upsert_pr(connection, &pr)?;
            write::add_pin(connection, &pin)?;
            Ok(())
        })
        .unwrap();
    let options = Options {
        now: dataset.now + adoken_model::time::DAY,
        auto_unpin: true,
        stale_after: None,
    };
    let (before, candidates) = snapshot::read_candidates(&store, options).unwrap();
    assert_eq!(candidates[0].subject, subject);
    pr.status = adoken_model::PrStatus::Active;
    pr.closed = None;
    pr.updated = options.now;
    pr.title = "Reopened while refresh waited".into();
    store
        .write(|connection| write::upsert_pr(connection, &pr))
        .unwrap();
    let after = settle_refresh(&store, options, before, candidates).unwrap();
    assert_eq!(after.pins[0].subject(), &subject);
    assert_eq!(after.pins[0].title, pr.title);
    assert!(!after.pins[0].headline.is_closed());
    assert_eq!(
        store.with_conn(adoken_store::read::pins).unwrap()[0].subject,
        subject
    );
}

#[test]
fn stale_auto_unpin_cannot_delete_a_newly_recreated_pin_identity() {
    let (_dir, store, dataset) = demo_fixture();
    let mut pr = dataset.prs[0].clone();
    pr.status = adoken_model::PrStatus::Completed;
    pr.closed = Some(dataset.now);
    let mut original = fixture_pin(pr.id, 0);
    original.subject = PinSubject::Pr(pr.id);
    store
        .write(|connection| {
            write::upsert_pr(connection, &pr)?;
            write::add_pin(connection, &original)?;
            Ok(())
        })
        .unwrap();
    let options = Options {
        now: dataset.now + adoken_model::time::DAY,
        auto_unpin: true,
        stale_after: None,
    };
    let (before, candidates) = snapshot::read_candidates(&store, options).unwrap();
    let mut replacement = original.clone();
    replacement.pinned_at = options.now;
    replacement.label = Some("New user-authored pin".into());
    store
        .write(|connection| {
            write::unpin(connection, &original.subject)?;
            write::add_pin(connection, &replacement)?;
            Ok(())
        })
        .unwrap();
    settle_refresh(&store, options, before, candidates).unwrap();
    assert_eq!(
        store.with_conn(adoken_store::read::pins).unwrap()[0],
        replacement
    );
}

#[test]
fn initially_resultless_definition_records_first_completion_then_detects_flip() {
    let (_dir, store, dataset) = demo_fixture();
    let mut definition = dataset.pipelines[0].clone();
    definition.id = 987_654;
    let subject = PinSubject::Definition {
        project_id: definition.project_id.clone(),
        id: definition.id,
    };
    let mut pin = fixture_pin(0, 0);
    pin.subject = subject.clone();
    store
        .write(|connection| {
            write::upsert_pipeline(connection, &definition)?;
            write::add_pin(connection, &pin)?;
            Ok(())
        })
        .unwrap();
    let options = Options {
        now: dataset.now,
        auto_unpin: false,
        stale_after: None,
    };
    assert_eq!(
        refresh_blocking(&store, options).unwrap().pins[0]
            .pin
            .seen_result,
        None
    );
    let mut run = dataset.runs[0].clone();
    run.id = 987_655;
    run.project_id = definition.project_id.clone();
    run.pipeline_id = definition.id;
    run.status = RunStatus::Completed;
    run.result = Some(RunResult::Succeeded);
    run.queued = dataset.now;
    store
        .write(|connection| write::upsert_run(connection, &run))
        .unwrap();
    let first = refresh_blocking(&store, options).unwrap();
    assert_eq!(first.pins[0].pin.seen_result, Some(RunResult::Succeeded));
    assert!(!first.pins[0].changed);
    assert_eq!(first.pins[0].pin.opened_at, None);
    assert_eq!(first.opens_this_week, 0);
    run.id += 1;
    run.queued += 1;
    run.result = Some(RunResult::Failed);
    store
        .write(|connection| write::upsert_run(connection, &run))
        .unwrap();
    assert!(refresh_blocking(&store, options).unwrap().pins[0].changed);
    assert_eq!(
        store.with_conn(adoken_store::read::pins).unwrap()[0].seen_result,
        Some(RunResult::Succeeded)
    );
}

#[test]
fn refused_undo_keeps_original_position_label_and_toast_until_room_is_available() {
    let pin = fixture_pin(7, 4);
    let subject = pin.subject.clone();
    let mut undo = vec![Undo {
        pin: pin.clone(),
        toast: "original-undo".into(),
    }];
    assert!(take_undo(&mut undo, &subject, false, MAX_PINS).is_err());
    assert_eq!(undo[0].pin, pin);
    assert_eq!(undo[0].toast.as_ref(), "original-undo");
    let restored = take_undo(&mut undo, &subject, false, MAX_PINS - 1)
        .unwrap()
        .unwrap();
    assert_eq!(restored.pin, pin);
    assert_eq!(restored.toast.as_ref(), "original-undo");
    assert!(
        take_undo(&mut undo, &subject, false, MAX_PINS - 1)
            .unwrap()
            .is_none()
    );
}

#[test]
fn closed_work_item_title_edits_do_not_extend_auto_unpin_deadline() {
    let (_dir, store, dataset) = demo_fixture();
    let mut item = dataset
        .work_items
        .iter()
        .find(|item| item.state_category == adoken_model::StateCategory::InProgress)
        .unwrap()
        .clone();
    item.state_category = adoken_model::StateCategory::Completed;
    item.state = "Closed".into();
    item.rev += 1;
    item.changed = dataset.now;
    let mut pin = fixture_pin(item.id, 0);
    pin.subject = PinSubject::WorkItem(item.id);
    store
        .write(|connection| {
            write::upsert_work_item(connection, &item)?;
            write::add_pin(connection, &pin)?;
            Ok(())
        })
        .unwrap();
    item.title = "Edited without reopening".into();
    item.rev += 1;
    item.changed = dataset.now + 23 * adoken_model::time::HOUR;
    store
        .write(|connection| write::upsert_work_item(connection, &item))
        .unwrap();
    let options = Options {
        now: dataset.now + adoken_model::time::DAY - 1,
        auto_unpin: true,
        stale_after: None,
    };
    let before = refresh_blocking(&store, options).unwrap();
    assert_eq!(before.pins[0].closed_at, Some(dataset.now));
    assert_eq!(before.pins[0].title, item.title);
    let after = refresh_blocking(
        &store,
        Options {
            now: options.now + 1,
            ..options
        },
    )
    .unwrap();
    assert!(after.pins.is_empty());
    assert!(
        store
            .with_conn(adoken_store::read::pins)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn reopening_clears_work_item_deadline_and_reclosure_starts_a_new_day() {
    let (_dir, store, dataset) = demo_fixture();
    let mut item = dataset
        .work_items
        .iter()
        .find(|item| item.state_category == adoken_model::StateCategory::InProgress)
        .unwrap()
        .clone();
    item.state_category = adoken_model::StateCategory::Completed;
    item.state = "Closed".into();
    item.rev += 1;
    item.changed = dataset.now;
    let mut pin = fixture_pin(item.id, 0);
    pin.subject = PinSubject::WorkItem(item.id);
    store
        .write(|connection| {
            write::upsert_work_item(connection, &item)?;
            write::add_pin(connection, &pin)?;
            Ok(())
        })
        .unwrap();
    item.state_category = adoken_model::StateCategory::InProgress;
    item.state = "Active".into();
    item.rev += 1;
    item.changed = dataset.now + 2 * adoken_model::time::HOUR;
    store
        .write(|connection| write::upsert_work_item(connection, &item))
        .unwrap();
    let options = Options {
        now: dataset.now + 2 * adoken_model::time::DAY,
        auto_unpin: true,
        stale_after: None,
    };
    assert_eq!(
        refresh_blocking(&store, options).unwrap().pins[0].closed_at,
        None
    );
    item.state_category = adoken_model::StateCategory::Removed;
    item.state = "Removed".into();
    item.rev += 1;
    item.changed = options.now;
    store
        .write(|connection| write::upsert_work_item(connection, &item))
        .unwrap();
    let before = refresh_blocking(
        &store,
        Options {
            now: item.changed + adoken_model::time::DAY - 1,
            ..options
        },
    )
    .unwrap();
    assert_eq!(before.pins[0].closed_at, Some(item.changed));
    assert!(
        refresh_blocking(
            &store,
            Options {
                now: item.changed + adoken_model::time::DAY,
                ..options
            }
        )
        .unwrap()
        .pins
        .is_empty()
    );
}
