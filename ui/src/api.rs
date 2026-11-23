//! Bridge to the native side. Inside the app every call goes through
//! Tauri's IPC. Opened in a plain browser (useful while designing the UI),
//! documents fall back to localStorage and native-only features report that
//! they need the app.

use naute_core::model::{DocMeta, Document};
use naute_core::note::Block;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    async fn tauri_invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen, catch)]
    async fn tauri_listen(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> Result<JsValue, JsValue>;
}

pub fn native() -> bool {
    let w = web_sys::window().unwrap();
    js_sys::Reflect::get(&w, &"__TAURI__".into()).map(|v| !v.is_undefined()).unwrap_or(false)
}

fn ser() -> serde_wasm_bindgen::Serializer {
    serde_wasm_bindgen::Serializer::json_compatible()
}

async fn call<T: DeserializeOwned>(cmd: &str, args: Value) -> Result<T, String> {
    let js = args.serialize(&ser()).map_err(|e| e.to_string())?;
    match tauri_invoke(cmd, js).await {
        Ok(v) => serde_wasm_bindgen::from_value(v).map_err(|e| e.to_string()),
        Err(e) => Err(e.as_string().unwrap_or_else(|| format!("{e:?}"))),
    }
}

/// Listen to a backend event for the lifetime of the app.
pub fn listen<T: DeserializeOwned + 'static>(event: &'static str, mut f: impl FnMut(T) + 'static) {
    if !native() {
        return;
    }
    let cb = Closure::<dyn FnMut(JsValue)>::new(move |ev: JsValue| {
        if let Ok(payload) = js_sys::Reflect::get(&ev, &"payload".into()) {
            if let Ok(v) = serde_wasm_bindgen::from_value::<T>(payload) {
                f(v);
            }
        }
    });
    wasm_bindgen_futures::spawn_local(async move {
        let _ = tauri_listen(event, &cb).await;
        cb.forget();
    });
}

// ---------------------------------------------------------------- browser fallback

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn ls_get(key: &str) -> Option<String> {
    storage()?.get_item(key).ok().flatten()
}

fn ls_set(key: &str, v: &str) {
    if let Some(s) = storage() {
        let _ = s.set_item(key, v);
    }
}

fn ls_index() -> Vec<String> {
    ls_get("naute:index").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

// ---------------------------------------------------------------- documents

pub async fn list_documents() -> Result<Vec<DocMeta>, String> {
    if native() {
        return call("list_documents", json!({})).await;
    }
    let mut out: Vec<DocMeta> = ls_index()
        .iter()
        .filter_map(|id| ls_get(&format!("naute:doc:{id}")))
        .filter_map(|s| serde_json::from_str::<Document>(&s).ok())
        .map(|d| d.meta)
        .collect();
    out.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated.partial_cmp(&a.updated).unwrap()));
    Ok(out)
}

pub async fn load_document(id: &str) -> Result<Document, String> {
    if native() {
        return call("load_document", json!({ "id": id })).await;
    }
    ls_get(&format!("naute:doc:{id}")).and_then(|s| serde_json::from_str(&s).ok()).ok_or_else(|| "not found".into())
}

pub async fn save_document(doc: &Document) -> Result<(), String> {
    if native() {
        return call("save_document", json!({ "doc": doc })).await;
    }
    let mut idx = ls_index();
    if !idx.contains(&doc.meta.id) {
        idx.push(doc.meta.id.clone());
        ls_set("naute:index", &serde_json::to_string(&idx).unwrap());
    }
    ls_set(&format!("naute:doc:{}", doc.meta.id), &serde_json::to_string(doc).unwrap());
    Ok(())
}

pub async fn delete_document(id: &str) -> Result<(), String> {
    if native() {
        return call("delete_document", json!({ "id": id })).await;
    }
    let idx: Vec<String> = ls_index().into_iter().filter(|x| x != id).collect();
    ls_set("naute:index", &serde_json::to_string(&idx).unwrap());
    if let Some(s) = storage() {
        let _ = s.remove_item(&format!("naute:doc:{id}"));
    }
    Ok(())
}

pub async fn import_document(now: f64) -> Result<Option<DocMeta>, String> {
    if native() {
        return call("import_document", json!({ "now": now })).await;
    }
    Err("native-only".into())
}

// ---------------------------------------------------------------- settings

pub async fn get_settings() -> Value {
    if native() {
        return call("get_settings", json!({})).await.unwrap_or(json!({}));
    }
    ls_get("naute:settings").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}))
}

pub async fn set_settings(v: &Value) {
    if native() {
        let _: Result<(), String> = call("set_settings", json!({ "settings": v })).await;
    } else {
        ls_set("naute:settings", &v.to_string());
    }
}

// ---------------------------------------------------------------- export

#[derive(Deserialize, Clone, Debug)]
pub struct Saved {
    pub path: String,
    pub can_reveal: bool,
}

pub async fn export_document(
    doc: &Document,
    format: &str,
    blocks: Option<Vec<Block>>,
    page: Option<usize>,
    lang: &str,
) -> Result<Option<Saved>, String> {
    if !native() {
        return Err("native-only".into());
    }
    call(
        "export_document",
        json!({ "req": { "doc": doc, "format": format, "blocks": blocks, "page": page, "lang": lang } }),
    )
    .await
}

pub async fn reveal_file(path: &str) {
    let _: Result<(), String> = call("reveal_file", json!({ "path": path })).await;
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct AppInfo {
    pub version: String,
    pub platform: String,
    #[allow(dead_code)]
    pub mobile: bool,
}

pub async fn app_info() -> AppInfo {
    if native() {
        call("app_info", json!({})).await.unwrap_or_default()
    } else {
        AppInfo { version: "0.1.0".into(), platform: "web".into(), mobile: false }
    }
}

// ---------------------------------------------------------------- dictation

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub languages: Vec<String>,
    pub multilingual: bool,
    pub tier: String,
    pub downloaded: bool,
    pub downloading: bool,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Progress {
    pub id: String,
    pub received: u64,
    pub total: u64,
    pub state: String,
    pub error: Option<String>,
}

pub async fn stt_available() -> bool {
    native() && call::<bool>("stt_available", json!({})).await.unwrap_or(false)
}

pub async fn stt_models() -> Vec<ModelInfo> {
    if !native() {
        return vec![];
    }
    call("stt_models", json!({})).await.unwrap_or_default()
}

pub async fn stt_download(id: &str) -> Result<(), String> {
    call("stt_download", json!({ "id": id })).await
}

pub async fn stt_cancel(id: &str) {
    let _: Result<(), String> = call("stt_cancel", json!({ "id": id })).await;
}

pub async fn stt_delete(id: &str) -> Result<(), String> {
    call("stt_delete", json!({ "id": id })).await
}

pub async fn stt_transcribe(id: &str, audio_b64: String, language: Option<String>) -> Result<String, String> {
    if !native() {
        return Err("native-only".into());
    }
    call("stt_transcribe", json!({ "id": id, "audio": audio_b64, "language": language })).await
}
