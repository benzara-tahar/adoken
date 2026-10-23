use adoken_model::{Millis, PersonId, PrStatus, RunStatus, StateCategory, WorkItemType};

/// All `None` = every PR, newest `updated` first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrFilter {
    pub status: Option<PrStatus>,
    pub author: Option<PersonId>,
    pub reviewer: Option<PersonId>,
    /// Author OR reviewer.
    pub involving: Option<PersonId>,
    pub project_id: Option<String>,
    pub repo_id: Option<String>,
    pub updated_since: Option<Millis>,
    pub limit: Option<usize>,
}

/// All `None` = every non-deleted item, newest `changed` first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkItemFilter {
    pub assigned_to: Option<PersonId>,
    pub project_id: Option<String>,
    pub kind: Option<WorkItemType>,
    pub state_category: Option<StateCategory>,
    /// Only `Proposed` / `InProgress` items.
    pub open_only: bool,
    pub changed_since: Option<Millis>,
    pub limit: Option<usize>,
}

/// All `None` = every run, newest `queued` first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunFilter {
    pub requested_for: Option<PersonId>,
    pub pr_id: Option<i64>,
    /// Full ref.
    pub branch: Option<String>,
    pub project_id: Option<String>,
    pub pipeline_id: Option<i64>,
    pub status: Option<RunStatus>,
    pub limit: Option<usize>,
}
