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
    pub event_sn: Seqnum,
    pub event_depth: Option<i64>,
}
impl BatchToken {
    pub fn new(event_sn: Seqnum, event_depth: Option<i64>) -> Self {
        Self {
            event_sn,
            event_depth,
        }
    }
    pub fn zero() -> Self {
        Self {
            event_sn: 0,
            event_depth: Some(0),
        }
    }
    pub const MIN: Self = Self {
        event_sn: 0,
        event_depth: Some(0),
    };
    pub const MAX: Self = Self {
        event_sn: Seqnum::MAX,
        event_depth: Some(i64::MAX),
    };
}
impl FromStr for BatchToken {
    type Err = MatrixError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut parts = input.split('_');
        let mut event_sn = None;
        let mut event_depth = None;
        while let Some(part) = parts.next() {
            if part.starts_with("s") {
                event_sn = Some(
                    part[1..]
                        .parse::<Seqnum>()
                        .map_err(|_| MatrixError::unknown("invalid event_sn"))?,
                );
            } else if part.starts_with("d") {
                event_depth = Some(
                    part[1..]
                        .parse::<i64>()
                        .map_err(|_| MatrixError::unknown("invalid event_depth"))?,
                );
            } else {
                return Err(MatrixError::unknown(format!(
                    "invalid stream token: {}",
                    part
                )));
            }
        }

        Ok(BatchToken {
            event_sn: event_sn.ok_or_else(|| MatrixError::unknown("missing event_sn"))?,
            event_depth,
        })
    }
}

impl std::fmt::Display for BatchToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(event_depth) = self.event_depth {
            write!(f, "s{}_d{}", self.event_sn, event_depth)
        } else {
            write!(f, "s{}", self.event_sn)
        }
    }
}
