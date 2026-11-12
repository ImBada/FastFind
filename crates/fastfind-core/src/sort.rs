//! Sorting (`SearchUtils.sortFileInfo` and the `[FileInfo]` sort extensions).

use crate::file_info::FileInfo;
use rayon::prelude::*;
use std::cmp::Ordering;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortOption {
    /// "A-Z"
    Name = 0,
    /// "File-Folder [A-Z]"
    FileFolder = 1,
    /// "File-Folder [Z-A]"
    FileFolderDesc = 2,
    /// "Size [Folder bottom]"
    Size = 3,
    /// "A-Z w/ Folder-File"
    Path = 4,
    /// "NONE"
    None = 5,
}

impl SortOption {
    pub const ALL: [SortOption; 6] = [
        SortOption::Name,
        SortOption::FileFolder,
        SortOption::FileFolderDesc,
        SortOption::Size,
        SortOption::Path,
        SortOption::None,
    ];

    pub fn from_index(i: i64) -> SortOption {
        match i {
            1 => SortOption::FileFolder,
            2 => SortOption::FileFolderDesc,
            3 => SortOption::Size,
            4 => SortOption::Path,
            5 => SortOption::None,
            _ => SortOption::Name,
        }
    }

    /// Localization key (as used in `SortManagerView`).
    pub fn label_key(self) -> &'static str {
        match self {
            SortOption::Name => "A-Z",
            SortOption::FileFolder => "File-Folder [A-Z]",
            SortOption::FileFolderDesc => "File-Folder [Z-A]",
            SortOption::Size => "Size [Folder bottom]",
            SortOption::Path => "A-Z w/ Folder-File",
            SortOption::None => "NONE",
        }
    }
}

fn path_cmp(a: &FileInfo, b: &FileInfo) -> Ordering {
    // Directories get an extra empty component so they sort before their own contents.
    let mut ia = a.path.split('/').chain(a.is_dir.then_some(""));
    let mut ib = b.path.split('/').chain(b.is_dir.then_some(""));
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => match x.cmp(y) {
                Ordering::Equal => continue,
                o => return o,
            },
        }
    }
}

/// Sort in place. Ties are broken by path so the order is deterministic (Swift's was hash-order dependent).
pub fn sort_file_infos(list: &mut [Arc<FileInfo>], option: SortOption, reverse: bool) {
    let tie = |a: &FileInfo, b: &FileInfo| a.path.cmp(&b.path);
    match option {
        SortOption::Name => list.par_sort_by(|a, b| {
            let o = a.filename.cmp(&b.filename);
            (if reverse { o.reverse() } else { o }).then_with(|| tie(a, b))
        }),
        SortOption::FileFolder | SortOption::FileFolderDesc => {
            let name_desc = option == SortOption::FileFolderDesc;
            list.par_sort_by(|a, b| {
                if a.is_dir != b.is_dir {
                    // Files first; reversed puts folders first.
                    let files_first = if a.is_dir { Ordering::Greater } else { Ordering::Less };
                    return if reverse { files_first.reverse() } else { files_first };
                }
                let o = a.filename.cmp(&b.filename);
                (if name_desc { o.reverse() } else { o }).then_with(|| tie(a, b))
            })
        }
        SortOption::Size => list.par_sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                // Folders always at the bottom.
                return if a.is_dir { Ordering::Greater } else { Ordering::Less };
            }
            let o = b.size.cmp(&a.size);
            (if reverse { o.reverse() } else { o })
                .then_with(|| a.filename.cmp(&b.filename))
                .then_with(|| tie(a, b))
        }),
        SortOption::Path => list.par_sort_by(|a, b| {
            let o = path_cmp(a, b);
            if reverse {
                o.reverse()
            } else {
                o
            }
        }),
        SortOption::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fi(p: &str, size: u64, dir: bool) -> Arc<FileInfo> {
        Arc::new(FileInfo::new(p.into(), size, dir))
    }

    fn names(v: &[Arc<FileInfo>]) -> Vec<&str> {
        v.iter().map(|f| &*f.path).collect()
    }

    #[test]
    fn sorts() {
        let base = vec![
            fi("/r/b", 5, false),
            fi("/r/a", 1, true),
            fi("/r/a/c", 9, false),
            fi("/r/d", 7, false),
        ];

        let mut v = base.clone();
        sort_file_infos(&mut v, SortOption::Name, false);
        assert_eq!(names(&v), ["/r/a", "/r/b", "/r/a/c", "/r/d"]);

        let mut v = base.clone();
        sort_file_infos(&mut v, SortOption::FileFolder, false);
        assert_eq!(names(&v), ["/r/b", "/r/a/c", "/r/d", "/r/a"]);
        sort_file_infos(&mut v, SortOption::FileFolder, true);
        assert_eq!(names(&v), ["/r/a", "/r/b", "/r/a/c", "/r/d"]);
        sort_file_infos(&mut v, SortOption::FileFolderDesc, false);
        assert_eq!(names(&v), ["/r/d", "/r/a/c", "/r/b", "/r/a"]);

        let mut v = base.clone();
        sort_file_infos(&mut v, SortOption::Size, false);
        assert_eq!(names(&v), ["/r/a/c", "/r/d", "/r/b", "/r/a"]);
        sort_file_infos(&mut v, SortOption::Size, true);
        assert_eq!(names(&v), ["/r/b", "/r/d", "/r/a/c", "/r/a"]);

        let mut v = base.clone();
        sort_file_infos(&mut v, SortOption::Path, false);
        assert_eq!(names(&v), ["/r/a", "/r/a/c", "/r/b", "/r/d"]);
        sort_file_infos(&mut v, SortOption::Path, true);
        assert_eq!(names(&v), ["/r/d", "/r/b", "/r/a/c", "/r/a"]);
    }
}
