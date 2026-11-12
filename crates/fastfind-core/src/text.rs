//! String helpers ported from `StringExtension.swift`.

use unicode_normalization::UnicodeNormalization;

/// `String.toHalfWidthJPN()` — full-width Latin/punctuation to half-width.
pub fn to_half_width_jpn(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let v = c as u32;
        match v {
            0xFF1A => out.push_str(": "),
            // Full-width Latin chars sit 0xFEE0 above their ASCII counterparts.
            0xFF01..=0xFF5E => out.push(char::from_u32(v - 0xFEE0).unwrap_or(c)),
            0x3000 => out.push(' '),
            0x3001 => out.push_str(", "),
            0x3002 => out.push_str(". "),
            0xFF70 => out.push('-'),
            _ => out.push(c),
        }
    }
    out
}

/// `String.replacingPathColon()` — POSIX `:` is shown as `/` in Finder; backtick → apostrophe.
pub fn replacing_path_colon(s: &str) -> String {
    s.replace(':', "/").replace('`', "'")
}

/// `precomposedStringWithCanonicalMapping` (NFC).
pub fn nfc(s: &str) -> String {
    s.nfc().collect()
}

/// `trimmingCharacters(in: .whitespacesAndNewlines)`.
pub fn trim_ws(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace())
}

/// Case-insensitive occurrences of each word in `text`, as byte ranges.
/// Mirrors the `AttributedString.range(of:options:.caseInsensitive)` loop used for highlighting.
pub fn highlight_ranges(text: &str, words: &[String]) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    for word in words.iter().filter(|w| !w.is_empty()) {
        let wchars: Vec<char> = word.chars().collect();
        let mut cursor = 0usize;
        while cursor < text.len() {
            match find_ci(&text[cursor..], &wchars) {
                Some((start, end)) => {
                    ranges.push(cursor + start..cursor + end);
                    cursor += end.max(start + 1);
                }
                None => break,
            }
        }
    }
    ranges.sort_by_key(|r| r.start);
    ranges
}

fn chars_eq_ci(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

fn find_ci(hay: &str, needle: &[char]) -> Option<(usize, usize)> {
    'outer: for (start, _) in hay.char_indices() {
        let mut it = hay[start..].char_indices();
        for &nc in needle {
            match it.next() {
                Some((_, hc)) if chars_eq_ci(hc, nc) => {}
                _ => continue 'outer,
            }
        }
        let end = it.next().map(|(i, _)| start + i).unwrap_or(hay.len());
        return Some((start, end));
    }
    None
}

/// `ByteCountFormatter(countStyle: .file)` output for the `ko` and `en` localizations.
pub fn format_file_size(bytes: u64, korean: bool) -> String {
    if bytes < 1000 {
        return if korean {
            format!("{bytes}바이트")
        } else if bytes == 1 {
            "1 byte".to_string()
        } else {
            format!("{bytes} bytes")
        };
    }
    const UNITS: [(&str, i32); 5] = [("KB", 0), ("MB", 1), ("GB", 2), ("TB", 2), ("PB", 2)];
    let mut unit = 0usize;
    let mut div = 1000f64;
    while unit + 1 < UNITS.len() && bytes as f64 >= div * 1000.0 {
        unit += 1;
        div *= 1000.0;
    }
    loop {
        let (name, decimals) = UNITS[unit];
        let scale = 10f64.powi(decimals);
        let rounded = (bytes as f64 / div * scale).round() / scale;
        if rounded >= 1000.0 && unit + 1 < UNITS.len() {
            unit += 1;
            div *= 1000.0;
            continue;
        }
        let mut num = format!("{:.*}", decimals as usize, rounded);
        if num.contains('.') {
            num = num.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        return if korean {
            format!("{num}{name}")
        } else {
            format!("{num} {name}")
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_width() {
        assert_eq!(to_half_width_jpn("ＡＢｃ１！"), "ABc1!");
        assert_eq!(to_half_width_jpn("a：b"), "a: b");
        assert_eq!(to_half_width_jpn("あ、い。う　ｰ"), "あ, い. う -");
    }

    #[test]
    fn path_colon() {
        assert_eq!(replacing_path_colon("a:b`c"), "a/b'c");
    }

    #[test]
    fn sizes_match_foundation() {
        let en: Vec<String> = [
            1u64,
            999,
            1000,
            1499,
            1500,
            999_499,
            999_500,
            1_000_000,
            1_050_000,
            2_000_000,
            54_321_000,
            999_950_000,
            1_234_567_890,
            1_200_000_000,
            2_000_000_000,
            1_234_567_890_123,
        ]
        .iter()
        .map(|&b| format_file_size(b, false))
        .collect();
        assert_eq!(
            en,
            [
                "1 byte",
                "999 bytes",
                "1 KB",
                "1 KB",
                "2 KB",
                "999 KB",
                "1 MB",
                "1 MB",
                "1.1 MB",
                "2 MB",
                "54.3 MB",
                "1 GB",
                "1.23 GB",
                "1.2 GB",
                "2 GB",
                "1.23 TB"
            ]
        );
        assert_eq!(format_file_size(54_321_000, true), "54.3MB");
        assert_eq!(format_file_size(999, true), "999바이트");
        assert_eq!(format_file_size(1_234_567_890, true), "1.23GB");
    }

    #[test]
    fn highlight() {
        let r = highlight_ranges("Megami no Café Terrace", &["megami".into(), "no".into()]);
        assert_eq!(r, vec![0..6, 7..9]);
        let r = highlight_ranges("여신의 카페테라스", &["카페".into()]);
        assert_eq!(&"여신의 카페테라스"[r[0].clone()], "카페");
        let r = highlight_ranges("aAa", &["a".into()]);
        assert_eq!(r, vec![0..1, 1..2, 2..3]);
    }
}
