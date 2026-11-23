//! Text measurement and line breaking. The UI measures with the browser's
//! canvas, the native exporter with the bundled TrueType fonts; both feed the
//! same breaking algorithm so what you see on screen is what lands in a PDF.

use crate::model::FontFamily;

pub trait Measure {
    /// Advance width of `text` in px at `size` px.
    fn width(&self, text: &str, font: FontFamily, size: f32, bold: bool, italic: bool) -> f32;
}

/// A crude fallback used in tests and when no font is available.
pub struct ApproxMeasure;

impl Measure for ApproxMeasure {
    fn width(&self, text: &str, font: FontFamily, size: f32, bold: bool, _italic: bool) -> f32 {
        let per = match font {
            FontFamily::Mono => 0.6,
            _ => 0.52,
        } * if bold { 1.05 } else { 1.0 };
        text.chars().map(|c| if is_wide(c) { 1.0 } else { per }).sum::<f32>() * size
    }
}

/// CJK and other full-width characters can break anywhere.
pub fn is_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF |
        0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x20000..=0x3FFFD)
}

/// Split text into tokens that may not be broken: words (with their trailing
/// space) or single wide characters.
fn tokens(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if is_wide(c) {
            if start < i {
                out.push(&line[start..i]);
            }
            let end = chars.peek().map(|(j, _)| *j).unwrap_or(line.len());
            out.push(&line[i..end]);
            start = end;
        } else if c == ' ' {
            let end = chars.peek().map(|(j, _)| *j).unwrap_or(line.len());
            out.push(&line[start..end]);
            start = end;
        }
    }
    if start < line.len() {
        out.push(&line[start..]);
    }
    out
}

/// Greedy line breaking. Explicit newlines are kept; words longer than the
/// box are split by character.
pub fn wrap(
    m: &dyn Measure,
    text: &str,
    max_width: f32,
    font: FontFamily,
    size: f32,
    bold: bool,
    italic: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        for tok in tokens(para) {
            let candidate = format!("{cur}{tok}");
            if cur.is_empty() || m.width(candidate.trim_end(), font, size, bold, italic) <= max_width {
                cur = candidate;
                // A single token wider than the box: break it by character.
                if m.width(cur.trim_end(), font, size, bold, italic) > max_width && cur.chars().count() > 1 {
                    let mut piece = String::new();
                    for ch in cur.chars() {
                        let next = format!("{piece}{ch}");
                        if !piece.is_empty() && m.width(&next, font, size, bold, italic) > max_width {
                            lines.push(std::mem::take(&mut piece));
                            piece.push(ch);
                        } else {
                            piece = next;
                        }
                    }
                    cur = piece;
                }
            } else {
                lines.push(cur.trim_end().to_string());
                cur = tok.to_string();
            }
        }
        lines.push(cur.trim_end().to_string());
    }
    lines
}

pub fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' => {}
            c => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_words_and_keeps_newlines() {
        let m = ApproxMeasure;
        let lines = wrap(&m, "aaaa bbbb cccc\ndd", 10.0 * 0.52 * 10.0, FontFamily::Sans, 10.0, false, false);
        assert_eq!(lines, vec!["aaaa bbbb", "cccc", "dd"]);
    }

    #[test]
    fn breaks_cjk_anywhere() {
        let m = ApproxMeasure;
        let lines = wrap(&m, "你好世界", 20.0, FontFamily::Sans, 10.0, false, false);
        assert_eq!(lines, vec!["你好", "世界"]);
    }

    #[test]
    fn splits_long_word() {
        let m = ApproxMeasure;
        let lines = wrap(&m, "abcdefghij", 5.2 * 4.0, FontFamily::Sans, 10.0, false, false);
        assert_eq!(lines.len(), 3);
    }
}
