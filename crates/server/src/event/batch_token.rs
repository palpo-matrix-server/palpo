use std::str::FromStr;

use crate::core::identifiers::*;
use crate::core::serde::{CanonicalJsonObject, RawJsonValue};
use crate::core::{Direction, Seqnum, UnixMillis, signatures};
use crate::data::connect;
use crate::data::room::DbEvent;
use crate::data::schema::*;
use crate::utils::SeqnumQueueGuard;
use crate::{AppError, AppResult, MatrixError};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
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
    pub fn event_sn(&self) -> Seqnum {
        self.stream_ordering.abs()
    }

    pub const MIN: Self = Self {
        stream_ordering: 0,
        topological_ordering: Some(0),
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
        let mut stream_ordering = None;
        let mut topological_ordering = None;
        while let Some(part) = parts.next() {
            if part.starts_with("s") {
                stream_ordering = Some(
                    part[1..]
                        .parse::<Seqnum>()
                        .map_err(|_| MatrixError::unknown("invalid event_sn"))?,
                );
            } else if part.starts_with("t") {
                topological_ordering = Some(
                    part[1..]
                        .parse::<i64>()
                        .map_err(|_| MatrixError::unknown("invalid topological_ordering"))?,
                );
            } else {
                return Err(MatrixError::unknown(format!(
                    "invalid stream token: {}",
                    part
                )));
            }
        }

        Ok(BatchToken {
            stream_ordering: stream_ordering
                .ok_or_else(|| MatrixError::unknown("missing stream ordering"))?,
            topological_ordering,
        })
    }
}

impl std::fmt::Display for BatchToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(topological_ordering) = self.topological_ordering {
            write!(f, "s{}_t{}", self.stream_ordering, topological_ordering)
        } else {
            write!(f, "s{}", self.stream_ordering)
        }
    }
}
