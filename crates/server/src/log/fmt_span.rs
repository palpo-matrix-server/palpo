use tracing_subscriber::fmt::format::FmtSpan;

use crate::Result;

#[inline]
pub fn from_str(str: &str) -> Result<FmtSpan, FmtSpan> {
	match str.to_uppercase().as_str() {
		| "ENTER" => Ok(FmtSpan::ENTER),
		| "EXIT" => Ok(FmtSpan::EXIT),
		| "NEW" => Ok(FmtSpan::NEW),
		| "CLOSE" => Ok(FmtSpan::CLOSE),
		| "ACTIVE" => Ok(FmtSpan::ACTIVE),
		| "FULL" => Ok(FmtSpan::FULL),
		| "NONE" => Ok(FmtSpan::NONE),
		| _ => Err(FmtSpan::NONE),
	}
}
