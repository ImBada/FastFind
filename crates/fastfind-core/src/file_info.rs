//! Port of `FileInfo.swift`.

use crate::text::{nfc, replacing_path_colon, to_half_width_jpn};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

static CONVERT_FULL_WIDTH: AtomicBool = AtomicBool::new(false);

/// `SearchEngine.isConvertFullWidth` — affects how new `FileInfo`s normalise their filename.
pub fn set_convert_full_width(value: bool) {
    CONVERT_FULL_WIDTH.store(value, Ordering::Relaxed);
}

pub fn convert_full_width() -> bool {
    CONVERT_FULL_WIDTH.load(Ordering::Relaxed)
}

#[derive(Debug, Clone)]
pub struct FileInfo {
    /// NFC-normalised path; the identity of the item (`FileInfo.id` / `filePath`).
    pub path: Arc<str>,
    /// The path exactly as the file system returned it (used for file operations).
    pub os_path: PathBuf,
    pub filename: String,
    pub filename_lower: String,
    pub parent: String,
    pub parent_lower: String,
    pub grandparent: String,
    pub grandparent_lower: String,
    pub size: u64,
    pub is_dir: bool,
    /// The item is a symlink (recorded as itself, never followed).
    pub is_symlink: bool,
}

impl PartialEq for FileInfo {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}
impl Eq for FileInfo {}

impl std::hash::Hash for FileInfo {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.path.hash(state)
    }
}

/// `URL.lastPathComponent` semantics for the components we need ("/" for the root).
fn last_component(p: Option<&Path>) -> String {
    p.and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/".to_string())
}

/// A path component as shown and matched: `:` as Finder shows it, half-width when enabled, NFC.
fn display_name(component: &str, convert: bool) -> String {
    let cleaned = replacing_path_colon(component);
    if convert {
        nfc(&to_half_width_jpn(&cleaned))
    } else {
        nfc(&cleaned)
    }
}

impl FileInfo {
    pub fn new(os_path: PathBuf, size: u64, is_dir: bool) -> Self {
        Self::with_conversion(os_path, size, is_dir, convert_full_width())
    }

    /// [`FileInfo::new`] with the full-width conversion given rather than the app-wide setting.
    pub fn with_conversion(os_path: PathBuf, size: u64, is_dir: bool, convert: bool) -> Self {
        // Swift converted only the file name, so a query (always converted) could never match a
        // full-width folder name in folder mode; the folder names follow the same rule here.
        let filename = display_name(&last_component(Some(&os_path)), convert);
        let parent_dir = os_path.parent();
        let parent = display_name(&last_component(parent_dir), convert);
        let grandparent = display_name(&last_component(parent_dir.and_then(|p| p.parent())), convert);
        let path: Arc<str> = nfc(&os_path.to_string_lossy()).into();
        FileInfo {
            path,
            filename_lower: filename.to_lowercase(),
            filename,
            parent_lower: parent.to_lowercase(),
            parent,
            grandparent_lower: grandparent.to_lowercase(),
            grandparent,
            size: if is_dir { 0 } else { size },
            is_dir,
            is_symlink: false,
            os_path,
        }
    }

    /// `FileInfo(filePath:fileURL:)` — lstat, as the index walk: a symlink is recorded as itself,
    /// its size the link's own rather than its target's.
    pub fn from_disk(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let meta = std::fs::symlink_metadata(&path).ok();
        let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
        let is_symlink = meta.as_ref().is_some_and(|m| m.file_type().is_symlink());
        let size = meta.as_ref().map_or(0, |m| m.len());
        FileInfo {
            is_symlink,
            ..FileInfo::new(path, size, is_dir)
        }
    }

    /// The real file name on disk (`fileURL.lastPathComponent`, NFC).
    pub fn disk_name(&self) -> String {
        nfc(&last_component(Some(&self.os_path)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        let fi = FileInfo::new("/Volumes/X/여신 | Megami/a:b.mp4".into(), 10, false);
        assert_eq!(fi.filename, "a/b.mp4");
        assert_eq!(fi.parent, "여신 | Megami");
        assert_eq!(fi.grandparent, "X");
        assert_eq!(fi.size, 10);
        let root = FileInfo::new("/a".into(), 1, false);
        assert_eq!(root.parent, "/");
        assert_eq!(root.grandparent, "/");
    }

    #[test]
    fn folder_names_follow_full_width_conversion() {
        let fi = FileInfo::with_conversion("/ＲＯＯＴ/ＡＢＣ/ｘ.mp4".into(), 1, false, true);
        assert_eq!(fi.filename_lower, "x.mp4");
        assert_eq!(fi.parent_lower, "abc");
        assert_eq!(fi.grandparent_lower, "root");
        let raw = FileInfo::with_conversion("/ＲＯＯＴ/ＡＢＣ/ｘ.mp4".into(), 1, false, false);
        assert_eq!(raw.parent, "ＡＢＣ");
    }

    #[test]
    fn nfc_identity() {
        let decomposed = "/x/\u{1100}\u{1161}.txt"; // 가 in NFD
        let fi = FileInfo::new(decomposed.into(), 0, false);
        assert_eq!(&*fi.path, "/x/가.txt");
        assert_eq!(fi.filename, "가.txt");
    }
}
