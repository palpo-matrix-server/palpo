//! Information about the project. This module contains version, build, system,
//! etc information which can be queried by admins or used by developers.

pub mod cargo;
pub mod rustc;
pub mod version;

pub use version::version;