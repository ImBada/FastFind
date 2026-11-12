//! In-memory file index (`FileInfoCache` + `SearchEngine.cacheFiles`).

use crate::file_info::FileInfo;
use parking_lot::RwLock;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Batch size for enumerator-based indexing.
pub const INDEXING_BATCH_SIZE: usize = 200;
/// Progress report interval.
pub const PROGRESS_REPORT_INTERVAL: Duration = Duration::from_millis(500);

const UF_HIDDEN: u32 = 0x0000_8000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexError {
    Cancelled,
    /// The folder itself could not be read (no permission, or not there).
    Unreadable,
}

#[derive(Default)]
struct State {
    items: Vec<Arc<FileInfo>>,
    positions: HashMap<Arc<str>, usize>,
    is_cached: bool,
    skipped: Vec<String>,
    /// Bumped by every `clear`: a walk started for an older generation adds nothing.
    generation: u64,
}

impl State {
    fn insert(&mut self, info: Arc<FileInfo>) {
        match self.positions.get(&info.path) {
            Some(&at) => self.items[at] = info,
            None => {
                self.positions.insert(info.path.clone(), self.items.len());
                self.items.push(info);
            }
        }
    }

    fn remove(&mut self, path: &str) -> bool {
        let Some(at) = self.positions.remove(path) else {
            return false;
        };
        self.items.swap_remove(at);
        if let Some(moved) = self.items.get(at) {
            self.positions.insert(moved.path.clone(), at);
        }
        true
    }
}

/// Thread-safe index shared between the UI and background workers.
#[derive(Default)]
pub struct Index {
    state: RwLock<State>,
}

impl Index {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_cached(&self) -> bool {
        self.state.read().is_cached
    }

    pub fn set_cached(&self, cached: bool) {
        self.state.write().is_cached = cached;
    }

    pub fn len(&self) -> usize {
        self.state.read().items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn skipped_directories(&self) -> Vec<String> {
        self.state.read().skipped.clone()
    }

    /// `removeAll()`. Returns the generation to index the emptied index for.
    pub fn clear(&self) -> u64 {
        let mut state = self.state.write();
        state.items.clear();
        state.positions.clear();
        state.skipped.clear();
        state.is_cached = false;
        state.generation += 1;
        state.generation
    }

    pub fn insert_batch(&self, batch: impl IntoIterator<Item = Arc<FileInfo>>) -> usize {
        let mut state = self.state.write();
        for info in batch {
            state.insert(info);
        }
        state.items.len()
    }

    /// Adds a walk's batch unless the index was cleared since that walk began.
    fn insert_walked(&self, generation: u64, batch: &mut Vec<Arc<FileInfo>>) -> Result<usize, IndexError> {
        let mut state = self.state.write();
        if state.generation != generation {
            return Err(IndexError::Cancelled);
        }
        for info in batch.drain(..) {
            state.insert(info);
        }
        Ok(state.items.len())
    }

    pub fn get(&self, path: &str) -> Option<Arc<FileInfo>> {
        let state = self.state.read();
        state.positions.get(path).map(|&at| state.items[at].clone())
    }

    /// Walk `root` (not including it), single-threaded like `FileManager.enumerator`, skipping hidden
    /// items and `*.noindex` folders, never following symlinks. Calls `progress` with the running
    /// total at most every [`PROGRESS_REPORT_INTERVAL`], and marks the index cached when done.
    /// `generation` is what [`Index::clear`] returned: once the index is cleared again, this walk
    /// stops adding to it and ends as cancelled, even if it misses `cancel`.
    pub fn index_directory(
        &self,
        root: &Path,
        generation: u64,
        cancel: &AtomicBool,
        mut progress: impl FnMut(usize),
    ) -> Result<(), IndexError> {
        // No (device, inode) bookkeeping: symlinks are never followed, so the walk cannot loop, and
        // network or FUSE volumes may report clashing or zero inodes for different folders.
        let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
        let mut batch: Vec<Arc<FileInfo>> = Vec::with_capacity(INDEXING_BATCH_SIZE);
        let mut skipped = Vec::new();
        let mut last_report = Instant::now();

        while let Some(dir) = stack.pop() {
            if cancel.load(Ordering::Relaxed) {
                return Err(IndexError::Cancelled);
            }
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                // Swift went on to report an empty folder as indexed.
                Err(_) if dir == root => return Err(IndexError::Unreadable),
                Err(_) => {
                    skipped.push(dir.to_string_lossy().into_owned());
                    continue;
                }
            };
            let mut subdirs = Vec::new();
            for entry in entries.flatten() {
                if cancel.load(Ordering::Relaxed) {
                    return Err(IndexError::Cancelled);
                }
                let name = entry.file_name();
                // Dot-names are left out before the lstat below; `left_out` repeats this check for
                // `add`, which has no walk around it.
                if name.as_encoded_bytes().first() == Some(&b'.') {
                    continue;
                }
                // lstat: a symlink is recorded as itself and never descended into.
                let Ok(meta) = entry.metadata() else {
                    skipped.push(entry.path().to_string_lossy().into_owned());
                    continue;
                };
                if left_out(&name, &meta) {
                    continue;
                }
                let path = entry.path();
                let is_dir = meta.is_dir();
                if is_dir {
                    subdirs.push(path.clone());
                }
                let is_symlink = meta.file_type().is_symlink();
                batch.push(Arc::new(FileInfo {
                    is_symlink,
                    ..FileInfo::new(path, meta.len(), is_dir)
                }));
                if batch.len() >= INDEXING_BATCH_SIZE {
                    let count = self.insert_walked(generation, &mut batch)?;
                    if last_report.elapsed() >= PROGRESS_REPORT_INTERVAL {
                        progress(count);
                        last_report = Instant::now();
                    }
                }
            }
            // Pre-order depth first, in directory order.
            stack.extend(subdirs.into_iter().rev());
        }
        let count = self.insert_walked(generation, &mut batch)?;
        progress(count);
        let mut state = self.state.write();
        if state.generation != generation {
            return Err(IndexError::Cancelled);
        }
        state.skipped.extend(skipped);
        state.is_cached = true;
        Ok(())
    }

    /// `SearchEngine.addCache` — no-op until the first index completes.
    /// An item the walk would leave out (renamed to a dot-name or `*.noindex`, …) stays out.
    pub fn add(&self, info: FileInfo) {
        if let (Some(name), Ok(meta)) = (info.os_path.file_name(), fs::symlink_metadata(&info.os_path)) {
            if left_out(name, &meta) {
                return;
            }
        }
        let generation = {
            let state = self.state.read();
            if !state.is_cached {
                return;
            }
            state.generation
        };
        if info.is_dir {
            let never = AtomicBool::new(false);
            let _ = self.index_directory(&info.os_path, generation, &never, |_| {});
        }
        let _ = self.insert_walked(generation, &mut vec![Arc::new(info)]);
    }

    /// `SearchEngine.removeCache` — removes the item and, for a folder, everything beneath it.
    pub fn remove(&self, info: &FileInfo) {
        if !self.is_cached() {
            return;
        }
        let mut state = self.state.write();
        if info.is_dir {
            let prefix = format!("{}/", info.path);
            let doomed: Vec<Arc<str>> = state
                .items
                .iter()
                .filter(|item| item.path.starts_with(&prefix))
                .map(|item| item.path.clone())
                .collect();
            for path in doomed {
                state.remove(&path);
            }
        }
        state.remove(&info.path);
    }

    /// `SearchEngine.updateCache`.
    pub fn update(&self, old: &FileInfo, new: FileInfo) {
        self.remove(old);
        self.add(new);
    }

    /// Full scan (`searchCacheFilter`). `lowered` must already be lower-cased.
    pub fn search(&self, lowered: &[String], by_folder: bool, by_folder_double: bool) -> Vec<Arc<FileInfo>> {
        let state = self.state.read();
        if !state.is_cached {
            return Vec::new();
        }
        filter_items(&state.items, lowered, by_folder, by_folder_double)
    }
}

/// What indexing leaves out: dot-names, items flagged hidden, and `*.noindex` folders.
fn left_out(name: &std::ffi::OsStr, meta: &fs::Metadata) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
        || meta.st_flags_hidden()
        || (meta.is_dir() && name.to_string_lossy().to_lowercase().ends_with(".noindex"))
}

trait HiddenFlag {
    fn st_flags_hidden(&self) -> bool;
}

impl HiddenFlag for fs::Metadata {
    fn st_flags_hidden(&self) -> bool {
        use std::os::macos::fs::MetadataExt as _;
        self.st_flags() & UF_HIDDEN != 0
    }
}

/// The text a query is matched against: the file name, or `[grandparent/]parent/name` in folder mode.
pub fn matches(info: &FileInfo, lowered: &[String], by_folder: bool, by_folder_double: bool, buf: &mut String) -> bool {
    let name: &str = if !by_folder {
        &info.filename_lower
    } else {
        buf.clear();
        if by_folder_double {
            buf.push_str(&info.grandparent_lower);
            buf.push('/');
        }
        buf.push_str(&info.parent_lower);
        buf.push('/');
        buf.push_str(&info.filename_lower);
        buf
    };
    lowered.iter().all(|s| name.contains(s.as_str()))
}

pub fn filter_items(
    items: &[Arc<FileInfo>],
    lowered: &[String],
    by_folder: bool,
    by_folder_double: bool,
) -> Vec<Arc<FileInfo>> {
    items
        .par_iter()
        .with_min_len(4096)
        .map_init(String::new, |buf, item| {
            matches(item, lowered, by_folder, by_folder_double, buf).then(|| item.clone())
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    #[test]
    fn walk_skips_hidden_noindex_and_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(&root.join("real/sub/f.txt"));
        touch(&root.join("real/a.txt"));
        touch(&root.join(".dot/x"));
        touch(&root.join("pkg.NoIndex/in/z"));
        touch(&root.join("hiddenflag"));
        assert!(Command::new("chflags")
            .arg("hidden")
            .arg(root.join("hiddenflag"))
            .status()
            .unwrap()
            .success());
        std::os::unix::fs::symlink("real", root.join("linkdir")).unwrap();
        std::os::unix::fs::symlink("..", root.join("real/loop")).unwrap();

        let index = Index::new();
        let never = AtomicBool::new(false);
        let generation = index.clear();
        index.index_directory(root, generation, &never, |_| {}).unwrap();
        assert!(index.is_cached());

        let mut names: Vec<String> = index.state.read().items.iter().map(|i| i.filename.clone()).collect();
        names.sort();
        assert_eq!(names, ["a.txt", "f.txt", "linkdir", "loop", "real", "sub"]);
        let link = index.search(&["linkdir".into()], false, false);
        assert!(!link[0].is_dir);
        assert!(link[0].is_symlink);
        assert!(!index.search(&["a.txt".into()], false, false)[0].is_symlink);

        // Folder-mode matching sees "parent/name".
        assert_eq!(index.search(&["sub/f".into()], true, false).len(), 1);
        assert_eq!(index.search(&["real/sub/f".into()], true, true).len(), 1);
        assert_eq!(index.search(&["real/sub/f".into()], true, false).len(), 0);

        // Removing a folder removes its contents.
        let real = index.get(&FileInfo::new(root.join("real"), 0, true).path).unwrap();
        index.remove(&real);
        let mut names: Vec<String> = index.state.read().items.iter().map(|i| i.filename.clone()).collect();
        names.sort();
        assert_eq!(names, ["linkdir"]);
    }

    #[test]
    fn an_unreadable_folder_is_an_error() {
        let index = Index::new();
        let never = AtomicBool::new(false);
        let generation = index.clear();
        let missing = Path::new("/nonexistent/fastfind-test");
        assert_eq!(
            index.index_directory(missing, generation, &never, |_| {}),
            Err(IndexError::Unreadable)
        );
        assert!(!index.is_cached());
    }

    #[test]
    fn cancel_stops_walk() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("a/b"));
        let index = Index::new();
        let cancel = AtomicBool::new(true);
        let generation = index.clear();
        assert_eq!(
            index.index_directory(tmp.path(), generation, &cancel, |_| {}),
            Err(IndexError::Cancelled)
        );
    }

    #[test]
    fn a_walk_from_before_a_clear_adds_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("old/file.txt"));
        let index = Index::new();
        let never = AtomicBool::new(false);
        let stale = index.clear();
        let current = index.clear();
        // The old walk finishing late neither adds its files nor marks the new index cached.
        assert_eq!(
            index.index_directory(tmp.path(), stale, &never, |_| {}),
            Err(IndexError::Cancelled)
        );
        assert!(index.is_empty());
        assert!(!index.is_cached());
        index.index_directory(tmp.path(), current, &never, |_| {}).unwrap();
        assert_eq!(index.len(), 2);
        assert!(index.is_cached());
    }

    #[test]
    fn added_items_follow_the_walk_rules() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(&root.join(".stuff/a.txt"));
        touch(&root.join("stuff.noindex/b.txt"));
        touch(&root.join("stuff/c.txt"));
        let index = Index::new();
        index.set_cached(true);
        // As after renaming folders to these names, or dropping them in.
        for name in [".stuff", "stuff.noindex", "stuff"] {
            index.add(FileInfo::from_disk(root.join(name)));
        }
        let mut names: Vec<String> = index.state.read().items.iter().map(|i| i.filename.clone()).collect();
        names.sort();
        assert_eq!(names, ["c.txt", "stuff"]);
    }

    #[test]
    fn search_requires_completed_index() {
        let index = Index::new();
        index.insert_batch([Arc::new(FileInfo::new("/x/abc".into(), 1, false))]);
        assert!(index.search(&["abc".into()], false, false).is_empty());
        index.set_cached(true);
        assert_eq!(index.search(&["abc".into()], false, false).len(), 1);
    }
}
