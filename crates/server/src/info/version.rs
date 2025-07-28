//! one true function for returning the application version with the necessary
//! PALPO_VERSION_EXTRA env variables used if specified
//!
//! Set the environment variable `PALPO_VERSION_EXTRA` to any UTF-8 string
//! to include it in parenthesis after the SemVer version. A common value are
//! git commit hashes.

use std::sync::OnceLock;

static BRANDING: &str = "Tuwunel";
static SEMANTIC: &str = env!("CARGO_PKG_VERSION");
crate::macros::git_commit! {}
crate::macros::git_semantic! {}

static VERSION: OnceLock<String> = OnceLock::new();
static USER_AGENT: OnceLock<String> = OnceLock::new();

#[inline]
#[must_use]
pub fn name() -> &'static str {
    BRANDING
}

#[inline]
pub fn version() -> &'static str {
    VERSION.get_or_init(|| {
        option_env!("PALPO_VERSION_EXTRA")
            .map_or_else(detailed, |extra| {
                extra
                    .is_empty()
                    .then(detailed)
                    .unwrap_or_else(|| format!("{} ({extra})", detailed()))
            })
    })
}

#[inline]
pub fn user_agent() -> &'static str {
    USER_AGENT.get_or_init(|| format!("{}/{}", name(), semantic()))
}
fn detailed() -> String {
    let tag_dirty = semantic().rsplit_once('-').is_some_and(|(_, s)| !s.is_empty());

    if !GIT_COMMIT.is_empty() && tag_dirty {
        format!("{} ({})", semantic(), GIT_COMMIT)
    } else {
        semantic().to_owned()
    }
}

fn semantic() -> &'static str {
    if !GIT_SEMANTIC.is_empty() {
        GIT_SEMANTIC
    } else {
        SEMANTIC
    }
}
