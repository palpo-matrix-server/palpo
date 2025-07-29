//! Convenience methods and types to sanitize HTML messages.

use crate::html::{Html, HtmlSanitizerMode, SanitizerConfig};

/// Sanitize the given HTML string.
///
/// This removes the [tags and attributes] that are not listed in the Matrix specification.
///
/// It can also optionally remove the [rich reply] fallback.
///
/// [tags and attributes]: https://spec.matrix.org/latest/client-server-api/#mroommessage-msgtypes
/// [rich reply]: https://spec.matrix.org/latest/client-server-api/#rich-replies
pub fn sanitize_html(
    s: &str,
    mode: HtmlSanitizerMode,
    remove_reply_fallback: RemoveReplyFallback,
) -> String {
    let mut conf = match mode {
        HtmlSanitizerMode::Strict => SanitizerConfig::strict(),
        HtmlSanitizerMode::Compat => SanitizerConfig::compat(),
    };

    if remove_reply_fallback == RemoveReplyFallback::Yes {
        conf = conf.remove_reply_fallback();
    }

    sanitize_inner(s, &conf)
}

/// Whether to remove the [rich reply] fallback while sanitizing.
///
/// [rich reply]: https://spec.matrix.org/latest/client-server-api/#rich-replies
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::exhaustive_enums)]
pub enum RemoveReplyFallback {
    /// Remove the rich reply fallback.
    Yes,

    /// Don't remove the rich reply fallback.
    No,
}

/// Remove the [rich reply] fallback of the given HTML string.
///
/// Due to the fact that the HTML is parsed, note that malformed HTML and comments will be stripped
/// from the output.
///
/// [rich reply]: https://spec.matrix.org/latest/client-server-api/#rich-replies
pub fn remove_html_reply_fallback(s: &str) -> String {
    let conf = SanitizerConfig::new().remove_reply_fallback();
    sanitize_inner(s, &conf)
}

fn sanitize_inner(s: &str, conf: &SanitizerConfig) -> String {
    let html = Html::parse(s);
    html.sanitize_with(conf);
    html.to_string()
}
