//! The exact REST calls of spec 0015 (pure, so tests assert them byte for
//! byte), FR-8's field-level merge decision, and FR-9's rule-error parser.

use adoken_model::{LinkKind, Person, PersonId, TextFormat, WiLink, WorkItem, WorkItemExtras};
use serde_json::{Value, json};
use url::Url;

use super::WiError;
use crate::attribution::UserAction;
use crate::engine::API_VERSION;
use crate::transport::ApiRequest;

/// Work item comments exist only as a preview API (0002 §5.1, FR-25).
pub const COMMENTS_VERSION: &str = "7.1-preview.4";
/// Comment pages a per-item sync reads (FR-30).
pub const COMMENT_PAGES: usize = 10;
/// Classification trees are read in one call at the maximum depth (FR-14).
pub const CLASSIFICATION_DEPTH: &str = "14";

/// One user edit of one work item (FR-5).
#[derive(Clone, Debug, PartialEq)]
pub struct WiWrite {
    pub id: i64,
    /// The revision the user was looking at when the edit started (FR-5);
    /// a newer stored revision rebases it when the edited field is
    /// unchanged there (FR-8, FR-10).
    pub base_rev: i64,
    pub change: WiChange,
    /// Fields a process rule asked for, sent in the same PATCH (FR-9):
    /// `(reference name, value)`.
    pub extra: Vec<(String, Value)>,
}

/// What the edit changes. `base` values are what the user saw (optimistic
/// values included) when the edit started: FR-8 compares them with the
/// server's.
#[derive(Clone, Debug, PartialEq)]
pub enum WiChange {
    State {
        to: String,
        base: String,
    },
    Title {
        to: String,
        base: String,
    },
    /// `to: None` unassigns (sends `""`).
    Assign {
        to: Option<Person>,
        base: Option<PersonId>,
    },
    Area {
        to: String,
        base: String,
    },
    Iteration {
        to: String,
        base: String,
    },
    /// `to` is sent as is: Markdown for a Markdown field, HTML (converted by
    /// the caller) for an HTML field (FR-17). `base` is the stored source.
    Description {
        to: String,
        format: TextFormat,
        base: Option<String>,
    },
    /// `add /relations/-`; with `replace_parent`, the old parent's relation
    /// is removed in the same PATCH (Change parent, FR-21).
    AddLink {
        kind: LinkKind,
        target: i64,
        replace_parent: Option<i64>,
    },
    /// `remove /relations/{index}`, the index taken from the relations at
    /// the revision sent (FR-22).
    RemoveLink {
        kind: LinkKind,
        target: i64,
    },
}

/// The element an edit's status belongs to (the view draws one status line
/// per element).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WiField {
    Title,
    State,
    AssignedTo,
    Area,
    Iteration,
    Description,
    Link { kind: LinkKind, target: i64 },
}

impl WiField {
    /// The six fields with one status line each (links have one per row).
    pub const EDITABLE: [WiField; 6] = [
        WiField::Title,
        WiField::State,
        WiField::AssignedTo,
        WiField::Area,
        WiField::Iteration,
        WiField::Description,
    ];
}

impl WiChange {
    pub fn field(&self) -> WiField {
        match self {
            WiChange::State { .. } => WiField::State,
            WiChange::Title { .. } => WiField::Title,
            WiChange::Assign { .. } => WiField::AssignedTo,
            WiChange::Area { .. } => WiField::Area,
            WiChange::Iteration { .. } => WiField::Iteration,
            WiChange::Description { .. } => WiField::Description,
            WiChange::AddLink { kind, target, .. } | WiChange::RemoveLink { kind, target } => {
                WiField::Link {
                    kind: *kind,
                    target: *target,
                }
            }
        }
    }

    /// The network log's origin ("You: change state", spec 0016 FR-18).
    pub fn action(&self) -> UserAction {
        match self {
            WiChange::State { .. } => UserAction::ChangeState,
            WiChange::Title { .. } => UserAction::EditTitle,
            WiChange::Assign { .. } => UserAction::Assign,
            WiChange::Area { .. } => UserAction::ChangeArea,
            WiChange::Iteration { .. } => UserAction::ChangeIteration,
            WiChange::Description { .. } => UserAction::EditDescription,
            WiChange::AddLink {
                replace_parent: Some(_),
                ..
            } => UserAction::ChangeParent,
            WiChange::AddLink { .. } => UserAction::AddLink,
            WiChange::RemoveLink { .. } => UserAction::RemoveLink,
        }
    }

    /// The new value as the pending line shows it.
    pub fn display(&self) -> String {
        match self {
            WiChange::State { to, .. }
            | WiChange::Title { to, .. }
            | WiChange::Area { to, .. }
            | WiChange::Iteration { to, .. }
            | WiChange::Description { to, .. } => to.clone(),
            WiChange::Assign { to, .. } => to
                .as_ref()
                .map_or_else(|| "Unassigned".to_owned(), |p| p.display_name.clone()),
            WiChange::AddLink { target, .. } | WiChange::RemoveLink { target, .. } => {
                format!("#{target}")
            }
        }
    }

    /// Refused locally, nothing sent (FR-12).
    pub fn validate(&self) -> Result<(), WiError> {
        match self {
            WiChange::Title { to, .. } if to.trim().is_empty() => {
                Err(WiError::Invalid("Title can't be empty".into()))
            }
            _ => Ok(()),
        }
    }
    /// The same change with its base set to `server`'s current value: Apply
    /// mine after a conflict (FR-8) resends exactly this.
    pub fn rebased_on(&self, server: &WorkItem, extras: &WorkItemExtras) -> WiChange {
        let mut out = self.clone();
        match &mut out {
            WiChange::State { base, .. } => *base = server.state.clone(),
            WiChange::Title { base, .. } => *base = server.title.clone(),
            WiChange::Area { base, .. } => *base = server.area_path.clone(),
            WiChange::Iteration { base, .. } => *base = server.iteration_path.clone(),
            WiChange::Assign { base, .. } => *base = server.assigned_to.clone(),
            WiChange::Description { base, .. } => {
                *base = extras.description.as_ref().map(|d| d.source.clone())
            }
            WiChange::AddLink { replace_parent, .. } => {
                if replace_parent.is_some() {
                    *replace_parent = extras.parent();
                }
            }
            WiChange::RemoveLink { .. } => {}
        }
        out
    }
}

/// Azure DevOps' distinct display name, `"Display Name <sign-in name>"`
/// (FR-13): two people with one name can't be confused.
pub fn distinct_name(p: &Person) -> String {
    match &p.unique_name {
        Some(u) if !u.is_empty() => format!("{} <{u}>", p.display_name),
        _ => p.display_name.clone(),
    }
}

/// `{org}/_apis/wit/workItems/{id}`: a relation's target.
pub fn work_item_url(org_url: &Url, id: i64) -> String {
    format!("{}_apis/wit/workItems/{id}", org_url.as_str())
}

fn add(path: &str, value: impl Into<Value>) -> Value {
    json!({"op": "add", "path": path, "value": value.into()})
}

fn link_index(links: &[WiLink], kind: LinkKind, target: i64) -> Result<u32, WiError> {
    links
        .iter()
        .find(|l| l.kind == Some(kind) && l.target_id == Some(target))
        .map(|l| l.index)
        .ok_or_else(|| WiError::Invalid(format!("The link to #{target} is already gone")))
}

/// FR-5: `test /rev` first, then the change, then any rule fields (FR-9).
/// Relation indexes come from `links` (the relations at `base_rev`).
pub fn patch_ops(
    org_url: &Url,
    write: &WiWrite,
    base_rev: i64,
    links: &[WiLink],
) -> Result<Value, WiError> {
    let mut ops = vec![json!({"op": "test", "path": "/rev", "value": base_rev})];
    match &write.change {
        WiChange::State { to, .. } => ops.push(add("/fields/System.State", to.as_str())),
        WiChange::Title { to, .. } => ops.push(add("/fields/System.Title", to.trim())),
        WiChange::Assign { to, .. } => ops.push(add(
            "/fields/System.AssignedTo",
            to.as_ref().map(distinct_name).unwrap_or_default(),
        )),
        WiChange::Area { to, .. } => ops.push(add("/fields/System.AreaPath", to.as_str())),
        WiChange::Iteration { to, .. } => {
            ops.push(add("/fields/System.IterationPath", to.as_str()))
        }
        WiChange::Description { to, format, .. } => {
            ops.push(add("/fields/System.Description", to.as_str()));
            // Markdown fields say so; an HTML field stays HTML (never
            // converted: Azure DevOps makes that irreversible, Q-2).
            if *format == TextFormat::Markdown {
                ops.push(add("/multilineFieldsFormat/System.Description", "Markdown"));
            }
        }
        WiChange::AddLink {
            kind,
            target,
            replace_parent,
        } => {
            if let Some(old) = replace_parent {
                let index = link_index(links, LinkKind::Parent, *old)?;
                ops.push(json!({"op": "remove", "path": format!("/relations/{index}")}));
            }
            ops.push(add(
                "/relations/-",
                json!({"rel": kind.rel(), "url": work_item_url(org_url, *target), "attributes": {}}),
            ));
        }
        WiChange::RemoveLink { kind, target } => {
            let index = link_index(links, *kind, *target)?;
            ops.push(json!({"op": "remove", "path": format!("/relations/{index}")}));
        }
    }
    for (field, value) in &write.extra {
        ops.push(add(&format!("/fields/{field}"), value.clone()));
    }
    Ok(Value::Array(ops))
}

fn url(org_url: &Url, path: &str, query: &[(&str, &str)], version: &str) -> Url {
    let mut url = org_url.join(path).expect("relative ADO path");
    url.query_pairs_mut()
        .extend_pairs(query)
        .append_pair("api-version", version);
    url
}

/// `PATCH {org}/_apis/wit/workitems/{id}?$expand=relations` (FR-5).
pub fn patch_request(
    org_url: &Url,
    write: &WiWrite,
    base_rev: i64,
    links: &[WiLink],
) -> Result<ApiRequest, WiError> {
    Ok(ApiRequest::json_patch(
        url(
            org_url,
            &format!("_apis/wit/workitems/{}", write.id),
            &[("$expand", "relations")],
            API_VERSION,
        ),
        patch_ops(org_url, write, base_rev, links)?,
    ))
}

/// `GET {org}/{project}/_apis/wit/workitems/{id}?$expand=relations`: per-item
/// sync, FR-8 re-read, the GET after a comment.
pub fn item_request(org_url: &Url, project_id: &str, id: i64) -> ApiRequest {
    ApiRequest::get(url(
        org_url,
        &format!("{project_id}/_apis/wit/workitems/{id}"),
        &[("$expand", "relations")],
        API_VERSION,
    ))
}

/// One page of a work item's comments, newest first (FR-30).
pub fn comments_request(
    org_url: &Url,
    project_id: &str,
    id: i64,
    token: Option<&str>,
) -> ApiRequest {
    let mut query = vec![("$top", "200"), ("order", "desc")];
    if let Some(t) = token {
        query.push(("continuationToken", t));
    }
    ApiRequest::get(url(
        org_url,
        &format!("{project_id}/_apis/wit/workItems/{id}/comments"),
        &query,
        COMMENTS_VERSION,
    ))
}

/// `POST …/comments?format=markdown` with `{"text": markdown}` (FR-25).
pub fn post_comment_request(
    org_url: &Url,
    project_id: &str,
    id: i64,
    markdown: &str,
) -> ApiRequest {
    ApiRequest::post(
        url(
            org_url,
            &format!("{project_id}/_apis/wit/workItems/{id}/comments"),
            &[("format", "markdown")],
            COMMENTS_VERSION,
        ),
        json!({ "text": markdown }),
    )
}

fn segment_url(org_url: &Url, segments: &[&str], query: &[(&str, &str)]) -> Url {
    let mut url = org_url.clone();
    {
        let mut path = url.path_segments_mut().expect("http(s) URL");
        path.pop_if_empty();
        for s in segments {
            path.push(s);
        }
    }
    url.query_pairs_mut()
        .extend_pairs(query)
        .append_pair("api-version", API_VERSION);
    url
}

/// `GET {project}/_apis/wit/workitemtypes/{type}`: states and transitions
/// (FR-11).
pub fn type_request(org_url: &Url, project_id: &str, type_name: &str) -> ApiRequest {
    ApiRequest::get(segment_url(
        org_url,
        &[project_id, "_apis", "wit", "workitemtypes", type_name],
        &[],
    ))
}

/// `GET {project}/_apis/wit/workitemtypes/{type}/fields?$expand=allowedValues`
/// (FR-9).
pub fn fields_request(org_url: &Url, project_id: &str, type_name: &str) -> ApiRequest {
    ApiRequest::get(segment_url(
        org_url,
        &[
            project_id,
            "_apis",
            "wit",
            "workitemtypes",
            type_name,
            "fields",
        ],
        &[("$expand", "allowedValues")],
    ))
}

/// `GET _apis/wit/fields/{reference}`: a field's type, when its allowed
/// values don't make it a list (FR-9).
pub fn field_request(org_url: &Url, reference: &str) -> ApiRequest {
    ApiRequest::get(segment_url(
        org_url,
        &["_apis", "wit", "fields", reference],
        &[],
    ))
}

/// `GET {project}/_apis/wit/classificationnodes?$depth=14`: areas and
/// iterations in one call (FR-14).
pub fn class_tree_request(org_url: &Url, project_id: &str) -> ApiRequest {
    ApiRequest::get(url(
        org_url,
        &format!("{project_id}/_apis/wit/classificationnodes"),
        &[("$depth", CLASSIFICATION_DEPTH)],
        API_VERSION,
    ))
}

/// The identities host of `org_url`: `vssps.dev.azure.com/{org}` or
/// `{org}.vssps.visualstudio.com`; any other host (a loopback mock) is used
/// as is.
pub fn identities_base(org_url: &Url) -> Url {
    let mut base = org_url.clone();
    let host = org_url.host_str().unwrap_or_default().to_owned();
    if host.eq_ignore_ascii_case("dev.azure.com") {
        base.set_host(Some("vssps.dev.azure.com")).ok();
    } else if let Some(org) = host.strip_suffix(".visualstudio.com")
        && !org.contains('.')
    {
        base.set_host(Some(&format!("{org}.vssps.visualstudio.com")))
            .ok();
    }
    base
}

/// Directory search (FR-13): `GET vssps …/_apis/identities?searchFilter=General`.
pub fn people_request(org_url: &Url, text: &str) -> ApiRequest {
    ApiRequest::get(url(
        &identities_base(org_url),
        "_apis/identities",
        &[
            ("searchFilter", "General"),
            ("filterValue", text),
            ("queryMembership", "None"),
        ],
        API_VERSION,
    ))
}

/// The display name of the field a `TF401320` rule error names (FR-9):
/// `TF401320: Rule Error for field Root cause. Error code: Required, …`.
pub fn parse_rule_error(message: &str) -> Option<String> {
    const MARK: &str = "Rule Error for field ";
    let at = message.find("TF401320")?;
    let rest = &message[at..];
    let start = rest.find(MARK)? + MARK.len();
    let rest = &rest[start..];
    let end = rest
        .find(". Error code")
        .or_else(|| rest.find(".\n"))
        .unwrap_or(rest.len());
    let field = rest[..end].trim().trim_end_matches('.').trim();
    (!field.is_empty()).then(|| field.to_owned())
}

/// FR-8's decision on a newer server revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Merge {
    /// Nobody touched what I edited: resend on the new revision.
    Same,
    /// Someone changed it; `theirs` is their value (display form, `None`
    /// for empty).
    Changed { theirs: Option<String> },
    /// The server already reads the way I wanted (a link already added or
    /// removed elsewhere): nothing to send.
    Done,
}

/// Compare what I edited with `server` (FR-8; FR-19: descriptions are never
/// merged, only resent when untouched).
pub fn merge(change: &WiChange, server: &WorkItem, extras: &WorkItemExtras) -> Merge {
    let same = |a: &str, b: &str, theirs: &str| {
        if a == b {
            Merge::Same
        } else {
            Merge::Changed {
                theirs: (!theirs.is_empty()).then(|| theirs.to_owned()),
            }
        }
    };
    match change {
        WiChange::State { base, .. } => same(base, &server.state, &server.state),
        WiChange::Title { base, .. } => same(base, &server.title, &server.title),
        WiChange::Area { base, .. } => same(base, &server.area_path, &server.area_path),
        WiChange::Iteration { base, .. } => {
            same(base, &server.iteration_path, &server.iteration_path)
        }
        WiChange::Assign { base, .. } => {
            if *base == server.assigned_to {
                Merge::Same
            } else {
                Merge::Changed {
                    theirs: server.assigned_to.clone(),
                }
            }
        }
        WiChange::Description { base, .. } => {
            let theirs = extras.description.as_ref().map(|d| d.source.clone());
            if *base == theirs {
                Merge::Same
            } else {
                Merge::Changed { theirs }
            }
        }
        WiChange::AddLink {
            kind,
            target,
            replace_parent,
        } => {
            if extras.link(*kind, *target).is_some() {
                return Merge::Done;
            }
            match replace_parent {
                Some(old) => {
                    let parent = extras.parent();
                    if parent == Some(*old) {
                        Merge::Same
                    } else {
                        Merge::Changed {
                            theirs: parent.map(|p| format!("#{p}")),
                        }
                    }
                }
                None if *kind == LinkKind::Parent && extras.parent().is_some() => Merge::Changed {
                    theirs: extras.parent().map(|p| format!("#{p}")),
                },
                None => Merge::Same,
            }
        }
        WiChange::RemoveLink { kind, target } => {
            if extras.link(*kind, *target).is_some() {
                Merge::Same
            } else {
                Merge::Done
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-7 (parser): recorded rule messages name the field; anything else
    /// isn't a rule error.
    #[test]
    fn ac7_rule_errors_name_the_field() {
        let recorded = [
            (
                "TF401320: Rule Error for field Root cause. Error code: Required, HasValues, LimitedToValues, AllowsOldValue, InvalidEmpty.",
                Some("Root cause"),
            ),
            (
                "TF401320: Rule Error for field Resolved Reason. Error code: Required, InvalidEmpty.",
                Some("Resolved Reason"),
            ),
            (
                "VS403691: Update to work item 42 had two or more updates for field with reference name 'System.State'. TF401320: Rule Error for field Story Points. Error code: Required.",
                Some("Story Points"),
            ),
            (
                "TF401289: The current user does not have permissions to save work items under the specified area path.",
                None,
            ),
            (
                "TF26071: This work item has been changed by someone else since you opened it.",
                None,
            ),
        ];
        for (message, field) in recorded {
            assert_eq!(parse_rule_error(message).as_deref(), field, "{message}");
        }
    }

    #[test]
    fn identities_live_on_the_vssps_host() {
        let dev = Url::parse("https://dev.azure.com/contoso/").unwrap();
        assert_eq!(
            people_request(&dev, "dan").url().as_str(),
            "https://vssps.dev.azure.com/contoso/_apis/identities?searchFilter=General&filterValue=dan&queryMembership=None&api-version=7.1"
        );
        let legacy = Url::parse("https://contoso.visualstudio.com/").unwrap();
        assert_eq!(
            identities_base(&legacy).host_str(),
            Some("contoso.vssps.visualstudio.com")
        );
    }
}
