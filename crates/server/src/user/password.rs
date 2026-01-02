use diesel::prelude::*;

use super::DbUser;
use crate::core::UnixMillis;
use crate::core::identifiers::*;
use crate::data::connect;
use crate::data::schema::*;
use crate::data::user::NewDbPassword;
use crate::{AppResult, MatrixError};

pub fn verify_password(user: &DbUser, password: &str) -> AppResult<()> {
    println!("===============verify_password=== {user:?} , password: {password}");
    if user.deactivated_at.is_some() {
        println!("===============verify_password 1");
        return Err(MatrixError::user_deactivated("the user has been deactivated").into());
    }
    println!("===============verify_password 2");
    let hash = crate::user::get_password_hash(&user.id)
        .map_err(|_| MatrixError::unauthorized("wrong username or password."))?;
    if hash.is_empty() {
        println!("===============verify_password 3");
        return Err(MatrixError::user_deactivated("the user has been deactivated").into());
    }

    println!("===============verify_password 4  hash:{hash}  password:{password}");
    let hash_matches = argon2::verify_encoded(&hash, password.as_bytes()).unwrap_or(false);

    println!("===============verify_password 5");
    if !hash_matches {
        println!("===============verify_password 6");
        Err(MatrixError::unauthorized("wrong username or password.").into())
    } else {
        println!("===============verify_password 7");
        Ok(())
    }
}

pub fn get_password_hash(user_id: &UserId) -> AppResult<String> {
    user_passwords::table
        .filter(user_passwords::user_id.eq(user_id))
        .order_by(user_passwords::id.desc())
        .select(user_passwords::hash)
        .first::<String>(&mut connect()?)
        .map_err(Into::into)
}

/// Set/update password hash for a user
pub fn set_password(user_id: &UserId, password: &str) -> AppResult<()> {
    let hash = crate::utils::hash_password(password)?;
    println!("===============set_password  user_id:{user_id}  hash:{hash}");
    diesel::insert_into(user_passwords::table)
        .values(NewDbPassword {
            user_id: user_id.to_owned(),
            hash: hash.to_owned(),
            created_at: UnixMillis::now(),
        })
        .execute(&mut connect()?)?;
    Ok(())
}
