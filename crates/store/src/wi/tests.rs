use adoken_model::*;
use rusqlite_migration::{M, Migrations};

use super::*;
use crate::{MIGRATION_SLICE, Store, write};

fn open() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("adoken.sqlite")).unwrap();
    (dir, store)
}

fn person(id: &str, name: &str) -> Person {
    Person {
        id: id.into(),
        display_name: name.into(),
        unique_name: Some(format!("{id}@contoso.com")),
        is_group: false,
        image_url: None,
    }
}

fn seed(s: &Store) {
    s.write(|tx| {
        write::upsert_person(tx, &person("me", "Ada Me"), true)?;
        write::upsert_person(tx, &person("bo", "Bo Other"), false)?;
        write::upsert_project(
            tx,
            &Project {
                id: "p1".into(),
                name: "Payments".into(),
            },
        )?;
        write::upsert_work_item(tx, &wi(42, 7))?;
        Ok(())
    })
    .unwrap();
}

fn wi(id: i64, rev: i64) -> WorkItem {
    WorkItem {
        id,
        project_id: "p1".into(),
        rev,
        kind: WorkItemType::Bug,
        title: "Checkout times out".into(),
        state: "Active".into(),
        state_category: StateCategory::InProgress,
        assigned_to: Some("me".into()),
        created_by: "bo".into(),
        changed_by: "bo".into(),
        area_path: "Payments".into(),
        iteration_path: "Payments\\Sprint 1".into(),
        priority: Some(2),
        tags: Vec::new(),
        parent_id: Some(7),
        linked_pr_ids: Vec::new(),
        description: Some("Total is wrong.".into()),
        comment_count: 0,
        created: 10,
        changed: 10 * rev,
    }
}

fn link(kind: LinkKind, target: i64) -> WiLink {
    WiLink {
        index: 0,
        rel: kind.rel().into(),
        kind: Some(kind),
        target_id: Some(target),
        pr_id: None,
        url: format!("https://dev.azure.com/contoso/_apis/wit/workItems/{target}"),
        name: None,
        comment: None,
    }
}

fn full() -> FullFields {
    FullFields {
        description: Some(RichText {
            source: "<div>Total is <b>wrong</b>.</div>".into(),
            format: TextFormat::Html,
        }),
        texts: vec![(
            REPRO_STEPS.into(),
            RichText {
                source: "1. Open checkout".into(),
                format: TextFormat::Markdown,
            },
        )],
        links: vec![link(LinkKind::Parent, 7), link(LinkKind::Related, 9)],
        reason: Some("Approved".into()),
    }
}

/// FR-32: a full answer records sources, relations (indexed by server order,
/// at that rev) and both times; a later title-index answer only moves
/// `synced_ms`; an answer older than the stored row confirms nothing.
#[test]
fn answers_record_freshness_and_never_regress_the_row() {
    let (_d, s) = open();
    seed(&s);
    s.write(|tx| record_answer(tx, 42, 7, 1_000, Some(&full())))
        .unwrap();
    let x = s.with_conn(|c| extras(c, 42)).unwrap().unwrap();
    assert_eq!((x.synced_ms, x.full_synced_ms), (Some(1_000), Some(1_000)));
    assert_eq!(x.description.unwrap().format, TextFormat::Html);
    assert_eq!(x.repro_steps.unwrap().source, "1. Open checkout");
    assert_eq!(x.links_rev, Some(7));
    assert_eq!(
        x.links
            .iter()
            .map(|l| (l.index, l.target_id))
            .collect::<Vec<_>>(),
        [(0, Some(7)), (1, Some(9))]
    );

    s.write(|tx| record_answer(tx, 42, 7, 2_000, None)).unwrap();
    let x = s.with_conn(|c| extras(c, 42)).unwrap().unwrap();
    assert_eq!((x.synced_ms, x.full_synced_ms), (Some(2_000), Some(1_000)));
    assert_eq!(x.links.len(), 2, "a title-index row keeps the relations");

    s.write(|tx| {
        record_answer(
            tx,
            42,
            6,
            3_000,
            Some(&FullFields {
                links: Vec::new(),
                ..FullFields::default()
            }),
        )
    })
    .unwrap();
    let x = s.with_conn(|c| extras(c, 42)).unwrap().unwrap();
    assert_eq!(x.synced_ms, Some(2_000), "rev 6 is older than the row");
    assert_eq!(x.links.len(), 2);
    assert_eq!(
        s.with_conn(|c| linked_from(c, 7, LinkKind::Parent))
            .unwrap(),
        [42]
    );
}

/// AC-16 (store side): drafts and recent assignees are local state: they
/// survive a restart and a migration that drops and rebuilds the cache.
#[test]
fn ac16_drafts_and_recents_survive_restart_and_cache_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("adoken.sqlite");
    {
        let s = Store::open(&path).unwrap();
        seed(&s);
        s.write(|tx| {
            save_draft(
                tx,
                &WiDraft {
                    work_item_id: 42,
                    kind: DraftKind::Description,
                    body: "## Steps\n- open".into(),
                    base_rev: 7,
                    updated: 5,
                },
            )?;
            save_draft(
                tx,
                &WiDraft {
                    work_item_id: 42,
                    kind: DraftKind::Comment,
                    body: "@<d291> please verify".into(),
                    base_rev: 7,
                    updated: 6,
                },
            )?;
            touch_recent_assignee(tx, &person("dana", "Dana Ruiz"), 9)
        })
        .unwrap();
    }
    let mut conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    let resync: Vec<M<'static>> = MIGRATION_SLICE
        .iter()
        .cloned()
        .chain([M::up(
            "DELETE FROM project; DROP TABLE wi_link; DROP TABLE work_item; DELETE FROM identity;",
        )])
        .collect();
    Migrations::new(resync).to_latest(&mut conn).unwrap();
    let got = drafts(&conn, 42).unwrap();
    assert_eq!(
        got.iter()
            .map(|d| (d.kind, d.body.as_str()))
            .collect::<Vec<_>>(),
        [
            (DraftKind::Comment, "@<d291> please verify"),
            (DraftKind::Description, "## Steps\n- open")
        ]
    );
    assert_eq!(
        recent_assignees(&conn, 5).unwrap()[0]
            .unique_name
            .as_deref(),
        Some("dana@contoso.com")
    );
    // Discard / confirmed save: gone.
    delete_draft(&conn, 42, DraftKind::Comment).unwrap();
    assert_eq!(drafts(&conn, 42).unwrap().len(), 1);
}

#[test]
fn blank_draft_deletes_and_recents_keep_the_newest_five() {
    let (_d, s) = open();
    s.write(|tx| {
        for i in 0..7 {
            touch_recent_assignee(tx, &person(&format!("p{i}"), "P"), i)?;
        }
        touch_recent_assignee(tx, &person("p0", "P"), 100)?;
        save_draft(
            tx,
            &WiDraft {
                work_item_id: 1,
                kind: DraftKind::Comment,
                body: "x".into(),
                base_rev: 1,
                updated: 1,
            },
        )?;
        save_draft(
            tx,
            &WiDraft {
                work_item_id: 1,
                kind: DraftKind::Comment,
                body: "  \n".into(),
                base_rev: 1,
                updated: 2,
            },
        )
    })
    .unwrap();
    let ids: Vec<String> = s
        .with_conn(|c| recent_assignees(c, 10))
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(ids, ["p0", "p6", "p5", "p4", "p3"]);
    assert!(s.with_conn(|c| drafts(c, 1)).unwrap().is_empty());
}

/// FR-34 backfill: an existing store refetches my items (the `wi.hot` cursor
/// restarts) and my discussions once; nobody else's.
#[test]
fn migration_0009_refetches_my_items_and_discussions_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("adoken.sqlite");
    let mut conn = rusqlite::Connection::open(&path).unwrap();
    Migrations::from_slice(&MIGRATION_SLICE[..8])
        .to_latest(&mut conn)
        .unwrap();
    {
        let tx = conn.transaction().unwrap();
        write::upsert_person(&tx, &person("me", "Ada Me"), true).unwrap();
        write::upsert_person(&tx, &person("bo", "Bo Other"), false).unwrap();
        write::upsert_project(
            &tx,
            &Project {
                id: "p1".into(),
                name: "Payments".into(),
            },
        )
        .unwrap();
        let mut mine = wi(1, 3);
        mine.comment_count = 2;
        let mut theirs = wi(2, 3);
        theirs.assigned_to = Some("bo".into());
        theirs.comment_count = 4;
        tx.execute_batch(
            "INSERT INTO sync_cursor(scope, project_id, high_water, baseline_done, last_ok_ms)
             VALUES ('wi.hot', '', '2026-09-30T10:00:00Z', 1, 99)",
        )
        .unwrap();
        // Inserted without the 0009 columns (the old schema).
        for item in [&mine, &theirs] {
            tx.execute(
                "INSERT INTO work_item(id, project_id, rev, type, title, state, state_category,
                   assigned_to_id, created_by_id, changed_by_id, area_path, iteration_path,
                   comment_count, created_ms, changed_ms)
                 VALUES (?1, 'p1', 3, 'Bug', 't', 'Active', 'InProgress', ?2, 'bo', 'bo', 'a', 'i',
                   ?3, 1, 1)",
                rusqlite::params![item.id, item.assigned_to, item.comment_count],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    drop(conn);
    let s = Store::open(&path).unwrap();
    let due = s
        .with_conn(|c| crate::read::work_items_comments_due(c, 10))
        .unwrap();
    assert_eq!(due, [(1, "p1".to_owned())]);
    let cursor = s.cursor("wi.hot", "").unwrap().unwrap();
    assert_eq!(cursor.high_water, None);
    assert_eq!(
        cursor.last_ok_ms,
        Some(99),
        "freshness of the lane survives"
    );
}
