//! Group reviewers (spec 0019 group C, contract C1): Azure DevOps teams and
//! policy groups added as reviewers (`Person.is_group`, the wire's
//! `isContainer`) are hidden from reviewer lists unless
//! `[review] show_group_reviewers = true`.
//!
//! Hiding is display only (FR-17): tabs, vote counts, Complete's blocker,
//! pins and Today's stages keep counting every reviewer. While groups are
//! hidden, the one fact that blocks a merge stays visible: required groups
//! that haven't voted ([`pending_required_groups`], FR-16).
//!
//! Read API for every view that draws reviewers:
//! - [`shown`]: the setting now (`false` until read, and in stories).
//! - [`observe`]: a view repaints when the setting changes (Settings switch,
//!   `⌘P`, or an outside edit of `settings.toml`, within one poll). The
//!   first call reads the file off the UI thread.
//! - [`set_shown`] / [`toggle`]: apply now, then write `settings.toml`.
//! - [`visible_reviewers`], [`pending_required_groups`], [`waiting_label`],
//!   [`waiting_tooltip`]: the display rules.

use std::collections::HashMap;

use adoken_model::{Person, PersonId, PullRequest, Reviewer, Vote};
use gpui::{App, AppContext as _, Context, Entity, Global, Subscription, Task};

/// `settings.toml` table and key (0009 FR-1).
pub const SETTINGS_TABLE: &str = "review";
pub const SHOW_GROUP_REVIEWERS_KEY: &str = "show_group_reviewers";

/// `[review] show_group_reviewers` out of the whole file's text; missing or
/// malformed means `false`.
pub fn from_toml(text: &str) -> bool {
    toml::from_str::<toml::Table>(text).is_ok_and(|root| from_document(&root))
}

/// [`from_toml`] over an already parsed file.
pub fn from_document(root: &toml::Table) -> bool {
    root.get(SETTINGS_TABLE)
        .and_then(toml::Value::as_table)
        .and_then(|t| t.get(SHOW_GROUP_REVIEWERS_KEY))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false)
}

/// A reviewer is a group when its identity says so; an identity missing
/// from the people map counts as a person (FR-15, the safe side).
pub fn is_group(people: &HashMap<PersonId, Person>, id: &str) -> bool {
    people.get(id).is_some_and(|p| p.is_group)
}

/// `pr`'s reviewers to draw, in the PR's order: everyone when groups are
/// shown, else people only.
pub fn visible_reviewers<'a>(
    pr: &'a PullRequest,
    people: &'a HashMap<PersonId, Person>,
    show_groups: bool,
) -> impl Iterator<Item = &'a Reviewer> + 'a {
    pr.reviewers
        .iter()
        .filter(move |r| show_groups || !is_group(people, &r.person))
}

/// The list row's reviewer groups by vote ([`super::data::reviewer_groups`])
/// without group identities while they're hidden; a vote group left empty
/// goes. Counting elsewhere keeps using `reviewer_groups` (FR-17).
pub fn visible_groups<'a>(
    pr: &'a PullRequest,
    people: &HashMap<PersonId, Person>,
    show_groups: bool,
) -> Vec<(Vote, Vec<&'a Reviewer>)> {
    let mut groups = super::data::reviewer_groups(pr);
    if !show_groups {
        for (_, members) in &mut groups {
            members.retain(|r| !is_group(people, &r.person));
        }
        groups.retain(|(_, members)| !members.is_empty());
    }
    groups
}

/// FR-16: hidden groups that block the merge: required, not declined, no
/// vote yet. Empty while groups are shown.
pub fn pending_required_groups<'a>(
    pr: &'a PullRequest,
    people: &'a HashMap<PersonId, Person>,
    show_groups: bool,
) -> Vec<&'a Reviewer> {
    if show_groups {
        return Vec::new();
    }
    pr.reviewers
        .iter()
        .filter(|r| {
            r.is_required
                && !r.has_declined
                && r.vote == Vote::NoVote
                && is_group(people, &r.person)
        })
        .collect()
}

/// A group's name without its `[Project]\` prefix.
pub fn group_name(display: &str) -> &str {
    match display
        .strip_prefix('[')
        .and_then(|rest| rest.split_once("]\\"))
    {
        Some((_, name)) => name,
        None => display,
    }
}

/// "Waiting on 1 required group" / "Waiting on 2 required groups".
pub fn waiting_label(count: usize) -> String {
    match count {
        1 => "Waiting on 1 required group".to_owned(),
        n => format!("Waiting on {n} required groups"),
    }
}

/// The indicator's tooltip and accessible label: the label plus the
/// groups' names ("Waiting on 1 required group: React PR reviewers").
pub fn waiting_tooltip(groups: &[&Reviewer], people: &HashMap<PersonId, Person>) -> String {
    let names: Vec<&str> = groups
        .iter()
        .map(|r| {
            people
                .get(&r.person)
                .map_or(r.person.as_str(), |p| group_name(&p.display_name))
        })
        .collect();
    format!("{}: {}", waiting_label(groups.len()), names.join(", "))
}

/// The setting, owned by the review module and hot-reloaded.
pub struct GroupReviewers {
    show: bool,
    _settings: Subscription,
    _read: Option<Task<()>>,
}

struct Installed(Entity<GroupReviewers>);

impl Global for Installed {}

impl GroupReviewers {
    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(path) = crate::settings::path(cx) else {
            return;
        };
        let read = cx
            .background_executor()
            .spawn(async move { from_toml(&std::fs::read_to_string(path).unwrap_or_default()) });
        self._read = Some(cx.spawn(async move |this, cx| {
            let show = read.await;
            this.update(cx, |this, cx| {
                // Our own write may still be queued: the file is behind.
                if this.show != show && !crate::settings::is_writing(cx) {
                    this.show = show;
                    cx.notify();
                }
            })
            .ok();
        }));
    }
}

fn installed(cx: &mut App) -> Entity<GroupReviewers> {
    if let Some(Installed(entity)) = cx.try_global::<Installed>() {
        return entity.clone();
    }
    let entity = cx.new(|cx| {
        let mut this = GroupReviewers {
            show: false,
            _settings: crate::settings::observe(cx, |this: &mut GroupReviewers, cx| {
                this.reload(cx)
            }),
            _read: None,
        };
        this.reload(cx);
        this
    });
    cx.set_global(Installed(entity.clone()));
    // `⌘P` › "Toggle group reviewers" works from any view.
    cx.on_action(|_: &super::ToggleGroupReviewers, cx| toggle(cx));
    entity
}

/// `[review] show_group_reviewers` now; `false` until first read.
pub fn shown(cx: &App) -> bool {
    cx.try_global::<Installed>()
        .is_some_and(|Installed(entity)| entity.read(cx).show)
}

/// Repaint `V` whenever the setting changes. The first call anywhere
/// reads `settings.toml` (off the UI thread).
pub fn observe<V: 'static>(cx: &mut Context<V>) -> Subscription {
    let entity = installed(cx);
    cx.observe(&entity, |_, _, cx| cx.notify())
}

/// The setting now, in memory: every view repaints on the next frame
/// (Settings writes the file itself).
pub fn apply(show: bool, cx: &mut App) {
    let entity = installed(cx);
    entity.update(cx, |this, cx| {
        if this.show != show {
            this.show = show;
            cx.notify();
        }
    });
}

/// Show or hide group reviewers: every view repaints on the next frame,
/// then `settings.toml` is written.
pub fn set_shown(show: bool, cx: &mut App) {
    apply(show, cx);
    let mut table = toml::Table::new();
    table.insert(SHOW_GROUP_REVIEWERS_KEY.into(), show.into());
    crate::settings::write_table(cx, SETTINGS_TABLE, table);
}

/// `⌘P` › "Group reviewers: …": flip the setting.
pub fn toggle(cx: &mut App) {
    let show = !shown(cx);
    set_shown(show, cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_model::{MergeStatus, PrStatus};

    fn person(id: &str, name: &str, group: bool) -> Person {
        Person {
            id: id.into(),
            display_name: name.into(),
            unique_name: None,
            is_group: group,
            image_url: None,
        }
    }

    fn reviewer(id: &str, vote: Vote, required: bool, declined: bool) -> Reviewer {
        Reviewer {
            person: id.into(),
            vote,
            is_required: required,
            has_declined: declined,
            added_at: None,
            voted_at: None,
        }
    }

    fn pr(reviewers: Vec<Reviewer>) -> PullRequest {
        PullRequest {
            id: 1,
            repo_id: "r".into(),
            project_id: "p".into(),
            title: "t".into(),
            description: String::new(),
            author: "a".into(),
            reviewers,
            status: PrStatus::Active,
            is_draft: false,
            merge_status: MergeStatus::Succeeded,
            source_branch: "refs/heads/f".into(),
            target_branch: "refs/heads/main".into(),
            last_source_commit: None,
            created: 0,
            updated: 0,
            closed: None,
            iterations: 1,
            last_push: None,
        }
    }

    fn people() -> HashMap<PersonId, Person> {
        [
            person("dana", "Dana", false),
            person("react", "[Contoso]\\React PR reviewers", true),
            person("copilot", "[Contoso]\\Copilot reviewers", true),
            person("service", "[Contoso]\\Service reviewers", true),
        ]
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect()
    }

    #[test]
    fn ac_c4_missing_or_malformed_key_reads_false() {
        assert!(!from_toml(""));
        assert!(!from_toml("[review]\n"));
        assert!(!from_toml("[review]\nshow_group_reviewers = \"yes\"\n"));
        assert!(!from_toml("not toml ["));
        assert!(from_toml("[review]\nshow_group_reviewers = true\n"));
    }

    #[test]
    fn ac_c1_groups_hidden_people_and_unknown_identities_shown() {
        let people = people();
        let pr = pr(vec![
            reviewer("dana", Vote::Approved, false, false),
            reviewer("react", Vote::NoVote, true, false),
            reviewer("copilot", Vote::Approved, false, false),
            reviewer("tom", Vote::NoVote, true, false),
        ]);
        let ids = |show| {
            visible_reviewers(&pr, &people, show)
                .map(|r| r.person.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(false), ["dana", "tom"]);
        assert_eq!(ids(true), ["dana", "react", "copilot", "tom"]);
        let pending = pending_required_groups(&pr, &people, false);
        assert_eq!(
            waiting_tooltip(&pending, &people),
            "Waiting on 1 required group: React PR reviewers"
        );
        assert!(pending_required_groups(&pr, &people, true).is_empty());
    }

    #[test]
    fn ac_c2_only_required_groups_without_a_vote_are_pending() {
        let people = people();
        let pending = |reviewers| pending_required_groups(&pr(reviewers), &people, false).len();
        assert_eq!(
            pending(vec![reviewer("react", Vote::NoVote, false, false)]),
            0
        );
        assert_eq!(
            pending(vec![reviewer("react", Vote::Approved, true, false)]),
            0
        );
        assert_eq!(
            pending(vec![reviewer("react", Vote::WaitingForAuthor, true, false)]),
            0
        );
        assert_eq!(
            pending(vec![reviewer("react", Vote::NoVote, true, true)]),
            0
        );
        // A required person without a vote isn't a group.
        assert_eq!(
            pending(vec![reviewer("dana", Vote::NoVote, true, false)]),
            0
        );
        let two = pr(vec![
            reviewer("react", Vote::NoVote, true, false),
            reviewer("service", Vote::NoVote, true, false),
        ]);
        let groups = pending_required_groups(&two, &people, false);
        assert_eq!(
            waiting_tooltip(&groups, &people),
            "Waiting on 2 required groups: React PR reviewers, Service reviewers"
        );
    }

    #[test]
    fn group_names_drop_the_project_prefix_only() {
        assert_eq!(
            group_name("[Contoso]\\React PR reviewers"),
            "React PR reviewers"
        );
        assert_eq!(group_name("Release managers"), "Release managers");
        assert_eq!(group_name("[odd name"), "[odd name");
    }
}
