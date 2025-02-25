use crate::{Error, validate_id};

pub fn validate(s: &str) -> Result<(), Error> {
    validate_id(s, b'!')
}
