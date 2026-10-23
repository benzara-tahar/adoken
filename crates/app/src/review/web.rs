//! Pull requests on the web (spec 0004 §3 step 9, user decision
//! 2026-09-30): a PR's canonical Azure DevOps URL, and Copy PR id / Copy
//! link / Open in Azure DevOps for the open review and the list selection;
//! Copy title / source branch / target branch (spec 0019 group E).
//! Copies are confirmed by a brief toast ("Copied !4821"); it appears and
//! goes without motion, so reduce motion changes nothing. The demo org's
//! links point at the fake `contoso` org: copying works, opening is off.

use std::collections::HashMap;

use adoken_model::{Project, PullRequest, Repo};
use adoken_store::Store;
use adoken_theme::{elevation, radius, size, space, typography};
use adoken_ui::{Icon, IconName, Theme, TypeStyleExt};
use gpui::{App, ClipboardItem, SharedString, div, prelude::*, px};

pub const COPY_ID_KEYS: &str = "shift-y";
pub const COPY_LINK_KEYS: &str = "y";
pub const OPEN_KEYS: &str = "o";
/// Same words as work items and pipelines.
pub const DEMO_OPEN_REASON: &str = "Demo org: fake data, nothing opens in Azure DevOps";
const NO_URL_REASON: &str = "This pull request's web address isn't known yet";
/// Principles §4: toasts are 360 wide at most and last 5 s.
const TOAST_WIDTH: f32 = 360.0;
pub const TOAST: std::time::Duration = std::time::Duration::from_secs(5);

/// `https://dev.azure.com/{org}/{project}/_git/{repo}/pullrequest/{id}`.
/// `org_url` is the stored org URL (`https://dev.azure.com/{org}/`, or a
/// legacy `https://{org}.visualstudio.com/` whose path is kept); names
/// are percent-encoded path segments (a space is `%20`). `None` when
/// `org_url` isn't a URL.
pub fn web_url(org_url: &str, project: &str, repo: &str, id: i64) -> Option<String> {
    let mut url = url::Url::parse(org_url).ok()?;
    url.path_segments_mut().ok()?.pop_if_empty().extend([
        project,
        "_git",
        repo,
        "pullrequest",
        &id.to_string(),
    ]);
    Some(url.into())
}

/// What a PR's link needs besides the PR: the org URL and the project and
/// repository names. Read off the UI thread with the list or the review.
#[derive(Clone, Debug, Default)]
pub struct Directory {
    org_url: Option<String>,
    projects: HashMap<String, String>,
    repos: HashMap<String, String>,
    demo: bool,
}

impl Directory {
    pub fn new(
        org_url: Option<String>,
        projects: impl IntoIterator<Item = Project>,
        repos: impl IntoIterator<Item = Repo>,
        demo: bool,
    ) -> Self {
        Self {
            org_url,
            projects: projects.into_iter().map(|p| (p.id, p.name)).collect(),
            repos: repos.into_iter().map(|r| (r.id, r.name)).collect(),
            demo,
        }
    }

    pub fn read(store: &Store) -> adoken_store::Result<Self> {
        Ok(Self::new(
            store.org()?.map(|org| org.url),
            store.projects()?,
            store.repos()?,
            store.meta("demo")?.is_some(),
        ))
    }

    pub fn link(&self, pr: &PullRequest) -> PrLink {
        let url = self
            .org_url
            .as_deref()
            .zip(self.projects.get(&pr.project_id))
            .zip(self.repos.get(&pr.repo_id))
            .and_then(|((org, project), repo)| web_url(org, project, repo, pr.id));
        PrLink {
            id: pr.id,
            url,
            demo: self.demo,
        }
    }
}

/// One PR's id and web address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrLink {
    pub id: i64,
    /// `None` until the org and the PR's project and repository are synced.
    pub url: Option<String>,
    /// Demo org: the URL is the fake `contoso` org's.
    pub demo: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebAction {
    CopyId,
    CopyLink,
    Open,
}

impl PrLink {
    /// `!4821`.
    pub fn id_text(&self) -> String {
        format!("!{}", self.id)
    }

    /// Why Copy link is off; `None` when it works.
    pub fn copy_blocker(&self) -> Option<&'static str> {
        self.url.is_none().then_some(NO_URL_REASON)
    }

    /// Why Open in Azure DevOps is off; `None` when it works.
    pub fn open_blocker(&self) -> Option<&'static str> {
        if self.demo {
            Some(DEMO_OPEN_REASON)
        } else {
            self.copy_blocker()
        }
    }

    /// Runs `action` and returns the toast for it: what was copied, or why
    /// nothing happened. A successful open shows the browser instead.
    pub fn run(&self, action: WebAction, cx: &mut App) -> Option<SharedString> {
        let copy = |text: String, cx: &mut App| {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        };
        match action {
            WebAction::CopyId => {
                let id = self.id_text();
                copy(id.clone(), cx);
                Some(format!("Copied {id}").into())
            }
            WebAction::CopyLink => match (&self.url, self.copy_blocker()) {
                (Some(url), None) => {
                    copy(url.clone(), cx);
                    Some("Copied link".into())
                }
                (_, why) => why.map(SharedString::from),
            },
            WebAction::Open => match (&self.url, self.open_blocker()) {
                (Some(url), None) => {
                    cx.open_url(url);
                    None
                }
                (_, why) => why.map(SharedString::from),
            },
        }
    }
}

/// What Copy title / Copy source branch / Copy target branch copy (spec
/// 0019 FR-22, FR-23).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyTarget {
    Title,
    SourceBranch,
    TargetBranch,
}

/// Toast names longer than this truncate in the middle (design §5).
const TOAST_NAME_CHARS: usize = 40;

/// The text `target` copies from `pr` and its toast: the title trimmed;
/// a branch as displayed (`refs/heads/` dropped, other refs whole).
pub fn copy_text(pr: &PullRequest, target: CopyTarget) -> (String, String) {
    match target {
        CopyTarget::Title => (pr.title.trim().to_owned(), "Copied title".to_owned()),
        CopyTarget::SourceBranch | CopyTarget::TargetBranch => {
            let branch = if target == CopyTarget::SourceBranch {
                &pr.source_branch
            } else {
                &pr.target_branch
            };
            let name = adoken_model::short_branch(branch).to_owned();
            let toast = format!("Copied {}", truncate_middle(&name, TOAST_NAME_CHARS));
            (name, toast)
        }
    }
}

/// `feature/very…/name`: keeps both ends of a long name.
pub fn truncate_middle(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_owned();
    }
    let keep = max.saturating_sub(1);
    let head: String = text.chars().take(keep.div_ceil(2)).collect();
    let tail: String = text.chars().skip(count - keep / 2).collect();
    format!("{head}…{tail}")
}

/// Copy `target` of `pr` to the clipboard; returns the toast.
pub fn copy_pr_text(pr: &PullRequest, target: CopyTarget, cx: &mut App) -> SharedString {
    let (text, toast) = copy_text(pr, target);
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    toast.into()
}

/// A toast's meaning: done (a check) or refused (a cross, spec 0019
/// FR-21).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ToastKind {
    #[default]
    Done,
    Failed,
}

/// Principles §4 toast: bottom-right, `bg.overlay`, `elevation.2`,
/// `radius.md`, announced politely. The owner removes it after [`TOAST`].
pub fn toast(text: SharedString, kind: ToastKind, theme: &Theme) -> impl IntoElement + use<> {
    let colors = &theme.colors;
    let (icon, color) = match kind {
        ToastKind::Done => (IconName::Check, colors.accent_fg),
        ToastKind::Failed => (IconName::CircleX, colors.status_danger_fg),
    };
    div()
        .id("pr-link-toast")
        .role(gpui::Role::Status)
        .aria_label(text.clone())
        .absolute()
        .bottom(px(space::S16))
        .right(px(space::S16))
        .max_w(px(TOAST_WIDTH))
        .flex()
        .items_center()
        .gap(px(space::S8))
        .px(px(space::S12))
        .py(px(space::S8))
        .rounded(px(radius::MD))
        .border_1()
        .border_color(colors.border_default)
        .bg(colors.bg_overlay)
        .shadow(theme.shadows(elevation::LEVEL_2))
        .child(Icon::new(icon).size(size::ICON_MD).color(color))
        .child(
            div()
                .min_w_0()
                .text_color(colors.text_primary)
                .type_style(typography::BODY, theme)
                .child(text),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(id: i64) -> PullRequest {
        let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
        PullRequest {
            id,
            project_id: "p1".into(),
            repo_id: "r1".into(),
            ..ds.prs[0].clone()
        }
    }

    fn directory(org_url: Option<&str>, demo: bool) -> Directory {
        Directory::new(
            org_url.map(str::to_owned),
            [Project {
                id: "p1".into(),
                name: "Mobile App".into(),
            }],
            [Repo {
                id: "r1".into(),
                project_id: "p1".into(),
                name: "ios client".into(),
                default_branch: None,
            }],
            demo,
        )
    }

    #[test]
    fn web_urls_escape_names_and_keep_the_org_url_form() {
        let url = |org| web_url(org, "Mobile App", "ios client", 4821);
        let expected =
            "https://dev.azure.com/contoso/Mobile%20App/_git/ios%20client/pullrequest/4821";
        assert_eq!(
            url("https://dev.azure.com/contoso/").as_deref(),
            Some(expected)
        );
        assert_eq!(
            url("https://dev.azure.com/contoso").as_deref(),
            Some(expected)
        );
        assert_eq!(
            url("https://contoso.visualstudio.com/").as_deref(),
            Some(
                "https://contoso.visualstudio.com/Mobile%20App/_git/ios%20client/pullrequest/4821"
            )
        );
        assert_eq!(
            url("https://contoso.visualstudio.com/DefaultCollection/").as_deref(),
            Some(
                "https://contoso.visualstudio.com/DefaultCollection/Mobile%20App/_git/ios%20client/pullrequest/4821"
            )
        );
        // A `#` or `?` in a name must not end the path.
        assert_eq!(
            web_url("https://dev.azure.com/contoso/", "R&D #2", "a?b", 7).as_deref(),
            Some("https://dev.azure.com/contoso/R&D%20%232/_git/a%3Fb/pullrequest/7")
        );
        assert_eq!(url("not a url"), None);
    }

    #[test]
    fn ac_e1_e2_title_trimmed_branches_as_displayed() {
        let mut p = pr(1);
        p.title = "  Fix auth regression ".into();
        p.source_branch = "refs/heads/feature/fix-auth".into();
        p.target_branch = "refs/heads/main".into();
        let copy = |p: &PullRequest, target| copy_text(p, target);
        assert_eq!(
            copy(&p, CopyTarget::Title),
            ("Fix auth regression".into(), "Copied title".into())
        );
        assert_eq!(
            copy(&p, CopyTarget::SourceBranch),
            ("feature/fix-auth".into(), "Copied feature/fix-auth".into())
        );
        assert_eq!(copy(&p, CopyTarget::TargetBranch).0, "main");
        // A ref outside refs/heads/ is copied whole.
        p.source_branch = "refs/pull/12/merge".into();
        assert_eq!(copy(&p, CopyTarget::SourceBranch).0, "refs/pull/12/merge");
        // Long names keep both ends in the toast, never on the clipboard.
        p.source_branch = format!("refs/heads/feature/{}/name", "very".repeat(12));
        let (text, toast) = copy(&p, CopyTarget::SourceBranch);
        assert!(text.ends_with("/name") && text.len() > 40);
        assert!(toast.starts_with("Copied feature/very") && toast.ends_with("/name"));
        assert_eq!(toast.chars().count(), "Copied ".len() + 40);
    }

    #[test]
    fn links_need_the_org_url_and_names_and_demo_never_opens() {
        let real = directory(Some("https://dev.azure.com/contoso/"), false).link(&pr(1));
        assert_eq!((real.copy_blocker(), real.open_blocker()), (None, None));

        let demo = directory(Some("https://dev.azure.com/contoso"), true).link(&pr(1));
        assert!(
            demo.url.is_some(),
            "the demo org copies its fake contoso link"
        );
        assert_eq!(demo.copy_blocker(), None);
        assert_eq!(demo.open_blocker(), Some(DEMO_OPEN_REASON));

        let unsynced = PullRequest {
            repo_id: "unknown".into(),
            ..pr(1)
        };
        let dir = directory(Some("https://dev.azure.com/contoso/"), false);
        assert_eq!(dir.link(&unsynced).url, None);
        assert!(dir.link(&unsynced).open_blocker().is_some());
        assert_eq!(directory(None, false).link(&pr(1)).url, None);
        assert_eq!(real.id_text(), "!1");
    }

    /// Keyboard-first: each link action has one key in both the PR list and
    /// the review, nothing else there (or global) takes that key, and ⌘P
    /// lists the action with it.
    #[test]
    fn link_keys_work_in_list_and_review_without_conflicts_and_show_in_the_palette() {
        use gpui::{Action, KeyBinding};

        use crate::help::binding_infos;
        use crate::review::{CopyPrId, CopyPrLink, OpenPrInAdo, palette_commands};

        let all = crate::key_bindings();
        let bound = binding_infos(&all);
        let commands = palette_commands();
        let actions: [(Box<dyn Action>, &str); 3] = [
            (Box::new(CopyPrId), COPY_ID_KEYS),
            (Box::new(CopyPrLink), COPY_LINK_KEYS),
            (Box::new(OpenPrInAdo), OPEN_KEYS),
        ];
        for (action, keys) in actions {
            let name = action.name();
            let label = &binding_infos(&[KeyBinding::new(keys, CopyPrId, None)])[0].keys;
            for context in ["PrList", crate::review::pr::KEY_CONTEXT] {
                let taken: Vec<&str> = bound
                    .iter()
                    .filter(|b| &b.keys == label)
                    .filter(|b| b.context.is_none() || b.context.as_deref() == Some(context))
                    .map(|b| b.action)
                    .collect();
                assert_eq!(taken, [name], "{label} in {context}");
            }
            let command = commands
                .iter()
                .find(|c| c.action.name() == name)
                .unwrap_or_else(|| panic!("{name} in ⌘P"));
            assert_eq!(command.keys, Some(keys), "{}", command.title);
        }
    }
}
