//! File operations (`FileOps`, `FS`, `VJDeck` in Tools.swift). Everything here may run off the main
//! thread except where noted; the index is kept in sync the same way the Swift cache was.

use crate::platform;
use fastfind_core::{FileInfo, Index};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// File operations under way (drops, renames, moves to the Trash): an update waits for none before
/// it replaces and quits the app, which would cut a copy short.
static BUSY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Held for the length of a file operation; see [`busy`].
pub struct Busy(());

impl Busy {
    pub fn start() -> Self {
        BUSY.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Busy(())
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A file operation is under way.
pub fn busy() -> bool {
    BUSY.load(std::sync::atomic::Ordering::SeqCst) > 0
}

/// `FS.truncatedFilename` — cut a name to `max_bytes` UTF-8 bytes, keeping the extension.
pub fn truncated_filename(filename: &str, max_bytes: usize) -> String {
    if filename.len() <= max_bytes {
        return filename.to_string();
    }
    let (stem, ext) = match Path::new(filename).extension().and_then(|e| e.to_str()) {
        Some(ext) if !ext.is_empty() => (&filename[..filename.len() - ext.len() - 1], format!(".{ext}")),
        _ => (filename, String::new()),
    };
    let available = max_bytes.saturating_sub(ext.len());
    let mut truncated = String::new();
    for c in stem.chars() {
        if truncated.len() + c.len_utf8() > available {
            break;
        }
        truncated.push(c);
    }
    truncated + &ext
}

/// Where `original` goes when renamed to `name` as typed, or `None` when `name` is no file name.
/// A `/` is how Finder (and the results list) shows a `:` on disk, so it is stored as one, as Finder
/// does, instead of moving the item into a subfolder.
pub fn rename_target(original: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    Some(original.with_file_name(name.replace('/', ":")))
}

/// `FileOps.performFindAndReplace` — rename each `(from, to)` pair and update the index.
/// Returns the pairs that were renamed, each with where the item finally is. The deepest items go
/// first, so renaming a folder never strands a pair for something inside it (Swift went in no
/// particular order).
pub fn rename_items(index: &Index, pairs: &[(PathBuf, PathBuf)]) -> Vec<(PathBuf, PathBuf)> {
    let _busy = Busy::start();
    let mut ordered: Vec<&(PathBuf, PathBuf)> = pairs.iter().collect();
    ordered.sort_by_key(|(from, _)| std::cmp::Reverse(from.components().count()));
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (from, to) in ordered {
        if from == to {
            continue;
        }
        let old = FileInfo::from_disk(from);
        match platform::move_item(from, to) {
            Ok(()) => {
                index.update(&old, FileInfo::from_disk(to));
                eprintln!("File renamed to: {}", to.display());
                // What was renamed inside this folder moved along with it.
                for (_, inner) in &mut done {
                    if let Ok(rest) = inner.strip_prefix(from) {
                        *inner = to.join(rest);
                    }
                }
                done.push((from.clone(), to.clone()));
            }
            Err(e) => eprintln!("Error renaming file: {e}"),
        }
    }
    done
}

#[derive(Debug, Clone)]
pub struct CopyJob {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub overwrite: bool,
}

/// A drop that would destroy its own source: replacing a folder the source is inside of (`/r/a/a`
/// onto `/r`), or putting a folder inside itself.
fn destroys_source(source: &Path, destination: &Path) -> bool {
    source != destination && (source.starts_with(destination) || destination.starts_with(source))
}

/// First half of `dropCopyFilesToDirectory`: split dropped paths into new items and ones that
/// would overwrite an existing item in `folder`. Drops that would destroy their source are left out.
pub fn plan_drop(paths: &[PathBuf], folder: &Path) -> (Vec<CopyJob>, Vec<CopyJob>) {
    let mut fresh = Vec::new();
    let mut existing = Vec::new();
    for source in paths {
        let Some(name) = source.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let name = if name.len() > 255 {
            let cut = truncated_filename(&name, 255);
            eprintln!("Filename truncated for destination: {name} -> {cut}");
            cut
        } else {
            name
        };
        let destination = folder.join(name);
        if destroys_source(source, &destination) {
            eprintln!("Refused: {} cannot go to {}", source.display(), destination.display());
            continue;
        }
        if destination.symlink_metadata().is_ok() {
            existing.push(CopyJob {
                source: source.clone(),
                destination,
                overwrite: true,
            });
        } else {
            fresh.push(CopyJob {
                source: source.clone(),
                destination,
                overwrite: false,
            });
        }
    }
    (fresh, existing)
}

/// The names listed in the overwrite alert (identical source/destination pairs excluded).
pub fn overwrite_names(jobs: &[CopyJob]) -> Vec<String> {
    jobs.iter()
        .filter(|j| j.source != j.destination)
        .filter_map(|j| j.source.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect()
}

fn device_of(path: &Path) -> Option<u64> {
    std::fs::symlink_metadata(path).ok().map(|m| m.dev())
}

/// `copyOrMoveFile` — same volume: move; otherwise copy. Keeps the index in sync. An item being
/// replaced goes to the Trash (Swift deleted it outright).
pub fn copy_or_move(index: &Index, job: &CopyJob) {
    let _busy = Busy::start();
    let CopyJob {
        source,
        destination,
        overwrite,
    } = job;
    if source.symlink_metadata().is_err() {
        eprintln!("Error: source file does not exist");
        return;
    }
    if source == destination {
        eprintln!("Error: source and destination are identical");
        return;
    }
    if destroys_source(source, destination) {
        eprintln!("Refused: {} cannot go to {}", source.display(), destination.display());
        return;
    }
    let same_volume = match (device_of(source), destination.parent().and_then(device_of)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    };
    let old = FileInfo::from_disk(source);
    let replacing = *overwrite && destination.symlink_metadata().is_ok();
    // Replacing, the new item lands next to the old one first, so a failed move or copy leaves the
    // old one in place (Swift deleted it before trying).
    let landing = if replacing {
        staging_path(destination)
    } else {
        destination.clone()
    };
    let arrived = if same_volume {
        platform::move_item(source, &landing)
    } else {
        platform::copy_item(source, &landing)
    };
    if let Err(e) = arrived {
        eprintln!("Move/copy failed: {e}");
        return;
    }
    if replacing {
        let replaced = FileInfo::from_disk(destination);
        if let Err(e) = platform::move_to_trash(destination) {
            eprintln!("Move/copy failed: {e}");
            // Undone: the source goes back, or the copy goes away.
            if same_volume {
                if let Err(e) = platform::move_item(&landing, source) {
                    eprintln!("Could not move {} back: {e}", landing.display());
                }
            } else {
                discard(&landing);
            }
            return;
        }
        // Gone from here, and so is everything it held (Swift left those in the index).
        index.remove(&replaced);
        if let Err(e) = platform::move_item(&landing, destination) {
            eprintln!("Move/copy failed: {e} (the new item is at {})", landing.display());
            return;
        }
    }
    if same_volume {
        index.update(&old, FileInfo::from_disk(destination));
    } else {
        index.add(FileInfo::from_disk(destination));
    }
}

/// A hidden name beside `destination` for an item about to replace it.
fn staging_path(destination: &Path) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    destination.with_file_name(format!(".fastfind-replacing-{}-{n}", std::process::id()))
}

/// Deletes a copy this app just made.
fn discard(path: &Path) {
    let removed = match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) => Err(e),
    };
    if let Err(e) = removed {
        eprintln!("Could not remove {}: {e}", path.display());
    }
}

/// `showAlertBeforeMoveToTrash`'s action, after confirmation.
pub fn trash_items(index: &Index, items: &[Arc<FileInfo>]) {
    let _busy = Busy::start();
    for item in items {
        match platform::move_to_trash(&item.os_path) {
            Ok(()) => index.remove(item),
            Err(e) => eprintln!("Failed to move file to trash: {e}"),
        }
    }
}

/// `VJDeck.parseDeckNumbers` — comma separated, only 1…4.
/// Each deck once, in the order given (Swift kept repeats, listing a deck twice).
pub fn parse_deck_numbers(input: &str) -> Vec<u8> {
    let mut decks = Vec::new();
    for deck in input
        .split(',')
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .filter(|n| (1..=4).contains(n))
    {
        if !decks.contains(&(deck as u8)) {
            decks.push(deck as u8);
        }
    }
    decks
}

/// `URLComponents.queryItems` encoding (verified against Foundation).
fn query_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        let keep = b.is_ascii_alphanumeric() || b"!$'()*+,-./:;?@_~".contains(&b);
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn vjdeck_url(deck: u8, file: &str) -> String {
    format!("vjdeck://load?deck={deck}&file={}", query_encode(file))
}

/// A folder of its own under `VJDeckImport` for one temporary link, so links to equally named
/// (or the same) files never replace or delete each other while VJDeck has yet to read them.
fn vjdeck_link_folder() -> Option<PathBuf> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir()
        .join("VJDeckImport")
        .join(format!("{}-{stamp}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// `VJDeck.importFiles` (main thread: opens URLs through NSWorkspace).
pub fn vjdeck_import(items: &[Arc<FileInfo>], deck: u8) {
    let mut link_folders = Vec::new();
    for item in items {
        // The path as stored on disk: a volume keeping names decomposed (NFD) may not resolve the
        // normalised form the list uses.
        let path = item.os_path.to_string_lossy().into_owned();
        let long_component = path.split('/').any(|c| c.len() > 255);
        let mut link_folder: Option<PathBuf> = None;
        let file_path = match long_component.then(vjdeck_link_folder).flatten() {
            Some(dir) => {
                // The raw symlink() syscall bypasses Foundation's NAME_MAX check; exFAT copes with
                // the name.
                let name = item
                    .os_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let link = dir.join(truncated_filename(&name, 255));
                let linked = std::os::unix::fs::symlink(&item.os_path, &link).is_ok();
                link_folder = Some(dir);
                if linked {
                    link.to_string_lossy().into_owned()
                } else {
                    path.to_string()
                }
            }
            None => path.to_string(),
        };
        platform::open_url(&vjdeck_url(deck, &file_path));
        link_folders.extend(link_folder);
    }
    // One cleanup a minute later for the whole import, once VJDeck has read the links.
    if !link_folders.is_empty() {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(60));
            for dir in link_folders {
                let _ = std::fs::remove_dir_all(dir);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_operation_counts_as_busy() {
        let first = Busy::start();
        let second = Busy::start();
        assert!(busy());
        drop(first);
        assert!(busy(), "one is still running");
        drop(second);
    }

    #[test]
    fn rename_targets() {
        let original = Path::new("/r/a.mp4");
        assert_eq!(rename_target(original, "b.mp4"), Some(PathBuf::from("/r/b.mp4")));
        assert_eq!(
            rename_target(original, "AC/DC.mp4"),
            Some(PathBuf::from("/r/AC:DC.mp4"))
        );
        assert_eq!(rename_target(original, ""), None);
        assert_eq!(rename_target(original, "."), None);
        assert_eq!(rename_target(original, ".."), None);
    }

    #[test]
    fn a_folder_and_what_it_holds_are_renamed_together() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("bar")).unwrap();
        std::fs::write(root.join("bar/bar.mp4"), "").unwrap();
        let index = Index::new();
        // The folder comes first, as it does in results sorted by name.
        let pairs = vec![
            (root.join("bar"), root.join("baz")),
            (root.join("bar/bar.mp4"), root.join("bar/baz.mp4")),
        ];
        let done = rename_items(&index, &pairs);
        assert!(root.join("baz/baz.mp4").exists());
        // Reported where each item ended up, inside the renamed folder.
        assert!(done.contains(&(root.join("bar/bar.mp4"), root.join("baz/baz.mp4"))));
        assert!(done.contains(&(root.join("bar"), root.join("baz"))));
    }

    #[test]
    fn drops_that_would_destroy_their_source_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("a/a")).unwrap();
        std::fs::create_dir_all(root.join("x/sub")).unwrap();
        std::fs::write(root.join("f.txt"), "").unwrap();
        std::fs::write(root.join("x/f.txt"), "").unwrap();

        // `/r/a/a` onto `/r` would replace `/r/a`, which holds the source.
        let (fresh, existing) = plan_drop(&[root.join("a/a")], root);
        assert!(fresh.is_empty() && existing.is_empty());
        // A folder dropped into its own subfolder.
        let (fresh, existing) = plan_drop(&[root.join("x")], &root.join("x/sub"));
        assert!(fresh.is_empty() && existing.is_empty());
        // An ordinary replacement is still offered.
        let (fresh, existing) = plan_drop(&[root.join("f.txt")], &root.join("x"));
        assert!(fresh.is_empty());
        assert_eq!(existing.len(), 1);
        assert!(existing[0].overwrite);
        // Dropping an item back into its own folder stays a no-op, not a refusal.
        let (_, existing) = plan_drop(&[root.join("f.txt")], root);
        assert_eq!(existing[0].source, existing[0].destination);
    }

    #[test]
    fn truncation_keeps_extension_and_char_boundaries() {
        let name = format!("{}.mp4", "가".repeat(100)); // 300 bytes + 4
        let cut = truncated_filename(&name, 255);
        assert!(cut.len() <= 255);
        assert!(cut.ends_with(".mp4"));
        assert_eq!(truncated_filename("short.txt", 255), "short.txt");
    }

    #[test]
    fn deck_numbers() {
        assert_eq!(parse_deck_numbers("1, 2,5,x,4"), vec![1, 2, 4]);
        assert_eq!(parse_deck_numbers("2,1,2,1"), vec![2, 1]);
        assert!(parse_deck_numbers("").is_empty());
    }

    #[test]
    fn vjdeck_url_matches_foundation() {
        assert_eq!(
            vjdeck_url(1, "/Volumes/A B/x&y=z+w?#%한글 [a].mp4"),
            "vjdeck://load?deck=1&file=/Volumes/A%20B/x%26y%3Dz+w?%23%25%ED%95%9C%EA%B8%80%20%5Ba%5D.mp4"
        );
        assert_eq!(
            vjdeck_url(2, "a'b!c$d(e)f*g,h;i:j@k~l_m-n.o`p^q{r}s|t\"u<v>w\\x"),
            "vjdeck://load?deck=2&file=a'b!c$d(e)f*g,h;i:j@k~l_m-n.o%60p%5Eq%7Br%7Ds%7Ct%22u%3Cv%3Ew%5Cx"
        );
    }
}
