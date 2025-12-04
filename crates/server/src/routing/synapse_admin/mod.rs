//! Synapse Admin API implementation for MAS (Matrix Authentication Service) integration
//!
//! Base path: `/_synapse/admin/`

mod user_lookup;

use salvo::prelude::*;

use crate::{AppResult, hoops};
use crate::core::MatrixError;
use crate::exts::DepotExt;

/// Admin authentication middleware - requires the user to be an admin
#[handler]
pub async fn require_admin(depot: &mut Depot) -> AppResult<()> {
    let authed = depot.authed_info()?;
    if !authed.is_admin() {
        return Err(MatrixError::forbidden("Requires admin privileges", None).into());
    }
    Ok(())
}

pub fn router() -> Router {
    Router::with_path("_synapse/admin")
        .hoop(hoops::auth_by_access_token)
        .hoop(require_admin)
        // v1 endpoints
        .push(user_lookup::router())
}
