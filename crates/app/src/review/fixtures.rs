//! Spec 0019 §6 fixtures for stories, view tests, benches and the smoke
//! run: **S** (5 files, 7 threads) and **L+T** (300 files, 40k changed
//! lines, 500 human threads on 150 files), plus the group reviewers PR of
//! AC-C1. Deterministic; blobs are in memory.

use std::collections::HashMap;

use adoken_model::{
    ChangeType, Comment, LineRange, MergeStatus, Millis, Person, PersonId, PrDetail, PrFileChange,
    PrIteration, PrStatus, PrThread, PullRequest, Reviewer, ThreadAnchor, ThreadStatus, Vote, time,
};

/// A PR with everything a review needs, blobs in memory.
pub struct Fixture {
    pub detail: PrDetail,
    pub blobs: HashMap<String, String>,
    pub people: Vec<Person>,
    pub me: PersonId,
    pub now: Millis,
}

/// 2026-10-01 09:00 UTC.
pub const NOW: Millis = 1_790_845_200_000;
pub const ME: &str = "me";

fn person(id: &str, name: &str, group: bool) -> Person {
    Person {
        id: id.into(),
        display_name: name.into(),
        unique_name: None,
        is_group: group,
        image_url: None,
    }
}

fn people() -> Vec<Person> {
    vec![
        person(ME, "Ada Me", false),
        person("dana", "Dana Ruiz", false),
        person("tom", "Tom Ito", false),
        person("react", "[Contoso]\\React PR reviewers", true),
        person("copilot", "[Contoso]\\Copilot reviewers", true),
    ]
}

fn reviewer(id: &str, vote: Vote, required: bool) -> Reviewer {
    Reviewer {
        person: id.into(),
        vote,
        is_required: required,
        has_declined: false,
        added_at: Some(NOW - time::DAY),
        voted_at: (vote != Vote::NoVote).then_some(NOW - time::HOUR),
    }
}

/// A PR with `id`, `title`, authored by Dana.
pub fn pull_request(id: i64, title: &str) -> PullRequest {
    PullRequest {
        id,
        repo_id: "repo".into(),
        project_id: "project".into(),
        title: title.into(),
        description: String::new(),
        author: "dana".into(),
        reviewers: vec![reviewer(ME, Vote::NoVote, false)],
        status: PrStatus::Active,
        is_draft: false,
        merge_status: MergeStatus::Succeeded,
        source_branch: "refs/heads/feature/retry".into(),
        target_branch: "refs/heads/main".into(),
        last_source_commit: Some("c2c2c2c2".into()),
        created: NOW - 3 * time::DAY,
        updated: NOW - 2 * time::HOUR,
        closed: None,
        iterations: 2,
        last_push: Some(NOW - 2 * time::HOUR),
    }
}

fn iterations() -> Vec<PrIteration> {
    (1..=2)
        .map(|id| PrIteration {
            id,
            source_commit: format!("c{id}c{id}c{id}c{id}"),
            created: NOW - (3 - id as i64) * time::DAY,
            description: None,
            commit_count: Some(1),
            commits_truncated: false,
        })
        .collect()
}

fn comment(id: i64, author: &str, body: &str, at: Millis) -> Comment {
    Comment {
        id,
        parent_id: None,
        author: author.into(),
        body: body.into(),
        created: at,
        updated: at,
        is_system: false,
        is_deleted: false,
    }
}

/// A human thread; `path` `None` is PR-level; `right` / `left` line ranges.
#[allow(clippy::too_many_arguments)]
fn thread(
    id: i64,
    pr: i64,
    status: ThreadStatus,
    path: Option<&str>,
    right: Option<(u32, u32)>,
    left: Option<(u32, u32)>,
    iteration: u32,
    body: &str,
    at: Millis,
) -> PrThread {
    PrThread {
        id,
        pr_id: pr,
        status,
        anchor: path.map(|path| ThreadAnchor {
            file_path: format!("/{path}"),
            right: right.map(|(start, end)| LineRange { start, end }),
            left: left.map(|(start, end)| LineRange { start, end }),
            iteration_blob: None,
        }),
        iteration: Some(iteration),
        comments: vec![comment(id * 10, "tom", body, at)],
        updated: at,
    }
}

fn lines(prefix: &str, count: u32) -> String {
    (1..=count)
        .map(|i| format!("{prefix} line {i}\n"))
        .collect()
}

/// `text` with lines `changed` replaced.
fn edit(text: &str, changed: impl Fn(u32) -> bool) -> String {
    text.lines()
        .enumerate()
        .map(|(ix, line)| {
            if changed(ix as u32 + 1) {
                format!("{line} // changed\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect()
}

fn change(path: &str, kind: ChangeType, old: Option<&str>, new: Option<&str>) -> PrFileChange {
    PrFileChange {
        path: format!("/{path}"),
        original_path: None,
        change_type: kind,
        old_content: old.map(str::to_owned),
        new_content: new.map(str::to_owned),
        change_tracking_id: None,
    }
}

/// Fixture **S** (spec 0019 §6): `docs/AUTH.md` (1 pending thread),
/// `src/auth/token.rs` (2 active: lines 10 and 40–42),
/// `src/auth/token_test.rs` (1 fixed), `src/pay/ledger.rs` (1 active on
/// older code), `src/pay/retry.rs` (none); 1 active PR-level thread and 1
/// active thread on `src/old.rs`, not in the file list. Active 6,
/// Resolved 1, All 7.
pub fn small() -> Fixture {
    let id = 4821;
    let mut blobs = HashMap::new();
    let mut put = |id: &str, text: String| {
        blobs.insert(id.to_owned(), text);
    };
    let auth = lines("auth doc", 12);
    put("auth-old", auth.clone());
    put("auth-new", edit(&auth, |l| l == 3));
    let token = lines("token", 60);
    put("token-old", token.clone());
    put(
        "token-new",
        edit(&token, |l| l == 10 || (40..=42).contains(&l)),
    );
    put("token-test-new", lines("token test", 20));
    let ledger = lines("ledger", 30);
    put("ledger-old", ledger.clone());
    put("ledger-new", edit(&ledger, |l| l == 5));
    let retry = lines("retry", 30);
    put("retry-old", retry.clone());
    put("retry-new", edit(&retry, |l| l == 12));
    let files = vec![
        change(
            "docs/AUTH.md",
            ChangeType::Edit,
            Some("auth-old"),
            Some("auth-new"),
        ),
        change(
            "src/auth/token.rs",
            ChangeType::Edit,
            Some("token-old"),
            Some("token-new"),
        ),
        change(
            "src/auth/token_test.rs",
            ChangeType::Add,
            None,
            Some("token-test-new"),
        ),
        change(
            "src/pay/ledger.rs",
            ChangeType::Edit,
            Some("ledger-old"),
            Some("ledger-new"),
        ),
        change(
            "src/pay/retry.rs",
            ChangeType::Edit,
            Some("retry-old"),
            Some("retry-new"),
        ),
    ];
    let hour = time::HOUR;
    let threads = vec![
        thread(
            1,
            id,
            ThreadStatus::Active,
            None,
            None,
            None,
            2,
            "Can we split the migration?",
            NOW - 3 * hour,
        ),
        thread(
            2,
            id,
            ThreadStatus::Pending,
            Some("docs/AUTH.md"),
            Some((3, 3)),
            None,
            2,
            "Document the refresh path.",
            NOW - 5 * hour,
        ),
        thread(
            3,
            id,
            ThreadStatus::Active,
            Some("src/auth/token.rs"),
            Some((40, 42)),
            None,
            2,
            "Should the key survive a restart?",
            NOW - 2 * hour,
        ),
        thread(
            4,
            id,
            ThreadStatus::Active,
            Some("src/auth/token.rs"),
            Some((10, 10)),
            None,
            2,
            "Off by one?",
            NOW - 4 * hour,
        ),
        thread(
            5,
            id,
            ThreadStatus::Fixed,
            Some("src/auth/token_test.rs"),
            Some((5, 5)),
            None,
            2,
            "Add a case for expiry.",
            NOW - 6 * hour,
        ),
        // Iteration 1, its line gone and no blob to map through: older code.
        thread(
            6,
            id,
            ThreadStatus::Active,
            Some("src/pay/ledger.rs"),
            Some((7, 7)),
            None,
            1,
            "Why round here?",
            NOW - 26 * hour,
        ),
        thread(
            7,
            id,
            ThreadStatus::Active,
            Some("src/old.rs"),
            Some((2, 2)),
            None,
            1,
            "Is this still used?",
            NOW - 30 * hour,
        ),
    ];
    let mut pr = pull_request(id, "Retry idempotency keys on payment capture");
    pr.reviewers = vec![
        reviewer("dana", Vote::Approved, false),
        reviewer("react", Vote::NoVote, true),
        reviewer("copilot", Vote::Approved, false),
        reviewer("tom", Vote::NoVote, true),
    ];
    Fixture {
        detail: PrDetail {
            pr,
            iterations: iterations(),
            files,
            threads,
        },
        blobs,
        people: people(),
        me: ME.into(),
        now: NOW,
    }
}

/// Fixture **L+T** (spec 0019 §6): `files` files (300) with about
/// `changed_lines` changed lines in total (40k) and the thread mix of 500
/// human threads on half the files (380 active, 20 pending, 100 resolved;
/// 30 on older code; 10 on paths not in the PR), 25 PR-level threads and
/// 60 system threads.
pub fn large(files: usize, changed_lines: usize) -> Fixture {
    let id = 9001;
    let mut blobs = HashMap::new();
    let per_file = (changed_lines / files.max(1)).max(1) as u32;
    let length = per_file * 2 + 20;
    let areas = ["auth", "pay", "sync", "ui", "store", "search"];
    let mut changes = Vec::with_capacity(files);
    let mut paths = Vec::with_capacity(files);
    for ix in 0..files {
        let area = areas[ix % areas.len()];
        let path = if ix % 7 == 0 {
            format!("src/{area}/tests/case_{ix:03}_test.rs")
        } else {
            format!("src/{area}/mod_{}/file_{ix:03}.rs", ix % 5)
        };
        let old = lines(&format!("f{ix}"), length);
        let new = edit(&old, |l| l % 2 == 0 && l <= per_file * 2);
        let (old_id, new_id) = (format!("l-old-{ix}"), format!("l-new-{ix}"));
        blobs.insert(old_id.clone(), old);
        blobs.insert(new_id.clone(), new);
        changes.push(change(
            &path,
            ChangeType::Edit,
            Some(&old_id),
            Some(&new_id),
        ));
        paths.push(path);
    }
    let mut threads = Vec::new();
    let mut next = 1i64;
    let mut add = |t: PrThread| threads.push(t);
    // 500 human threads: 460 inline on 150 files, 30 on older code, 10 gone.
    let with_threads = (files / 2).max(1);
    for n in 0..460usize {
        let file = (n * 2) % (with_threads * 2);
        let status = match n {
            n if n < 350 => ThreadStatus::Active,
            n if n < 370 => ThreadStatus::Pending,
            _ => ThreadStatus::Fixed,
        };
        let line = 2 + 2 * (n as u32 / with_threads as u32 % per_file.max(1));
        add(thread(
            next,
            id,
            status,
            Some(&paths[file % files]),
            Some((line, line)),
            None,
            2,
            "Worth a second look.",
            NOW - (n as i64 + 1) * time::MINUTE,
        ));
        next += 1;
    }
    for n in 0..30usize {
        add(thread(
            next,
            id,
            ThreadStatus::Active,
            Some(&paths[(n * 2) % files]),
            Some((length + 50, length + 50)),
            None,
            1,
            "On code that moved.",
            NOW - time::DAY,
        ));
        next += 1;
    }
    for n in 0..10usize {
        add(thread(
            next,
            id,
            ThreadStatus::Fixed,
            Some(&format!("src/gone/removed_{n}.rs")),
            Some((3, 3)),
            None,
            1,
            "Gone since.",
            NOW - time::DAY,
        ));
        next += 1;
    }
    for n in 0..25usize {
        add(thread(
            next,
            id,
            ThreadStatus::Active,
            None,
            None,
            None,
            2,
            "General question.",
            NOW - n as i64 * time::HOUR,
        ));
        next += 1;
    }
    for _ in 0..60usize {
        let mut system = thread(
            next,
            id,
            ThreadStatus::Unknown,
            None,
            None,
            None,
            2,
            "Policy update",
            NOW,
        );
        system.comments[0].is_system = true;
        add(system);
        next += 1;
    }
    Fixture {
        detail: PrDetail {
            pr: pull_request(id, "Large refactor of the sync engine"),
            iterations: iterations(),
            files: changes,
            threads,
        },
        blobs,
        people: people(),
        me: ME.into(),
        now: NOW,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_fixture_has_the_spec_shape() {
        let f = large(300, 40_000);
        assert_eq!(f.detail.files.len(), 300);
        let human: Vec<_> = f
            .detail
            .threads
            .iter()
            .filter(|t| crate::review::data::is_human(t))
            .collect();
        assert_eq!(human.len(), 525, "500 file threads + 25 PR-level");
        assert_eq!(human.iter().filter(|t| t.status.is_resolved()).count(), 100);
        let files: std::collections::HashSet<_> = human
            .iter()
            .filter_map(|t| t.anchor.as_ref().map(|a| a.file_path.clone()))
            .collect();
        assert_eq!(files.len(), 150 + 10);
    }
}
