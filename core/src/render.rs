//! Canvas pages to SVG. Used for the editor, thumbnails, the presenter and
//! every export (PNG, PDF and PPTX all start from this SVG).

use crate::model::*;
use crate::text::{wrap, xml_escape, Measure};
use std::fmt::Write;

pub fn svg_font_name(f: FontFamily) -> &'static str {
    match f {
        FontFamily::Sans => "IBM Plex Sans",
        FontFamily::Serif => "Literata",
        FontFamily::Mono => "IBM Plex Mono",
    }
}

/// Distance from the top of a line box to its baseline.
fn baseline(size: f32, lh: f32) -> f32 {
    size * (lh - 1.0) / 2.0 + size * 0.84
}

pub fn page_svg(canvas: &CanvasBody, page: &Page, m: &dyn Measure) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><rect width="{w}" height="{h}" fill="{bg}"/>"#,
        w = canvas.width * canvas.unit.px(),
        h = canvas.height * canvas.unit.px(),
        bg = xml_escape(&page.background)
    );
    for el in &page.elements {
        s.push_str(&element_svg(el, m));
    }
    s.push_str("</svg>");
    s
}

/// Pixel size of a canvas.
pub fn canvas_px(canvas: &CanvasBody) -> (f32, f32) {
    (canvas.width * canvas.unit.px(), canvas.height * canvas.unit.px())
}

fn text_lines(
    out: &mut String,
    m: &dyn Measure,
    text: &str,
    st: &TextStyle,
    x: f32,
    y: f32,
    w: f32,
    max_h: Option<f32>,
    pad: f32,
) {
    let inner = (w - 2.0 * pad).max(1.0);
    let lines = wrap(m, text, inner, st.font, st.size, st.bold, st.italic);
    let lh = st.size * st.line_height;
    let (anchor, ax) = match st.align {
        Align::Left => ("start", x + pad),
        Align::Center => ("middle", x + w / 2.0),
        Align::Right => ("end", x + w - pad),
    };
    let _ = write!(
        out,
        r#"<text font-family="{}" font-size="{}" font-weight="{}" font-style="{}" fill="{}" text-anchor="{}" xml:space="preserve">"#,
        svg_font_name(st.font),
        st.size,
        if st.bold { 600 } else { 400 },
        if st.italic { "italic" } else { "normal" },
        xml_escape(&st.color),
        anchor
    );
    for (i, line) in lines.iter().enumerate() {
        let top = y + pad + i as f32 * lh;
        if let Some(mh) = max_h {
            if top + lh > y + mh + 0.5 && i > 0 {
                break;
            }
        }
        if line.is_empty() {
            continue;
        }
        let _ = write!(out, r#"<tspan x="{:.2}" y="{:.2}">{}</tspan>"#, ax, top + baseline(st.size, st.line_height), xml_escape(line));
    }
    out.push_str("</text>");
}

/// Height the text of a text element needs at its current width.
pub fn text_height(m: &dyn Measure, text: &str, st: &TextStyle, w: f32) -> f32 {
    let lines = wrap(m, text, w.max(1.0), st.font, st.size, st.bold, st.italic);
    lines.len().max(1) as f32 * st.size * st.line_height
}

pub fn element_svg(el: &Element, m: &dyn Measure) -> String {
    let mut s = String::new();
    let cx = el.x + el.w / 2.0;
    let cy = el.y + el.h / 2.0;
    let _ = write!(s, r#"<g data-id="{}""#, xml_escape(&el.id));
    if el.rotation.abs() > 0.01 {
        let _ = write!(s, r#" transform="rotate({:.2} {:.2} {:.2})""#, el.rotation, cx, cy);
    }
    if el.opacity < 0.999 {
        let _ = write!(s, r#" opacity="{:.3}""#, el.opacity.clamp(0.0, 1.0));
    }
    s.push('>');
    match &el.kind {
        ElementKind::Text { text, style } => {
            text_lines(&mut s, m, text, style, el.x, el.y, el.w, None, 0.0);
        }
        ElementKind::Rect { fill, stroke, stroke_width, radius } => {
            let r = radius.min(el.w / 2.0).min(el.h / 2.0).max(0.0);
            let _ = write!(
                s,
                r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{r}" fill="{}" stroke="{}" stroke-width="{}"/>"#,
                el.x,
                el.y,
                el.w,
                el.h,
                paint(fill),
                paint(stroke),
                stroke_width
            );
        }
        ElementKind::Ellipse { fill, stroke, stroke_width } => {
            let _ = write!(
                s,
                r#"<ellipse cx="{cx}" cy="{cy}" rx="{}" ry="{}" fill="{}" stroke="{}" stroke-width="{}"/>"#,
                el.w / 2.0,
                el.h / 2.0,
                paint(fill),
                paint(stroke),
                stroke_width
            );
        }
        ElementKind::Line { stroke, stroke_width } => {
            let _ = write!(
                s,
                r#"<line x1="{}" y1="{cy}" x2="{}" y2="{cy}" stroke="{}" stroke-width="{}" stroke-linecap="round"/>"#,
                el.x,
                el.x + el.w,
                paint(stroke),
                stroke_width
            );
        }
        ElementKind::Image { src } => {
            let clip = format!("c{}", el.id);
            let _ = write!(
                s,
                r#"<clipPath id="{clip}"><rect x="{}" y="{}" width="{}" height="{}"/></clipPath><image clip-path="url(#{clip})" x="{}" y="{}" width="{}" height="{}" preserveAspectRatio="xMidYMid slice" xlink:href="{}" href="{}"/>"#,
                el.x,
                el.y,
                el.w,
                el.h,
                el.x,
                el.y,
                el.w,
                el.h,
                xml_escape(src),
                xml_escape(src)
            );
        }
        ElementKind::Table { cells, header, style, stroke, header_fill } => {
            let rows = cells.len().max(1);
            let cols = cells.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
            let cw = el.w / cols as f32;
            let ch = el.h / rows as f32;
            if *header {
                let _ = write!(
                    s,
                    r#"<rect x="{}" y="{}" width="{}" height="{ch}" fill="{}"/>"#,
                    el.x,
                    el.y,
                    el.w,
                    paint(header_fill)
                );
            }
            for (r, row) in cells.iter().enumerate() {
                for (c, val) in row.iter().enumerate() {
                    if val.is_empty() {
                        continue;
                    }
                    let mut st = style.clone();
                    if *header && r == 0 {
                        st.bold = true;
                    }
                    let pad = (style.size * 0.45).max(4.0);
                    text_lines(&mut s, m, val, &st, el.x + c as f32 * cw, el.y + r as f32 * ch, cw, Some(ch - pad), pad);
                }
            }
            let sw = (style.size / 18.0).clamp(1.0, 3.0);
            let _ = write!(
                s,
                r#"<g stroke="{}" stroke-width="{sw}" fill="none"><rect x="{}" y="{}" width="{}" height="{}"/>"#,
                paint(stroke),
                el.x,
                el.y,
                el.w,
                el.h
            );
            for r in 1..rows {
                let y = el.y + r as f32 * ch;
                let _ = write!(s, r#"<line x1="{}" y1="{y}" x2="{}" y2="{y}"/>"#, el.x, el.x + el.w);
            }
            for c in 1..cols {
                let x = el.x + c as f32 * cw;
                let _ = write!(s, r#"<line x1="{x}" y1="{}" x2="{x}" y2="{}"/>"#, el.y, el.y + el.h);
            }
            s.push_str("</g>");
        }
    }
    s.push_str("</g>");
    s
}

fn paint(c: &str) -> String {
    if c.trim().is_empty() {
        "none".into()
    } else {
        xml_escape(c)
    }
}

// ---------------------------------------------------------------- factories

pub fn default_text(x: f32, y: f32, canvas_w: f32, text: &str) -> Element {
    let size = (canvas_w / 18.0).clamp(14.0, 160.0).round();
    Element {
        id: new_id(),
        x,
        y,
        w: (canvas_w * 0.6).max(120.0),
        h: size * 1.3,
        rotation: 0.0,
        opacity: 1.0,
        locked: false,
        kind: ElementKind::Text { text: text.into(), style: TextStyle { size, ..Default::default() } },
    }
}

pub fn default_shape(key: &str, x: f32, y: f32, canvas_w: f32) -> Element {
    let side = (canvas_w * 0.25).max(40.0);
    let kind = match key {
        "ellipse" => ElementKind::Ellipse { fill: "#C15F3C".into(), stroke: String::new(), stroke_width: 0.0 },
        "line" => ElementKind::Line { stroke: "#26241F".into(), stroke_width: (canvas_w / 300.0).max(2.0) },
        _ => ElementKind::Rect { fill: "#B1ADA1".into(), stroke: String::new(), stroke_width: 0.0, radius: 0.0 },
    };
    let h = if key == "line" { (canvas_w / 60.0).max(12.0) } else { side };
    Element { id: new_id(), x, y, w: side, h, rotation: 0.0, opacity: 1.0, locked: false, kind }
}

pub fn default_table(x: f32, y: f32, canvas_w: f32) -> Element {
    let size = (canvas_w / 48.0).clamp(10.0, 48.0).round();
    let cells = vec![vec![String::new(); 3]; 4];
    Element {
        id: new_id(),
        x,
        y,
        w: canvas_w * 0.6,
        h: size * 2.4 * 4.0,
        rotation: 0.0,
        opacity: 1.0,
        locked: false,
        kind: ElementKind::Table {
            cells,
            header: true,
            style: TextStyle { size, ..Default::default() },
            stroke: "#B1ADA1".into(),
            header_fill: "#F4F3EE".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::ApproxMeasure;

    #[test]
    fn renders_all_kinds() {
        let mut d = Document::new(DocKind::Design, "x".into(), 0.0, None);
        if let Body::Canvas(c) = &mut d.body {
            let p = &mut c.pages[0];
            p.elements.push(default_text(10.0, 10.0, 1080.0, "Hello <world> & co"));
            p.elements.push(default_shape("rect", 0.0, 0.0, 1080.0));
            p.elements.push(default_shape("ellipse", 0.0, 0.0, 1080.0));
            p.elements.push(default_shape("line", 0.0, 0.0, 1080.0));
            p.elements.push(default_table(0.0, 0.0, 1080.0));
            let svg = page_svg(c, &c.pages[0], &ApproxMeasure);
            assert!(svg.contains("Hello &lt;world&gt; &amp; co"));
            assert!(svg.starts_with("<svg"));
            assert!(svg.ends_with("</svg>"));
        }
    }
}
