//! File exports. Canvas pages and note pages are SVG first; from there resvg
//! makes PNGs, svg2pdf makes PDFs, and PPTX packs rendered slides with
//! editable text boxes on top.

use crate::fonts::{usvg_options, FontMeasure};
use naute_core::model::*;
use naute_core::note::{self, Block};
use naute_core::render::{canvas_px, page_svg};
use naute_core::sheet::{compute, Value};
use naute_core::text::xml_escape;
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref};
use resvg::{tiny_skia, usvg};
use std::collections::HashMap;
use std::io::{Cursor, Write};

pub type R<T> = Result<T, String>;

fn tree(svg: &str) -> R<usvg::Tree> {
    usvg::Tree::from_str(svg, &usvg_options()).map_err(|e| format!("svg: {e}"))
}

pub fn svg_to_png(svg: &str, scale: f32) -> R<Vec<u8>> {
    let t = tree(svg)?;
    let size = t.size();
    let w = ((size.width() * scale).round() as u32).clamp(1, 16_000);
    let h = ((size.height() * scale).round() as u32).clamp(1, 16_000);
    let mut pix = tiny_skia::Pixmap::new(w, h).ok_or("pixmap")?;
    let sx = w as f32 / size.width();
    let sy = h as f32 / size.height();
    resvg::render(&t, tiny_skia::Transform::from_scale(sx, sy), &mut pix.as_mut());
    pix.encode_png().map_err(|e| e.to_string())
}

/// Multi-page PDF. Each SVG becomes one page sized in points from its px
/// size (96 px = 72 pt), so a 210×297 mm design gives an A4 page.
pub fn svgs_to_pdf(svgs: &[String], title: &str) -> R<Vec<u8>> {
    let mut alloc = Ref::new(1);
    let catalog = alloc.bump();
    let tree_id = alloc.bump();
    let info = alloc.bump();
    let mut pdf = Pdf::new();
    let mut page_ids = Vec::new();
    let mut chunks = Vec::new();
    for svg in svgs {
        let t = tree(svg)?;
        let (chunk, xref) = svg2pdf::to_chunk(&t, svg2pdf::ConversionOptions::default()).map_err(|e| format!("pdf: {e}"))?;
        let mut map = HashMap::new();
        let chunk = chunk.renumber(|old| *map.entry(old).or_insert_with(|| alloc.bump()));
        let xref = map[&xref];
        let page_id = alloc.bump();
        let content_id = alloc.bump();
        let w = t.size().width() * 0.75;
        let h = t.size().height() * 0.75;
        let mut content = Content::new();
        content.transform([w, 0.0, 0.0, h, 0.0, 0.0]).x_object(Name(b"S1"));
        chunks.push((page_id, content_id, xref, w, h, content.finish(), chunk));
        page_ids.push(page_id);
    }
    pdf.catalog(catalog).pages(tree_id);
    pdf.pages(tree_id).kids(page_ids.iter().copied()).count(page_ids.len() as i32);
    pdf.document_info(info).title(pdf_writer::TextStr(title)).producer(pdf_writer::TextStr("Naute"));
    for (page_id, content_id, xref, w, h, content, chunk) in chunks {
        let mut page = pdf.page(page_id);
        page.media_box(Rect::new(0.0, 0.0, w, h));
        page.parent(tree_id);
        page.contents(content_id);
        page.resources().x_objects().pair(Name(b"S1"), xref);
        page.finish();
        pdf.stream(content_id, &content);
        pdf.extend(&chunk);
    }
    Ok(pdf.finish())
}

fn zip_files(files: Vec<(String, Vec<u8>)>) -> R<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in files {
            z.start_file(name, opts).map_err(|e| e.to_string())?;
            z.write_all(&data).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf.into_inner())
}

// ---------------------------------------------------------------- canvas

pub fn canvas_svgs(c: &CanvasBody, page: Option<usize>) -> Vec<String> {
    let m = FontMeasure;
    match page {
        Some(i) => c.pages.get(i).map(|p| vec![page_svg(c, p, &m)]).unwrap_or_default(),
        None => c.pages.iter().map(|p| page_svg(c, p, &m)).collect(),
    }
}

/// PNG export scale: aim for at least ~2000 px on the long side for small
/// canvases, never upscale huge ones.
fn png_scale(c: &CanvasBody) -> f32 {
    let (w, h) = canvas_px(c);
    let long = w.max(h);
    if long < 1000.0 {
        (2000.0 / long).min(4.0)
    } else {
        1.0
    }
}

pub fn canvas_png(c: &CanvasBody, page: Option<usize>, base: &str) -> R<(Vec<u8>, &'static str)> {
    let svgs = canvas_svgs(c, page);
    let scale = png_scale(c);
    if svgs.len() == 1 {
        return Ok((svg_to_png(&svgs[0], scale)?, "png"));
    }
    let mut files = Vec::new();
    for (i, s) in svgs.iter().enumerate() {
        files.push((format!("{base}-{:02}.png", i + 1), svg_to_png(s, scale)?));
    }
    Ok((zip_files(files)?, "zip"))
}

pub fn canvas_svg(c: &CanvasBody, page: Option<usize>, base: &str) -> R<(Vec<u8>, &'static str)> {
    let svgs = canvas_svgs(c, page);
    if svgs.len() == 1 {
        return Ok((svgs[0].clone().into_bytes(), "svg"));
    }
    let files = svgs.into_iter().enumerate().map(|(i, s)| (format!("{base}-{:02}.svg", i + 1), s.into_bytes())).collect();
    Ok((zip_files(files)?, "zip"))
}

// ---------------------------------------------------------------- pptx

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const EMU: f32 = 9525.0;
const SP_TREE_HEAD: &str = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#;

fn emu(v: f32) -> i64 {
    (v * EMU).round() as i64
}

fn hex(c: &str) -> String {
    let h = c.trim().trim_start_matches('#');
    if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        h.to_ascii_uppercase()
    } else if h.len() == 3 {
        h.chars().flat_map(|c| [c, c]).collect::<String>().to_ascii_uppercase()
    } else {
        "26241F".into()
    }
}

fn theme() -> String {
    let fill = r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#;
    let ln = r#"<a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>"#;
    let eff = "<a:effectStyle><a:effectLst/></a:effectStyle>";
    format!(
        r#"{XML}<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Naute"><a:themeElements><a:clrScheme name="Naute"><a:dk1><a:srgbClr val="26241F"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="55524A"/></a:dk2><a:lt2><a:srgbClr val="F4F3EE"/></a:lt2><a:accent1><a:srgbClr val="C15F3C"/></a:accent1><a:accent2><a:srgbClr val="B1ADA1"/></a:accent2><a:accent3><a:srgbClr val="7A766C"/></a:accent3><a:accent4><a:srgbClr val="8C4A30"/></a:accent4><a:accent5><a:srgbClr val="D9D6CC"/></a:accent5><a:accent6><a:srgbClr val="3E3B35"/></a:accent6><a:hlink><a:srgbClr val="C15F3C"/></a:hlink><a:folHlink><a:srgbClr val="8C4A30"/></a:folHlink></a:clrScheme><a:fontScheme name="Naute"><a:majorFont><a:latin typeface="Literata"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="IBM Plex Sans"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Naute"><a:fillStyleLst>{fill}{fill}{fill}</a:fillStyleLst><a:lnStyleLst>{ln}{ln}{ln}</a:lnStyleLst><a:effectStyleLst>{eff}{eff}{eff}</a:effectStyleLst><a:bgFillStyleLst>{fill}{fill}{fill}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#
    )
}

fn text_box(id: usize, el: &Element, text: &str, st: &TextStyle) -> String {
    let algn = match st.align {
        Align::Left => "l",
        Align::Center => "ctr",
        Align::Right => "r",
    };
    let font = naute_core::render::svg_font_name(st.font);
    let sz = (st.size * 75.0).round() as i64;
    let mut paras = String::new();
    for line in text.split('\n') {
        paras.push_str(&format!(
            r#"<a:p><a:pPr algn="{algn}"><a:lnSpc><a:spcPct val="{}"/></a:lnSpc></a:pPr>"#,
            (st.line_height * 100_000.0 / 1.2).round() as i64
        ));
        if !line.is_empty() {
            paras.push_str(&format!(
                r#"<a:r><a:rPr lang="en-US" sz="{sz}" b="{}" i="{}" dirty="0"><a:solidFill><a:srgbClr val="{}"/></a:solidFill><a:latin typeface="{font}"/><a:ea typeface="{font}"/></a:rPr><a:t>{}</a:t></a:r>"#,
                st.bold as u8,
                st.italic as u8,
                hex(&st.color),
                xml_escape(line)
            ));
        }
        paras.push_str(&format!(r#"<a:endParaRPr lang="en-US" sz="{sz}"/></a:p>"#));
    }
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Text {id}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm rot="{}"><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="square" lIns="0" tIns="0" rIns="0" bIns="0" rtlCol="0"><a:spAutoFit/></a:bodyPr><a:lstStyle/>{paras}</p:txBody></p:sp>"#,
        (el.rotation * 60_000.0).round() as i64,
        emu(el.x),
        emu(el.y),
        emu(el.w),
        emu(el.h)
    )
}

pub fn canvas_pptx(c: &CanvasBody) -> R<Vec<u8>> {
    let (w, h) = canvas_px(c);
    let m = FontMeasure;
    let n = c.pages.len();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut overrides = String::new();
    let mut sld_ids = String::new();
    let mut pres_rels = String::from(
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>"#,
    );
    let scale = (1920.0 / w.max(h)).clamp(1.0, 3.0);
    for (i, page) in c.pages.iter().enumerate() {
        let k = i + 1;
        // Background, shapes and images are flattened; text stays editable.
        let mut flat = page.clone();
        flat.elements.retain(|e| !matches!(e.kind, ElementKind::Text { .. }));
        let png = svg_to_png(&page_svg(c, &flat, &m), scale)?;
        files.push((format!("ppt/media/image{k}.png"), png));
        let mut texts = String::new();
        for (j, el) in page.elements.iter().enumerate() {
            if let ElementKind::Text { text, style } = &el.kind {
                texts.push_str(&text_box(j + 3, el, text, style));
            }
        }
        let slide = format!(
            r#"{XML}<p:sld {NS}><p:cSld><p:spTree>{SP_TREE_HEAD}<p:pic><p:nvPicPr><p:cNvPr id="2" name="Background"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId2"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{}" cy="{}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>{texts}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#,
            emu(w),
            emu(h)
        );
        files.push((format!("ppt/slides/slide{k}.xml"), slide.into_bytes()));
        files.push((
            format!("ppt/slides/_rels/slide{k}.xml.rels"),
            format!(
                r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image{k}.png"/></Relationships>"#
            )
            .into_bytes(),
        ));
        overrides.push_str(&format!(
            r#"<Override PartName="/ppt/slides/slide{k}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#
        ));
        sld_ids.push_str(&format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 255 + k, k + 2));
        pres_rels.push_str(&format!(
            r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{k}.xml"/>"#,
            k + 2
        ));
    }
    if n == 0 {
        return Err("empty".into());
    }
    files.push((
        "[Content_Types].xml".into(),
        format!(
            r#"{XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>{overrides}</Types>"#
        )
        .into_bytes(),
    ));
    files.push((
        "_rels/.rels".into(),
        format!(r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#).into_bytes(),
    ));
    files.push((
        "ppt/presentation.xml".into(),
        format!(
            r#"{XML}<p:presentation {NS} saveSubsetFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>{sld_ids}</p:sldIdLst><p:sldSz cx="{}" cy="{}"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#,
            emu(w),
            emu(h)
        )
        .into_bytes(),
    ));
    files.push((
        "ppt/_rels/presentation.xml.rels".into(),
        format!(r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{pres_rels}</Relationships>"#).into_bytes(),
    ));
    files.push((
        "ppt/slideMasters/slideMaster1.xml".into(),
        format!(
            r#"{XML}<p:sldMaster {NS}><p:cSld><p:spTree>{SP_TREE_HEAD}</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#
        )
        .into_bytes(),
    ));
    files.push((
        "ppt/slideMasters/_rels/slideMaster1.xml.rels".into(),
        format!(r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>"#).into_bytes(),
    ));
    files.push((
        "ppt/slideLayouts/slideLayout1.xml".into(),
        format!(r#"{XML}<p:sldLayout {NS} type="blank" preserve="1"><p:cSld name="Blank"><p:spTree>{SP_TREE_HEAD}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#).into_bytes(),
    ));
    files.push((
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels".into(),
        format!(r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#).into_bytes(),
    ));
    files.push(("ppt/theme/theme1.xml".into(), theme().into_bytes()));
    zip_files(files)
}

// ---------------------------------------------------------------- sheets

fn used_range(s: &SheetBody) -> (u32, u32) {
    let mut rows = 0;
    let mut cols = 0;
    for (k, c) in &s.cells {
        if c.input.trim().is_empty() {
            continue;
        }
        if let Some((r, cc)) = parse_ref(k) {
            rows = rows.max(r + 1);
            cols = cols.max(cc + 1);
        }
    }
    (rows, cols)
}

/// Formulas typed with `;` separators become `,` for Excel; text inside
/// quotes is left alone.
fn excel_formula(f: &str) -> String {
    let mut out = String::new();
    let mut in_str = false;
    for c in f.chars() {
        match c {
            '"' => {
                in_str = !in_str;
                out.push(c)
            }
            ';' if !in_str => out.push(','),
            c => out.push(c),
        }
    }
    out
}

pub fn sheet_xlsx(s: &SheetBody, title: &str) -> R<Vec<u8>> {
    use rust_xlsxwriter::{Format, FormatAlign, Formula, Workbook};
    let vals = compute(s);
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    let name: String = title.chars().filter(|c| !"[]:*?/\\".contains(*c)).take(31).collect();
    if !name.trim().is_empty() {
        let _ = ws.set_name(name.trim());
    }
    for (c, w) in &s.col_widths {
        let _ = ws.set_column_width_pixels(*c as u16, *w as u16);
    }
    for (k, cell) in &s.cells {
        let Some((r, c)) = parse_ref(k) else { continue };
        if cell.input.trim().is_empty() {
            continue;
        }
        let mut fmt = Format::new();
        if cell.bold {
            fmt = fmt.set_bold();
        }
        fmt = match cell.align {
            Some(Align::Center) => fmt.set_align(FormatAlign::Center),
            Some(Align::Right) => fmt.set_align(FormatAlign::Right),
            Some(Align::Left) => fmt.set_align(FormatAlign::Left),
            None => fmt,
        };
        if let Some(fill) = &cell.fill {
            if let Ok(rgb) = u32::from_str_radix(&hex(fill), 16) {
                fmt = fmt.set_background_color(rgb);
            }
        }
        let (r, c) = (r, c as u16);
        let res = if cell.input.trim_start().starts_with('=') {
            let mut f = Formula::new(excel_formula(cell.input.trim()));
            if let Some(v) = vals.get(k) {
                f = f.set_result(v.display());
            }
            ws.write_formula_with_format(r, c, f, &fmt).map(|_| ())
        } else {
            match vals.get(k) {
                Some(Value::Num(n)) => ws.write_number_with_format(r, c, *n, &fmt).map(|_| ()),
                _ => ws.write_string_with_format(r, c, &cell.input, &fmt).map(|_| ()),
            }
        };
        res.map_err(|e| e.to_string())?;
    }
    wb.save_to_buffer().map_err(|e| e.to_string())
}

pub fn sheet_csv(s: &SheetBody) -> Vec<u8> {
    let vals = compute(s);
    let (rows, cols) = used_range(s);
    let mut out = String::from("\u{feff}");
    for r in 0..rows {
        let line: Vec<String> = (0..cols)
            .map(|c| {
                let v = vals.get(&cell_ref(r, c)).map(|v| v.display()).unwrap_or_default();
                if v.contains([',', '"', '\n', ';']) {
                    format!("\"{}\"", v.replace('"', "\"\""))
                } else {
                    v
                }
            })
            .collect();
        out.push_str(&line.join(","));
        out.push_str("\r\n");
    }
    out.into_bytes()
}

pub fn sheet_pdf(s: &SheetBody, title: &str) -> R<Vec<u8>> {
    use naute_core::text::Measure;
    let vals = compute(s);
    let (rows, cols) = used_range(s);
    let (pw, ph, margin) = (1122.5_f32, 793.7_f32, 40.0_f32);
    let widths: Vec<f32> = (0..cols.max(1)).map(|c| s.col_widths.get(&c).copied().unwrap_or(110.0)).collect();
    let total: f32 = widths.iter().sum::<f32>() + 36.0;
    let scale = ((pw - 2.0 * margin) / total).min(1.0);
    let rh = 24.0 * scale;
    let fs = 11.0 * scale;
    let m = FontMeasure;
    let per_page = (((ph - 2.0 * margin - 30.0) / rh).floor() as u32).max(1);
    let mut svgs = Vec::new();
    let mut start = 0;
    loop {
        let end = (start + per_page).min(rows);
        let mut body = String::new();
        body.push_str(&format!(
            r##"<text x="{margin}" y="{}" font-family="Literata" font-size="16" font-weight="600" fill="#26241F">{}</text>"##,
            margin + 4.0,
            xml_escape(title)
        ));
        let top = margin + 22.0;
        let mut x = margin;
        body.push_str(&format!(r##"<rect x="{x}" y="{top}" width="{}" height="{rh}" fill="#F4F3EE"/>"##, total * scale));
        x += 36.0 * scale;
        for (c, w) in widths.iter().enumerate() {
            body.push_str(&format!(
                r##"<text x="{}" y="{}" font-family="IBM Plex Mono" font-size="{}" fill="#7A766C" text-anchor="middle">{}</text>"##,
                x + w * scale / 2.0,
                top + rh * 0.68,
                fs * 0.9,
                col_name(c as u32)
            ));
            x += w * scale;
        }
        for (i, r) in (start..end).enumerate() {
            let y = top + rh * (i as f32 + 1.0);
            body.push_str(&format!(
                r##"<text x="{}" y="{}" font-family="IBM Plex Mono" font-size="{}" fill="#7A766C" text-anchor="middle">{}</text>"##,
                margin + 18.0 * scale,
                y + rh * 0.68,
                fs * 0.9,
                r + 1
            ));
            let mut x = margin + 36.0 * scale;
            for (c, w) in widths.iter().enumerate() {
                let key = cell_ref(r, c as u32);
                let cw = w * scale;
                if let Some(cell) = s.cells.get(&key) {
                    if let Some(fill) = &cell.fill {
                        body.push_str(&format!(r#"<rect x="{x}" y="{y}" width="{cw}" height="{rh}" fill="{}"/>"#, xml_escape(fill)));
                    }
                    let v = vals.get(&key).cloned().unwrap_or(Value::Empty);
                    let mut text = v.display();
                    while !text.is_empty() && m.width(&text, FontFamily::Sans, fs, cell.bold, false) > cw - 8.0 {
                        text.pop();
                    }
                    let align = cell.align.unwrap_or(if v.is_number() { Align::Right } else { Align::Left });
                    let (tx, anchor) = match align {
                        Align::Left => (x + 4.0, "start"),
                        Align::Center => (x + cw / 2.0, "middle"),
                        Align::Right => (x + cw - 4.0, "end"),
                    };
                    body.push_str(&format!(
                        r##"<text x="{tx}" y="{}" font-family="IBM Plex Sans" font-size="{fs}" font-weight="{}" fill="#26241F" text-anchor="{anchor}">{}</text>"##,
                        y + rh * 0.68,
                        if cell.bold { 600 } else { 400 },
                        xml_escape(&text)
                    ));
                }
                x += cw;
            }
        }
        // Grid
        let n = end - start + 1;
        let gh = rh * n as f32;
        let mut x = margin;
        let mut grid = format!(r##"<g stroke="#D9D6CC" stroke-width="0.6">"##);
        for i in 0..=n {
            let y = top + rh * i as f32;
            grid.push_str(&format!(r#"<line x1="{margin}" x2="{}" y1="{y}" y2="{y}"/>"#, margin + total * scale));
        }
        grid.push_str(&format!(r#"<line x1="{x}" x2="{x}" y1="{top}" y2="{}"/>"#, top + gh));
        x += 36.0 * scale;
        for w in &widths {
            grid.push_str(&format!(r#"<line x1="{x}" x2="{x}" y1="{top}" y2="{}"/>"#, top + gh));
            x += w * scale;
        }
        grid.push_str(&format!(r#"<line x1="{x}" x2="{x}" y1="{top}" y2="{}"/></g>"#, top + gh));
        svgs.push(format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{pw}" height="{ph}" viewBox="0 0 {pw} {ph}"><rect width="{pw}" height="{ph}" fill="#FFFFFF"/>{body}{grid}</svg>"##
        ));
        start = end;
        if start >= rows {
            break;
        }
    }
    svgs_to_pdf(&svgs, title)
}

// ---------------------------------------------------------------- notes

pub fn note_pdf(title: &str, blocks: &[Block]) -> R<Vec<u8>> {
    let pages = note::layout_pages(&FontMeasure, title, blocks, &note::A4);
    svgs_to_pdf(&pages, title)
}

pub fn note_docx(title: &str, blocks: &[Block]) -> R<Vec<u8>> {
    use docx_rs::*;
    fn runs(p: Paragraph, runs: &[naute_core::note::Run], size: Option<usize>, force_bold: bool) -> Paragraph {
        let mut p = p;
        for r in runs {
            let mut run = Run::new().add_text(&r.text).fonts(RunFonts::new().ascii("IBM Plex Sans").hi_ansi("IBM Plex Sans"));
            if r.bold || force_bold {
                run = run.bold();
            }
            if r.italic {
                run = run.italic();
            }
            if r.underline {
                run = run.underline("single");
            }
            if let Some(s) = size {
                run = run.size(s);
            }
            p = p.add_run(run);
        }
        p
    }
    let serif = RunFonts::new().ascii("Literata").hi_ansi("Literata");
    let mut doc = Docx::new().add_paragraph(
        Paragraph::new().add_run(Run::new().add_text(title).bold().size(44).fonts(serif.clone())),
    );
    for b in blocks {
        doc = match b {
            Block::Heading { level, runs: rs } => {
                let size = match level {
                    1 => 34,
                    2 => 28,
                    _ => 24,
                };
                let mut p = Paragraph::new();
                for r in rs {
                    p = p.add_run(Run::new().add_text(&r.text).bold().size(size).fonts(serif.clone()));
                }
                doc.add_paragraph(p)
            }
            Block::Paragraph { runs: rs } => doc.add_paragraph(runs(Paragraph::new(), rs, None, false)),
            Block::Bullet { runs: rs } => doc.add_paragraph(runs(Paragraph::new().add_run(Run::new().add_text("•  ")), rs, None, false)),
            Block::Numbered { runs: rs, n } => {
                doc.add_paragraph(runs(Paragraph::new().add_run(Run::new().add_text(format!("{n}.  "))), rs, None, false))
            }
            Block::Check { runs: rs, checked } => doc.add_paragraph(runs(
                Paragraph::new().add_run(Run::new().add_text(if *checked { "☑  " } else { "☐  " })),
                rs,
                None,
                false,
            )),
            Block::Quote { runs: rs } => doc.add_paragraph(runs(Paragraph::new().indent(Some(720), None, None, None), rs, None, false)),
            Block::Table { rows } => {
                let trs: Vec<TableRow> = rows
                    .iter()
                    .enumerate()
                    .map(|(i, row)| {
                        TableRow::new(
                            row.iter()
                                .map(|c| {
                                    let mut run = Run::new().add_text(c);
                                    if i == 0 {
                                        run = run.bold();
                                    }
                                    TableCell::new().add_paragraph(Paragraph::new().add_run(run))
                                })
                                .collect(),
                        )
                    })
                    .collect();
                doc.add_table(Table::new(trs))
            }
            Block::Image { .. } => doc,
            Block::Rule => doc.add_paragraph(Paragraph::new().add_run(Run::new().add_text("———"))),
        };
    }
    let mut buf = Cursor::new(Vec::new());
    doc.build().pack(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use naute_core::render::{default_shape, default_table, default_text};

    fn deck() -> CanvasBody {
        let mut c = CanvasBody { width: 1920.0, height: 1080.0, unit: Unit::Px, pages: vec![blank_page("#F4F3EE"), blank_page("#FFFFFF")] };
        c.pages[0].elements.push(default_text(100.0, 100.0, 1920.0, "Bonjour Naute — 你好"));
        c.pages[0].elements.push(default_shape("ellipse", 800.0, 400.0, 1920.0));
        c.pages[1].elements.push(default_table(100.0, 100.0, 1920.0));
        c
    }

    #[test]
    fn exports_canvas() {
        let c = deck();
        let (png, ext) = canvas_png(&c, Some(0), "x").unwrap();
        assert_eq!(ext, "png");
        assert_eq!(&png[1..4], b"PNG");
        let (zip, ext) = canvas_png(&c, None, "x").unwrap();
        assert_eq!(ext, "zip");
        assert_eq!(&zip[0..2], b"PK");
        let pdf = svgs_to_pdf(&canvas_svgs(&c, None), "Deck").unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        let pptx = canvas_pptx(&c).unwrap();
        assert_eq!(&pptx[0..2], b"PK");
    }

    /// `NAUTE_SAMPLES=dir cargo test -p naute --no-default-features -- --ignored`
    /// writes one file per format, to eyeball exports.
    #[test]
    #[ignore]
    fn write_samples() {
        let dir = std::path::PathBuf::from(std::env::var("NAUTE_SAMPLES").unwrap_or_else(|_| "samples".into()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = deck();
        c.pages[0].background = "#F4F3EE".into();
        if let ElementKind::Table { cells, .. } = &mut c.pages[1].elements[0].kind {
            *cells = vec![
                vec!["Plan".into(), "Prix".into(), "Dictée".into()],
                vec!["Libre".into(), "0 €".into(), "Whisper Small".into()],
                vec!["Pro".into(), "4 €".into(), "Parakeet V3".into()],
                vec!["Équipe".into(), "9 €".into(), "Whisper Turbo".into()],
            ];
        }
        std::fs::write(dir.join("slide.png"), canvas_png(&c, Some(0), "x").unwrap().0).unwrap();
        std::fs::write(dir.join("deck.pdf"), svgs_to_pdf(&canvas_svgs(&c, None), "Deck").unwrap()).unwrap();
        std::fs::write(dir.join("deck.pptx"), canvas_pptx(&c).unwrap()).unwrap();
        let r = |t: &str| vec![naute_core::note::Run { text: t.into(), ..Default::default() }];
        let blocks = vec![
            Block::Heading { level: 1, runs: r("Objectifs du trimestre") },
            Block::Paragraph { runs: vec![
                naute_core::note::Run { text: "Livrer la version mobile ".into(), ..Default::default() },
                naute_core::note::Run { text: "avant la fin du mois".into(), bold: true, ..Default::default() },
                naute_core::note::Run { text: ", avec la dictée locale activée par défaut et des exports fidèles sur toutes les plateformes.".into(), ..Default::default() },
            ] },
            Block::Check { runs: r("Finaliser les maquettes"), checked: true },
            Block::Check { runs: r("Tester Parakeet V3 sur Android"), checked: false },
            Block::Quote { runs: r("La meilleure interface est celle qu’on oublie.") },
            Block::Table { rows: vec![vec!["Tâche".into(), "Qui".into()], vec!["Design".into(), "Kurodaki".into()]] },
        ];
        std::fs::write(dir.join("note.pdf"), note_pdf("Réunion produit", &blocks).unwrap()).unwrap();
        std::fs::write(dir.join("note.docx"), note_docx("Réunion produit", &blocks).unwrap()).unwrap();
        let mut s = SheetBody::default();
        for (k, v) in [("A1", "Poste"), ("B1", "Montant"), ("A2", "Train"), ("B2", "124,5"), ("A3", "Hôtel"), ("B3", "310"), ("A4", "Total"), ("B4", "=SUM(B2:B3)")] {
            s.cells.insert(k.into(), Cell { input: v.into(), bold: k == "A1" || k == "B1", ..Default::default() });
        }
        std::fs::write(dir.join("sheet.xlsx"), sheet_xlsx(&s, "Budget").unwrap()).unwrap();
        std::fs::write(dir.join("sheet.pdf"), sheet_pdf(&s, "Budget").unwrap()).unwrap();
    }

    #[test]
    fn exports_sheet_and_note() {
        let mut s = SheetBody::default();
        s.cells.insert("A1".into(), Cell { input: "12".into(), bold: true, ..Default::default() });
        s.cells.insert("A2".into(), Cell { input: "=SUM(A1; 3)".into(), ..Default::default() });
        assert!(sheet_xlsx(&s, "Budget").unwrap().starts_with(b"PK"));
        assert!(sheet_pdf(&s, "Budget").unwrap().starts_with(b"%PDF"));
        let csv = String::from_utf8(sheet_csv(&s)).unwrap();
        assert!(csv.contains("15"));
        let blocks = vec![Block::Paragraph { runs: vec![naute_core::note::Run { text: "Hello".into(), ..Default::default() }] }];
        assert!(note_pdf("T", &blocks).unwrap().starts_with(b"%PDF"));
        assert!(note_docx("T", &blocks).unwrap().starts_with(b"PK"));
    }
}
