use std::fmt::Write;

use super::{Level, color};
use crate::Result;

pub fn html<S>(out: &mut S, level: &Level, span: &str, msg: &str) -> Result
where
	S: Write + ?Sized,
{
	let color = color::code_tag(level);
	let level = level.as_str().to_uppercase();
	write!(
		out,
		"<font data-mx-color=\"{color}\"><code>{level:>5}</code></font> <code>{span:^12}</code> \
		 <code>{msg}</code><br>"
	)?;

	Ok(())
}

pub fn markdown<S>(out: &mut S, level: &Level, span: &str, msg: &str) -> Result
where
	S: Write + ?Sized,
{
	let level = level.as_str().to_uppercase();
	writeln!(out, "`{level:>5}` `{span:^12}` `{msg}`")?;

	Ok(())
}

pub fn markdown_table<S>(out: &mut S, level: &Level, span: &str, msg: &str) -> Result
where
	S: Write + ?Sized,
{
	let level = level.as_str().to_uppercase();
	writeln!(out, "| {level:>5} | {span:^12} | {msg} |")?;

	Ok(())
}

pub fn markdown_table_head<S>(out: &mut S) -> Result
where
	S: Write + ?Sized,
{
	write!(out, "| level | span | message |\n| ------: | :-----: | :------- |\n")?;

	Ok(())
}
