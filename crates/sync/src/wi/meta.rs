//! Metadata payloads of spec 0015 (FR-9, FR-11, FR-13, FR-14, FR-33):
//! work item type states and transitions, type fields, a field's
//! definition, classification trees and directory identities.

use std::collections::HashMap;

use adoken_model::{
    ClassNode, ClassTree, FieldKind, Millis, NodeKind, Person, StateCategory, TypeMeta, WiState,
};
use adoken_store::wi::ListedField;
use serde::Deserialize;
use serde_json::Value;

use crate::wire::parse_time;

/// Metadata older than this still paints at once and refreshes in the
/// background on use (FR-33).
pub const META_TTL_MS: Millis = 24 * adoken_model::time::HOUR;

/// Whether cached metadata fetched at `fetched_ms` needs a refresh.
pub fn is_stale(fetched_ms: Millis, now: Millis) -> bool {
    now - fetched_ms >= META_TTL_MS
}

#[derive(Deserialize)]
struct WireState {
    name: String,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    category: Option<String>,
}

#[derive(Deserialize)]
struct WireTransition {
    to: String,
}

#[derive(Deserialize)]
struct WireType {
    #[serde(default)]
    states: Vec<WireState>,
    /// `from → [{to}]`; `""` is the initial transition.
    #[serde(default)]
    transitions: Option<HashMap<String, Vec<WireTransition>>>,
}

/// `GET workitemtypes/{type}` → states (with category and color) and
/// transitions.
pub fn parse_type(body: &[u8], now: Millis) -> serde_json::Result<TypeMeta> {
    let t: WireType = serde_json::from_slice(body)?;
    Ok(TypeMeta {
        states: t
            .states
            .into_iter()
            .map(|s| WiState {
                category: s
                    .category
                    .as_deref()
                    .and_then(StateCategory::parse)
                    .unwrap_or_else(|| StateCategory::of_state(&s.name)),
                name: s.name,
                color: s.color.filter(|c| !c.is_empty()),
            })
            .collect(),
        transitions: t
            .transitions
            .unwrap_or_default()
            .into_iter()
            .filter(|(from, _)| !from.is_empty())
            .map(|(from, to)| (from, to.into_iter().map(|t| t.to).collect()))
            .collect(),
        fetched_ms: now,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireTypeField {
    reference_name: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    allowed_values: Vec<Value>,
}

/// `GET workitemtypes/{type}/fields?$expand=allowedValues`.
pub fn parse_fields(body: &[u8]) -> serde_json::Result<Vec<ListedField>> {
    #[derive(Deserialize)]
    struct Fields {
        #[serde(default)]
        value: Vec<WireTypeField>,
    }
    let f: Fields = serde_json::from_slice(body)?;
    Ok(f.value
        .into_iter()
        .map(|f| ListedField {
            name: if f.name.is_empty() {
                f.reference_name.clone()
            } else {
                f.name
            },
            reference: f.reference_name,
            allowed: f
                .allowed_values
                .into_iter()
                .filter_map(|v| match v {
                    Value::String(s) => Some(s),
                    Value::Null => None,
                    other => Some(other.to_string()),
                })
                .collect(),
        })
        .collect())
}

/// `GET _apis/wit/fields/{reference}` → how adoken can fill it (FR-9):
/// text-like types are text, identities are people, the rest only Open in
/// Azure DevOps.
pub fn parse_field_kind(body: &[u8]) -> serde_json::Result<FieldKind> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Field {
        #[serde(default, rename = "type")]
        kind: String,
        #[serde(default)]
        is_identity: bool,
        #[serde(default)]
        is_picklist: bool,
    }
    let f: Field = serde_json::from_slice(body)?;
    Ok(
        if f.is_identity || f.kind.eq_ignore_ascii_case("identity") {
            FieldKind::Person
        } else if f.is_picklist {
            FieldKind::Unknown
        } else {
            match f.kind.to_ascii_lowercase().as_str() {
                "string" | "plaintext" => FieldKind::Text,
                _ => FieldKind::Unknown,
            }
        },
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireNode {
    name: String,
    #[serde(default)]
    structure_type: Option<String>,
    #[serde(default)]
    attributes: Option<HashMap<String, Value>>,
    #[serde(default)]
    children: Vec<WireNode>,
}

/// `GET classificationnodes?$depth=14` → both trees, pre-order. Paths are
/// built from names (`Project\Infra\Payments`), the form the work item
/// fields take (the API's own `path` has an extra `\Area\` segment).
pub fn parse_class_tree(body: &[u8], now: Millis) -> serde_json::Result<ClassTree> {
    #[derive(Deserialize)]
    struct Roots {
        #[serde(default)]
        value: Vec<WireNode>,
    }
    fn walk(
        n: WireNode,
        kind: NodeKind,
        parent: Option<&str>,
        depth: u16,
        out: &mut Vec<ClassNode>,
    ) {
        let path = match parent {
            Some(p) => format!("{p}\\{}", n.name),
            None => n.name.clone(),
        };
        let date = |k: &str| {
            n.attributes
                .as_ref()
                .and_then(|a| a.get(k))
                .and_then(Value::as_str)
                .and_then(parse_time)
        };
        out.push(ClassNode {
            kind,
            start_ms: date("startDate"),
            finish_ms: date("finishDate"),
            name: n.name.clone(),
            path: path.clone(),
            depth,
        });
        for child in n.children {
            walk(child, kind, Some(&path), depth + 1, out);
        }
    }
    let roots: Roots = serde_json::from_slice(body)?;
    let mut tree = ClassTree {
        fetched_ms: now,
        ..ClassTree::default()
    };
    for root in roots.value {
        match root.structure_type.as_deref() {
            Some(s) if s.eq_ignore_ascii_case("iteration") => {
                walk(root, NodeKind::Iteration, None, 0, &mut tree.iterations)
            }
            _ => walk(root, NodeKind::Area, None, 0, &mut tree.areas),
        }
    }
    Ok(tree)
}

/// `GET vssps …/_apis/identities?searchFilter=General` → active people and
/// groups (`is_group`), with their sign-in names.
pub fn parse_people(body: &[u8]) -> serde_json::Result<Vec<Person>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Identity {
        id: String,
        #[serde(default)]
        provider_display_name: String,
        #[serde(default)]
        custom_display_name: Option<String>,
        #[serde(default = "yes")]
        is_active: bool,
        #[serde(default)]
        is_container: bool,
        #[serde(default)]
        properties: HashMap<String, Value>,
    }
    fn yes() -> bool {
        true
    }
    #[derive(Deserialize)]
    struct Identities {
        #[serde(default)]
        value: Vec<Identity>,
    }
    let list: Identities = serde_json::from_slice(body)?;
    Ok(list
        .value
        .into_iter()
        .filter(|i| i.is_active)
        .map(|i| {
            let prop = |k: &str| {
                i.properties
                    .get(k)
                    .and_then(|p| p.get("$value"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            };
            Person {
                unique_name: prop("Account").or_else(|| prop("Mail")),
                display_name: i
                    .custom_display_name
                    .clone()
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| i.provider_display_name.clone()),
                id: i.id.to_ascii_lowercase(),
                is_group: i.is_container,
                image_url: None,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_tree_paths_are_field_values_and_dates_parse() {
        let body = serde_json::json!({"count": 2, "value": [
            {"name": "Payments", "structureType": "area", "path": "\\Payments\\Area", "children": [
                {"name": "Infra", "structureType": "area", "children": [
                    {"name": "Settlement", "structureType": "area"}
                ]}
            ]},
            {"name": "Payments", "structureType": "iteration", "children": [
                {"name": "Sprint 44", "structureType": "iteration",
                 "attributes": {"startDate": "2026-09-28T00:00:00Z", "finishDate": "2026-10-09T00:00:00Z"}}
            ]}
        ]});
        let tree = parse_class_tree(&serde_json::to_vec(&body).unwrap(), 5).unwrap();
        assert_eq!(
            tree.areas
                .iter()
                .map(|n| (n.path.as_str(), n.depth))
                .collect::<Vec<_>>(),
            [
                ("Payments", 0),
                ("Payments\\Infra", 1),
                ("Payments\\Infra\\Settlement", 2)
            ]
        );
        assert_eq!(tree.iterations[1].path, "Payments\\Sprint 44");
        assert_eq!(
            tree.iterations[1].start_ms,
            parse_time("2026-09-28T00:00:00Z")
        );
    }

    #[test]
    fn directory_results_keep_sign_in_names_and_flag_groups() {
        let body = serde_json::json!({"count": 3, "value": [
            {"id": "D291", "providerDisplayName": "Dana Ruiz", "isActive": true, "isContainer": false,
             "properties": {"Account": {"$type": "System.String", "$value": "dana@contoso.com"}}},
            {"id": "g1", "providerDisplayName": "[Payments]\\Team", "isContainer": true, "properties": {}},
            {"id": "old", "providerDisplayName": "Gone", "isActive": false}
        ]});
        let people = parse_people(&serde_json::to_vec(&body).unwrap()).unwrap();
        assert_eq!(people.len(), 2);
        assert_eq!(people[0].id, "d291");
        assert_eq!(people[0].unique_name.as_deref(), Some("dana@contoso.com"));
        assert!(people[1].is_group);
    }
}
