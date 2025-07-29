use super::Level;

/// @returns (Foreground, Background)
#[inline]
#[must_use]
pub fn html(level: &Level) -> (&'static str, &'static str) {
	match *level {
		| Level::TRACE => ("#000000", "#A0A0A0"),
		| Level::DEBUG => ("#000000", "#FFFFFF"),
		| Level::ERROR => ("#000000", "#FF0000"),
		| Level::WARN => ("#000000", "#FFFF00"),
		| Level::INFO => ("#FFFFFF", "#008E00"),
	}
}

/// @returns (Foreground)
#[inline]
#[must_use]
pub fn code_tag(level: &Level) -> &'static str {
	match *level {
		| Level::TRACE => "#888888",
		| Level::DEBUG => "#C8C8C8",
		| Level::ERROR => "#FF0000",
		| Level::WARN => "#FFFF00",
		| Level::INFO => "#00FF00",
	}
}
