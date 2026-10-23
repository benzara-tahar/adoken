//! The Azure DevOps REST calls onboarding makes (0001 §5.1, §5.3).

use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use url::Url;

use crate::http::{Endpoints, Http, HttpError, decode};
use crate::{AuthProvider, Organization, Pat};

use super::{Identity, ProjectRef};

const API_VERSION: &str = "7.1";
const PAT_API_VERSION: &str = "7.1-preview.1";
const PROJECTS_CONTINUATION_HEADER: &str = "x-ms-continuationtoken";
/// Hard stop for project paging; far beyond any real org.
const MAX_PROJECT_PAGES: usize = 50;

/// The Entra identity behind `az` (Profiles `me`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub id: String,
    pub display_name: String,
    pub email: Option<String>,
}

/// An organization the signed-in user belongs to (Accounts API).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgRef {
    pub id: String,
    pub name: String,
}

impl OrgRef {
    pub fn organization(&self) -> Option<Organization> {
        Organization::parse(&self.name).ok()
    }
}

/// A PAT the lifecycle API just minted. The secret goes straight to the keychain.
#[derive(Debug)]
pub(crate) struct CreatedPat {
    pub pat: Pat,
    pub authorization_id: Option<String>,
    pub valid_to: Option<OffsetDateTime>,
    pub scope: Option<String>,
}

#[derive(Debug)]
pub(crate) enum PatApiError {
    /// `patTokenError` other than `none`.
    Code(String),
    Http(HttpError),
}

#[derive(Debug, Clone)]
pub(crate) struct Api {
    pub http: Http,
    pub endpoints: Endpoints,
}

fn with_query(mut url: Url, pairs: &[(&str, &str)]) -> Url {
    {
        let mut query = url.query_pairs_mut();
        for (key, value) in pairs {
            query.append_pair(key, value);
        }
    }
    url
}

fn join(base: &Url, path: &str) -> Url {
    base.join(path).expect("static relative path joins")
}

impl Api {
    fn org_url(&self, org: &Organization, path: &str, query: &[(&str, &str)]) -> Url {
        with_query(join(&self.endpoints.org(org), path), query)
    }

    pub async fn profile_me(&self, auth: &dyn AuthProvider) -> Result<Profile, HttpError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Body {
            id: String,
            display_name: Option<String>,
            email_address: Option<String>,
        }
        let url = with_query(
            join(&self.endpoints.app_vssps, "_apis/profile/profiles/me"),
            &[("api-version", API_VERSION)],
        );
        let body: Body = self.http.json(self.http.get(url), auth).await?;
        Ok(Profile {
            display_name: body
                .display_name
                .or_else(|| body.email_address.clone())
                .unwrap_or_default(),
            id: body.id,
            email: body.email_address,
        })
    }

    pub async fn accounts(
        &self,
        member_id: &str,
        auth: &dyn AuthProvider,
    ) -> Result<Vec<OrgRef>, HttpError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Account {
            account_id: String,
            account_name: String,
        }
        #[derive(Deserialize)]
        struct Body {
            value: Vec<Account>,
        }
        let url = with_query(
            join(&self.endpoints.app_vssps, "_apis/accounts"),
            &[("memberId", member_id), ("api-version", API_VERSION)],
        );
        let body: Body = self.http.json(self.http.get(url), auth).await?;
        let mut orgs: Vec<OrgRef> = body
            .value
            .into_iter()
            .map(|account| OrgRef {
                id: account.account_id,
                name: account.account_name,
            })
            .collect();
        orgs.sort_by_key(|org| org.name.to_lowercase());
        Ok(orgs)
    }

    /// All projects (paged via `x-ms-continuationtoken`), or the first `top`.
    pub async fn projects(
        &self,
        org: &Organization,
        auth: &dyn AuthProvider,
        top: Option<u32>,
    ) -> Result<Vec<ProjectRef>, HttpError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Project {
            id: String,
            name: String,
            description: Option<String>,
            last_update_time: Option<String>,
        }
        #[derive(Deserialize)]
        struct Body {
            value: Vec<Project>,
        }
        let top = top.map(|top| top.to_string());
        let mut projects = Vec::new();
        let mut continuation: Option<String> = None;
        for _ in 0..MAX_PROJECT_PAGES {
            let mut query = vec![("api-version", API_VERSION)];
            if let Some(top) = &top {
                query.push(("$top", top));
            }
            if let Some(token) = &continuation {
                query.push(("continuationToken", token));
            }
            let url = self.org_url(org, "_apis/projects", &query);
            let response = self.http.send(self.http.get(url), auth).await?;
            let next = response
                .headers()
                .get(PROJECTS_CONTINUATION_HEADER)
                .and_then(|value| value.to_str().ok())
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            let body: Body = decode(response).await?;
            projects.extend(body.value.into_iter().map(|project| {
                ProjectRef {
                    id: project.id,
                    name: project.name,
                    description: project.description.filter(|d| !d.trim().is_empty()),
                    last_update_time: project
                        .last_update_time
                        .and_then(|t| OffsetDateTime::parse(&t, &Rfc3339).ok()),
                }
            }));
            match next {
                Some(next) if top.is_none() => continuation = Some(next),
                _ => break,
            }
        }
        Ok(projects)
    }

    /// `GET {org}/_apis/connectionData` → the authenticated user. An
    /// anonymous answer means the credential wasn't accepted.
    pub async fn connection_data(
        &self,
        org: &Organization,
        auth: &dyn AuthProvider,
    ) -> Result<Identity, HttpError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct User {
            id: String,
            provider_display_name: Option<String>,
            custom_display_name: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Body {
            authenticated_user: User,
        }
        let url = self.org_url(org, "_apis/connectionData", &[]);
        let body: Body = self.http.json(self.http.get(url), auth).await?;
        let user = body.authenticated_user;
        let anonymous = user.id.is_empty()
            || user.id == "00000000-0000-0000-0000-000000000000"
            || user.provider_display_name.as_deref() == Some("Anonymous");
        if anonymous {
            auth.on_unauthorized();
            return Err(HttpError::Unauthorized {
                message: "anonymous".into(),
            });
        }
        Ok(Identity {
            display_name: user
                .custom_display_name
                .or(user.provider_display_name)
                .unwrap_or_default(),
            id: user.id,
        })
    }

    /// PAT Lifecycle create (0001 §3 step 8). `allOrgs` is always false (FR-4).
    pub async fn create_pat(
        &self,
        org: &Organization,
        auth: &dyn AuthProvider,
        display_name: &str,
        scope: &str,
        valid_to: OffsetDateTime,
    ) -> Result<CreatedPat, PatApiError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Token {
            authorization_id: Option<String>,
            valid_to: Option<String>,
            scope: Option<String>,
            token: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Body {
            pat_token: Option<Token>,
            pat_token_error: Option<String>,
        }
        let url = with_query(
            join(&self.endpoints.org_vssps(org), "_apis/tokens/pats"),
            &[("api-version", PAT_API_VERSION)],
        );
        let body = json!({
            "displayName": display_name,
            "scope": scope,
            "validTo": valid_to.format(&Rfc3339).expect("UTC timestamps format"),
            "allOrgs": false,
        });
        let response = self
            .http
            .execute(self.http.post_json(url, &body), auth)
            .await
            .map_err(PatApiError::Http)?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|_| PatApiError::Http(HttpError::Offline("response interrupted".into())))?;
        let parsed = serde_json::from_slice::<Body>(&bytes).ok();
        if let Some(code) = parsed
            .as_ref()
            .and_then(|body| body.pat_token_error.as_deref())
            .filter(|code| !code.eq_ignore_ascii_case("none"))
        {
            return Err(PatApiError::Code(code.to_owned()));
        }
        if !status.is_success() {
            #[derive(Deserialize)]
            struct AdoError {
                message: Option<String>,
            }
            let message = serde_json::from_slice::<AdoError>(&bytes)
                .ok()
                .and_then(|error| error.message)
                .unwrap_or_else(|| status.canonical_reason().unwrap_or("error").to_owned());
            return Err(PatApiError::Http(match status.as_u16() {
                403 => HttpError::Forbidden { message },
                404 => HttpError::NotFound { message },
                code => HttpError::Status {
                    status: code,
                    message,
                },
            }));
        }
        let token = parsed.and_then(|body| body.pat_token).ok_or_else(|| {
            PatApiError::Http(HttpError::Decode("no patToken in the response".into()))
        })?;
        let secret = token
            .token
            .filter(|secret| !secret.is_empty())
            .ok_or_else(|| {
                PatApiError::Http(HttpError::Decode("patToken without a token".into()))
            })?;
        Ok(CreatedPat {
            pat: Pat::from_secret(secret),
            authorization_id: token.authorization_id,
            valid_to: token
                .valid_to
                .and_then(|t| OffsetDateTime::parse(&t, &Rfc3339).ok()),
            scope: token.scope,
        })
    }

    /// PAT Lifecycle revoke (Replace token, sign-out).
    pub async fn revoke_pat(
        &self,
        org: &Organization,
        auth: &dyn AuthProvider,
        authorization_id: &str,
    ) -> Result<(), HttpError> {
        let url = with_query(
            join(&self.endpoints.org_vssps(org), "_apis/tokens/pats"),
            &[
                ("authorizationId", authorization_id),
                ("api-version", PAT_API_VERSION),
            ],
        );
        self.http.send(self.http.delete(url), auth).await?;
        Ok(())
    }

    // ── Scope probes (0001 §5.3): read-only, `$top=1` ──────────────────────

    pub async fn probe_projects(
        &self,
        org: &Organization,
        auth: &dyn AuthProvider,
    ) -> Result<Option<ProjectRef>, HttpError> {
        Ok(self.projects(org, auth, Some(1)).await?.into_iter().next())
    }

    pub async fn probe_code(
        &self,
        org: &Organization,
        project_id: Option<&str>,
        auth: &dyn AuthProvider,
    ) -> Result<(), HttpError> {
        let path = match project_id {
            Some(project) => format!("{project}/_apis/git/repositories"),
            None => "_apis/git/repositories".to_owned(),
        };
        let url = self.org_url(org, &path, &[("$top", "1"), ("api-version", API_VERSION)]);
        self.http.send(self.http.get(url), auth).await.map(drop)
    }

    pub async fn probe_work(
        &self,
        org: &Organization,
        project_id: &str,
        auth: &dyn AuthProvider,
    ) -> Result<(), HttpError> {
        let url = self.org_url(
            org,
            &format!("{project_id}/_apis/wit/wiql"),
            &[("$top", "1"), ("api-version", API_VERSION)],
        );
        let body = json!({ "query": "SELECT [System.Id] FROM WorkItems WHERE [System.Id] = 0" });
        self.http
            .send(self.http.post_json(url, &body), auth)
            .await
            .map(drop)
    }

    pub async fn probe_build(
        &self,
        org: &Organization,
        project_id: &str,
        auth: &dyn AuthProvider,
    ) -> Result<(), HttpError> {
        let url = self.org_url(
            org,
            &format!("{project_id}/_apis/build/builds"),
            &[("$top", "1"), ("api-version", API_VERSION)],
        );
        self.http.send(self.http.get(url), auth).await.map(drop)
    }

    pub async fn probe_identity(
        &self,
        org: &Organization,
        me: &str,
        auth: &dyn AuthProvider,
    ) -> Result<(), HttpError> {
        let url = with_query(
            join(&self.endpoints.org_vssps(org), "_apis/identities"),
            &[("identityIds", me), ("api-version", API_VERSION)],
        );
        self.http.send(self.http.get(url), auth).await.map(drop)
    }
}
