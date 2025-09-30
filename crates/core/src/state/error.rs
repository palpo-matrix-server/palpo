use thiserror::Error;

use crate::OwnedEventId;

/// Result type for state resolution.
pub type StateResult<T> = std::result::Result<T, StateError>;

/// Represents the various errors that arise when resolving state.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum StateError {
    /// The given event was not found.
    #[error("failed to find event {0}")]
    NotFound(OwnedEventId),

    /// Forbidden.
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// An auth event is invalid.
    #[error("invalid auth event: {0}")]
    AuthEvent(String),

    /// A state event doesn't have a `state_key`.
    #[error("state event has no `state_key`")]
    MissingStateKey,

    /// Provided `fetch_conflicted_state_subgraph` function failed.
    #[error("fetch conflicted state subgraph failed")]
    FetchConflictedStateSubgraphFailed,

    #[error("other state error: {0}")]
    Other(String),
}

impl StateError {
    pub fn auth_event(error: impl Into<String>) -> Self {
        StateError::AuthEvent(error.into())
    }
    pub fn forbidden(error: impl Into<String>) -> Self {
        StateError::Forbidden(error.into())
    }
    pub fn other(error: impl Into<String>) -> Self {
        StateError::Other(error.into())
    }
}

impl From<String> for StateError {
    fn from(error: String) -> Self {
        StateError::Other(error)
    }
}
