use crate::{Error, validate_delimited_id};

pub fn validate(s: &str) -> Result<(), Error> {
    validate_delimited_id(s, b'#')
}
