use salvo::prelude::*;


use crate::{json_ok, JsonResult};

#[handler]
pub(super) fn delete_account_data(_req: &mut Request, _res: &mut Response) -> JsonResult<()> {
    json_ok(())
}