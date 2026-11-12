//! Search-string parsing (`SearchUtils.extractSearchStrings`, `SearchHandler.isSearchStringJustAdded`).

use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

static SEARCH_STRING_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"]*)"|(\S+)"#).unwrap());

/// Split the query into tokens: `"quoted phrases"` or whitespace-separated words, de-duplicated in order.
pub fn extract_search_strings(s: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for caps in SEARCH_STRING_RE.captures_iter(s) {
        let token = match (caps.get(1), caps.get(2)) {
            (Some(q), _) => q.as_str(),
            (None, Some(u)) => u.as_str(),
            _ => continue,
        };
        if token.is_empty() || token == "\"" {
            continue;
        }
        if seen.insert(token.to_string()) {
            ordered.push(token.to_string());
        }
    }
    ordered
}

/// True when `new` only narrows `last` (same earlier tokens, the last one extended, or tokens appended),
/// so the previous results can be filtered instead of searching the whole index.
pub fn is_search_string_just_added(last: &[String], new: &[String]) -> bool {
    if last.is_empty() || last.len() > new.len() {
        return false;
    }
    let last_index = last.len() - 1;
    if last[..last_index] != new[..last_index] {
        return false;
    }
    let new_s: Vec<char> = new[last_index].chars().collect();
    let old_s: Vec<char> = last[last_index].chars().collect();
    new_s.len() > old_s.len() && new_s.starts_with(&old_s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract() {
        assert_eq!(extract_search_strings("megami no 카페"), ["megami", "no", "카페"]);
        assert_eq!(extract_search_strings(r#""a b" c a b c"#), ["a b", "c", "a", "b"]);
        assert_eq!(extract_search_strings(r#""" x ""#), ["x"]);
        assert!(extract_search_strings("   ").is_empty());
    }

    #[test]
    fn just_added() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(is_search_string_just_added(&v(&["ab"]), &v(&["abc"])));
        assert!(!is_search_string_just_added(&v(&["ab"]), &v(&["ab"])));
        assert!(!is_search_string_just_added(&v(&["ab"]), &v(&["ac"])));
        assert!(!is_search_string_just_added(&[], &v(&["a"])));
        assert!(!is_search_string_just_added(&v(&["a", "b"]), &v(&["a"])));
        assert!(!is_search_string_just_added(&v(&["a", "b"]), &v(&["x", "bc"])));
        assert!(is_search_string_just_added(&v(&["a", "b"]), &v(&["a", "bc"])));
        // Appending a new token keeps the old last token unchanged -> not "just added" (mirrors Swift).
        assert!(!is_search_string_just_added(&v(&["a"]), &v(&["a", "b"])));
    }
}
