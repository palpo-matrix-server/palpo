use std::borrow::Cow;
use std::fmt::Write;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use chksum::sha2_256;
use fast32::base32::CROCKFORD;

#[derive(Debug)]
pub struct Checksum(Vec<u8>);
impl Checksum {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }
    pub fn to_base32_crockford(&self) -> String {
        CROCKFORD.encode(self.as_bytes())
    }
    pub fn to_hex_uppercase(&self) -> String {
        let mut result = String::with_capacity(self.0.len() * 2);
        for b in &self.0 {
            write!(result, "{:02X}", b).unwrap();
        }
        result
    }
}

pub fn hash_file_sha2_256(file: impl AsRef<Path>) -> Result<Checksum, std::io::Error> {
    // let mut file = File::open(file.as_ref())?;
    let chksum = sha2_256::chksum(file.as_ref()).unwrap();
    Ok(Checksum(chksum.as_bytes().to_vec()))
}
pub fn hash_str_sha2_256(value: impl AsRef<str>) -> Result<Checksum, std::io::Error> {
    let bytes = value.as_ref().as_bytes();
    Ok(Checksum(bytes.to_vec()))
}

//https://docs.rs/crate/checksums/0.6.0/source/src/hashing/mod.rs
pub fn hash_string(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(result, "{:02X}", b).unwrap();
    }
    result
}

/// Calculate a new has h for the given password
pub fn hash_password(password: &str) -> Result<String, argon2::Error> {
    let hashing_config = argon2::Config {
        variant: argon2::Variant::Argon2id,
        ..Default::default()
    };

    let salt = super::random_string(32);
    argon2::hash_encoded(password.as_bytes(), salt.as_bytes(), &hashing_config)
}

#[tracing::instrument(skip(keys))]
pub fn hash_keys(keys: &[&[u8]]) -> Vec<u8> {
    // We only hash the pdu's event ids, not the whole pdu
    let bytes = keys.join(&0xff);
    sha2_256::chksum(bytes).unwrap().as_bytes().to_owned()
}
