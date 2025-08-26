use thiserror::Error;

use crate::OwnedEventId;

/// Result type for state resolution.
pub type StateResult<T> = std::result::Result<T, StateError>;

/// Represents the various errors that arise when resolving state.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum StateError {
    /// The given event was not found.
    #[error("Failed to find event {0}")]
    NotFound(OwnedEventId),

    /// An auth event is invalid.
    #[error("Invalid auth event: {0}")]
    AuthEvent(String),

    /// A state event doesn't have a `state_key`.
    #[error("State event has no `state_key`")]
    MissingStateKey,

    /// Provided `fetch_conflicted_state_subgraph` function failed.
    #[error("`fetch_conflicted_state_subgraph` failed")]
    FetchConflictedStateSubgraphFailed,
}
