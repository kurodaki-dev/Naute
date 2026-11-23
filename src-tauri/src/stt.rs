//! On-device dictation with the models Handy ships, run through
//! transcribe.cpp. Audio is captured by the UI, resampled to 16 kHz mono and
//! handed over here; it never leaves the device. Models are downloaded once
//! from Handy's Hugging Face mirrors and verified against their SHA-256.

use crate::storage::data_dir;
use base64::Engine;
use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};
use tokio::io::AsyncWriteExt;

pub struct CatalogModel {
    pub id: &'static str,
    pub name: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
    pub file: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    /// Comma separated ISO codes, or "*" for Whisper's ~100 languages.
    pub languages: &'static str,
    /// Hint shown in the model list: speed / accuracy trade-off.
    pub tier: &'static str,
}

const WHISPER: &str = "*";

/// The curated part of Handy's model catalog, with quantisations chosen to
/// stay comfortable on phones.
pub const CATALOG: &[CatalogModel] = &[
    CatalogModel { id: "parakeet-tdt-0.6b-v3", name: "Parakeet V3", repo: "handy-computer/parakeet-tdt-0.6b-v3-gguf", revision: "85ac09ea12fc4b1112fa76810059364bc6adc9de", file: "parakeet-tdt-0.6b-v3-Q5_K_M.gguf", size: 548946272, sha256: "cc722e76adc1a629fc0b2535de879d99b8160d07ad4c0215e2ca7d7ea0ae4b8f", languages: "bg,hr,cs,da,nl,en,et,fi,fr,de,el,hu,it,lv,lt,mt,pl,pt,ro,ru,sk,sl,es,sv,uk", tier: "fast" },
    CatalogModel { id: "parakeet-tdt-0.6b-v2", name: "Parakeet V2", repo: "handy-computer/parakeet-tdt-0.6b-v2-gguf", revision: "07cee0616125a08ef619729bb47f40ef747e4bc4", file: "parakeet-tdt-0.6b-v2-Q5_K_M.gguf", size: 539012608, sha256: "dfa904520b95451599683613fea47766c378c5fdf5d8cddf48151226b6eaec85", languages: "en", tier: "fast" },
    CatalogModel { id: "whisper-base", name: "Whisper Base", repo: "handy-computer/whisper-base-gguf", revision: "e0f69524f648720eca44c024d1d0dbb7027d1fa0", file: "whisper-base-Q8_0.gguf", size: 84962880, sha256: "81c069428bc8a24551a8169cf31cf09bcfd9d4cf50389ae281323c9aa9648c81", languages: WHISPER, tier: "light" },
    CatalogModel { id: "whisper-small", name: "Whisper Small", repo: "handy-computer/whisper-small-gguf", revision: "c0214bd34be9296695486f838e0142f900803159", file: "whisper-small-Q5_K_M.gguf", size: 193749056, sha256: "326cd00c3e7217c751667c7c1600eaf7e0de174e186ca2c16b4bf590251c3c3b", languages: WHISPER, tier: "balanced" },
    CatalogModel { id: "whisper-medium", name: "Whisper Medium", repo: "handy-computer/whisper-medium-gguf", revision: "ec78f06fded51aa82cde751678b78f76f78c8b7f", file: "whisper-medium-Q5_K_M.gguf", size: 582746048, sha256: "4e2a8904a866b3aa7ef70d7640ec6abc5f0a05524cd950ea4b66ace12122bf53", languages: WHISPER, tier: "accurate" },
    CatalogModel { id: "whisper-large-v3-turbo", name: "Whisper Turbo", repo: "handy-computer/whisper-large-v3-turbo-gguf", revision: "5eaf945c7978e564bae5b28a5b1639dd93c2bfb1", file: "whisper-large-v3-turbo-Q5_K_M.gguf", size: 619628128, sha256: "977b5db4e004349dffd1ab9caa10ba5aaba3fc3edd3ba72cadb84328a3203e36", languages: WHISPER, tier: "accurate" },
    CatalogModel { id: "whisper-large-v3", name: "Whisper Large", repo: "handy-computer/whisper-large-v3-gguf", revision: "e3e29bee6389c7da4a141406f07bb80ddac5337c", file: "whisper-large-v3-Q5_K_M.gguf", size: 1161143008, sha256: "6053d0fd69a0fd48b8fea5ea7a52b9e0cde389343566fa30e453a1b2b258dc38", languages: WHISPER, tier: "best" },
    CatalogModel { id: "canary-180m-flash", name: "Canary 180M Flash", repo: "handy-computer/canary-180m-flash-gguf", revision: "46b86384d1db59108d6e060a0fc00924cb58499f", file: "canary-180m-flash-Q8_0.gguf", size: 218447648, sha256: "f942ab746040b531a29ab78ca3afc8dc9564efd8e4c942b5541e53644be9c408", languages: "en,de,es,fr", tier: "light" },
    CatalogModel { id: "moonshine-base", name: "Moonshine Base", repo: "handy-computer/moonshine-base-gguf", revision: "3ef112378a8cf46ac8b278d9bfa2d15c846704b8", file: "moonshine-base-Q8_0.gguf", size: 77476480, sha256: "7f0027dfd857d310b63a85ef57cadf183da712cc374f85a648f8bc18aaa2efc8", languages: "en", tier: "light" },
    CatalogModel { id: "SenseVoiceSmall", name: "SenseVoice", repo: "handy-computer/SenseVoiceSmall-gguf", revision: "4a08b8e900b38a977e32eb08d5d0697d6e72ba04", file: "SenseVoiceSmall-Q8_0.gguf", size: 252684608, sha256: "6c759ee4c9748c9b3f7a5a60ca74f0f7e685fb9d45d1378fce7cfd62f59adf29", languages: "zh,yue,en,ja,ko", tier: "fast" },
    CatalogModel { id: "Qwen3-ASR-0.6B", name: "Qwen3-ASR 0.6B", repo: "handy-computer/Qwen3-ASR-0.6B-gguf", revision: "e4e16599b900eb0cb36e524514756bb92eb092b7", file: "Qwen3-ASR-0.6B-Q5_K_M.gguf", size: 645356192, sha256: "062a7bcb18675c2fe5ffd0e8b354eac6a1ceada9b2af9ef7168746ae16b54358", languages: "zh,en,yue,ar,de,fr,es,pt,id,it,ko,ru,th,vi,ja,tr,hi,ms,nl,sv,da,fi,pl,cs,fil,fa,el,ro,hu,mk", tier: "accurate" },
];

#[derive(Default)]
pub struct SttState {
    downloads: Mutex<HashMap<String, Arc<AtomicBool>>>,
    #[cfg(feature = "stt")]
    loaded: Mutex<Option<(String, transcribe_cpp::Model)>>,
}

#[derive(Serialize)]
pub struct ModelInfo {
    id: String,
    name: String,
    size: u64,
    languages: Vec<String>,
    multilingual: bool,
    tier: String,
    downloaded: bool,
    downloading: bool,
}

#[derive(Serialize, Clone)]
pub struct Progress {
    id: String,
    received: u64,
    total: u64,
    state: &'static str,
    error: Option<String>,
}

fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let d = data_dir(app)?.join("models");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

fn find(id: &str) -> Result<&'static CatalogModel, String> {
    CATALOG.iter().find(|m| m.id == id).ok_or_else(|| format!("unknown model {id}"))
}

fn model_path(app: &AppHandle, m: &CatalogModel) -> Result<PathBuf, String> {
    Ok(models_dir(app)?.join(m.file))
}

#[tauri::command]
pub fn stt_available() -> bool {
    cfg!(feature = "stt")
}

#[tauri::command]
pub fn stt_models(app: AppHandle, state: State<'_, SttState>) -> Result<Vec<ModelInfo>, String> {
    let dl = state.downloads.lock().unwrap();
    CATALOG
        .iter()
        .map(|m| {
            let p = model_path(&app, m)?;
            Ok(ModelInfo {
                id: m.id.into(),
                name: m.name.into(),
                size: m.size,
                languages: if m.languages == WHISPER { vec![] } else { m.languages.split(',').map(String::from).collect() },
                multilingual: m.languages == WHISPER || m.languages.contains(','),
                tier: m.tier.into(),
                downloaded: p.metadata().map(|md| md.len() == m.size).unwrap_or(false),
                downloading: dl.contains_key(m.id),
            })
        })
        .collect()
}

#[tauri::command]
pub async fn stt_download(app: AppHandle, state: State<'_, SttState>, id: String) -> Result<(), String> {
    let m = find(&id)?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut dl = state.downloads.lock().unwrap();
        if dl.contains_key(&id) {
            return Ok(());
        }
        dl.insert(id.clone(), cancel.clone());
    }
    let result = download(&app, m, &cancel).await;
    state.downloads.lock().unwrap().remove(&id);
    let (st, err) = match &result {
        Ok(()) => ("done", None),
        Err(e) if e == "cancelled" => ("cancelled", None),
        Err(e) => ("error", Some(e.clone())),
    };
    let _ = app.emit("stt-progress", Progress { id: id.clone(), received: 0, total: m.size, state: st, error: err });
    result
}

async fn download(app: &AppHandle, m: &CatalogModel, cancel: &AtomicBool) -> Result<(), String> {
    let dest = model_path(app, m)?;
    let part = dest.with_extension("part");
    let url = format!("https://huggingface.co/{}/resolve/{}/{}", m.repo, m.revision, m.file);
    let client = reqwest::Client::builder().user_agent("Naute").build().map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(m.size);
    let mut file = tokio::fs::File::create(&part).await.map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    let mut last_emit = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = tokio::fs::remove_file(&part).await;
            return Err("cancelled".into());
        }
        let chunk = chunk.map_err(|e| e.to_string())?;
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        received += chunk.len() as u64;
        if received - last_emit > 1_000_000 || received == total {
            last_emit = received;
            let _ = app.emit("stt-progress", Progress { id: m.id.into(), received, total, state: "downloading", error: None });
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    let digest = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>();
    if digest != m.sha256 {
        let _ = tokio::fs::remove_file(&part).await;
        return Err("checksum mismatch".into());
    }
    tokio::fs::rename(&part, &dest).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn stt_cancel(state: State<'_, SttState>, id: String) {
    if let Some(flag) = state.downloads.lock().unwrap().get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
pub fn stt_delete(app: AppHandle, state: State<'_, SttState>, id: String) -> Result<(), String> {
    let m = find(&id)?;
    #[cfg(feature = "stt")]
    {
        let mut l = state.loaded.lock().unwrap();
        if l.as_ref().map(|(i, _)| i == &id).unwrap_or(false) {
            *l = None;
        }
    }
    #[cfg(not(feature = "stt"))]
    let _ = state;
    let p = model_path(&app, m)?;
    if p.exists() {
        std::fs::remove_file(p).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Map UI language codes to what speech models expect.
fn speech_lang(code: &str) -> String {
    match code {
        "nb" | "nn" => "no".into(),
        "zh" => "zh".into(),
        c => c.split('-').next().unwrap_or(c).to_string(),
    }
}

/// `audio` is base64 of little-endian f32 samples, 16 kHz mono.
#[tauri::command]
pub async fn stt_transcribe(
    app: AppHandle,
    state: State<'_, SttState>,
    id: String,
    audio: String,
    language: Option<String>,
) -> Result<String, String> {
    let m = find(&id)?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(audio).map_err(|e| e.to_string())?;
    let pcm: Vec<f32> = bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    if pcm.len() < 1600 {
        return Ok(String::new());
    }
    let path = model_path(&app, m)?;
    if !path.exists() {
        return Err("model-missing".into());
    }
    let lang = language.filter(|l| !l.is_empty() && l != "auto").map(|l| speech_lang(&l)).filter(|l| {
        m.languages == WHISPER || (m.languages.contains(',') && m.languages.split(',').any(|x| x == l))
    });
    run(&state, m, path, pcm, lang).await
}

#[cfg(feature = "stt")]
async fn run(state: &State<'_, SttState>, m: &CatalogModel, path: PathBuf, pcm: Vec<f32>, lang: Option<String>) -> Result<String, String> {
    use transcribe_cpp::{Model, RunOptions};
    // Load (or reuse) the model, then transcribe on a blocking thread.
    let model = {
        let mut l = state.loaded.lock().unwrap();
        match l.as_ref() {
            Some((id, model)) if id == m.id => model.clone(),
            _ => {
                *l = None;
                let model = Model::load(&path).map_err(|e| e.to_string())?;
                *l = Some((m.id.to_string(), model.clone()));
                model
            }
        }
    };
    tauri::async_runtime::spawn_blocking(move || {
        let mut session = model.session().map_err(|e| e.to_string())?;
        let opts = RunOptions { language: lang.clone(), ..Default::default() };
        let res = match session.run(&pcm, &opts) {
            Ok(r) => r,
            // Some families take no language hint: retry with autodetect.
            Err(_) if lang.is_some() => session.run(&pcm, &RunOptions::default()).map_err(|e| e.to_string())?,
            Err(e) => return Err(e.to_string()),
        };
        Ok(res.text.trim().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(not(feature = "stt"))]
async fn run(_: &State<'_, SttState>, _: &CatalogModel, _: PathBuf, _: Vec<f32>, _: Option<String>) -> Result<String, String> {
    Err("stt-unavailable".into())
}
