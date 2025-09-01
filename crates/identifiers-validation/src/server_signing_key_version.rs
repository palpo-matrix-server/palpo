use crate::Error;

pub fn validate(s: &str) -> Result<(), Error> {
    if s.is_empty() {
        Err(Error::Empty);
    } else if !s.chars().all(|c| c.is_alphanumeric() || c == '_') {
        Err(Error::InvalidCharacters);
    } else {
        Ok(())
    }
}
