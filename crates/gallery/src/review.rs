//! PR review stories (spec 0004) over the demo dataset: the full-width PR
//! list (also with every row badge), the review opened from it with its
//! back button, the list's repository filter (chip, picker, filtered-empty),
//! the header, file tree (also loading / failed), and the diff in both
//! layouts (also loading, short and slow); plus the write path:
//! line composer (editing / posting / failed / narrow pane), vote menu,
//! Complete sheet (normal + new commits since review), and read-only; and
//! the "Copied !id" toast over a default-size window.
//!
//! Spec 0019 (design `pr-review-polish.md` §8) on fixture S: the tree
//! filter (highlights, nothing matched, threads only), the Threads view
//! (Active, All, empty Active, excerpts loading, notes, read-only), the
//! header's status control / Publish / group indicator, and on the demo
//! list: group reviewers hidden and shown, the row menu's status items,
//! Abandon over the list, "Sending…", and the copy toast.

use std::collections::HashMap;

use adoken_app::review::data::{self, Layout, ListTab};
use adoken_app::review::fixtures::{self, Fixture};
use adoken_app::review::pr::{ComposerState, SheetState};
use adoken_app::review::threads::ThreadScope;
use adoken_app::review::writes::StatusAction;
use adoken_app::review::{Part, PrReview, ReviewView, story};
use adoken_demo::RICH_PR_IDS;
use adoken_model::{ChangeType, PrFileChange, PrStatus, PullRequest, ThreadStatus};
use adoken_store::PrFilter;
use adoken_sync::writes::WriteError;
use adoken_ui::ChipTone;
use gpui::{AnyElement, App, Context, Window, div, point, prelude::*, px};

/// 4312: Rust, threads incl. older iterations. 4318: add/edit/delete/rename.
const RUST_PR: i64 = RICH_PR_IDS[0];
const TS_PR: i64 = RICH_PR_IDS[1];

/// Kept across re-renders (`use_keyed_state`) so the view is built once.
fn render(
    key: &'static str,
    id: i64,
    layout: Layout,
    part: Part,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    render_with(key, id, layout, part, |_, _| {}, window, cx)
}

/// [`render`], then `setup` puts the review into the state under test.
fn render_with(
    key: &'static str,
    id: i64,
    layout: Layout,
    part: Part,
    setup: impl FnOnce(&mut PrReview, &mut Context<PrReview>),
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let review = window.use_keyed_state(key, cx, |window, cx| {
        let mut review = story::review(id, layout, part, window, cx);
        setup(&mut review, cx);
        review
    });
    div().size_full().child(review).into_any_element()
}

pub fn header_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render(
        "review-header",
        RUST_PR,
        Layout::Unified,
        Part::Header,
        window,
        cx,
    )
}

pub fn file_tree_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render(
        "review-file-tree",
        TS_PR,
        Layout::Unified,
        Part::Tree,
        window,
        cx,
    )
}

pub fn diff_unified_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render(
        "review-diff-unified",
        RUST_PR,
        Layout::Unified,
        Part::Diff,
        window,
        cx,
    )
}

pub fn diff_split_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render(
        "review-diff-split",
        RUST_PR,
        Layout::Split,
        Part::Diff,
        window,
        cx,
    )
}

/// A PR opened before its file list was synced, the download past the
/// 150 ms grace: skeleton rows and "Loading files…".
pub fn file_tree_loading_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(
        "review-file-tree-loading",
        TS_PR,
        Layout::Unified,
        Part::Tree,
        |review, _| review.story_files_loading(None),
        window,
        cx,
    )
}

/// The file list's download failed: why, and Retry (⇧L).
pub fn file_tree_failed_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(
        "review-file-tree-failed",
        TS_PR,
        Layout::Unified,
        Part::Tree,
        |review, _| review.story_files_loading(Some("You're offline.")),
        window,
        cx,
    )
}

/// The selected file's blobs downloading, past the grace: spinner + name.
pub fn diff_loading_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(
        "review-diff-loading",
        RUST_PR,
        Layout::Unified,
        Part::Diff,
        |review, _| review.story_diff_loading(adoken_app::review::loading::GRACE),
        window,
        cx,
    )
}

/// The same download past 3 s: the pane says what it's waiting for.
pub fn diff_loading_slow_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(
        "review-diff-loading-slow",
        RUST_PR,
        Layout::Unified,
        Part::Diff,
        |review, _| {
            review.story_diff_loading(
                adoken_app::review::loading::SLOW + std::time::Duration::from_secs(1),
            )
        },
        window,
        cx,
    )
}

/// The Pull requests view on a seeded demo store: the full-width "To
/// review" list, first row selected (spec 0004 §3.1, one pane at a time).
pub fn list_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let view = window.use_keyed_state("review-list", cx, |window, cx| {
        let now = adoken_demo::DEMO_NOW;
        let dir = crate::story_dir("review-list");
        let store = adoken_demo::open_store_at(&dir, now).expect("seed the demo store");
        story::list(store, now, window, cx)
    });
    div().size_full().child(view).into_any_element()
}

/// The All tab with every row badge: a long title wrapping to two lines
/// then an ellipsis, reviewers in every vote group (a required reviewer
/// who hasn't voted dimmed), failing and running CI, comment counts, Draft,
/// Merge conflict, Auto-complete, Completing…, Completed / Abandoned; the
/// third row selected.
pub fn list_badges_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let view = window.use_keyed_state("review-list-badges", cx, |window, cx| {
        let now = adoken_demo::DEMO_NOW;
        let dir = crate::story_dir("review-list-badges");
        let store = adoken_demo::open_store_at(&dir, now).expect("seed the demo store");
        story::seed_row_badges(&store, now);
        let mut view = story::list(store, now, window, cx);
        view.set_tab(ListTab::All, cx);
        story::select_row(&mut view, 2, cx);
        view
    });
    div().size_full().child(view).into_any_element()
}

/// `↩` on a row: the review full-width, its header's back button (`Esc`,
/// `⌘[`) returning to the list.
pub fn open_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let view = window.use_keyed_state("review-open", cx, |window, cx| {
        // The demo org's access: stories that build a non-demo Shell (resource
        // meter, activity) leave the global read-only for later stories.
        adoken_app::review::writes::set_access(adoken_app::review::writes::Access::Local, cx);
        let now = adoken_demo::DEMO_NOW;
        let dir = crate::story_dir("review-open");
        let store = adoken_demo::open_store_at(&dir, now).expect("seed the demo store");
        let mut view = story::list(store, now, window, cx);
        view.open_pr(TS_PR, window, cx);
        view
    });
    div().size_full().child(view).into_any_element()
}

/// The Pull requests view (To review tab) on a seeded demo store whose
/// saved repository filter is `repo(prs, me, repos)`, so the view restores
/// it on load like a restart; `picker` opens the repository picker.
fn repo_filter_story(
    key: &'static str,
    repo: fn(&[PullRequest], &str, &[adoken_model::Repo]) -> Option<String>,
    picker: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let view = window.use_keyed_state(key, cx, |window, cx| {
        let now = adoken_demo::DEMO_NOW;
        let store =
            adoken_demo::open_store_at(&crate::story_dir(key), now).expect("seed the demo store");
        let me = store
            .me()
            .expect("read me")
            .map(|p| p.id)
            .unwrap_or_default();
        let prs = store.prs(&Default::default()).expect("read PRs");
        let repos = store.repos().expect("read repos");
        data::migrate(&store).expect("review tables");
        data::set_repo_filter_preference(&store, repo(&prs, &me, &repos).as_deref())
            .expect("save the repository filter");
        let mut view = story::list(store, now, window, cx);
        if picker {
            view.open_repo_picker(window, cx);
        }
        view
    });
    div().size_full().child(view).into_any_element()
}

/// Filtered to the repository with the most reviews waiting: the chip and
/// Clear under the tabs, counts narrowed to it.
pub fn list_repo_filter_story(window: &mut Window, cx: &mut App) -> AnyElement {
    repo_filter_story(
        "review-list-repo-filter",
        |prs, me, _| {
            data::repo_options(prs, ListTab::ToReview, me, &HashMap::new(), None)
                .into_iter()
                .next()
                .map(|option| option.id)
        },
        false,
        window,
        cx,
    )
}

/// `r`: the repository picker, repositories with reviews waiting and their
/// counts, "All repositories" current.
pub fn list_repo_picker_story(window: &mut Window, cx: &mut App) -> AnyElement {
    repo_filter_story("review-list-repo-picker", |_, _, _| None, true, window, cx)
}

/// Filtered to a repository with nothing waiting on my review.
pub fn list_repo_empty_story(window: &mut Window, cx: &mut App) -> AnyElement {
    repo_filter_story(
        "review-list-repo-empty",
        |prs, me, repos| {
            let repo = repos
                .iter()
                .find(|r| data::filter(prs, ListTab::ToReview, me, Some(&r.id)).is_empty())
                .expect("a demo repository with no review waiting");
            Some(repo.id.clone())
        },
        false,
        window,
        cx,
    )
}

/// Draft text for the composer stories: a line comment on !4312's retry
/// loop (the thread above it asks about the checkout budget).
const DRAFT: &str = "Can we cap this at 3 attempts? With the default backoff the worst case is ~14 s, over the 10 s checkout budget.";

fn composer(
    key: &'static str,
    state: ComposerState,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    render_with(
        key,
        RUST_PR,
        Layout::Unified,
        Part::Diff,
        move |review, cx| review.story_composer(DRAFT, state, cx),
        window,
        cx,
    )
}

/// `c` on a diff line: the inline composer, draft typed, ⌘↵ to post.
pub fn composer_story(window: &mut Window, cx: &mut App) -> AnyElement {
    composer("review-composer", ComposerState::Editing, window, cx)
}

/// The diff pane of a 1200 px window with the sidebar (232), the list pane
/// (380) and the review's file tree (300 + 1 px divider) open.
const NARROW_PANE: f32 = 287.0;

/// The inline composer in that narrow pane: the card shrinks with it, the
/// draft scrolls to the caret inside the input, and the Esc hint ellipsizes.
pub fn composer_narrow_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let review = composer("review-composer-narrow", ComposerState::Editing, window, cx);
    div()
        .size_full()
        .child(div().h_full().w(px(NARROW_PANE)).child(review))
        .into_any_element()
}

/// ⌘↵ pressed: the comment is on its way.
pub fn composer_posting_story(window: &mut Window, cx: &mut App) -> AnyElement {
    composer(
        "review-composer-posting",
        ComposerState::Posting,
        window,
        cx,
    )
}

/// The post failed (Azure DevOps refused): the reason on the composer, the
/// draft kept.
pub fn composer_failed_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let error = WriteError::Rejected {
        status: 503,
        message: Some("Service Unavailable".into()),
    };
    composer(
        "review-composer-failed",
        ComposerState::Failed(error.to_string().into()),
        window,
        cx,
    )
}

/// `⇧V`: the vote menu over the header, with a reverted-vote notice.
pub fn vote_menu_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(
        "review-vote-menu",
        RUST_PR,
        Layout::Unified,
        Part::Header,
        |review, cx| {
            let error = WriteError::Throttled { until: 0 };
            review.story_notice(ChipTone::Danger, &format!("Vote not saved. {error}"), cx);
            review.story_vote_menu(cx);
        },
        window,
        cx,
    )
}

fn sheet(key: &'static str, state: SheetState, window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(
        key,
        RUST_PR,
        Layout::Unified,
        Part::All,
        move |review, cx| review.story_sheet(false, state, cx),
        window,
        cx,
    )
}

/// `⇧M`: the Complete sheet, pinned to the commit on screen.
pub fn complete_sheet_story(window: &mut Window, cx: &mut App) -> AnyElement {
    sheet("review-complete-sheet", SheetState::Idle, window, cx)
}

/// Complete refused by Azure DevOps: the source branch moved (AC-10).
pub fn complete_new_commits_story(window: &mut Window, cx: &mut App) -> AnyElement {
    sheet(
        "review-complete-new-commits",
        SheetState::NewCommits,
        window,
        cx,
    )
}

/// Read-only token: no review control, one passive hint (AC-11).
pub fn read_only_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(
        "review-read-only",
        RUST_PR,
        Layout::Unified,
        Part::Header,
        |review, cx| review.story_read_only(cx),
        window,
        cx,
    )
}

/// Complete accepted, Azure DevOps still merging: "Completing…" in the
/// header, Complete disabled with the reason.
pub fn completing_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(
        "review-completing",
        RUST_PR,
        Layout::Unified,
        Part::Header,
        |review, cx| review.story_completing(cx),
        window,
        cx,
    )
}

/// The async completion failed (merge rejected by policy): Complete is
/// available again and the reason shows.
pub fn complete_failed_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(
        "review-complete-failed",
        RUST_PR,
        Layout::Unified,
        Part::Header,
        |review, cx| review.story_complete_failed(cx),
        window,
        cx,
    )
}

/// Demo work items linked to !4312 in the work items stories: a bug and a
/// user story of the same project, plus an id adoken hasn't synced.
fn linked_ids() -> Vec<i64> {
    let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
    let project = &ds.pr(RUST_PR).expect("demo PR").project_id;
    let pick = |kind| {
        ds.work_items
            .iter()
            .find(|w| w.project_id == *project && w.kind == kind)
            .map(|w| w.id)
            .expect("a demo work item of that type")
    };
    vec![
        pick(adoken_model::WorkItemType::Bug),
        pick(adoken_model::WorkItemType::UserStory),
        987_654,
    ]
}

/// `⇧S` or a click on the status chip: the changes valid for an active PR
/// (Mark as draft, Abandon…), spec 0004 FR-16.
pub fn status_menu_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(
        "review-status-menu",
        RUST_PR,
        Layout::Unified,
        Part::Header,
        |review, cx| review.story_status_menu(cx),
        window,
        cx,
    )
}

/// `⇧A`: Abandon asks first.
pub fn abandon_confirm_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(
        "review-abandon-confirm",
        RUST_PR,
        Layout::Unified,
        Part::All,
        |review, cx| review.story_abandon_confirm(cx),
        window,
        cx,
    )
}

/// The header's Work items section: two synced items (type, id, title,
/// state, unlink) and one adoken hasn't synced, then Link work item… (⇧W),
/// spec 0004 FR-17.
pub fn work_items_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(
        "review-work-items",
        RUST_PR,
        Layout::Unified,
        Part::Header,
        |review, cx| review.story_linked_items(&linked_ids(), cx),
        window,
        cx,
    )
}

/// `⇧W`: the Link picker searching synced work items ("settlement").
pub fn work_item_picker_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(
        "review-work-item-picker",
        RUST_PR,
        Layout::Unified,
        Part::All,
        |review, cx| review.story_link_picker("settlement", cx),
        window,
        cx,
    )
}

/// A token without `vso.work_write`: Link work item… gives way to the
/// reason, with no token action; linked items still show and open.
pub fn link_scope_missing_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render_with(
        "review-link-scope-missing",
        RUST_PR,
        Layout::Unified,
        Part::Header,
        |review, cx| {
            review.story_linked_items(&linked_ids()[..1], cx);
            review.story_link_scope_missing(cx);
        },
        window,
        cx,
    )
}

/// `⇧Y`: !4318 copied, the toast confirms it; its review is open in a
/// default-size window (1200 × 760), so the header's `!id`, Copy link and
/// Open (disabled: demo org) show at the width most people see.
pub fn copied_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let view = window.use_keyed_state("review-copied", cx, |window, cx| {
        let now = adoken_demo::DEMO_NOW;
        let dir = crate::story_dir("review-copied");
        let store = adoken_demo::open_store_at(&dir, now).expect("seed the demo store");
        let mut view = story::list(store, now, window, cx);
        view.open_pr(TS_PR, window, cx);
        story::show_copied(&mut view, &format!("Copied !{TS_PR}"), cx);
        view
    });
    div().size_full().child(view).into_any_element()
}

// ── spec 0019 ─────────────────────────────────────────────────────────────

/// A review over `fixture` (every diff computed, busiest file selected),
/// then `setup`.
fn fixture_review(
    key: &'static str,
    fixture: impl FnOnce() -> Fixture,
    part: Part,
    setup: impl FnOnce(&mut PrReview, &mut Context<PrReview>),
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let review = window.use_keyed_state(key, cx, |window, cx| {
        let f = fixture();
        let mut review = story::review_from(
            f.detail,
            f.blobs,
            f.people,
            f.me,
            f.now,
            Layout::Unified,
            part,
            window,
            cx,
        );
        setup(&mut review, cx);
        review
    });
    div().size_full().child(review).into_any_element()
}

/// `/` `auth`: three files and their folders, matches highlighted in
/// `AUTH.md` and the `auth` folder; "Files 3 of 5".
pub fn tree_filter_story(window: &mut Window, cx: &mut App) -> AnyElement {
    fixture_review(
        "review-tree-filter",
        fixtures::small,
        Part::Tree,
        |review, cx| review.story_filter("auth", false, cx),
        window,
        cx,
    )
}

/// A filter nothing matches: the empty state and Clear filter `⌘⌫`.
pub fn tree_filter_empty_story(window: &mut Window, cx: &mut App) -> AnyElement {
    fixture_review(
        "review-tree-filter-empty",
        fixtures::small,
        Part::Tree,
        |review, cx| review.story_filter("auth -test zzz", false, cx),
        window,
        cx,
    )
}

/// `⇧T`: only the files with active threads, the toggle pressed.
pub fn tree_threads_only_story(window: &mut Window, cx: &mut App) -> AnyElement {
    fixture_review(
        "review-tree-threads-only",
        fixtures::small,
        Part::Tree,
        |review, cx| review.story_filter("", true, cx),
        window,
        cx,
    )
}

/// `t`: every active thread grouped (Pull request, files in tree order,
/// files no longer in the PR), excerpts above the conversations.
pub fn threads_story(window: &mut Window, cx: &mut App) -> AnyElement {
    fixture_review(
        "review-threads",
        fixtures::small,
        Part::All,
        |review, cx| review.story_threads(ThreadScope::Active, false, cx),
        window,
        cx,
    )
}

/// Scope All: the fixed thread on `token_test.rs` joins the list.
pub fn threads_all_story(window: &mut Window, cx: &mut App) -> AnyElement {
    fixture_review(
        "review-threads-all",
        fixtures::small,
        Part::All,
        |review, cx| review.story_threads(ThreadScope::All, false, cx),
        window,
        cx,
    )
}

/// Every thread resolved: "No active threads" and Show all.
pub fn threads_empty_active_story(window: &mut Window, cx: &mut App) -> AnyElement {
    fixture_review(
        "review-threads-empty-active",
        || {
            let mut f = fixtures::small();
            for thread in &mut f.detail.threads {
                thread.status = ThreadStatus::Fixed;
            }
            f
        },
        Part::All,
        |review, cx| review.story_threads(ThreadScope::Active, false, cx),
        window,
        cx,
    )
}

/// Excerpts whose files' diffs aren't computed yet: placeholder rows of
/// the final height.
pub fn threads_loading_story(window: &mut Window, cx: &mut App) -> AnyElement {
    fixture_review(
        "review-threads-loading",
        fixtures::small,
        Part::All,
        |review, cx| {
            review.story_uncomputed();
            review.story_threads(ThreadScope::Active, false, cx);
        },
        window,
        cx,
    )
}

/// The end of the list: an older-code thread, a generated file's thread
/// and a file no longer in the PR, each with its note instead of code.
pub fn threads_notes_story(window: &mut Window, cx: &mut App) -> AnyElement {
    fixture_review(
        "review-threads-notes",
        || {
            let mut f = fixtures::small();
            f.detail.files.push(PrFileChange {
                path: "/src/pay/Cargo.lock".into(),
                original_path: None,
                change_type: ChangeType::Edit,
                old_content: Some("lock-old".into()),
                new_content: Some("lock-new".into()),
                change_tracking_id: None,
            });
            let mut lock = f.detail.threads[3].clone();
            lock.id = 8;
            if let Some(anchor) = lock.anchor.as_mut() {
                anchor.file_path = "/src/pay/Cargo.lock".into();
            }
            lock.comments[0].body = "Did this bump on purpose?".into();
            f.detail.threads.push(lock);
            f
        },
        Part::All,
        |review, cx| review.story_threads(ThreadScope::Active, true, cx),
        window,
        cx,
    )
}

/// Read-only token: cards without Reply / Resolve.
pub fn threads_read_only_story(window: &mut Window, cx: &mut App) -> AnyElement {
    fixture_review(
        "review-threads-read-only",
        fixtures::small,
        Part::All,
        |review, cx| {
            review.story_read_only(cx);
            review.story_threads(ThreadScope::Active, false, cx);
        },
        window,
        cx,
    )
}

/// Groups hidden (the default): Dana and Tom, then "Waiting on 1 required
/// group"; the status control with its `⇧S` keycap.
pub fn header_groups_story(window: &mut Window, cx: &mut App) -> AnyElement {
    fixture_review(
        "review-header-groups",
        fixtures::small,
        Part::Header,
        |review, cx| review.story_group_reviewers(false, cx),
        window,
        cx,
    )
}

/// Show group reviewers on: both groups in the row, no chip.
pub fn header_groups_shown_story(window: &mut Window, cx: &mut App) -> AnyElement {
    fixture_review(
        "review-header-groups-shown",
        fixtures::small,
        Part::Header,
        |review, cx| review.story_group_reviewers(true, cx),
        window,
        cx,
    )
}

/// A draft with write access: Publish (`⇧P`) first in the actions row.
pub fn header_draft_story(window: &mut Window, cx: &mut App) -> AnyElement {
    fixture_review(
        "review-header-draft",
        || {
            let mut f = fixtures::small();
            f.detail.pr.is_draft = true;
            f
        },
        Part::Header,
        |review, cx| review.story_group_reviewers(false, cx),
        window,
        cx,
    )
}

/// The demo list on the All tab with every active PR given policy groups
/// (one required without a vote, one optional approved), `show` as set;
/// `setup` puts it into the state under test.
fn group_list(
    key: &'static str,
    show: bool,
    setup: impl FnOnce(&mut ReviewView, &mut Window, &mut Context<ReviewView>),
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let view = window.use_keyed_state(key, cx, |window, cx| {
        let now = adoken_demo::DEMO_NOW;
        let dir = crate::story_dir(key);
        let store = adoken_demo::open_store_at(&dir, now).expect("seed the demo store");
        story::seed_group_reviewers(&store, now);
        let mut view = story::list(store, now, window, cx);
        view.story_group_reviewers(show, cx);
        setup(&mut view, window, cx);
        view
    });
    div().size_full().child(view).into_any_element()
}

/// Hidden groups: each row ends its reviewers with the dimmed group count.
pub fn list_groups_story(window: &mut Window, cx: &mut App) -> AnyElement {
    group_list("review-list-groups", false, |_, _, _| {}, window, cx)
}

/// Groups shown: their initials avatars in the vote groups, no count.
pub fn list_groups_shown_story(window: &mut Window, cx: &mut App) -> AnyElement {
    group_list("review-list-groups-shown", true, |_, _, _| {}, window, cx)
}

/// The demo list (All tab) with the row of the first PR `pick` accepts
/// selected, then `setup`.
fn list_with(
    key: &'static str,
    pick: fn(&PullRequest) -> bool,
    setup: impl FnOnce(&mut ReviewView, &mut Window, &mut Context<ReviewView>),
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let view = window.use_keyed_state(key, cx, |window, cx| {
        let now = adoken_demo::DEMO_NOW;
        let dir = crate::story_dir(key);
        let store = adoken_demo::open_store_at(&dir, now).expect("seed the demo store");
        story::seed_row_badges(&store, now);
        let id = store
            .prs(&PrFilter::default())
            .expect("read PRs")
            .into_iter()
            .find(pick)
            .expect("the demo org has such a PR")
            .id;
        let mut view = story::list(store, now, window, cx);
        view.set_tab(ListTab::All, cx);
        assert!(story::select_pr(&mut view, id, cx), "!{id} is listed");
        setup(&mut view, window, cx);
        view
    });
    div().size_full().child(view).into_any_element()
}

fn active(pr: &PullRequest) -> bool {
    pr.status == PrStatus::Active && !pr.is_draft
}

fn draft(pr: &PullRequest) -> bool {
    pr.status == PrStatus::Active && pr.is_draft
}

fn abandoned(pr: &PullRequest) -> bool {
    pr.status == PrStatus::Abandoned
}

/// Where the row menu stories right-click: on the selected row's title.
fn row_menu(view: &mut ReviewView, window: &mut Window, cx: &mut Context<ReviewView>) {
    view.story_row_menu(point(px(420.0), px(140.0)), window, cx);
}

/// `⌘K` / right-click on an active row: Mark as draft and Abandon… after
/// Pin.
pub fn list_row_menu_story(window: &mut Window, cx: &mut App) -> AnyElement {
    list_with("review-list-row-menu", active, row_menu, window, cx)
}

/// A draft's row menu: Publish instead of Mark as draft.
pub fn list_row_menu_draft_story(window: &mut Window, cx: &mut App) -> AnyElement {
    list_with("review-list-row-menu-draft", draft, row_menu, window, cx)
}

/// An abandoned PR's row menu: Reactivate is its only status item.
pub fn list_row_menu_abandoned_story(window: &mut Window, cx: &mut App) -> AnyElement {
    list_with(
        "review-list-row-menu-abandoned",
        abandoned,
        row_menu,
        window,
        cx,
    )
}

/// `⇧A` on the list: the review's confirmation, over the list.
pub fn list_abandon_confirm_story(window: &mut Window, cx: &mut App) -> AnyElement {
    list_with(
        "review-list-abandon-confirm",
        active,
        |view, _, cx| view.story_list_abandon(cx),
        window,
        cx,
    )
}

/// `⇧P` on an active row: Draft with "Sending…" until Azure DevOps
/// answers.
pub fn list_sending_story(window: &mut Window, cx: &mut App) -> AnyElement {
    list_with(
        "review-list-sending",
        active,
        |view, _, cx| view.story_list_sending(StatusAction::MarkDraft, cx),
        window,
        cx,
    )
}

/// A click on the header title: "Copied title" over the open review.
pub fn copied_title_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let view = window.use_keyed_state("review-copied-title", cx, |window, cx| {
        let now = adoken_demo::DEMO_NOW;
        let dir = crate::story_dir("review-copied-title");
        let store = adoken_demo::open_store_at(&dir, now).expect("seed the demo store");
        let mut view = story::list(store, now, window, cx);
        view.open_pr(TS_PR, window, cx);
        story::show_copied(&mut view, "Copied title", cx);
        view
    });
    div().size_full().child(view).into_any_element()
}
