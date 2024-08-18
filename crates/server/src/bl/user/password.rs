use diesel::prelude::*;

use crate::core::identifiers::*;
use crate::core::UnixMillis;
use crate::schema::*;
use crate::{db, utils, AppResult, MatrixError};

#[derive(Identifiable, Debug, Clone)]
#[diesel(table_name = user_passwords)]
pub struct DbPassword {
    pub id: i64,
    pub user_id: OwnedUserId,
    pub hash: String,
    pub created_at: UnixMillis,
}
#[derive(Insertable, Queryable, Debug, Clone)]
#[diesel(table_name = user_passwords)]
pub struct NewDbPassword {
    pub user_id: OwnedUserId,
    pub hash: String,
    pub created_at: UnixMillis,
}

fn get_password_hash(user_id: &UserId) -> AppResult<String> {
    user_passwords::table
        .filter(user_passwords::user_id.eq(user_id))
        .order_by(user_passwords::id.desc())
        .select(user_passwords::hash)
        .first::<String>(&mut *db::connect()?)
        .map_err(Into::into)
}

pub fn vertify_password(user_id: &UserId, password: &str) -> AppResult<()> {
    let hash = get_password_hash(user_id).map_err(|_| MatrixError::forbidden("Wrong username or password."))?;
    if hash.is_empty() {
        return Err(MatrixError::user_deactivated("The user has been deactivated").into());
    }

    let hash_matches = argon2::verify_encoded(&hash, password.as_bytes()).unwrap_or(false);

    if !hash_matches {
        return Err(MatrixError::forbidden("Wrong username or password.").into());
    } else {
        Ok(())
    }
}

pub fn set_password(user_id: &UserId, password: &str) -> AppResult<()> {
    if let Ok(hash) = utils::hash_password(password) {
        diesel::insert_into(user_passwords::table)
            .values(NewDbPassword {
                user_id: user_id.to_owned(),
                hash,
                created_at: UnixMillis::now(),
            })
            .execute(&mut db::connect()?)?;
        Ok(())
    } else {
        Err(MatrixError::invalid_param("Password does not meet the requirements.").into())
    }
}
