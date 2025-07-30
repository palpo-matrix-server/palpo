use super::EMPTY;

type Pair<'a> = (&'a str, &'a str);

/// Split a string with default behaviors on non-match.
pub trait SplitInfallible<'a> {
	/// Split a string at the first occurrence of delim. If not found, the
	/// entire string is returned in \[0\], while \[1\] is empty.
	fn split_once_infallible(&self, delim: &str) -> Pair<'a>;

	/// Split a string from the last occurrence of delim. If not found, the
	/// entire string is returned in \[0\], while \[1\] is empty.
	fn rsplit_once_infallible(&self, delim: &str) -> Pair<'a>;
}

impl<'a> SplitInfallible<'a> for &'a str {
	#[inline]
	fn rsplit_once_infallible(&self, delim: &str) -> Pair<'a> {
		self.rsplit_once(delim).unwrap_or((self, EMPTY))
	}

	#[inline]
	fn split_once_infallible(&self, delim: &str) -> Pair<'a> {
		self.split_once(delim).unwrap_or((self, EMPTY))
	}
}
