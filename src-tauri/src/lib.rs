//! Naute — native side. Storage, exports, sharing and on-device dictation.
//! The interface itself is written in Rust too (see `ui/`), compiled to
//! WebAssembly and hosted in the system webview by Tauri.

mod export;
mod fonts;
mod storage;
mod stt;

use naute_core::model::*;
use naute_core::note::Block;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_fs::FsExt;

#[derive(Deserialize)]
pub struct ExportRequest {
    doc: Document,
    format: String,
    /// Notes: the structured content extracted from the editor.
    #[serde(default)]
    blocks: Option<Vec<Block>>,
    /// Canvas: export one page instead of all of them.
    #[serde(default)]
    page: Option<usize>,
    #[serde(default)]
    lang: Option<String>,
}

#[derive(Serialize)]
pub struct Saved {
    path: String,
    can_reveal: bool,
}

fn file_base(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '-' })
        .collect();
    let s = s.trim().trim_matches('-').to_string();
    if s.is_empty() {
        "naute".into()
    } else {
        s.chars().take(80).collect()
    }
}

fn build_export(req: &ExportRequest) -> Result<(Vec<u8>, &'static str), String> {
    let title = if req.doc.meta.title.trim().is_empty() { "Naute" } else { req.doc.meta.title.as_str() };
    let base = file_base(title);
    let blocks = req.blocks.clone().unwrap_or_default();
    match (&req.doc.body, req.format.as_str()) {
        (_, "naute") => Ok((serde_json::to_vec_pretty(&req.doc).map_err(|e| e.to_string())?, "naute")),
        (Body::Note(_), "pdf") => Ok((export::note_pdf(title, &blocks)?, "pdf")),
        (Body::Note(_), "md") => Ok((naute_core::note::to_markdown(title, &blocks).into_bytes(), "md")),
        (Body::Note(_), "txt") => Ok((naute_core::note::to_text(title, &blocks).into_bytes(), "txt")),
        (Body::Note(_), "html") => {
            Ok((naute_core::note::to_html(title, req.lang.as_deref().unwrap_or("en"), &blocks).into_bytes(), "html"))
        }
        (Body::Note(_), "docx") => Ok((export::note_docx(title, &blocks)?, "docx")),
        (Body::Sheet(s), "xlsx") => Ok((export::sheet_xlsx(s, title)?, "xlsx")),
        (Body::Sheet(s), "csv") => Ok((export::sheet_csv(s), "csv")),
        (Body::Sheet(s), "pdf") => Ok((export::sheet_pdf(s, title)?, "pdf")),
        (Body::Canvas(c), "pdf") => Ok((export::svgs_to_pdf(&export::canvas_svgs(c, req.page), title)?, "pdf")),
        (Body::Canvas(c), "png") => export::canvas_png(c, req.page, &base),
        (Body::Canvas(c), "svg") => export::canvas_svg(c, req.page, &base),
        (Body::Canvas(c), "pptx") => Ok((export::canvas_pptx(c)?, "pptx")),
        (_, f) => Err(format!("unsupported format {f}")),
    }
}

fn filter_name(ext: &str) -> &'static str {
    match ext {
        "pdf" => "PDF",
        "png" => "PNG",
        "svg" => "SVG",
        "zip" => "ZIP",
        "pptx" => "PowerPoint",
        "xlsx" => "Excel",
        "docx" => "Word",
        "csv" => "CSV",
        "md" => "Markdown",
        "html" => "HTML",
        "txt" => "Text",
        _ => "Naute",
    }
}

/// Render the export, ask where to put it (on Android and iOS this is the
/// system picker, which also offers cloud drives and other apps), write it.
#[tauri::command]
async fn export_document(app: AppHandle, req: ExportRequest) -> Result<Option<Saved>, String> {
    let (bytes, ext, title) = tauri::async_runtime::spawn_blocking(move || {
        build_export(&req).map(|(b, e)| (b, e, req.doc.meta.title.clone()))
    })
    .await
    .map_err(|e| e.to_string())??;
    let name = format!("{}.{ext}", file_base(&title));
    let picked = app.dialog().file().set_file_name(&name).add_filter(filter_name(ext), &[ext]).blocking_save_file();
    let Some(path) = picked else { return Ok(None) };
    let mut opts = tauri_plugin_fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    let mut f = app.fs().open(path.clone(), opts).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, &bytes).map_err(|e| e.to_string())?;
    let can_reveal = path.as_path().is_some() && cfg!(desktop);
    Ok(Some(Saved { path: path.to_string(), can_reveal }))
}

#[tauri::command]
fn reveal_file(path: String) -> Result<(), String> {
    #[cfg(desktop)]
    {
        tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| e.to_string())
    }
    #[cfg(not(desktop))]
    {
        let _ = path;
        Ok(())
    }
}

fn text_to_html(text: &str) -> String {
    text.split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            let t = naute_core::text::xml_escape(p.trim());
            if let Some(h) = t.strip_prefix("# ") {
                format!("<h1>{h}</h1>")
            } else if let Some(h) = t.strip_prefix("## ") {
                format!("<h2>{h}</h2>")
            } else {
                format!("<p>{}</p>", t.replace('\n', "<br>"))
            }
        })
        .collect()
}

fn csv_to_sheet(text: &str) -> SheetBody {
    let mut s = SheetBody::default();
    let delim = if text.lines().next().map(|l| l.matches(';').count() > l.matches(',').count()).unwrap_or(false) { ';' } else { ',' };
    let mut rows = 0;
    let mut cols = 0;
    for (r, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let mut fields = Vec::new();
        let mut cur = String::new();
        let mut q = false;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' if q && chars.peek() == Some(&'"') => {
                    cur.push('"');
                    chars.next();
                }
                '"' => q = !q,
                c if c == delim && !q => fields.push(std::mem::take(&mut cur)),
                c => cur.push(c),
            }
        }
        fields.push(cur);
        for (c, v) in fields.into_iter().enumerate() {
            if !v.is_empty() {
                s.cells.insert(cell_ref(r as u32, c as u32), Cell { input: v, ..Default::default() });
                cols = cols.max(c as u32 + 1);
            }
        }
        rows = r as u32 + 1;
    }
    s.rows = s.rows.max(rows + 10);
    s.cols = s.cols.max(cols + 2);
    s
}

/// Open a shared file: `.naute`, Markdown/text (becomes a note) or CSV
/// (becomes a sheet). The document gets a fresh id and is saved locally.
#[tauri::command]
async fn import_document(app: AppHandle, now: f64) -> Result<Option<DocMeta>, String> {
    let picked = app
        .dialog()
        .file()
        .add_filter("Naute, Markdown, CSV", &["naute", "json", "md", "txt", "csv"])
        .blocking_pick_file();
    let Some(path) = picked else { return Ok(None) };
    let mut f = app.fs().open(path.clone(), tauri_plugin_fs::OpenOptions::new()).map_err(|e| e.to_string())?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut f, &mut text).map_err(|e| e.to_string())?;
    let name = path.to_string();
    let stem = name.rsplit(['/', '\\']).next().unwrap_or("Import").rsplit_once('.').map(|x| x.0.to_string()).unwrap_or_else(|| "Import".into());
    let stem = stem.replace("%20", " ");
    let lower = name.to_lowercase();
    let mut doc = if lower.ends_with(".csv") {
        let mut d = Document::new(DocKind::Sheet, stem, now, None);
        d.body = Body::Sheet(csv_to_sheet(&text));
        d
    } else if lower.ends_with(".md") || lower.ends_with(".txt") {
        let mut d = Document::new(DocKind::Note, stem, now, None);
        d.body = Body::Note(NoteBody { html: text_to_html(&text) });
        d
    } else {
        serde_json::from_str::<Document>(&text).map_err(|_| "not-a-naute-file".to_string())?
    };
    doc.meta.id = new_id();
    doc.meta.updated = now;
    storage::save_document(app.clone(), doc.clone())?;
    Ok(Some(doc.meta))
}

#[tauri::command]
fn app_info() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "mobile": cfg!(mobile),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    seed_ids(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .manage(stt::SttState::default())
        .invoke_handler(tauri::generate_handler![
            storage::list_documents,
            storage::load_document,
            storage::save_document,
            storage::delete_document,
            storage::get_settings,
            storage::set_settings,
            export_document,
            import_document,
            reveal_file,
            app_info,
            stt::stt_available,
            stt::stt_models,
            stt::stt_download,
            stt::stt_cancel,
            stt::stt_delete,
            stt::stt_transcribe,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Naute");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_import() {
        let s = csv_to_sheet("name;qty\n\"a;b\";2\n");
        assert_eq!(s.cells["A2"].input, "a;b");
        assert_eq!(s.cells["B2"].input, "2");
    }
}
