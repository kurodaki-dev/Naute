//! Documents live as one JSON file each in the app's data folder.
//! Nothing leaves the device unless the user exports or shares it.

use naute_core::model::{DocMeta, Document};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

pub fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let d = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

fn docs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let d = data_dir(app)?.join("documents");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

fn safe_id(id: &str) -> Result<&str, String> {
    if !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        Ok(id)
    } else {
        Err("invalid id".into())
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_documents(app: AppHandle) -> Result<Vec<DocMeta>, String> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(docs_dir(&app)?).map_err(|e| e.to_string())?.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&p) {
            if let Ok(doc) = serde_json::from_str::<Document>(&text) {
                out.push(doc.meta);
            }
        }
    }
    out.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated.partial_cmp(&a.updated).unwrap_or(std::cmp::Ordering::Equal)));
    Ok(out)
}

#[tauri::command]
pub fn load_document(app: AppHandle, id: String) -> Result<Document, String> {
    let p = docs_dir(&app)?.join(format!("{}.json", safe_id(&id)?));
    let text = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_document(app: AppHandle, doc: Document) -> Result<(), String> {
    let p = docs_dir(&app)?.join(format!("{}.json", safe_id(&doc.meta.id)?));
    let bytes = serde_json::to_vec(&doc).map_err(|e| e.to_string())?;
    write_atomic(&p, &bytes)
}

#[tauri::command]
pub fn delete_document(app: AppHandle, id: String) -> Result<(), String> {
    let p = docs_dir(&app)?.join(format!("{}.json", safe_id(&id)?));
    std::fs::remove_file(p).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<serde_json::Value, String> {
    let p = data_dir(&app)?.join("settings.json");
    Ok(std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(serde_json::json!({})))
}

#[tauri::command]
pub fn set_settings(app: AppHandle, settings: serde_json::Value) -> Result<(), String> {
    let p = data_dir(&app)?.join("settings.json");
    write_atomic(&p, &serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?)
}
