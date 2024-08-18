//! (De)serializable types for the [Matrix Identity Service API][identity-api].
//! These types can be shared by client and identity service code.
//!
//! [identity-api]: https://spec.matrix.org/latest/identity-service-api/

use std::fmt;

pub mod association;
pub mod authentication;
pub mod discovery;
pub mod invitation;
pub mod keys;
pub mod lookup;
pub mod tos;
