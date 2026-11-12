//! Row presentation data (`FileListItemView.parsedRowData`).

use crate::file_info::FileInfo;
use crate::text::trim_ws;
use regex::Regex;
use std::sync::LazyLock;

// 곡명 {아티스트} (종류) [작품명].확장자
static ROW_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.+?)\s\(([^\(\)]+)\)\s?(?:\[([^\[\]]+)\])?\.(\w+)$").unwrap());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub text: String,
    /// Grandparent folder tags are drawn red, parent-folder tags indigo.
    pub is_grandparent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowData {
    pub tags: Vec<Tag>,
    pub parsed_output: String,
    pub parsed_type: String,
}

pub fn parse_row(fi: &FileInfo) -> RowData {
    let mut folder_names: Vec<String> = fi.parent.split('|').map(|s| trim_ws(s).to_string()).collect();
    if !fi.grandparent.is_empty() {
        folder_names.push(format!(":{}", trim_ws(&fi.grandparent)));
    }

    let (parsed_output, parsed_type) = match ROW_RE.captures(&fi.filename) {
        Some(c) => {
            let titles = &c[1];
            let kind = c[2].to_string();
            let anime = c.get(3).map(|m| m.as_str()).unwrap_or("");
            let dot = &c[4];
            let out = if folder_names.iter().any(|f| f == anime) || anime.is_empty() {
                format!("{titles} /{dot}")
            } else {
                format!("{titles} [{anime}] /{dot}")
            };
            (out, kind)
        }
        None => (fi.filename.clone(), String::new()),
    };

    let tags = folder_names
        .iter()
        .map(|name| Tag {
            text: name.trim_matches(':').to_string(),
            is_grandparent: name.starts_with(':'),
        })
        .collect();

    RowData {
        tags,
        parsed_output,
        parsed_type,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naming_rule() {
        let fi = FileInfo::new(
            "/V/2023 2분기/여신의 카페테라스 | 女神のカフェテラス | Megami no Café Terrace/ドラマチック | Dramatic {Miki Satou} (NCED1 v1) [여신의 카페테라스].webm".into(),
            54_321_000,
            false,
        );
        let r = parse_row(&fi);
        assert_eq!(r.parsed_output, "ドラマチック | Dramatic {Miki Satou} /webm");
        assert_eq!(r.parsed_type, "NCED1 v1");
        assert_eq!(
            r.tags
                .iter()
                .map(|t| (t.text.as_str(), t.is_grandparent))
                .collect::<Vec<_>>(),
            [
                ("여신의 카페테라스", false),
                ("女神のカフェテラス", false),
                ("Megami no Café Terrace", false),
                ("2023 2분기", true)
            ]
        );

        let other = FileInfo::new("/V/a/b/Song (OP1) [Other].mp4".into(), 1, false);
        let r = parse_row(&other);
        assert_eq!(r.parsed_output, "Song [Other] /mp4");
        assert_eq!(r.parsed_type, "OP1");

        let plain = FileInfo::new("/V/a/b/plain.txt".into(), 1, false);
        let r = parse_row(&plain);
        assert_eq!(r.parsed_output, "plain.txt");
        assert_eq!(r.parsed_type, "");
    }
}
