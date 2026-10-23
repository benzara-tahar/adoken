//! Azure DevOps REST 7.1 payload shapes (only the fields sync uses) and
//! conversions to model values.

use std::collections::HashMap;

use adoken_model::{
    ChangeType, Comment, LineRange, MergeStatus, Millis, Person, PipelineRun, PrFileChange,
    PrStatus, PrThread, RunReason, RunResult, RunStage, RunStatus, StageOutcome, StateCategory,
    TextFormat, ThreadAnchor, ThreadStatus, Vote, WorkItem, WorkItemComment, WorkItemType,
};
use serde::Deserialize;
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub fn parse_time(s: &str) -> Option<Millis> {
    OffsetDateTime::parse(s, &Rfc3339)
        .ok()
        .map(|t| (t.unix_timestamp_nanos() / 1_000_000) as Millis)
}

pub fn format_time(ms: Millis) -> String {
    OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000)
        .ok()
        .and_then(|t| t.format(&Rfc3339).ok())
        .unwrap_or_default()
}

#[derive(Clone, Debug, Deserialize)]
pub struct List<T> {
    pub value: Vec<T>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IdentityRef {
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub unique_name: Option<String>,
    #[serde(default)]
    pub is_container: bool,
    /// `https://dev.azure.com/{org}/_api/_common/identityImage?id=…` in PR
    /// payloads, `…/_apis/GraphProfile/MemberAvatars/{descriptor}` (which
    /// refuses PATs) in build and work item payloads (spec 0012).
    #[serde(default)]
    pub image_url: Option<String>,
}

impl IdentityRef {
    pub fn person(&self) -> Person {
        Person {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            unique_name: self.unique_name.clone(),
            is_group: self.is_container,
            image_url: self.identity_image().map(str::to_owned),
        }
    }

    /// `imageUrl` when it is the identity-image route (spec 0012 FR-1);
    /// the GraphProfile form is dropped.
    pub fn identity_image(&self) -> Option<&str> {
        self.image_url
            .as_deref()
            .map(str::trim)
            .filter(|u| u.contains("/_api/_common/identityImage?"))
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionData {
    pub authenticated_user: ConnectionUser,
    /// The organization (collection) id.
    #[serde(default)]
    pub instance_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionUser {
    pub id: String,
    #[serde(default)]
    pub provider_display_name: String,
    /// `{"Account": {"$type": "System.String", "$value": "<sign-in name>"}}`.
    #[serde(default)]
    pub properties: HashMap<String, Value>,
}

impl ConnectionUser {
    /// Sign-in name (the identity's `uniqueName`); WIQL and build filters
    /// match people by it.
    pub fn account(&self) -> Option<&str> {
        self.properties
            .get("Account")
            .and_then(|a| a.get("$value"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProjectRef {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub default_branch: Option<String>,
    pub project: ProjectRef,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reviewer {
    #[serde(flatten)]
    pub identity: IdentityRef,
    #[serde(default)]
    pub vote: i8,
    #[serde(default)]
    pub is_required: bool,
    #[serde(default)]
    pub has_declined: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitRef {
    pub commit_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitPullRequest {
    pub pull_request_id: i64,
    pub repository: GitRepo,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub status: String,
    #[serde(default)]
    pub is_draft: bool,
    pub created_by: IdentityRef,
    pub creation_date: String,
    #[serde(default)]
    pub closed_date: Option<String>,
    pub source_ref_name: String,
    pub target_ref_name: String,
    #[serde(default)]
    pub merge_status: Option<String>,
    #[serde(default)]
    pub last_merge_source_commit: Option<CommitRef>,
    #[serde(default)]
    pub reviewers: Vec<Reviewer>,
    /// Who completed or abandoned it.
    #[serde(default)]
    pub closed_by: Option<IdentityRef>,
    /// Resolved from the iteration for the new source head, never PR authorship.
    #[serde(skip)]
    pub push_actor: Option<IdentityRef>,
}

impl GitPullRequest {
    pub fn status(&self) -> PrStatus {
        PrStatus::parse(&self.status).unwrap_or(PrStatus::Active)
    }
    pub fn merge_status(&self) -> MergeStatus {
        self.merge_status
            .as_deref()
            .and_then(MergeStatus::parse)
            .unwrap_or(MergeStatus::NotSet)
    }
    pub fn created(&self) -> Millis {
        parse_time(&self.creation_date).unwrap_or(0)
    }
    /// ADO sends `0001-01-01T00:00:00` for open PRs.
    pub fn closed(&self) -> Option<Millis> {
        self.closed_date
            .as_deref()
            .and_then(parse_time)
            .filter(|t| *t > 0)
    }
    pub fn vote_of(r: &Reviewer) -> Vote {
        Vote::from_i8(r.vote)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct LinePos {
    pub line: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadContext {
    pub file_path: String,
    #[serde(default)]
    pub right_file_start: Option<LinePos>,
    #[serde(default)]
    pub right_file_end: Option<LinePos>,
    #[serde(default)]
    pub left_file_start: Option<LinePos>,
    #[serde(default)]
    pub left_file_end: Option<LinePos>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IterationContext {
    #[serde(default)]
    pub second_comparing_iteration: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrThreadContext {
    #[serde(default)]
    pub iteration_context: Option<IterationContext>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireComment {
    pub id: i64,
    #[serde(default)]
    pub parent_comment_id: i64,
    pub author: IdentityRef,
    #[serde(default)]
    pub content: Option<String>,
    pub published_date: String,
    pub last_updated_date: String,
    #[serde(default)]
    pub comment_type: Option<String>,
    #[serde(default)]
    pub is_deleted: bool,
}

impl WireComment {
    pub fn into_model(self) -> Comment {
        Comment {
            id: self.id,
            parent_id: (self.parent_comment_id > 0).then_some(self.parent_comment_id),
            author: self.author.id,
            body: normalize_mentions(self.content.as_deref().unwrap_or_default()),
            created: parse_time(&self.published_date).unwrap_or(0),
            updated: parse_time(&self.last_updated_date).unwrap_or(0),
            is_system: self.comment_type.as_deref() == Some("system"),
            is_deleted: self.is_deleted,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireThread {
    pub id: i64,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub thread_context: Option<ThreadContext>,
    #[serde(default)]
    pub pull_request_thread_context: Option<PrThreadContext>,
    #[serde(default)]
    pub comments: Vec<WireComment>,
    pub last_updated_date: String,
}

impl WireThread {
    pub fn into_model(self, pr_id: i64) -> PrThread {
        let range = |s: &Option<LinePos>, e: &Option<LinePos>| {
            s.as_ref().map(|s| LineRange {
                start: s.line,
                end: e.as_ref().map(|e| e.line).unwrap_or(s.line),
            })
        };
        let anchor = self.thread_context.as_ref().map(|c| ThreadAnchor {
            file_path: c.file_path.clone(),
            right: range(&c.right_file_start, &c.right_file_end),
            left: range(&c.left_file_start, &c.left_file_end),
            iteration_blob: None,
        });
        PrThread {
            id: self.id,
            pr_id,
            status: self
                .status
                .as_deref()
                .and_then(ThreadStatus::parse)
                .unwrap_or(ThreadStatus::Unknown),
            anchor,
            iteration: self
                .pull_request_thread_context
                .and_then(|c| c.iteration_context)
                .and_then(|c| c.second_comparing_iteration),
            updated: parse_time(&self.last_updated_date).unwrap_or(0),
            comments: self
                .comments
                .into_iter()
                .map(WireComment::into_model)
                .collect(),
        }
    }

    /// Identities referenced by the thread's comments.
    pub fn people(&self) -> Vec<Person> {
        self.comments.iter().map(|c| c.author.person()).collect()
    }
}

/// `GET …/pullRequests/{id}/iterations?includeCommits=true` (spec 0004 §5,
/// 0017 FR-4): one push each. `commonRefCommit` is the merge base the
/// `$compareTo=0` changes diff against (verified live 2026-09-30).
/// `commits` lists the iteration's commits (absent without
/// `includeCommits`); `hasMoreCommits` marks a list cut short.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireIteration {
    pub id: u32,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub created_date: String,
    #[serde(default)]
    pub source_ref_commit: Option<CommitRef>,
    #[serde(default)]
    pub common_ref_commit: Option<CommitRef>,
    #[serde(default)]
    pub author: Option<IdentityRef>,
    #[serde(default)]
    pub push: Option<PushRef>,
    #[serde(default)]
    pub commits: Option<Vec<CommitRef>>,
    #[serde(default)]
    pub has_more_commits: bool,
}

impl WireIteration {
    /// Commits this push added: its commits whose ids `previous` (the
    /// iteration before it) doesn't list. Right whether Azure DevOps lists
    /// the push's commits or every commit so far. `None` when either list
    /// is missing.
    fn commits_added(&self, previous: Option<&WireIteration>) -> Option<u32> {
        let commits = self.commits.as_ref()?;
        let before: Vec<&str> = match previous {
            Some(prev) => prev
                .commits
                .as_ref()?
                .iter()
                .map(|c| c.commit_id.as_str())
                .collect(),
            None => Vec::new(),
        };
        let added = commits
            .iter()
            .filter(|c| !before.iter().any(|b| b.eq_ignore_ascii_case(&c.commit_id)))
            .count();
        Some(added as u32)
    }

    pub fn into_model(self, previous: Option<&WireIteration>) -> adoken_model::PrIteration {
        let commit_count = self.commits_added(previous);
        adoken_model::PrIteration {
            id: self.id,
            source_commit: self
                .source_ref_commit
                .map(|c| c.commit_id.to_ascii_lowercase())
                .unwrap_or_default(),
            created: parse_time(&self.created_date).unwrap_or(0),
            description: self.description,
            commit_count,
            commits_truncated: commit_count.is_some() && self.has_more_commits,
        }
    }
}

/// A PR's iteration list as stored, oldest first, with per-push commit
/// counts (spec 0017 FR-4).
pub fn iterations_into_model(mut list: Vec<WireIteration>) -> Vec<adoken_model::PrIteration> {
    list.sort_by_key(|it| it.id);
    let mut out = Vec::with_capacity(list.len());
    for (ix, it) in list.iter().enumerate() {
        let previous = ix.checked_sub(1).map(|p| &list[p]);
        out.push(it.clone().into_model(previous));
    }
    out
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRef {
    #[serde(default)]
    pub pushed_by: Option<IdentityRef>,
}

/// `GET …/pullRequests/{id}/iterations/{it}/changes` (spec 0004 §5): one
/// page of the files an iteration changed.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IterationChanges {
    #[serde(default)]
    pub change_entries: Vec<ChangeEntry>,
    /// `$skip` for the next page; 0 on the last page.
    #[serde(default)]
    pub next_skip: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEntry {
    /// Stable per file across iterations; new line comments send it so
    /// Azure DevOps tracks them across later pushes (spec 0004 FR-6).
    #[serde(default)]
    pub change_tracking_id: Option<u32>,
    pub item: ChangeItem,
    /// Comma-separated flags, e.g. `"edit"`, `"edit, rename"`.
    #[serde(default)]
    pub change_type: String,
    /// The old path of a rename, and the only path of a delete (Azure DevOps
    /// sends `item.path: null` for deletes, verified live 2026-09-30).
    #[serde(default)]
    pub original_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeItem {
    /// `null` for deletes; see [`ChangeEntry::original_path`].
    #[serde(default)]
    pub path: Option<String>,
    /// The blob after the change (absent for deletes).
    #[serde(default)]
    pub object_id: Option<String>,
    /// The blob before the change (absent for adds).
    #[serde(default)]
    pub original_object_id: Option<String>,
}

impl ChangeEntry {
    /// The dominant change of the flag set: delete, add, rename, else edit.
    pub fn kind(&self) -> ChangeType {
        let has = |flag: &str| {
            self.change_type
                .split(',')
                .any(|f| f.trim().to_ascii_lowercase().contains(flag))
        };
        if has("delete") {
            ChangeType::Delete
        } else if has("add") {
            ChangeType::Add
        } else if has("rename") {
            ChangeType::Rename
        } else {
            ChangeType::Edit
        }
    }

    /// `None` when the entry names no file at all, so one odd entry never
    /// costs the rest of the list.
    pub fn into_model(self) -> Option<PrFileChange> {
        let change_type = self.kind();
        let ChangeItem {
            path,
            object_id,
            original_object_id,
        } = self.item;
        let (path, original_path) = match change_type {
            ChangeType::Delete => (path.or(self.original_path)?, None),
            ChangeType::Rename => (path?, self.original_path),
            ChangeType::Add | ChangeType::Edit => (path?, None),
        };
        let (old_content, new_content) = match change_type {
            ChangeType::Add => (None, object_id),
            ChangeType::Delete => (original_object_id.or(object_id), None),
            ChangeType::Edit | ChangeType::Rename => (original_object_id, object_id),
        };
        Some(PrFileChange {
            path,
            original_path,
            change_type,
            old_content,
            new_content,
            // 0 isn't an id: unknown, never sent.
            change_tracking_id: self.change_tracking_id.filter(|id| *id > 0),
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct WiqlRef {
    pub id: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WiqlResult {
    #[serde(default)]
    pub work_items: Vec<WiqlRef>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Relation {
    #[serde(default)]
    pub rel: String,
    #[serde(default)]
    pub url: String,
    /// `name` ("Pull Request", "Fixed in Commit", "Parent"…), `comment`,
    /// `isLocked`.
    #[serde(default)]
    pub attributes: HashMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WireWorkItem {
    pub id: i64,
    pub rev: i64,
    #[serde(default)]
    pub fields: HashMap<String, Value>,
    #[serde(default)]
    pub relations: Option<Vec<Relation>>,
    /// Per long-text field: `"html"` (the default when absent) or
    /// `"markdown"`.
    #[serde(default, rename = "multilineFieldsFormat")]
    pub multiline_formats: HashMap<String, String>,
    /// A reporting-feed row (title index): selected fields only, so its
    /// description and relations are unknown, not empty (spec 0015 FR-32).
    #[serde(skip)]
    pub partial: bool,
}

/// Work item rich text → plain text (ADR 0001 stores plain text): tags
/// dropped, `<br>` and block ends become line breaks, entities decoded,
/// whitespace collapsed. Live descriptions are HTML with inline styles
/// (`<div style="box-sizing:border-box;"><span …>`), not text.
///
/// - Mention anchors (`<a data-vss-mention="version:2.0,{guid}">@Name</a>`,
///   work item discussions) become `@<guid>` tokens, like PR comments.
/// - Markup that never renders as text is dropped with its content:
///   `<!-- comments -->` (Word/Outlook pastes carry `<!--[if gte mso]>`
///   blocks), `<style>`, `<script>`, `<head>`, `<title>`.
pub fn html_to_text(html: &str) -> String {
    const BREAK_BEFORE: &[&str] = &[
        "br",
        "/div",
        "/p",
        "/li",
        "/tr",
        "/h1",
        "/h2",
        "/h3",
        "/h4",
        "/h5",
        "/h6",
        "/blockquote",
        "/pre",
    ];
    const HIDDEN: &[&str] = &["style", "script", "head", "title"];
    let mut raw = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(c) = rest.chars().next() {
        match c {
            '<' if rest.starts_with("<!--") => {
                rest = rest[4..].find("-->").map_or("", |end| &rest[4 + end + 3..]);
            }
            '<' => {
                let Some(end) = rest.find('>') else {
                    raw.push_str(rest);
                    break;
                };
                let inner = &rest[1..end];
                let tag = inner
                    .trim_start()
                    .split(|c: char| c.is_whitespace() || c == '>')
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('/')
                    .to_ascii_lowercase();
                rest = &rest[end + 1..];
                if HIDDEN.contains(&tag.as_str()) && !inner.trim_end().ends_with('/') {
                    rest = skip_past_close(rest, &tag);
                    continue;
                }
                if tag == "a"
                    && let Some(id) = mention_attr(inner)
                {
                    raw.push_str("@<");
                    raw.push_str(&id);
                    raw.push('>');
                    rest = skip_past_close(rest, "a");
                    continue;
                }
                // Markup breaks are soft: `<br></div>` is one line break.
                let at_line_start = raw.trim_end_matches([' ', '\t']).ends_with('\n');
                if BREAK_BEFORE.contains(&tag.as_str()) && !at_line_start {
                    raw.push('\n');
                } else if tag == "li" {
                    raw.push_str(if at_line_start { "- " } else { "\n- " });
                }
            }
            '&' => {
                let entity = rest[1..]
                    .find(';')
                    .filter(|n| *n <= 10)
                    .map(|n| &rest[1..1 + n]);
                let decoded = entity.and_then(|e| match e {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some(' '),
                    _ => e
                        .strip_prefix("#x")
                        .or_else(|| e.strip_prefix("#X"))
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .or_else(|| e.strip_prefix('#').and_then(|d| d.parse().ok()))
                        .and_then(char::from_u32),
                });
                match (entity, decoded) {
                    (Some(e), Some(ch)) => {
                        raw.push(ch);
                        rest = &rest[e.len() + 2..];
                    }
                    _ => {
                        raw.push('&');
                        rest = &rest[1..];
                    }
                }
            }
            c => {
                raw.push(if c == '\u{a0}' { ' ' } else { c });
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    // One space between words, at most one blank line between paragraphs.
    let mut out = String::with_capacity(raw.len());
    let mut blank = 0;
    for line in raw.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        out.push_str(&line);
    }
    out
}

/// Case-insensitive ASCII search.
fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.as_bytes()
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// The text after `</tag …>` (empty when the element never closes).
fn skip_past_close<'a>(rest: &'a str, tag: &str) -> &'a str {
    let close = format!("</{tag}");
    let Some(at) = find_ci(rest, &close) else {
        return "";
    };
    let after = &rest[at + close.len()..];
    after.find('>').map_or("", |end| &after[end + 1..])
}

/// `data-vss-mention="version:2.0,{guid}"` → the lowercased guid.
fn mention_attr(tag: &str) -> Option<String> {
    let at = find_ci(tag, "data-vss-mention")?;
    let value = tag[at + "data-vss-mention".len()..]
        .trim_start()
        .strip_prefix('=')?
        .trim_start();
    let quote = value.chars().next().filter(|q| *q == '"' || *q == '\'')?;
    let value = &value[1..value[1..].find(quote)? + 1];
    let id = value.rsplit(',').next()?.trim();
    is_guid(id).then(|| id.to_ascii_lowercase())
}

pub fn is_guid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// Lowercase the ids of `@<GUID>` mention tokens: PR comments carry them
/// in whatever case the client wrote, identity ids are lowercase.
pub fn normalize_mentions(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(i) = rest.find("@<") {
        out.push_str(&rest[..i + 2]);
        rest = &rest[i + 2..];
        if let Some(id) = rest.get(..36).filter(|id| is_guid(id))
            && rest[36..].starts_with('>')
        {
            out.push_str(&id.to_ascii_lowercase());
            rest = &rest[36..];
        }
    }
    out.push_str(rest);
    out
}

/// `vstfs:///Git/PullRequestId/{project}%2F{repo}%2F{id}` → id (either
/// escape case).
pub(crate) fn pr_link_id(url: &str) -> Option<i64> {
    let rest = url.strip_prefix("vstfs:///Git/PullRequestId/")?;
    let (_, id) = rest
        .rsplit_once("%2F")
        .or_else(|| rest.rsplit_once("%2f"))?;
    id.parse().ok()
}

const IDENTITY_FIELDS: [&str; 3] = ["System.AssignedTo", "System.CreatedBy", "System.ChangedBy"];

/// `"Display Name <unique name>"` → `("Display Name", Some("unique name"))`;
/// a bare `"Display Name"` has no unique name.
pub fn split_identity(s: &str) -> (&str, Option<&str>) {
    let s = s.trim();
    match (s.rfind('<'), s.strip_suffix('>')) {
        (Some(open), Some(inner)) if open < inner.len() => {
            let unique = inner[open + 1..].trim();
            (s[..open].trim(), (!unique.is_empty()).then_some(unique))
        }
        _ => (s, None),
    }
}

impl WireWorkItem {
    fn s(&self, k: &str) -> Option<&str> {
        self.fields.get(k).and_then(Value::as_str)
    }

    pub fn closed_at(&self) -> Option<Millis> {
        self.time("Microsoft.VSTS.Common.ClosedDate")
            .or_else(|| self.time("Microsoft.VSTS.Common.StateChangeDate"))
    }

    fn time(&self, field: &str) -> Option<Millis> {
        self.s(field).and_then(parse_time).filter(|at| *at > 0)
    }

    /// The closure date and when the item entered its current state, as
    /// the store records them (spec 0017: Loose ends "Open Nd").
    pub fn transitions(&self) -> adoken_store::write::Transitions {
        adoken_store::write::Transitions {
            closed_at: self.closed_at(),
            state_changed: self.time("Microsoft.VSTS.Common.StateChangeDate"),
        }
    }
    fn identity(&self, k: &str) -> Option<IdentityRef> {
        self.fields
            .get(k)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
    pub fn people(&self) -> Vec<Person> {
        IDENTITY_FIELDS
            .iter()
            .filter_map(|k| self.identity(k))
            .map(|i| i.person())
            .collect()
    }

    /// Whether identity fields arrive as strings (the reporting revisions
    /// feed) rather than identity objects (the work items API).
    pub fn has_identity_strings(&self) -> bool {
        IDENTITY_FIELDS
            .iter()
            .any(|k| self.fields.get(*k).is_some_and(Value::is_string))
    }

    /// The reporting revisions feed sends identities as
    /// `"Display Name <unique name>"` strings, without ids (verified against
    /// dev.azure.com, 7.1). Rewrite them as identity objects. The id comes
    /// from `resolve(unique_name, display_name)` (a person the store already
    /// knows); otherwise the lowercased unique name (or the display name)
    /// stands in, so the same person always gets the same id.
    pub fn normalize_identities(&mut self, resolve: impl Fn(Option<&str>, &str) -> Option<String>) {
        for k in IDENTITY_FIELDS {
            let Some(Value::String(s)) = self.fields.get(k) else {
                continue;
            };
            let s = s.clone();
            let (display, unique) = split_identity(&s);
            let id = resolve(unique, display).unwrap_or_else(|| match unique {
                Some(u) => u.to_lowercase(),
                None => display.to_owned(),
            });
            self.fields.insert(
                k.to_owned(),
                serde_json::json!({ "id": id, "displayName": display, "uniqueName": unique }),
            );
        }
    }
    pub fn project_name(&self) -> Option<&str> {
        self.s("System.TeamProject")
    }
    pub fn is_deleted(&self) -> bool {
        self.fields
            .get("System.IsDeleted")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// `None` when required fields are missing (e.g. a deleted stub).
    pub fn into_model(self, project_id: String) -> Option<WorkItem> {
        let state = self.s("System.State")?.to_owned();
        let created_by = self.identity("System.CreatedBy")?.id;
        let changed_by = self
            .identity("System.ChangedBy")
            .map(|i| i.id)
            .unwrap_or_else(|| created_by.clone());
        let linked_pr_ids = self
            .relations
            .iter()
            .flatten()
            .filter(|r| r.rel == "ArtifactLink")
            .filter_map(|r| pr_link_id(&r.url))
            .collect();
        Some(WorkItem {
            id: self.id,
            project_id,
            rev: self.rev,
            kind: WorkItemType::parse(self.s("System.WorkItemType")?)?,
            title: self.s("System.Title")?.to_owned(),
            state_category: StateCategory::of_state(&state),
            state,
            assigned_to: self.identity("System.AssignedTo").map(|i| i.id),
            created_by,
            changed_by,
            area_path: self.s("System.AreaPath").unwrap_or_default().to_owned(),
            iteration_path: self
                .s("System.IterationPath")
                .unwrap_or_default()
                .to_owned(),
            priority: self
                .fields
                .get("Microsoft.VSTS.Common.Priority")
                .and_then(Value::as_u64)
                .map(|p| p as u8),
            tags: self
                .s("System.Tags")
                .unwrap_or_default()
                .split(';')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .collect(),
            parent_id: self.fields.get("System.Parent").and_then(Value::as_i64),
            linked_pr_ids,
            description: self.s("System.Description").map(|d| {
                match self.multiline_formats.get("System.Description") {
                    Some(f) if f.eq_ignore_ascii_case("markdown") => d.to_owned(),
                    _ => html_to_text(d),
                }
            }),
            comment_count: self
                .fields
                .get("System.CommentCount")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            created: self.s("System.CreatedDate").and_then(parse_time)?,
            changed: self.s("System.ChangedDate").and_then(parse_time)?,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionsPage {
    /// Title-index rows: only the requested fields, never the description
    /// or relations (spec 0015 FR-29 "Partly synced").
    #[serde(default, deserialize_with = "title_index_rows")]
    pub values: Vec<WireWorkItem>,
    #[serde(default)]
    pub continuation_token: Option<String>,
    #[serde(default)]
    pub is_last_batch: bool,
}

fn title_index_rows<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<WireWorkItem>, D::Error> {
    let mut rows = Vec::<WireWorkItem>::deserialize(d)?;
    for row in &mut rows {
        row.partial = true;
    }
    Ok(rows)
}

#[derive(Clone, Debug, Deserialize)]
pub struct DefinitionRef {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub path: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WireDefinition {
    #[serde(flatten)]
    pub definition: DefinitionRef,
    pub project: ProjectRef,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireBuild {
    pub id: i64,
    pub build_number: String,
    pub status: String,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub reason: String,
    pub source_branch: String,
    #[serde(default)]
    pub source_version: Option<String>,
    #[serde(default)]
    pub requested_for: Option<IdentityRef>,
    pub queue_time: String,
    #[serde(default)]
    pub start_time: Option<String>,
    #[serde(default)]
    pub finish_time: Option<String>,
    pub definition: DefinitionRef,
    pub project: ProjectRef,
    #[serde(default)]
    pub trigger_info: HashMap<String, String>,
}

impl WireBuild {
    pub fn into_model(self) -> PipelineRun {
        let reason = RunReason::parse(&self.reason).unwrap_or(RunReason::Other);
        let pr_id = self
            .trigger_info
            .get("pr.number")
            .and_then(|n| n.parse().ok())
            .or_else(|| {
                self.source_branch
                    .strip_prefix("refs/pull/")
                    .and_then(|r| r.split('/').next())
                    .and_then(|n| n.parse().ok())
            });
        PipelineRun {
            id: self.id,
            project_id: self.project.id,
            pipeline_id: self.definition.id,
            pipeline_name: self.definition.name,
            run_number: self.build_number,
            status: RunStatus::parse(&self.status).unwrap_or(RunStatus::NotStarted),
            result: self.result.as_deref().and_then(RunResult::parse),
            reason,
            branch: self.source_branch,
            source_version: self.source_version,
            requested_for: self.requested_for.map(|i| i.id),
            pr_id,
            queued: parse_time(&self.queue_time).unwrap_or(0),
            started: self.start_time.as_deref().and_then(parse_time),
            finish: self.finish_time.as_deref().and_then(parse_time),
        }
    }
}

// ── build timeline ────────────────────────────────────────────────────────

/// `GET {project}/_apis/build/builds/{id}/timeline`. `records` can be
/// `null` for a run that hasn't started.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireTimeline {
    #[serde(default)]
    pub records: Option<Vec<TimelineRecord>>,
}

/// One timeline record. Verified shape (dev.azure.com, 7.1): `type` is
/// `Stage` / `Phase` / `Job` / `Task` / `Checkpoint`; records arrive in no
/// particular order; `order` can be `null` (checkpoints) or `i32::MAX`;
/// times are `null` or `0001-01-01…` before a record starts.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRecord {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub identifier: Option<String>,
    #[serde(default)]
    pub order: Option<i64>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub start_time: Option<String>,
    #[serde(default)]
    pub finish_time: Option<String>,
}

impl TimelineRecord {
    fn outcome(&self) -> StageOutcome {
        match (self.result.as_deref(), self.state.as_deref()) {
            (Some(r), _) if r.eq_ignore_ascii_case("succeeded") => StageOutcome::Succeeded,
            (Some(r), _) if r.eq_ignore_ascii_case("succeededWithIssues") => StageOutcome::Partial,
            (Some(r), _) if r.eq_ignore_ascii_case("failed") => StageOutcome::Failed,
            (Some(r), _)
                if r.eq_ignore_ascii_case("canceled") || r.eq_ignore_ascii_case("abandoned") =>
            {
                StageOutcome::Canceled
            }
            (Some(r), _) if r.eq_ignore_ascii_case("skipped") => StageOutcome::Skipped,
            (_, Some(s)) if s.eq_ignore_ascii_case("inProgress") => StageOutcome::Running,
            (_, Some(s)) if s.eq_ignore_ascii_case("completed") => StageOutcome::Skipped,
            _ => StageOutcome::Pending,
        }
    }

    fn stage(&self) -> RunStage {
        // `0001-01-01T00:00:00` means "not yet".
        let time = |t: &Option<String>| t.as_deref().and_then(parse_time).filter(|ms| *ms > 0);
        RunStage {
            name: self.name.clone(),
            outcome: self.outcome(),
            started: time(&self.start_time),
            finished: time(&self.finish_time),
        }
    }
}

/// The rows the run detail shows (spec 0008 FR-5): the stages in pipeline
/// order; a single-stage YAML pipeline (one implicit `__default` stage) or
/// a classic one (no stages) shows its jobs (`Phase` records) instead.
pub fn timeline_stages(records: &[TimelineRecord]) -> Vec<RunStage> {
    let of = |kind: &str| -> Vec<&TimelineRecord> {
        records
            .iter()
            .filter(|r| r.kind.eq_ignore_ascii_case(kind))
            .collect()
    };
    let stages = of("Stage");
    let implicit = stages.len() == 1
        && stages[0].identifier.as_deref().unwrap_or(&stages[0].name) == "__default";
    let mut rows = if stages.is_empty() || implicit {
        let phases = of("Phase");
        if phases.is_empty() { of("Job") } else { phases }
    } else {
        stages
    };
    rows.sort_by(|a, b| {
        let key = |r: &TimelineRecord| {
            (
                r.order.unwrap_or(i64::MAX),
                r.start_time
                    .as_deref()
                    .and_then(parse_time)
                    .filter(|ms| *ms > 0)
                    .unwrap_or(i64::MAX),
            )
        };
        key(a).cmp(&key(b)).then_with(|| a.name.cmp(&b.name))
    });
    rows.into_iter().map(TimelineRecord::stage).collect()
}

// ── work item comments ────────────────────────────────────────────────────

/// `GET {project}/_apis/wit/workItems/{id}/comments` (7.1-preview.4).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireCommentList {
    #[serde(default)]
    pub comments: Vec<WireWiComment>,
    #[serde(default)]
    pub continuation_token: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireWiMention {
    /// Identity id for `person` mentions, else a work item / PR id.
    #[serde(default)]
    pub mentioned_artifact: String,
    #[serde(rename = "type", default)]
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireWiComment {
    pub id: i64,
    pub work_item_id: i64,
    #[serde(default)]
    pub text: Option<String>,
    /// `html` (default) or `markdown`.
    #[serde(default)]
    pub format: Option<String>,
    pub created_by: IdentityRef,
    pub created_date: String,
    #[serde(default)]
    pub modified_date: Option<String>,
    #[serde(default)]
    pub mentions: Vec<WireWiMention>,
    #[serde(default)]
    pub is_deleted: bool,
}

impl WireWiComment {
    pub fn into_model(self) -> WorkItemComment {
        let text = self.text.as_deref().unwrap_or_default();
        let format = match self.format.as_deref() {
            Some(f) if f.eq_ignore_ascii_case("markdown") => TextFormat::Markdown,
            _ => TextFormat::Html,
        };
        let body = match format {
            TextFormat::Markdown => normalize_mentions(text),
            TextFormat::Html => html_to_text(text),
        };
        let mentions = self
            .mentions
            .iter()
            .filter(|m| m.kind.eq_ignore_ascii_case("person") && is_guid(&m.mentioned_artifact))
            .map(|m| m.mentioned_artifact.to_ascii_lowercase())
            .collect();
        let created = parse_time(&self.created_date).unwrap_or(0);
        WorkItemComment {
            id: self.id,
            work_item_id: self.work_item_id,
            author: self.created_by.id,
            body,
            mentions,
            created,
            updated: self
                .modified_date
                .as_deref()
                .and_then(parse_time)
                .unwrap_or(created),
            source: self.text,
            format,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_round_trip() {
        let ms = 1_790_328_600_123;
        assert_eq!(parse_time(&format_time(ms)), Some(ms));
        assert_eq!(parse_time("2026-09-25T09:30:00Z"), Some(1_790_328_600_000));
    }

    #[test]
    fn html_descriptions_become_plain_text() {
        let live = "<div><div style=\"box-sizing:border-box;\"><span style=\"display:inline !important;\">Total is wrong.</span><br style=\"box-sizing:border-box;\"> </div><div>&#128310; &amp; more&nbsp;text</div><div> </div><ul><li>one</li><li>two</li></ul></div>";
        assert_eq!(
            html_to_text(live),
            "Total is wrong.\n\u{1F536} & more text\n- one\n- two"
        );
        assert_eq!(
            html_to_text("Plain & simple\n\nParagraph"),
            "Plain & simple\n\nParagraph"
        );
        assert_eq!(html_to_text("a < b"), "a < b");
    }

    /// Work item 14's live description (fetched 2026-09-26). The 🔶 and
    /// `workItemId` the app shows are the item's own text, in plain
    /// `<div>`s with no data attributes, and Azure DevOps renders them the
    /// same way; conversion must keep them.
    #[test]
    fn live_description_keeps_authored_text() {
        let live = "<div><div style=\"box-sizing:border-box;\"><span style=\"box-sizing:border-box;display:inline !important;\">The total order amount shown during checkout does not match the actual sum of the item prices in the cart.</span><br style=\"box-sizing:border-box;\"> </div><div style=\"box-sizing:border-box;\"><span style=\"box-sizing:border-box;display:inline !important;\">&#128310;</span> </div><div style=\"box-sizing:border-box;\"> </div><div style=\"box-sizing:border-box;\">workItemId<br> </div><br> </div><div> </div>";
        assert_eq!(
            html_to_text(live),
            "The total order amount shown during checkout does not match the actual sum of the item prices in the cart.\n\u{1F536}\nworkItemId"
        );
    }

    #[test]
    fn non_text_markup_is_dropped_with_its_content() {
        let pasted = "<html><head><title>t</title><style>p{color:red}</style></head>\
            <!--[if gte mso 9]><xml><o:OfficeDocumentSettings/></xml><![endif]-->\
            <p>Kept</p><script type=\"x\">alert(1)</script><STYLE>.a{}</STYLE><p>too</p></html>";
        assert_eq!(html_to_text(pasted), "Kept\ntoo");
        assert_eq!(html_to_text("a <!-- never closed"), "a");
    }

    #[test]
    fn mention_anchors_become_guid_tokens() {
        let html = "<div><a href=\"#\" data-vss-mention=\"version:2.0,D2C4C61B-5A1A-6CF0-8A11-F7DD5385B607\">@Dana Example</a> can you look? cc <a data-vss-mention='version:1.0,6b3f0c2e-1d4a-4f5b-9c8d-7e6f5a4b3c2d' href=\"#\">@Sam</a>, <a href=\"https://x\">docs</a></div>";
        assert_eq!(
            html_to_text(html),
            "@<d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607> can you look? cc @<6b3f0c2e-1d4a-4f5b-9c8d-7e6f5a4b3c2d>, docs"
        );
        // A malformed mention id stays plain text.
        assert_eq!(
            html_to_text("<a data-vss-mention=\"version:2.0,nope\">@Nope</a>"),
            "@Nope"
        );
    }

    #[test]
    fn pr_mention_ids_are_lowercased() {
        assert_eq!(
            normalize_mentions("hi @<D2C4C61B-5A1A-6CF0-8A11-F7DD5385B607>, @<Team> @<"),
            "hi @<d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607>, @<Team> @<"
        );
    }

    fn record(
        kind: &str,
        name: &str,
        order: Option<i64>,
        state: &str,
        result: Option<&str>,
    ) -> TimelineRecord {
        TimelineRecord {
            id: name.into(),
            parent_id: None,
            kind: kind.into(),
            name: name.into(),
            identifier: Some(name.into()),
            order,
            state: Some(state.into()),
            result: result.map(Into::into),
            start_time: Some("2026-09-25T09:30:00Z".into()),
            finish_time: Some("0001-01-01T00:00:00".into()),
        }
    }

    /// Multi-stage runs list their stages in `order`, whatever the record
    /// order; jobs, tasks and checkpoints aren't rows.
    #[test]
    fn stages_are_ordered_and_mapped() {
        let records = vec![
            record("Stage", "Deploy", Some(3), "pending", None),
            record("Task", "Run tests", Some(1), "completed", Some("failed")),
            record("Stage", "Test", Some(2), "inProgress", None),
            record(
                "Checkpoint",
                "Checkpoint",
                None,
                "completed",
                Some("succeeded"),
            ),
            record(
                "Stage",
                "Build",
                Some(1),
                "completed",
                Some("succeededWithIssues"),
            ),
            record("Stage", "Lint", None, "completed", Some("skipped")),
            record("Phase", "Job", Some(1), "completed", Some("abandoned")),
        ];
        let stages = timeline_stages(&records);
        let got: Vec<(&str, StageOutcome)> = stages
            .iter()
            .map(|s| (s.name.as_str(), s.outcome))
            .collect();
        assert_eq!(
            got,
            [
                ("Build", StageOutcome::Partial),
                ("Test", StageOutcome::Running),
                ("Deploy", StageOutcome::Pending),
                ("Lint", StageOutcome::Skipped),
            ]
        );
        assert_eq!(stages[0].started, parse_time("2026-09-25T09:30:00Z"));
        assert_eq!(stages[0].finished, None, "0001-01-01 means not finished");
        // Classic pipelines (no Stage records) show their jobs.
        let classic = timeline_stages(&[record(
            "Phase",
            "Agent job 1",
            Some(1),
            "completed",
            Some("canceled"),
        )]);
        assert_eq!(classic[0].outcome, StageOutcome::Canceled);
        assert!(timeline_stages(&[]).is_empty());
    }

    #[test]
    fn pr_link_ids() {
        assert_eq!(
            pr_link_id("vstfs:///Git/PullRequestId/abc%2Fdef%2F4312"),
            Some(4312)
        );
        assert_eq!(
            pr_link_id("vstfs:///Git/PullRequestId/abc%2fdef%2f4312"),
            Some(4312)
        );
        assert_eq!(pr_link_id("https://x"), None);
    }
}
