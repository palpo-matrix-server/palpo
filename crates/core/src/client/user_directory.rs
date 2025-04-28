//! `POST /_matrix/client/*/user_directory/search`
//!
//! Performs a search for users.
//! `/v3/` ([spec])
//!
//! [spec]: https://spec.matrix.org/latest/client-server-api/#post_matrixclientv3user_directorysearch

use salvo::oapi::{ToParameters, ToSchema};
use serde::{Deserialize, Serialize};

use crate::{OwnedMxcUri, OwnedUserId};

// const METADATA: Metadata = metadata! {
//     method: POST,
//     rate_limited: true,
//     authentication: AccessToken,
//     history: {
//         1.0 => "/_matrix/client/r0/user_directory/search",
//         1.1 => "/_matrix/client/v3/user_directory/search",
//     }
// };

#[derive(ToParameters, Deserialize, Debug)]
pub struct SearchUsersReqArgs {
    /// Language tag to determine the collation to use for the
    /// (case-insensitive) search.
    ///
    /// See [MDN] for the syntax.
    ///
    /// [MDN]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Accept-Language#Syntax
    #[salvo(parameter(parameter_in = Header))]
    pub accept_language: Option<String>,
}

/// Request type for the `search_users` endpoint.
#[derive(ToSchema, Deserialize, Debug)]
pub struct SearchUsersReqBody {
    /// The term to search for.
    pub search_term: String,

    /// The maximum number of results to return.
    ///
    /// Defaults to 10.
    #[serde(default = "default_limit", skip_serializing_if = "is_default_limit")]
    pub limit: usize,
    // /// Language tag to determine the collation to use for the (case-insensitive) search.
    // ///
    // /// See [MDN] for the syntax.
    // ///
    // /// [MDN]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Accept-Language#Syntax
    // #[palpo_api(header = ACCEPT_LANGUAGE)]
    // pub language: Option<String>,
}

/// Response type for the `search_users` endpoint.
#[derive(ToSchema, Serialize, Debug)]
pub struct SearchUsersResBody {
    /// Ordered by rank and then whether or not profile info is available.
    pub results: Vec<SearchedUser>,

    /// Indicates if the result list has been truncated by the limit.
    pub limited: bool,
}
impl SearchUsersResBody {
    /// Creates a new `Response` with the given results and limited flag
    pub fn new(results: Vec<SearchedUser>, limited: bool) -> Self {
        Self { results, limited }
    }
}

fn default_limit() -> usize {
    10
}

fn is_default_limit(limit: &usize) -> bool {
    limit == &default_limit()
}

/// User data as result of a search.
#[derive(ToSchema, Deserialize, Serialize, Clone, Debug)]
pub struct SearchedUser {
    /// The user's matrix user ID.
    pub user_id: OwnedUserId,

    /// The display name of the user, if one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// The avatar url, as an MXC, if one exists.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "crate::serde::empty_string_as_none"
    )]
    pub avatar_url: Option<OwnedMxcUri>,
}

impl SearchedUser {
    /// Create a new `User` with the given `UserId`.
    pub fn new(user_id: OwnedUserId) -> Self {
        Self {
            user_id,
            display_name: None,
            avatar_url: None,
        }
    }
}
