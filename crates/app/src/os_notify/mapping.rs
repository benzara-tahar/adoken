//! Pure mapping between notify's [`Banner`] and what macOS's
//! `UNUserNotificationCenter` gets (and hands back on click): request
//! identifiers, thread grouping, action categories, the dock badge label.
//! No AppKit here, so all of it is unit tested.

use adoken_model::Subject;
use adoken_notify::{Banner, BannerAction};

use crate::palette::catalog::Target;

/// `UNNotificationAction` identifiers.
pub const ACTION_OPEN: &str = "open";
pub const ACTION_SNOOZE: &str = "snooze-1h";

/// `UNNotificationCategory` identifiers, one per action set notify produces.
pub const CATEGORY_OPEN: &str = "dev.adoken.open";
pub const CATEGORY_OPEN_SNOOZE: &str = "dev.adoken.open-snooze";

/// Separates the thread from the notification id in a request identifier.
/// Thread ids are subject keys (`pr:912`) or `summary:<noun>`, never `#`.
const ID_SEPARATOR: char = '#';

/// One `UNNotificationRequest`, ready for the bridge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// Reposting the same identifier replaces the delivered banner: a
    /// coalesced subject updates its banner instead of stacking a new one,
    /// and a source's newer summary replaces the older one.
    pub id: String,
    pub title: String,
    pub body: String,
    /// `threadIdentifier`: Notification Center groups by it (spec 0005 FR-7:
    /// one group per subject).
    pub thread_id: String,
    pub category: Option<&'static str>,
}

pub fn request(banner: &Banner) -> Request {
    let id = match banner.notification {
        Some(n) => format!("{}{ID_SEPARATOR}{n}", banner.thread_id),
        None => banner.thread_id.clone(),
    };
    Request {
        id,
        title: banner.title.clone(),
        body: banner.body.clone(),
        thread_id: banner.thread_id.clone(),
        category: category(&banner.actions),
    }
}

/// The registered category matching an action set; `None` = no buttons.
pub fn category(actions: &[BannerAction]) -> Option<&'static str> {
    let snooze = actions.contains(&BannerAction::SnoozeOneHour);
    let open = actions.contains(&BannerAction::Open);
    match (open, snooze) {
        (_, true) => Some(CATEGORY_OPEN_SNOOZE),
        (true, false) => Some(CATEGORY_OPEN),
        (false, false) => None,
    }
}

/// Where a clicked banner goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// The subject's canonical view (PR → review, WI/run → detail).
    Target(Target),
    /// Summaries and account items: the inbox itself.
    Inbox,
}

/// A banner the user activated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Click {
    pub route: Route,
    /// The inbox row to focus; `None` for summaries.
    pub notification: Option<i64>,
    pub action: BannerAction,
}

/// Maps the delegate's `(request identifier, action identifier)` back to a
/// [`Click`]. `action` is `None` for the default action (body click) and for
/// "Open"; unknown action ids (e.g. the dismiss action) are ignored.
pub fn click(request_id: &str, action: Option<&str>) -> Option<Click> {
    let action = match action {
        None | Some(ACTION_OPEN) => BannerAction::Open,
        Some(ACTION_SNOOZE) => BannerAction::SnoozeOneHour,
        Some(_) => return None,
    };
    let (thread, notification) = match request_id.rsplit_once(ID_SEPARATOR) {
        Some((thread, n)) => (thread, Some(n.parse().ok()?)),
        None => (request_id, None),
    };
    Some(Click {
        route: route(thread),
        notification,
        action,
    })
}

/// Thread id → destination. Subject keys route to their view; anything
/// else (`summary:pipelines`, `account`) opens the inbox.
pub fn route(thread_id: &str) -> Route {
    match Subject::parse_key(thread_id) {
        Some(Subject::Pr(id)) => Route::Target(Target::PullRequest(id)),
        Some(Subject::WorkItem(id)) => Route::Target(Target::WorkItem(id)),
        Some(Subject::Build(id)) => Route::Target(Target::Run(id)),
        Some(Subject::Account) | None => Route::Inbox,
    }
}

/// Dock tile badge text; `None` clears it. The dock truncates long labels,
/// so large counts cap at "99+".
pub fn badge_label(count: usize) -> Option<String> {
    match count {
        0 => None,
        1..=99 => Some(count.to_string()),
        _ => Some("99+".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn banner(thread: &str, notification: Option<i64>, actions: Vec<BannerAction>) -> Banner {
        Banner {
            title: "Mentioned in !912".into(),
            body: "Ada: @you can you check the retry path?".into(),
            thread_id: thread.into(),
            notification,
            actions,
        }
    }

    #[test]
    fn subject_banners_group_by_subject_and_replace_per_row() {
        let a = request(&banner(
            "pr:912",
            Some(7),
            vec![BannerAction::Open, BannerAction::SnoozeOneHour],
        ));
        let b = request(&banner("pr:912", Some(8), vec![BannerAction::Open]));
        assert_eq!(a.thread_id, "pr:912");
        assert_eq!(a.thread_id, b.thread_id, "same subject → same group");
        assert_ne!(a.id, b.id, "different rows stay separate in the group");
        let again = request(&banner("pr:912", Some(7), vec![BannerAction::Open]));
        assert_eq!(a.id, again.id, "a coalesced row replaces its own banner");
        assert_eq!(a.category, Some(CATEGORY_OPEN_SNOOZE));
        assert_eq!(b.category, Some(CATEGORY_OPEN));
    }

    #[test]
    fn summaries_replace_the_previous_summary_of_their_source() {
        let first = request(&banner("summary:pipeline", None, vec![BannerAction::Open]));
        let next = request(&banner("summary:pipeline", None, vec![BannerAction::Open]));
        let other = request(&banner("summary:mention", None, vec![BannerAction::Open]));
        assert_eq!(first.id, next.id);
        assert_ne!(first.thread_id, other.thread_id);
        assert_eq!(category(&[]), None);
    }

    #[test]
    fn clicks_route_to_the_subject_view() {
        let id = request(&banner("pr:912", Some(7), vec![])).id;
        assert_eq!(
            click(&id, None),
            Some(Click {
                route: Route::Target(Target::PullRequest(912)),
                notification: Some(7),
                action: BannerAction::Open,
            })
        );
        assert_eq!(
            click("wi:88121#3", Some(ACTION_SNOOZE)),
            Some(Click {
                route: Route::Target(Target::WorkItem(88121)),
                notification: Some(3),
                action: BannerAction::SnoozeOneHour,
            })
        );
        assert_eq!(
            click("run:5521#4", Some(ACTION_OPEN)).map(|c| c.route),
            Some(Route::Target(Target::Run(5521)))
        );
    }

    #[test]
    fn summaries_and_unknown_threads_open_the_inbox() {
        assert_eq!(
            click("summary:pipeline", None),
            Some(Click {
                route: Route::Inbox,
                notification: None,
                action: BannerAction::Open,
            })
        );
        assert_eq!(route("account"), Route::Inbox);
        assert_eq!(route("pr:not-a-number"), Route::Inbox);
    }

    #[test]
    fn unknown_actions_and_malformed_ids_are_ignored() {
        assert_eq!(
            click(
                "pr:912#7",
                Some("com.apple.UNNotificationDismissActionIdentifier")
            ),
            None
        );
        assert_eq!(click("pr:912#seven", None), None);
    }

    #[test]
    fn badge_clears_at_zero_and_caps() {
        assert_eq!(badge_label(0), None);
        assert_eq!(badge_label(1).as_deref(), Some("1"));
        assert_eq!(badge_label(99).as_deref(), Some("99"));
        assert_eq!(badge_label(100).as_deref(), Some("99+"));
    }
}
