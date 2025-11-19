use std::str::FromStr;

use crate::core::identifiers::*;
use crate::core::serde::{CanonicalJsonObject, RawJsonValue};
use crate::core::{Direction, Seqnum, UnixMillis, signatures};
use crate::data::connect;
use crate::data::room::DbEvent;
use crate::data::schema::*;
use crate::utils::SeqnumQueueGuard;
use crate::{AppError, AppResult, MatrixError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchToken {
    pub stream_ordering: Seqnum,
    pub topological_ordering: Option<i64>,
}
impl BatchToken {
    pub fn new(stream_ordering: Seqnum, topological_ordering: Option<i64>) -> Self {
        Self {
            stream_ordering,
            topological_ordering,
        }
    }
    pub fn zero() -> Self {
        Self {
            stream_ordering: 0,
            topological_ordering: Some(0),
        }
    }
    pub const MIN: Self = Self {
        stream_ordering: 0,
        topological_ordering: 0,
    };
    pub const MAX: Self = Self {
        stream_ordering: Seqnum::MAX,
        topological_ordering: Some(i64::MAX),
    };
}
impl FromStr for BatchToken {
    type Err = MatrixError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut parts = input.split('_');
        let stream_ordering = None;
        let topological_ordering = None;
        while let Some(part) = parts.next() {
            if part.starts_with("s") {
                stream_ordering = Some(
                    part[1..]
                        .parse::<Seqnum>()
                        .map_err(|_| MatrixError::unknown("invalid stream ordering"))?,
                );
            } else if part.starts_with("t") {
                topological_ordering = Some(
                    part[1..]
                        .parse::<i64>()
                        .map_err(|_| MatrixError::unknown("invalid topological ordering"))?,
                );
            } else {
                return Err(MatrixError::unknown("invalid stream token part"));
            }
        }

        Ok(BatchToken {
            stream_ordering: stream_ordering
                .ok_or_else(|| MatrixError::unknown("missing stream ordering"))?,
            topological_ordering: topological_ordering
                .ok_or_else(|| MatrixError::unknown("missing topological ordering"))?,
        })
    }
}

impl std::fmt::Display for BatchToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "s{}_t{}",
            self.stream_ordering, self.topological_ordering
        )
    }
}
