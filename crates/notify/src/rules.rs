//! User rules (FR-4: ordered, first match wins) and mutes (FR-2, flow
//! step 3 `m` / `Shift+M`).

use adoken_model::{PersonId, RunResult, Subject};
use serde::{Deserialize, Serialize};

use crate::classify::{EventContext, Kind, Priority, Source};

/// Conditions of a rule; every set field must match (unset = any).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuleMatch {
    pub source: Option<Source>,
    pub kind: Option<Kind>,
    /// Project id or name.
    pub project: Option<String>,
    /// Repository name (case-insensitive) or id.
    pub repo: Option<String>,
    /// Pipeline definition name (case-insensitive) or id.
    pub pipeline: Option<String>,
    /// Actor identity id or display name.
    pub actor: Option<String>,
    /// Short branch name, e.g. `main`.
    pub branch: Option<String>,
    pub result: Option<RunResult>,
    /// Only when I'm a required reviewer.
    pub required_reviewer: bool,
}

fn eq_opt(wanted: &Option<String>, candidates: &[Option<&str>]) -> bool {
    match wanted {
        None => true,
        Some(wanted) => candidates
            .iter()
            .flatten()
            .any(|have| have.eq_ignore_ascii_case(wanted)),
    }
}

impl RuleMatch {
    pub fn matches(&self, event: &EventRef<'_>) -> bool {
        let ctx = event.ctx;
        let pipeline_id = ctx.pipeline_id.map(|id| id.to_string());
        self.source.is_none_or(|s| s == event.kind.source())
            && self.kind.is_none_or(|k| k == event.kind)
            && eq_opt(&self.project, &[ctx.project.as_deref()])
            && eq_opt(
                &self.repo,
                &[ctx.repo_name.as_deref(), ctx.repo_id.as_deref()],
            )
            && eq_opt(
                &self.pipeline,
                &[ctx.pipeline_name.as_deref(), pipeline_id.as_deref()],
            )
            && eq_opt(&self.actor, &[event.actor, ctx.actor_name.as_deref()])
            && eq_opt(&self.branch, &[ctx.branch.as_deref()])
            && self.result.is_none_or(|r| ctx.build_result == Some(r))
            && (!self.required_reviewer || ctx.required_reviewer)
    }

    /// "Pipelines · failed · main".
    pub fn label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(source) = self.source {
            parts.push(source.label().into());
        }
        if let Some(kind) = self.kind {
            parts.push(kind.as_str().into());
        }
        for (prefix, value) in [
            ("project", &self.project),
            ("repo", &self.repo),
            ("pipeline", &self.pipeline),
            ("from", &self.actor),
            ("branch", &self.branch),
        ] {
            if let Some(value) = value {
                parts.push(format!("{prefix}:{value}"));
            }
        }
        if let Some(result) = self.result {
            parts.push(result.as_str().into());
        }
        if self.required_reviewer {
            parts.push("I'm required reviewer".into());
        }
        if parts.is_empty() {
            "Everything".into()
        } else {
            parts.join(" · ")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleAction {
    SetPriority(Priority),
    Mute,
    SkipBanner,
}

impl RuleAction {
    pub fn label(self) -> String {
        match self {
            Self::SetPriority(p) => format!("Set {}", p.label()),
            Self::Mute => "Mute".into(),
            Self::SkipBanner => "Inbox only".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRule {
    pub name: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    pub when: RuleMatch,
    pub then: RuleAction,
}

fn enabled() -> bool {
    true
}

/// The parts of an incoming event rules and mutes look at.
#[derive(Clone, Copy, Debug)]
pub struct EventRef<'a> {
    pub kind: Kind,
    pub subject: &'a Subject,
    pub actor: Option<&'a str>,
    pub ctx: &'a EventContext,
}

/// First enabled matching rule's action (FR-4).
pub fn evaluate(rules: &[UserRule], event: &EventRef<'_>) -> Option<RuleAction> {
    rules
        .iter()
        .find(|rule| rule.enabled && rule.when.matches(event))
        .map(|rule| rule.then)
}

/// What `m` / `Shift+M` silence. Muting hides existing and future
/// notifications in scope; unmuting restores only future ones (AC-9).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mute {
    Subject { subject: Subject, title: String },
    Repo { id: String, name: String },
    Pipeline { id: i64, name: String },
    WorkItemType { name: String },
    Author { id: PersonId, name: String },
}

impl Mute {
    pub fn matches(&self, event: &EventRef<'_>) -> bool {
        let ctx = event.ctx;
        match self {
            Self::Subject { subject, .. } => subject == event.subject,
            Self::Repo { id, .. } => ctx.repo_id.as_deref() == Some(id),
            Self::Pipeline { id, .. } => ctx.pipeline_id == Some(*id),
            Self::WorkItemType { name } => ctx
                .work_item_type
                .as_deref()
                .is_some_and(|t| t.eq_ignore_ascii_case(name)),
            Self::Author { id, .. } => event.actor == Some(id.as_str()),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Subject { title, .. } => title.clone(),
            Self::Repo { name, .. } => format!("Repository {name}"),
            Self::Pipeline { name, .. } => format!("Pipeline {name}"),
            Self::WorkItemType { name } => format!("Work item type {name}"),
            Self::Author { name, .. } => format!("Activity from {name}"),
        }
    }

    /// `Shift+M` choices for an event: repo / pipeline / work item type /
    /// author, whichever the context has.
    pub fn scopes_for(actor: Option<&str>, ctx: &EventContext) -> Vec<Mute> {
        let mut out = Vec::new();
        if let (Some(id), Some(name)) = (&ctx.repo_id, &ctx.repo_name) {
            out.push(Self::Repo {
                id: id.clone(),
                name: name.clone(),
            });
        }
        if let (Some(id), Some(name)) = (ctx.pipeline_id, &ctx.pipeline_name) {
            out.push(Self::Pipeline {
                id,
                name: name.clone(),
            });
        }
        if let Some(name) = &ctx.work_item_type {
            out.push(Self::WorkItemType { name: name.clone() });
        }
        if let Some(id) = actor {
            out.push(Self::Author {
                id: id.to_owned(),
                name: ctx.actor_name.clone().unwrap_or_else(|| id.to_owned()),
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_model::SyncEventKind;

    fn build_ctx(branch: &str, result: RunResult) -> EventContext {
        EventContext {
            pipeline_id: Some(7),
            pipeline_name: Some("payments-ci".into()),
            repo_id: Some("r1".into()),
            repo_name: Some("web".into()),
            branch: Some(branch.into()),
            build_result: Some(result),
            ..EventContext::default()
        }
    }

    fn rule(name: &str, when: RuleMatch, then: RuleAction) -> UserRule {
        UserRule {
            name: name.into(),
            enabled: true,
            when,
            then,
        }
    }

    #[test]
    fn first_matching_rule_wins() {
        let mut rules = vec![
            rule("disabled", RuleMatch::default(), RuleAction::Mute),
            rule(
                "red main",
                RuleMatch {
                    pipeline: Some("PAYMENTS-CI".into()),
                    branch: Some("main".into()),
                    result: Some(RunResult::Failed),
                    ..RuleMatch::default()
                },
                RuleAction::SetPriority(Priority::P0),
            ),
            rule(
                "all pipelines",
                RuleMatch {
                    source: Some(Source::Pipelines),
                    ..RuleMatch::default()
                },
                RuleAction::SkipBanner,
            ),
        ];
        rules[0].enabled = false;
        let subject = Subject::Build(1);
        let kind = Kind::Sync(SyncEventKind::BuildCompleted);
        let red_main = build_ctx("main", RunResult::Failed);
        let event = EventRef {
            kind,
            subject: &subject,
            actor: None,
            ctx: &red_main,
        };
        assert_eq!(
            evaluate(&rules, &event),
            Some(RuleAction::SetPriority(Priority::P0))
        );
        let red_feature = build_ctx("feature/x", RunResult::Failed);
        let event = EventRef {
            ctx: &red_feature,
            ..event
        };
        assert_eq!(evaluate(&rules, &event), Some(RuleAction::SkipBanner));
    }

    #[test]
    fn required_reviewer_and_actor_conditions() {
        let when = RuleMatch {
            actor: Some("Priya Shah".into()),
            required_reviewer: true,
            ..RuleMatch::default()
        };
        let subject = Subject::Pr(1);
        let mut ctx = EventContext {
            actor_name: Some("Priya Shah".into()),
            ..EventContext::default()
        };
        fn event<'a>(subject: &'a Subject, ctx: &'a EventContext) -> EventRef<'a> {
            EventRef {
                kind: Kind::Sync(SyncEventKind::MentionedInPr),
                subject,
                actor: Some("id-priya"),
                ctx,
            }
        }
        assert!(!when.matches(&event(&subject, &ctx)));
        ctx.required_reviewer = true;
        assert!(when.matches(&event(&subject, &ctx)));
        let by_id = RuleMatch {
            actor: Some("id-priya".into()),
            ..RuleMatch::default()
        };
        assert!(by_id.matches(&event(&subject, &ctx)));
    }

    #[test]
    fn mutes_match_their_scope() {
        let ctx = build_ctx("main", RunResult::Failed);
        let subject = Subject::Build(9);
        let event = EventRef {
            kind: Kind::Sync(SyncEventKind::BuildCompleted),
            subject: &subject,
            actor: Some("a"),
            ctx: &ctx,
        };
        let scopes = Mute::scopes_for(Some("a"), &ctx);
        assert_eq!(scopes.len(), 3);
        assert!(scopes.iter().all(|m| m.matches(&event)));
        assert!(
            !Mute::Repo {
                id: "other".into(),
                name: "api".into()
            }
            .matches(&event)
        );
    }

    #[test]
    fn rules_round_trip_through_toml() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct File {
            rules: Vec<UserRule>,
        }
        let file = File {
            rules: vec![
                rule(
                    "lead",
                    RuleMatch {
                        kind: Some(Kind::Sync(SyncEventKind::MentionedInPr)),
                        actor: Some("Priya Shah".into()),
                        ..RuleMatch::default()
                    },
                    RuleAction::SetPriority(Priority::P0),
                ),
                rule("quiet", RuleMatch::default(), RuleAction::SkipBanner),
            ],
        };
        let text = toml::to_string(&file).unwrap();
        assert_eq!(toml::from_str::<File>(&text).unwrap(), file);
    }
}
