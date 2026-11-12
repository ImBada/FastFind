//! Query planning and incremental search (`SearchManager` + `SearchHandler`).

use crate::file_info::FileInfo;
use crate::index::{filter_items, Index};
use crate::query::{extract_search_strings, is_search_string_just_added};
use crate::sort::{sort_file_infos, SortOption};
use crate::text::{nfc, to_half_width_jpn, trim_ws};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Same query as the results on screen and not forced: nothing to do.
    Unchanged,
    /// Empty query: clear the results.
    Clear,
    /// Run a search for these tokens. `query` is the normalised text, recorded once shown.
    Search { query: String, strings: Vec<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOptions {
    pub by_folder: bool,
    pub by_folder_double: bool,
    pub sort: SortOption,
    pub reverse: bool,
    /// A forced search (after a file operation or an option change): it searches the index even when
    /// the query only narrows the one shown, since the results on screen may be out of date.
    pub fresh: bool,
}

/// What the results on screen came from: `SearchManager.lastSearchString` plus the narrowing basis
/// `SearchHandler` kept. Swift recorded these as a search started; here they are recorded only once
/// its results are shown, so a superseded or cancelled search leaves no trace, and each window
/// keeps its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shown {
    /// The trimmed (and, if enabled, half-width) text.
    pub query: String,
    pub strings: Vec<String>,
    /// Folder mode (and its depth) and folder of the full search these results descend from.
    pub by_folder: bool,
    pub by_folder_double: bool,
    pub directory: Option<PathBuf>,
}

impl Shown {
    /// Nothing on screen.
    pub fn none() -> Self {
        Self::default()
    }
}

/// `SearchManager.performSearch` up to the point where work is scheduled.
pub fn plan(input: &str, convert_full_width: bool, force: bool, shown: &Shown) -> Plan {
    let trimmed = trim_ws(input);
    let trimmed = if convert_full_width {
        to_half_width_jpn(trimmed)
    } else {
        trimmed.to_string()
    };
    if !force && shown.query == trimmed {
        return Plan::Unchanged;
    }
    if trimmed.is_empty() {
        return Plan::Clear;
    }
    let strings = extract_search_strings(&nfc(&trimmed));
    Plan::Search {
        query: trimmed,
        strings,
    }
}

/// `SearchHandler.searchAsync` followed by sorting. When the query only narrows the one on screen
/// (`shown`), `current` is filtered instead of the index. Returns the results and what they came
/// from.
pub fn run(
    index: &Index,
    query: String,
    strings: Vec<String>,
    options: SearchOptions,
    directory: Option<&Path>,
    current: &[Arc<FileInfo>],
    shown: &Shown,
) -> (Vec<Arc<FileInfo>>, Shown) {
    let lowered: Vec<String> = strings.iter().map(|s| s.to_lowercase()).collect();
    // Swift compared folder mode but not its depth: results matched one folder deep could then be
    // narrowed as if they had been matched two deep.
    let narrowing = !options.fresh
        && !strings.is_empty()
        && is_search_string_just_added(&shown.strings, &strings)
        && shown.by_folder == options.by_folder
        && shown.by_folder_double == options.by_folder_double
        && shown.directory.as_deref() == directory;
    let mut results = if narrowing {
        filter_items(current, &lowered, options.by_folder, options.by_folder_double)
    } else if strings.is_empty() || directory.is_none() {
        Vec::new()
    } else {
        index.search(&lowered, options.by_folder, options.by_folder_double)
    };
    sort_file_infos(&mut results, options.sort, options.reverse);
    let shown = Shown {
        query,
        strings,
        by_folder: options.by_folder,
        by_folder_double: options.by_folder_double,
        directory: directory.map(Path::to_path_buf),
    };
    (results, shown)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Index {
        let index = Index::new();
        index.insert_batch(
            [
                "/r/여신 | Megami/Dramatic (NCED1) [여신].webm",
                "/r/여신 | Megami",
                "/r/other/megaphone.txt",
                "/r/other/ＡＢＣ.txt",
            ]
            .iter()
            .map(|p| Arc::new(FileInfo::new(PathBuf::from(p), 1, !p.contains('.')))),
        );
        index.set_cached(true);
        index
    }

    fn opts(by_folder: bool) -> SearchOptions {
        SearchOptions {
            by_folder,
            by_folder_double: false,
            sort: SortOption::Name,
            reverse: false,
            fresh: false,
        }
    }

    fn search(
        index: &Index,
        text: &str,
        by_folder: bool,
        current: &[Arc<FileInfo>],
        shown: &Shown,
    ) -> (Vec<Arc<FileInfo>>, Shown) {
        let Plan::Search { query, strings } = plan(text, false, true, shown) else {
            panic!("no search for {text}")
        };
        run(
            index,
            query,
            strings,
            opts(by_folder),
            Some(Path::new("/r")),
            current,
            shown,
        )
    }

    #[test]
    fn plan_dedupes_and_trims() {
        let none = Shown::none();
        let mega = |strings: &[&str]| Plan::Search {
            query: strings.join(" "),
            strings: strings.iter().map(|s| s.to_string()).collect(),
        };
        assert_eq!(plan("  mega  ", false, false, &none), mega(&["mega"]));
        let shown = Shown {
            query: "mega".into(),
            strings: vec!["mega".into()],
            ..Shown::none()
        };
        assert_eq!(plan("mega", false, false, &shown), Plan::Unchanged);
        assert_eq!(plan("mega", false, true, &shown), mega(&["mega"]));
        assert_eq!(plan("", false, false, &shown), Plan::Clear);
        assert_eq!(plan("", false, false, &none), Plan::Unchanged);
        assert_eq!(plan("ＭＥＧＡ", true, false, &none), mega(&["MEGA"]));
    }

    #[test]
    fn a_search_never_shown_is_not_remembered() {
        // "abc" was planned and run, but its results were dropped: "abc" must search again.
        let shown = Shown {
            query: "ab".into(),
            strings: vec!["ab".into()],
            ..Shown::none()
        };
        assert!(matches!(plan("abc", false, false, &shown), Plan::Search { .. }));
        assert!(matches!(plan("abc", false, false, &shown), Plan::Search { .. }));
    }

    #[test]
    fn full_then_narrowed() {
        let index = index();
        let (r, shown) = search(&index, "mega", false, &[], &Shown::none());
        assert_eq!(r.len(), 2);
        assert_eq!(shown.query, "mega");
        // Narrowing filters the shown results, even if the index changed underneath.
        let (r2, shown2) = search(&Index::new(), "megap", false, &r, &shown);
        assert_eq!(r2.len(), 1);
        assert_eq!(r2[0].filename, "megaphone.txt");
        assert_eq!(shown2.strings, vec!["megap".to_string()]);
        // A different folder mode, or folder depth, searches the index again.
        let (r3, _) = search(&Index::new(), "megap", true, &r, &shown);
        assert!(r3.is_empty());
        let deeper = SearchOptions {
            by_folder_double: true,
            ..opts(false)
        };
        let Plan::Search { query, strings } = plan("megap", false, true, &shown) else {
            panic!()
        };
        let (r4, _) = run(&Index::new(), query, strings, deeper, Some(Path::new("/r")), &r, &shown);
        assert!(r4.is_empty());
        // A forced search never narrows: what is shown may be out of date.
        let fresh = SearchOptions {
            fresh: true,
            ..opts(false)
        };
        let Plan::Search { query, strings } = plan("megap", false, true, &shown) else {
            panic!()
        };
        let (r5, _) = run(&Index::new(), query, strings, fresh, Some(Path::new("/r")), &r, &shown);
        assert!(r5.is_empty());
    }

    #[test]
    fn folder_mode_includes_folder_contents() {
        let index = index();
        let (r, _) = search(&index, "megami", true, &[], &Shown::none());
        assert_eq!(r.len(), 2, "{r:?}");
        let (r, _) = search(&index, "megami", false, &[], &Shown::none());
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn no_directory_no_results() {
        let index = index();
        let Plan::Search { query, strings } = plan("mega", false, false, &Shown::none()) else {
            panic!()
        };
        let (r, _) = run(&index, query, strings, opts(false), None, &[], &Shown::none());
        assert!(r.is_empty());
    }
}
