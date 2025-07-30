use std::ffi::OsStr;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::AppResult;

pub fn is_safe_dir_path(dir_path: &str) -> bool {
    !dir_path.contains('.')
        && !dir_path.contains(':')
        && !dir_path.contains('\\')
        && !dir_path.starts_with('/')
}
pub struct TempPath(String);
impl TempPath {
    pub fn new(path: impl Into<String>) -> Self {
        TempPath(path.into())
    }
}
impl AsRef<str> for TempPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
impl Drop for TempPath {
    fn drop(&mut self) {
        ::std::fs::remove_dir_all(&self.0).ok();
    }
}
fn file_name_sanitized(file_name: &str) -> ::std::path::PathBuf {
    let no_null_filename = match file_name.find('\0') {
        Some(index) => &file_name[0..index],
        None => file_name,
    }
    .to_string();

    // zip files can contain both / and \ as separators regardless of the OS
    // and as we want to return a sanitized PathBuf that only supports the
    // OS separator let's convert incompatible separators to compatible ones
    let separator = ::std::path::MAIN_SEPARATOR;
    let opposite_separator = match separator {
        '/' => '\\',
        _ => '/',
    };
    let filename =
        no_null_filename.replace(&opposite_separator.to_string(), &separator.to_string());

    ::std::path::Path::new(&filename)
        .components()
        .filter(|component| matches!(*component, ::std::path::Component::Normal(..)))
        .fold(::std::path::PathBuf::new(), |mut path, ref cur| {
            path.push(cur.as_os_str());
            path
        })
}

pub fn get_file_ext<P: AsRef<Path>>(path: P) -> String {
    path.as_ref()
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_lowercase()
}

pub fn read_json<T: DeserializeOwned, P: AsRef<Path>>(path: P) -> AppResult<T> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);
    Ok(serde_json::from_reader::<_, T>(reader)?)
}

pub fn write_json<P: AsRef<Path>, C: Serialize>(
    path: P,
    contents: C,
    pretty: bool,
) -> AppResult<()> {
    std::fs::create_dir_all(get_parent_dir(path.as_ref()))?;
    if pretty {
        std::fs::write(path, serde_json::to_vec_pretty(&contents)?)?;
    } else {
        std::fs::write(path, serde_json::to_vec(&contents)?)?;
    }
    Ok(())
}

pub fn write_text<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> AppResult<()> {
    std::fs::create_dir_all(get_parent_dir(path.as_ref()))?;
    std::fs::write(path, contents)?;
    Ok(())
}

pub fn get_parent_dir<T>(path: T) -> PathBuf
where
    T: AsRef<Path>,
{
    let mut parent_dir = path.as_ref().to_owned();
    parent_dir.pop();
    parent_dir
}

pub fn is_image_ext(ext: &str) -> bool {
    ["gif", "jpg", "jpeg", "webp", "avif", "png", "svg"].contains(&ext)
}
pub fn is_video_ext(ext: &str) -> bool {
    ["mp4", "mov", "avi", "wmv", "webm"].contains(&ext)
}
pub fn is_audio_ext(ext: &str) -> bool {
    ["mp3", "flac", "wav", "aac", "ogg", "alac", "wma", "m4a"].contains(&ext)
}
pub fn is_font_ext(ext: &str) -> bool {
    ["ttf", "otf", "woff", "woff2"].contains(&ext)
}
