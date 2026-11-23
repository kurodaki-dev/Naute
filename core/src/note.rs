//! Structured view of a note, extracted from the editor's DOM by the UI, and
//! the exporters that only need text: Markdown, HTML, plain text, and the
//! page layout used for PDF.

use crate::model::FontFamily;
use crate::render::svg_font_name;
use crate::text::{is_wide, xml_escape, Measure};
use serde::{Deserialize, Serialize};
use std::fmt::Write;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Run {
    pub text: String,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Block {
    Heading { level: u8, runs: Vec<Run> },
    Paragraph { runs: Vec<Run> },
    Bullet { runs: Vec<Run> },
    Numbered { runs: Vec<Run>, n: u32 },
    Check { runs: Vec<Run>, checked: bool },
    Quote { runs: Vec<Run> },
    Table { rows: Vec<Vec<String>> },
    Image { src: String, w: f32, h: f32 },
    Rule,
}

fn plain(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

fn md_runs(runs: &[Run]) -> String {
    let mut s = String::new();
    for r in runs {
        if r.text.trim().is_empty() {
            s.push_str(&r.text);
            continue;
        }
        let lead = &r.text[..r.text.len() - r.text.trim_start().len()];
        let trail = &r.text[r.text.trim_end().len()..];
        let mut core = md_escape(r.text.trim());
        if r.italic {
            core = format!("*{core}*");
        }
        if r.bold {
            core = format!("**{core}**");
        }
        if r.underline {
            core = format!("<u>{core}</u>");
        }
        s.push_str(lead);
        s.push_str(&core);
        s.push_str(trail);
    }
    s
}

fn md_escape(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if matches!(c, '*' | '_' | '`' | '[' | ']' | '#' | '|' | '\\') {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

pub fn to_markdown(title: &str, blocks: &[Block]) -> String {
    let mut s = format!("# {}\n\n", md_escape(title));
    let mut prev_list = false;
    for b in blocks {
        let is_list = matches!(b, Block::Bullet { .. } | Block::Numbered { .. } | Block::Check { .. });
        if prev_list && !is_list {
            s.push('\n');
        }
        match b {
            Block::Heading { level, runs } => {
                let _ = write!(s, "{} {}\n\n", "#".repeat((*level as usize + 1).min(6)), md_runs(runs));
            }
            Block::Paragraph { runs } => {
                let _ = write!(s, "{}\n\n", md_runs(runs));
            }
            Block::Bullet { runs } => {
                let _ = writeln!(s, "- {}", md_runs(runs));
            }
            Block::Numbered { runs, n } => {
                let _ = writeln!(s, "{n}. {}", md_runs(runs));
            }
            Block::Check { runs, checked } => {
                let _ = writeln!(s, "- [{}] {}", if *checked { "x" } else { " " }, md_runs(runs));
            }
            Block::Quote { runs } => {
                let _ = write!(s, "> {}\n\n", md_runs(runs));
            }
            Block::Table { rows } => {
                for (i, row) in rows.iter().enumerate() {
                    let cells: Vec<String> = row.iter().map(|c| md_escape(c).replace('\n', " ")).collect();
                    let _ = writeln!(s, "| {} |", cells.join(" | "));
                    if i == 0 {
                        let _ = writeln!(s, "|{}|", vec![" --- "; row.len()].join("|"));
                    }
                }
                s.push('\n');
            }
            Block::Image { src, .. } => {
                if src.starts_with("data:") {
                    s.push_str("![image](embedded)\n\n");
                } else {
                    let _ = write!(s, "![image]({src})\n\n");
                }
            }
            Block::Rule => s.push_str("---\n\n"),
        }
        prev_list = is_list;
    }
    s.trim_end().to_string() + "\n"
}

pub fn to_text(title: &str, blocks: &[Block]) -> String {
    let mut s = format!("{title}\n\n");
    for b in blocks {
        match b {
            Block::Heading { runs, .. } | Block::Paragraph { runs } | Block::Quote { runs } => {
                let _ = write!(s, "{}\n\n", plain(runs));
            }
            Block::Bullet { runs } => {
                let _ = writeln!(s, "• {}", plain(runs));
            }
            Block::Numbered { runs, n } => {
                let _ = writeln!(s, "{n}. {}", plain(runs));
            }
            Block::Check { runs, checked } => {
                let _ = writeln!(s, "[{}] {}", if *checked { "x" } else { " " }, plain(runs));
            }
            Block::Table { rows } => {
                for row in rows {
                    let _ = writeln!(s, "{}", row.join("\t"));
                }
                s.push('\n');
            }
            Block::Image { .. } => {}
            Block::Rule => s.push_str("———\n\n"),
        }
    }
    s
}

fn html_runs(runs: &[Run]) -> String {
    let mut s = String::new();
    for r in runs {
        let mut t = xml_escape(&r.text);
        if r.underline {
            t = format!("<u>{t}</u>");
        }
        if r.italic {
            t = format!("<em>{t}</em>");
        }
        if r.bold {
            t = format!("<strong>{t}</strong>");
        }
        s.push_str(&t);
    }
    s
}

pub fn to_html(title: &str, lang: &str, blocks: &[Block]) -> String {
    let mut body = String::new();
    let mut open: Option<&str> = None;
    for b in blocks {
        let want = match b {
            Block::Bullet { .. } => Some("ul"),
            Block::Numbered { .. } => Some("ol"),
            Block::Check { .. } => Some("ul class=\"check\""),
            _ => None,
        };
        if open != want {
            if let Some(o) = open {
                let _ = write!(body, "</{}>", o.split(' ').next().unwrap());
            }
            if let Some(w) = want {
                let _ = write!(body, "<{w}>");
            }
            open = want;
        }
        match b {
            Block::Heading { level, runs } => {
                let l = (*level + 1).min(6);
                let _ = write!(body, "<h{l}>{}</h{l}>", html_runs(runs));
            }
            Block::Paragraph { runs } => {
                let _ = write!(body, "<p>{}</p>", html_runs(runs));
            }
            Block::Bullet { runs } | Block::Numbered { runs, .. } => {
                let _ = write!(body, "<li>{}</li>", html_runs(runs));
            }
            Block::Check { runs, checked } => {
                let _ = write!(
                    body,
                    "<li><input type=\"checkbox\" disabled{}> {}</li>",
                    if *checked { " checked" } else { "" },
                    html_runs(runs)
                );
            }
            Block::Quote { runs } => {
                let _ = write!(body, "<blockquote>{}</blockquote>", html_runs(runs));
            }
            Block::Table { rows } => {
                body.push_str("<table>");
                for (i, row) in rows.iter().enumerate() {
                    body.push_str("<tr>");
                    for c in row {
                        let tag = if i == 0 { "th" } else { "td" };
                        let _ = write!(body, "<{tag}>{}</{tag}>", xml_escape(c));
                    }
                    body.push_str("</tr>");
                }
                body.push_str("</table>");
            }
            Block::Image { src, .. } => {
                let _ = write!(body, "<img src=\"{}\" alt=\"\">", xml_escape(src));
            }
            Block::Rule => body.push_str("<hr>"),
        }
    }
    if let Some(o) = open {
        let _ = write!(body, "</{}>", o.split(' ').next().unwrap());
    }
    format!(
        r#"<!doctype html><html lang="{lang}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{t}</title><style>
body{{max-width:44rem;margin:3rem auto;padding:0 1.25rem;font:17px/1.6 "IBM Plex Sans",system-ui,sans-serif;color:#26241F;background:#F4F3EE}}
h1,h2,h3{{font-family:Literata,Georgia,serif;font-weight:600;line-height:1.25}}h1{{font-size:2.2rem}}
blockquote{{margin:1rem 0;padding-left:1rem;border-left:3px solid #C15F3C;color:#55524A}}
table{{border-collapse:collapse;width:100%;margin:1rem 0}}td,th{{border:1px solid #B1ADA1;padding:.4rem .6rem;text-align:left}}th{{background:#fff}}
ul.check{{list-style:none;padding-left:0}}img{{max-width:100%}}hr{{border:0;border-top:1px solid #B1ADA1}}
footer{{margin-top:3rem;font-size:.8rem;color:#8A867B}}
</style></head><body><h1>{t}</h1>{body}<footer>Naute</footer></body></html>"#,
        t = xml_escape(title)
    )
}

// ---------------------------------------------------------------- PDF layout

pub struct PageSetup {
    pub width: f32,
    pub height: f32,
    pub margin: f32,
}

pub const A4: PageSetup = PageSetup { width: 793.7, height: 1122.5, margin: 72.0 };

struct Word {
    text: String,
    bold: bool,
    italic: bool,
    width: f32,
    space_after: bool,
}

fn words(m: &dyn Measure, runs: &[Run], font: FontFamily, size: f32, force_bold: bool) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::new();
    for r in runs {
        let bold = r.bold || force_bold;
        let mut cur = String::new();
        let flush = |cur: &mut String, out: &mut Vec<Word>, space: bool| {
            if !cur.is_empty() {
                let w = m.width(cur, font, size, bold, r.italic);
                out.push(Word { text: std::mem::take(cur), bold, italic: r.italic, width: w, space_after: space });
            } else if space {
                if let Some(last) = out.last_mut() {
                    last.space_after = true;
                }
            }
        };
        for c in r.text.chars() {
            if c == ' ' || c == '\n' {
                flush(&mut cur, &mut out, true);
            } else if is_wide(c) {
                flush(&mut cur, &mut out, false);
                cur.push(c);
                flush(&mut cur, &mut out, false);
            } else {
                cur.push(c);
            }
        }
        flush(&mut cur, &mut out, false);
    }
    out
}

/// Lays a note out on fixed-size pages and returns one SVG per page.
pub fn layout_pages(m: &dyn Measure, title: &str, blocks: &[Block], page: &PageSetup) -> Vec<String> {
    let mut pages: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut y = page.margin;
    let content_w = page.width - 2.0 * page.margin;
    let bottom = page.height - page.margin;

    let new_page = |pages: &mut Vec<String>, cur: &mut String, y: &mut f32| {
        if !cur.is_empty() || !pages.is_empty() {
            pages.push(std::mem::take(cur));
        }
        *y = page.margin;
    };

    let space = |m: &dyn Measure, font, size| m.width(" ", font, size, false, false);

    // Paragraph-like flow of words with inline styles.
    let flow = |cur: &mut String,
                    pages: &mut Vec<String>,
                    y: &mut f32,
                    runs: &[Run],
                    font: FontFamily,
                    size: f32,
                    bold: bool,
                    color: &str,
                    indent: f32,
                    marker: Option<&str>,
                    after: f32| {
        let lh = size * 1.5;
        let ws = words(m, runs, font, size, bold);
        let max_w = content_w - indent;
        let mut lines: Vec<Vec<&Word>> = vec![vec![]];
        let mut lw = 0.0;
        let sp = space(m, font, size);
        for w in &ws {
            let line = lines.last_mut().unwrap();
            let extra = if line.is_empty() { 0.0 } else if line.last().unwrap().space_after { sp } else { 0.0 };
            if !line.is_empty() && lw + extra + w.width > max_w {
                lines.push(vec![w]);
                lw = w.width;
            } else {
                line.push(w);
                lw += extra + w.width;
            }
        }
        for (i, line) in lines.iter().enumerate() {
            if *y + lh > bottom {
                pages.push(std::mem::take(cur));
                *y = page.margin;
            }
            let base = *y + size * 0.25 + size * 0.84;
            if i == 0 {
                if let Some(mk) = marker {
                    let _ = write!(
                        cur,
                        r#"<text x="{:.2}" y="{:.2}" font-family="{}" font-size="{size}" fill="{}">{}</text>"#,
                        page.margin + indent - size * 1.1,
                        base,
                        svg_font_name(font),
                        color,
                        xml_escape(mk)
                    );
                }
            }
            let mut x = page.margin + indent;
            for (j, w) in line.iter().enumerate() {
                let _ = write!(
                    cur,
                    r#"<text x="{:.2}" y="{:.2}" font-family="{}" font-size="{size}" font-weight="{}" font-style="{}" fill="{}">{}</text>"#,
                    x,
                    base,
                    svg_font_name(font),
                    if w.bold { 600 } else { 400 },
                    if w.italic { "italic" } else { "normal" },
                    color,
                    xml_escape(&w.text)
                );
                x += w.width + if w.space_after && j + 1 < line.len() { sp } else { 0.0 };
            }
            *y += lh;
        }
        *y += after;
    };

    // Title
    flow(&mut cur, &mut pages, &mut y, &[Run { text: title.into(), ..Default::default() }], FontFamily::Serif, 30.0, true, "#26241F", 0.0, None, 14.0);

    for b in blocks {
        match b {
            Block::Heading { level, runs } => {
                y += 8.0;
                let size = match level {
                    1 => 23.0,
                    2 => 19.0,
                    _ => 16.0,
                };
                flow(&mut cur, &mut pages, &mut y, runs, FontFamily::Serif, size, true, "#26241F", 0.0, None, 6.0);
            }
            Block::Paragraph { runs } => {
                flow(&mut cur, &mut pages, &mut y, runs, FontFamily::Sans, 12.5, false, "#26241F", 0.0, None, 8.0)
            }
            Block::Bullet { runs } => {
                flow(&mut cur, &mut pages, &mut y, runs, FontFamily::Sans, 12.5, false, "#26241F", 18.0, Some("•"), 2.0)
            }
            Block::Numbered { runs, n } => {
                let mk = format!("{n}.");
                flow(&mut cur, &mut pages, &mut y, runs, FontFamily::Sans, 12.5, false, "#26241F", 22.0, Some(&mk), 2.0)
            }
            Block::Check { runs, checked } => {
                let mk = if *checked { "☑" } else { "☐" };
                flow(&mut cur, &mut pages, &mut y, runs, FontFamily::Sans, 12.5, false, "#26241F", 20.0, Some(mk), 2.0)
            }
            Block::Quote { runs } => {
                let start = y;
                let start_page = pages.len();
                flow(&mut cur, &mut pages, &mut y, runs, FontFamily::Serif, 13.5, false, "#55524A", 16.0, None, 0.0);
                if pages.len() == start_page {
                    let _ = write!(
                        cur,
                        r##"<rect x="{}" y="{:.2}" width="2.5" height="{:.2}" fill="#C15F3C"/>"##,
                        page.margin,
                        start + 2.0,
                        y - start - 2.0
                    );
                }
                y += 8.0;
            }
            Block::Rule => {
                if y + 20.0 > bottom {
                    new_page(&mut pages, &mut cur, &mut y);
                }
                let _ = write!(
                    cur,
                    r##"<line x1="{}" x2="{}" y1="{:.2}" y2="{:.2}" stroke="#B1ADA1" stroke-width="0.8"/>"##,
                    page.margin,
                    page.width - page.margin,
                    y + 8.0,
                    y + 8.0
                );
                y += 20.0;
            }
            Block::Image { src, w, h } => {
                let scale = if *w > content_w { content_w / w } else { 1.0 };
                let (iw, ih) = (w * scale, (h * scale).min(bottom - page.margin));
                if y + ih > bottom {
                    new_page(&mut pages, &mut cur, &mut y);
                }
                let _ = write!(
                    cur,
                    r#"<image x="{}" y="{:.2}" width="{iw:.2}" height="{ih:.2}" preserveAspectRatio="xMidYMid meet" xlink:href="{}"/>"#,
                    page.margin,
                    y,
                    xml_escape(src)
                );
                y += ih + 10.0;
            }
            Block::Table { rows } => {
                let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
                let cw = content_w / cols as f32;
                let size = 11.0;
                let lh = size * 1.45;
                y += 4.0;
                for (ri, row) in rows.iter().enumerate() {
                    let wrapped: Vec<Vec<String>> = (0..cols)
                        .map(|c| {
                            let t = row.get(c).cloned().unwrap_or_default();
                            crate::text::wrap(m, &t, cw - 12.0, FontFamily::Sans, size, ri == 0, false)
                        })
                        .collect();
                    let n = wrapped.iter().map(|l| l.len()).max().unwrap_or(1).max(1);
                    let rh = n as f32 * lh + 10.0;
                    if y + rh > bottom {
                        new_page(&mut pages, &mut cur, &mut y);
                    }
                    if ri == 0 {
                        let _ = write!(
                            cur,
                            r##"<rect x="{}" y="{:.2}" width="{content_w}" height="{rh:.2}" fill="#F4F3EE"/>"##,
                            page.margin,
                            y
                        );
                    }
                    for (c, lines) in wrapped.iter().enumerate() {
                        let x = page.margin + c as f32 * cw;
                        let _ = write!(
                            cur,
                            r##"<rect x="{x:.2}" y="{:.2}" width="{cw:.2}" height="{rh:.2}" fill="none" stroke="#B1ADA1" stroke-width="0.7"/>"##,
                            y
                        );
                        for (li, l) in lines.iter().enumerate() {
                            let _ = write!(
                                cur,
                                r##"<text x="{:.2}" y="{:.2}" font-family="IBM Plex Sans" font-size="{size}" font-weight="{}" fill="#26241F">{}</text>"##,
                                x + 6.0,
                                y + 5.0 + li as f32 * lh + size * 1.1,
                                if ri == 0 { 600 } else { 400 },
                                xml_escape(l)
                            );
                        }
                    }
                    y += rh;
                }
                y += 10.0;
            }
        }
    }
    pages.push(cur);
    pages
        .into_iter()
        .map(|content| {
            format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><rect width="{w}" height="{h}" fill="#FFFFFF"/>{content}</svg>"##,
                w = page.width,
                h = page.height
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::ApproxMeasure;

    fn r(t: &str) -> Vec<Run> {
        vec![Run { text: t.into(), ..Default::default() }]
    }

    #[test]
    fn markdown_export() {
        let blocks = vec![
            Block::Heading { level: 1, runs: r("Plan") },
            Block::Paragraph { runs: vec![Run { text: "bold".into(), bold: true, ..Default::default() }, Run { text: " text".into(), ..Default::default() }] },
            Block::Check { runs: r("done"), checked: true },
            Block::Table { rows: vec![vec!["a".into(), "b".into()], vec!["1".into(), "2".into()]] },
        ];
        let md = to_markdown("Title", &blocks);
        assert!(md.contains("## Plan"));
        assert!(md.contains("**bold** text"));
        assert!(md.contains("- [x] done"));
        assert!(md.contains("| a | b |"));
    }

    #[test]
    fn layout_paginates() {
        let blocks: Vec<Block> = (0..200).map(|i| Block::Paragraph { runs: r(&format!("Paragraph {i} with some words in it")) }).collect();
        let pages = layout_pages(&ApproxMeasure, "T", &blocks, &A4);
        assert!(pages.len() > 3);
        assert!(pages.iter().all(|p| p.starts_with("<svg") && p.ends_with("</svg>")));
    }
}
