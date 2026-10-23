//! Work items view stories (spec 0008, spec 0015 FR-37, design
//! work-item-detail.md §14) over a seeded demo store (fake Contoso data)
//! with the clock pinned at `DEMO_NOW`. Detail stories open the demo's
//! fixtures (`adoken_demo::rich`) and reach their state the way a user
//! would (intents, typing) or, for Azure DevOps' answers, by handing the
//! write controller the answer.

use std::sync::LazyLock;

use adoken_app::workitems::detail::Intent;
use adoken_app::workitems::detail::model::Stop;
use adoken_app::workitems::model::Tab;
use adoken_app::workitems::writes::{self, Access, Conflict, RuleViolation, WiEdits, WiOutcome};
use adoken_app::workitems::{StoryHook, WorkItemsView, story};
use adoken_demo::Dataset;
use adoken_demo::rich::{BLOCKED, HTML, MARKDOWN, SCENARIO};
use adoken_model::{FieldKind, LinkKind, WorkItem};
use adoken_sync::wi::{WiChange, WiError};
use adoken_theme::space;
use adoken_ui::ActiveTheme;
use gpui::{AnyElement, App, Window, div, prelude::*, px};

static DATASET: LazyLock<Dataset> =
    LazyLock::new(|| adoken_demo::dataset_at(adoken_demo::DEFAULT_SEED, adoken_demo::DEMO_NOW));

fn item(index: usize) -> &'static WorkItem {
    &DATASET.work_items[index]
}

/// A teammate (not me, not a group): conflicts and mentions.
fn teammate() -> &'static adoken_model::Person {
    DATASET
        .people
        .iter()
        .find(|p| p.id != DATASET.me && !p.is_group)
        .expect("a teammate")
}

fn render(
    key: &'static str,
    tab: Tab,
    open: Option<i64>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let view = window.use_keyed_state(key, cx, |window, cx| {
        let dir = crate::story_dir(key);
        let mut view = story::view(&dir, tab, window, cx);
        if let Some(id) = open {
            view.open_work_item(id, window, cx);
        }
        view
    });
    div().size_full().child(view).into_any_element()
}

/// The detail of fixture `index` with `hook` run once it has loaded.
fn detail(
    key: &'static str,
    index: usize,
    hook: Option<StoryHook>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let view = window.use_keyed_state(key, cx, |window, cx| {
        let dir = crate::story_dir(key);
        story::detail(&dir, index, hook, window, cx)
    });
    div().size_full().child(view).into_any_element()
}

fn hook(
    f: impl FnOnce(&mut WorkItemsView, &mut Window, &mut gpui::Context<WorkItemsView>) + 'static,
) -> Option<StoryHook> {
    Some(Box::new(f))
}

/// My work, grouped by state, with the scenario bug's detail open.
pub fn my_work_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render(
        "work-items",
        Tab::MyWork,
        Some(item(SCENARIO).id),
        window,
        cx,
    )
}

/// By area: the path picker and one area's open items by state.
pub fn by_area_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render("work-items-area", Tab::Area, None, window, cx)
}

/// A store with nothing synced: the list's empty state.
pub fn empty_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let view = window.use_keyed_state("work-items-empty", cx, |window, cx| {
        let dir = crate::story_dir("work-items-empty");
        story::empty(&dir, window, cx)
    });
    div().size_full().child(view).into_any_element()
}

// ── spec 0015 detail stories (design §14) ────────────────────────────────

/// Default width (pane 828): header, grid, rich description, links,
/// discussion, history.
pub fn detail_story(window: &mut Window, cx: &mut App) -> AnyElement {
    detail("work-item-detail", SCENARIO, None, window, cx)
}

/// Two columns with the properties rail (pane 1208, `⌘⇧↩`).
pub fn wide_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, _, _| view.story_full_width());
    detail("work-item-detail-wide", SCENARIO, hook, window, cx)
}

/// 1024 window: the detail covers the list, back button first.
pub fn stacked_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, _, _| view.story_stacked());
    detail("work-item-detail-stacked", SCENARIO, hook, window, cx)
}

/// 900 window at text scale 1.3: one-column grid, shortened toolbar.
pub fn narrow_scale_story(window: &mut Window, cx: &mut App) -> AnyElement {
    window.set_rem_size(px(16.0 * 1.3));
    let hook = hook(|view, _, _| view.story_stacked());
    detail("work-item-detail-narrow-scale", SCENARIO, hook, window, cx)
}

pub fn state_menu_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, window, cx| view.run_intent(Intent::State, window, cx));
    detail("work-item-detail-state-menu", SCENARIO, hook, window, cx)
}

/// "dan" typed: Assign to me, Unassigned, Recent, People and directory
/// results (spec FR-37).
pub fn assign_picker_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, window, cx| {
        view.run_intent(Intent::Assign, window, cx);
        let directory = vec![adoken_ui::picker::Person::new(
            "d2910000-0000-0000-0000-00000000dada",
            "Dana Ruiz",
            "dana.ruiz@contoso.com",
        )];
        view.story_people("dan", directory, cx);
    });
    detail("work-item-detail-assign-picker", SCENARIO, hook, window, cx)
}

pub fn area_picker_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, window, cx| view.run_intent(Intent::Area, window, cx));
    detail("work-item-detail-area-picker", SCENARIO, hook, window, cx)
}

/// Current / Next / Recent above the tree (seeded sprint dates).
pub fn iteration_picker_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, window, cx| view.run_intent(Intent::Iteration, window, cx));
    detail(
        "work-item-detail-iteration-picker",
        SCENARIO,
        hook,
        window,
        cx,
    )
}

/// The title field at 240 characters (counter shows).
pub fn editing_title_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, window, cx| {
        view.run_intent(Intent::Title, window, cx);
        let long = "Timezone bug in daily settlement job: merchants east of UTC see the previous day's totals because the job uses the server's calendar day boundary instead of each merchant's configured timezone, and refunds land on the wrong day too";
        view.story_title_text(&long[..240.min(long.len())], cx);
    });
    detail("work-item-detail-editing-title", SCENARIO, hook, window, cx)
}

/// Markdown field in Write, after an edit ("Draft saved").
pub fn description_editor_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, window, cx| {
        view.run_intent(Intent::Description, window, cx);
        if let Some((_, text)) = view.description_editor(cx) {
            view.story_description_text(&format!("{text}\n- Owner: finance ops\n"), cx);
        }
    });
    detail(
        "work-item-detail-description-editor",
        MARKDOWN,
        hook,
        window,
        cx,
    )
}

/// The same editor in Preview.
pub fn description_preview_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, window, cx| {
        view.run_intent(Intent::Description, window, cx);
        view.story_toggle_preview(window, cx);
    });
    detail(
        "work-item-detail-description-preview",
        MARKDOWN,
        hook,
        window,
        cx,
    )
}

/// An HTML field: "HTML (saved as HTML)".
pub fn description_html_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, window, cx| view.run_intent(Intent::Description, window, cx));
    detail("work-item-detail-description-html", HTML, hook, window, cx)
}

/// "…can't edit yet: a table, 2 images and text colors".
pub fn description_blocked_story(window: &mut Window, cx: &mut App) -> AnyElement {
    detail(
        "work-item-detail-description-blocked",
        BLOCKED,
        None,
        window,
        cx,
    )
}

/// A description draft (Draft chip) and a composer draft.
pub fn draft_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let key = "work-item-detail-draft";
    let view = window.use_keyed_state(key, cx, |window, cx| {
        let dir = crate::story_dir(key);
        let store = adoken_demo::open_store_at(&dir, adoken_demo::DEMO_NOW).expect("seed");
        let id = item(MARKDOWN).id;
        let rev = item(MARKDOWN).rev;
        store
            .write(|tx| {
                for (kind, body) in [
                    (
                        adoken_store::wi::DraftKind::Description,
                        "## Goal\n\nFinance reconciles **daily**.\n",
                    ),
                    (
                        adoken_store::wi::DraftKind::Comment,
                        "Bank feed export is ready, can you check the format?",
                    ),
                ] {
                    adoken_store::wi::save_draft(
                        tx,
                        &adoken_store::wi::WiDraft {
                            work_item_id: id,
                            kind,
                            body: body.into(),
                            base_rev: rev,
                            updated: adoken_demo::DEMO_NOW - 2 * 3_600_000,
                        },
                    )?;
                }
                Ok(())
            })
            .expect("drafts");
        story::detail(&dir, MARKDOWN, None, window, cx)
    });
    div().size_full().child(view).into_any_element()
}

/// Every relation group, cursor on a work item row (remove button).
pub fn links_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, window, cx| view.story_cursor(Stop::Link(1), window, cx));
    detail("work-item-detail-links", SCENARIO, hook, window, cx)
}

/// Add link step 2: "Add a child to #…" with refused rows.
pub fn link_picker_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, window, cx| {
        view.run_intent(Intent::AddLink(Some(LinkKind::Child)), window, cx)
    });
    detail("work-item-detail-link-picker", SCENARIO, hook, window, cx)
}

/// Add link step 1: the relation menu.
pub fn link_menu_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, window, cx| view.run_intent(Intent::AddLink(None), window, cx));
    detail("work-item-detail-link-menu", SCENARIO, hook, window, cx)
}

/// Composer focused with "@da" and the mention list.
pub fn composer_mention_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, window, cx| {
        view.run_intent(Intent::Comment, window, cx);
        view.story_composer_text("Can you verify on staging? @da", window, cx);
    });
    detail(
        "work-item-detail-composer-mention",
        SCENARIO,
        hook,
        window,
        cx,
    )
}

/// Assignee and state sending, a pending comment, a link removing.
pub fn pending_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, _, cx| {
        let wi = item(SCENARIO).clone();
        story::pend(
            &wi,
            WiChange::State {
                to: "Resolved".into(),
                base: wi.state.clone(),
            },
            cx,
        );
        story::pend(
            &wi,
            WiChange::Assign {
                to: Some(teammate().clone()),
                base: wi.assigned_to.clone(),
            },
            cx,
        );
        story::pend(
            &wi,
            WiChange::RemoveLink {
                kind: LinkKind::Related,
                target: item(HTML).id,
            },
            cx,
        );
        story::comment(&wi, "Fix is in !4295.", None, cx);
        view.story_expand_all();
    });
    detail("work-item-detail-pending", SCENARIO, hook, window, cx)
}

/// Assignee refused (403) with Retry / Dismiss; a failed comment.
pub fn failed_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|_, _, cx| {
        let wi = item(SCENARIO).clone();
        story::answer(
            &wi,
            WiChange::Assign {
                to: Some(teammate().clone()),
                base: wi.assigned_to.clone(),
            },
            Err(WiError::NotAllowed {
                message: Some(
                    "TF237111: You don't have permission to save work items in this area.".into(),
                ),
            }),
            cx,
        );
        story::comment(&wi, "Fix is in !4295.", Some(WiError::Unreachable), cx);
    });
    detail("work-item-detail-failed", SCENARIO, hook, window, cx)
}

/// State changed meanwhile by a teammate: Apply mine / Keep theirs.
pub fn conflict_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|_, _, cx| {
        let wi = item(SCENARIO).clone();
        let change = WiChange::State {
            to: "Resolved".into(),
            base: wi.state.clone(),
        };
        let conflict = Conflict {
            server_rev: wi.rev + 1,
            theirs: Some("Closed".into()),
            by: teammate().id.clone(),
            by_name: teammate().display_name.clone(),
            at: adoken_demo::DEMO_NOW - 60_000,
            rebased: change.clone(),
        };
        story::answer(&wi, change, Ok(WiOutcome::Conflict(conflict)), cx);
    });
    detail("work-item-detail-conflict", SCENARIO, hook, window, cx)
}

/// "Azure DevOps needs Root cause to move to Resolved." with its values.
pub fn rule_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|_, _, cx| {
        let wi = item(SCENARIO).clone();
        let rule = RuleViolation {
            field: "Root cause".into(),
            reference: Some("Custom.RootCause".into()),
            kind: FieldKind::List(vec![
                "Configuration".into(),
                "Code defect".into(),
                "Data".into(),
            ]),
            message: "TF401320: Rule Error for field Root cause. Error code: Required.".into(),
            server_rev: wi.rev,
        };
        story::answer(
            &wi,
            WiChange::State {
                to: "Resolved".into(),
                base: wi.state.clone(),
            },
            Ok(WiOutcome::Rule(rule)),
            cx,
        );
    });
    detail("work-item-detail-rule", SCENARIO, hook, window, cx)
}

/// Read-only token: plain values and one passive hint, no token action.
pub fn read_only_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|_, _, cx| writes::set_access(Access::None, cx));
    detail("work-item-detail-read-only", SCENARIO, hook, window, cx)
}

/// Every freshness variant of design §3.4, side by side.
pub fn freshness_story(_: &mut Window, cx: &mut App) -> AnyElement {
    use adoken_app::workitems::detail::{badge, freshness_button};
    use adoken_app::workitems::writes::{FreshKind, Freshness};
    let theme = cx.theme().clone();
    let variants = [
        (FreshKind::Fresh, "Synced just now"),
        (FreshKind::Fresh, "Synced 2m ago"),
        (FreshKind::Stale, "Updated 18m ago"),
        (FreshKind::Partly, "Partly synced"),
        (FreshKind::Syncing, "Syncing…"),
        (FreshKind::Failed, "Couldn't sync · Retry"),
        (FreshKind::Offline, "Offline · synced 2h ago"),
        (FreshKind::Waiting, "Waiting for Azure DevOps…"),
        (FreshKind::Demo, "Demo data"),
    ];
    div()
        .size_full()
        .p(px(space::S24))
        .bg(theme.colors.bg_surface)
        .flex()
        .flex_col()
        .gap(px(space::S8))
        .children(variants.into_iter().enumerate().map(|(ix, (kind, label))| {
            freshness_button(
                ix as i64,
                badge(
                    Freshness {
                        kind,
                        label: label.into(),
                        tooltip: label.into(),
                        confirmed: None,
                    },
                    &theme,
                ),
                false,
                &theme,
            )
        }))
        .into_any_element()
}

/// A title-index item: skeletons in Description and Links.
pub fn partly_synced_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let me = DATASET.me.as_str();
    let index = DATASET
        .work_items
        .iter()
        .enumerate()
        .position(|(ix, wi)| {
            ![SCENARIO, MARKDOWN, HTML, BLOCKED].contains(&ix)
                && wi.id % 6 == 0
                && wi.assigned_to.as_deref() != Some(me)
                && wi.created_by != me
                && wi.changed_by != me
        })
        .expect("a partly synced fixture");
    detail("work-item-detail-partly-synced", index, None, window, cx)
}

/// Offline: freshness offline, Comment disabled.
pub fn offline_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, window, cx| {
        if let Some(edits) = WiEdits::global(cx) {
            edits.update(cx, |edits, cx| edits.set_offline(true, cx));
        }
        view.run_intent(Intent::Comment, window, cx);
        view.story_composer_text("Checked on staging, still off by a day.", window, cx);
    });
    detail("work-item-detail-offline", SCENARIO, hook, window, cx)
}

/// Scrolled: the sticky compact header.
pub fn sticky_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, _, _| {
        view.story_expand_all();
        view.story_scroll(700.0);
    });
    detail("work-item-detail-sticky", SCENARIO, hook, window, cx)
}

/// 404 on sync: the banner over the last data, no edit affordances.
pub fn not_available_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|_, _, cx| {
        let id = item(SCENARIO).id;
        if let Some(edits) = WiEdits::global(cx) {
            edits.update(cx, |edits, cx| {
                let now = edits.now();
                edits.syncs.done(id, &Err(WiError::Gone), now);
                cx.notify();
            });
        }
    });
    detail("work-item-detail-not-available", SCENARIO, hook, window, cx)
}

/// Remove link confirmation.
pub fn remove_link_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let hook = hook(|view, window, cx| view.story_remove_link(1, window, cx));
    detail("work-item-detail-remove-link", SCENARIO, hook, window, cx)
}
