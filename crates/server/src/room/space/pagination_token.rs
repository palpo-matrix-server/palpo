use std::fmt::{Display, Formatter};
use std::str::FromStr;

use crate::core::{MatrixError, Seqnum};

// TODO: perhaps use some better form of token rather than just room count
#[derive(Debug, Eq, PartialEq)]
pub struct PaginationToken {
    /// Path down the hierarchy of the room to start the response at,
    /// excluding the root space.
    pub room_sns: Vec<Seqnum>,
    pub limit: usize,
    pub max_depth: usize,
    pub suggested_only: bool,
}

impl FromStr for PaginationToken {
    type Err = MatrixError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut values = value.split('_');
        let mut pag_tok = || {
            let room_sns = values
                .next()?
                .split(',')
                .filter_map(|room_s| i64::from_str(room_s).ok())
                .collect();

            let limit = usize::from_str(values.next()?).ok()?;
            let max_depth = usize::from_str(values.next()?).ok()?;
            let slice = values.next()?;
            let suggested_only = if values.next().is_none() {
                if slice == "true" {
                    true
                } else if slice == "false" {
                    false
                } else {
                    None?
                }
            } else {
                None?
            };

            Some(Self {
                room_sns,
                limit,
                max_depth,
                suggested_only,
            })
        };

        if let Some(token) = pag_tok() {
            Ok(token)
        } else {
            Err(MatrixError::invalid_param("invalid token"))
        }
    }
}

impl Display for PaginationToken {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let room_sns = self
            .room_sns
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");

        write!(
            f,
            "{room_sns}_{}_{}_{}",
            self.limit, self.max_depth, self.suggested_only
        )
    }
}
