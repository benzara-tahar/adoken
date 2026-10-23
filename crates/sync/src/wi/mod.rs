//! Work item detail and editing (spec 0015, slices A and B).
//!
//! - [`patch`]: the exact REST calls (pure), FR-8's merge decision, FR-9's
//!   rule-error parser.
//! - [`WiWriter`]: writes (FR-5..FR-10 per write; the per-item queue lives
//!   in the app), comments (FR-25), per-item sync (FR-30), metadata (FR-33)
//!   and directory search (FR-13), at the governor's user priority.
//! - [`apply_local`] / [`comment_local`]: the demo org (FR-37).
//! - [`link_guard`]: FR-21's local refusals.
//! - Every work item answer, from any lane, stores its raw texts, relations
//!   and freshness through [`store_extras`] (FR-32, FR-34).

mod local;
pub mod meta;
pub mod patch;
mod send;

pub(crate) use local::{Extras, store_extras};
pub use local::{apply_local, comment_local, distinct_name_of, link_guard, plain_text};
pub use meta::{META_TTL_MS, is_stale};
pub use patch::{
    COMMENTS_VERSION, Merge, WiChange, WiField, WiWrite, distinct_name, merge, parse_rule_error,
    patch_ops, work_item_url,
};
pub use send::{Conflict, ItemSynced, RuleViolation, WiOutcome, WiWriter};

use adoken_model::Millis;

use crate::governor::GovError;

/// The read-only hint (FR-28, design §9.4; spec 0001 FR-9: passive, no
/// token action); also the missing-scope cause.
pub const READ_ONLY_HINT: &str = "Editing work items needs Work Items (Read & write).";

/// Why a write, comment, sync or fetch didn't happen. `Display` is the
/// inline cause the view shows (FR-7, design §9.2).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WiError {
    /// 401 (or a scope 403) on the work item transport: the token lacks
    /// `vso.work_write`. Never "token rejected" (FR-7, FR-28).
    #[error("Editing work items needs Work Items (Read & write).")]
    MissingScope,
    /// 403 for a permission (area security), with Azure DevOps' message.
    #[error("Azure DevOps didn't let you edit work items in this area.")]
    NotAllowed { message: Option<String> },
    /// 404: deleted, or access lost (§States Error).
    #[error("This work item was deleted or you lost access to it.")]
    Gone,
    /// 5xx or a network error.
    #[error("Couldn't reach Azure DevOps. Nothing changed.")]
    Unreachable,
    #[error("You're offline. Nothing was sent.")]
    Offline,
    /// Signed in, but sync hasn't built its connection yet.
    #[error("Not connected to Azure DevOps yet. Nothing was sent.")]
    NotConnected,
    #[error("Azure DevOps is throttling requests. Try again in a moment.")]
    Throttled { until: Millis },
    /// The main transport latched the token as rejected (spec 0001 FR-15).
    #[error("Azure DevOps rejected adoken's token. Replace it in Settings › Account.")]
    Unauthorized,
    /// Any other refusal, with Azure DevOps' own message.
    #[error("{}", message.clone().unwrap_or_else(|| format!("Azure DevOps refused (HTTP {status}).")))]
    Rejected {
        status: u16,
        message: Option<String>,
    },
    /// Refused locally, nothing sent ("Title can't be empty").
    #[error("{0}")]
    Invalid(String),
    #[error("Couldn't save the change locally: {0}")]
    Local(String),
}

impl WiError {
    /// A write's failure (FR-7).
    pub(crate) fn from_write(error: GovError) -> Self {
        match error {
            GovError::Status { status: 401, .. } => Self::MissingScope,
            GovError::Status {
                status: 403,
                message,
                ..
            } => {
                let scope = message.as_deref().is_some_and(|m| {
                    let m = m.to_ascii_lowercase();
                    m.contains("scope") || m.contains("vso.work_write")
                });
                if scope {
                    Self::MissingScope
                } else {
                    Self::NotAllowed { message }
                }
            }
            other => Self::from_read(other),
        }
    }

    /// A read's failure.
    pub(crate) fn from_read(error: GovError) -> Self {
        match error {
            GovError::Unauthorized => Self::Unauthorized,
            GovError::Offline(_) => Self::Unreachable,
            GovError::Throttled { until } => Self::Throttled { until },
            GovError::Deferred => Self::Throttled { until: 0 },
            GovError::Status { status: 404, .. } => Self::Gone,
            GovError::Status { status, .. } if status >= 500 => Self::Unreachable,
            GovError::Status { status: 401, .. } => Self::MissingScope,
            GovError::Status {
                status, message, ..
            } => Self::Rejected { status, message },
        }
    }

    /// Azure DevOps' own message, shown on its own line under the cause.
    pub fn message(&self) -> Option<&str> {
        match self {
            Self::NotAllowed { message } | Self::Rejected { message, .. } => message.as_deref(),
            _ => None,
        }
    }

    /// The token can't write work items: the item switches to read-only
    /// mode and keeps my value as a Retry (FR-28).
    pub fn is_missing_scope(&self) -> bool {
        matches!(self, Self::MissingScope)
    }

    /// Retry makes sense (the value is kept).
    pub fn retryable(&self) -> bool {
        !matches!(self, Self::Gone | Self::Invalid(_))
    }

    /// The pass outcome of a per-item sync that failed (0016 FR-11).
    pub(crate) fn outcome(&self, host: &str) -> crate::activity::Outcome {
        use crate::activity::{FailReason, Outcome};
        let status = |status: u16, message: Option<String>| {
            Outcome::Failed(FailReason::Status {
                status,
                code: None,
                message,
            })
        };
        match self {
            Self::Throttled { until } => Outcome::Throttled { until: *until },
            Self::Unreachable => Outcome::Failed(FailReason::Unreachable {
                host: host.to_owned(),
            }),
            Self::Offline => Outcome::Canceled,
            Self::Unauthorized => Outcome::Failed(FailReason::Unauthorized),
            Self::MissingScope => status(401, None),
            Self::Gone => status(404, None),
            Self::NotAllowed { message } => status(403, message.clone()),
            Self::Rejected { status: s, message } => status(*s, message.clone()),
            other => Outcome::Failed(FailReason::Other(other.to_string())),
        }
    }
}

impl From<adoken_store::Error> for WiError {
    fn from(error: adoken_store::Error) -> Self {
        Self::Local(error.to_string())
    }
}
