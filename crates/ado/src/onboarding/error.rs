use std::time::Duration;

use crate::AuthError;
use crate::http::HttpError;

/// Everything onboarding can fail with, worded for the user
/// (`docs/design/onboarding.md` copy). `Display` is the message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OnboardError {
    /// `az` is missing, too old, timed out, or failed unexpectedly.
    #[error("{reason}")]
    AzUnavailable { reason: String },
    #[error("Sign-in cancelled.")]
    LoginCancelled,
    #[error("Azure CLI sign-in failed: {detail}")]
    LoginFailed { detail: String },
    #[error("You're offline. Check your connection, then try again.")]
    Offline,
    /// `Retry-After`: show a countdown, never auto-retry (0001 §3 States).
    #[error("Azure DevOps asked us to slow down. Try again in {}s.", retry_after.as_secs().max(1))]
    Throttled { retry_after: Duration },
    /// The PAT was rejected (401).
    #[error("That token didn't work. It may be expired, revoked, or for another organization.")]
    InvalidToken,
    /// The Entra token from `az` was rejected (401) by this organization.
    #[error(
        "Azure CLI isn't signed into this organization's directory. Run `az login --tenant <tenant-id>`, then try again."
    )]
    NotInTenant,
    /// PAT creation is blocked (accessDenied, admin consent, tenant policy):
    /// switch to the paste path with this reason (0001 AC-8).
    #[error(
        "Your organization doesn't allow adoken to create personal access tokens ({reason}). Ask an admin, or paste a token instead."
    )]
    PolicyBlocked { reason: String },
    /// Any other `patTokenError`, shown verbatim (0001 §3 step 8).
    #[error("Azure DevOps refused to create the token: {code}")]
    PatRejected { code: String },
    #[error("Organization `{org}` wasn't found on dev.azure.com.")]
    OrgNotFound { org: String },
    #[error("Couldn't use the Keychain: {detail}")]
    Keychain { detail: String },
    #[error("Couldn't save adoken's account details: {detail}")]
    Storage { detail: String },
    #[error("{0}")]
    Other(String),
}

impl OnboardError {
    /// User-facing text.
    pub fn message(&self) -> String {
        self.to_string()
    }

    /// Whether the flow should switch to "paste a token" (Path B).
    pub fn falls_back_to_paste(&self) -> bool {
        matches!(self, Self::PolicyBlocked { .. })
    }

    /// Map an HTTP failure made with a PAT.
    pub(crate) fn from_pat_http(err: HttpError) -> Self {
        match err {
            HttpError::Unauthorized { .. } | HttpError::Auth(AuthError::Rejected) => {
                Self::InvalidToken
            }
            other => Self::from_http(other),
        }
    }

    /// Map an HTTP failure made with the Entra token from `az`.
    pub(crate) fn from_entra_http(err: HttpError) -> Self {
        match err {
            HttpError::Unauthorized { .. } | HttpError::Auth(AuthError::Rejected) => {
                Self::NotInTenant
            }
            other => Self::from_http(other),
        }
    }

    fn from_http(err: HttpError) -> Self {
        match err {
            HttpError::Throttled { retry_after } => Self::Throttled { retry_after },
            HttpError::Offline(_) => Self::Offline,
            HttpError::Unauthorized { .. } | HttpError::Auth(AuthError::Rejected) => {
                Self::InvalidToken
            }
            HttpError::Auth(AuthError::Expired) => Self::InvalidToken,
            HttpError::Auth(AuthError::Unavailable(reason)) => Self::AzUnavailable { reason },
            HttpError::Forbidden { message }
            | HttpError::NotFound { message }
            | HttpError::Status { message, .. } => Self::Other(format!("Azure DevOps: {message}")),
            HttpError::Decode(detail) => {
                Self::Other(format!("Unexpected response from Azure DevOps: {detail}"))
            }
        }
    }
}
