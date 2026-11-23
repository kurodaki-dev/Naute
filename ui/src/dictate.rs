//! Dictation: capture the microphone in the webview, resample to 16 kHz
//! mono, and hand the samples to the on-device model. One recording at a time
//! across the whole app.

use crate::api;
use crate::i18n::t;
use crate::state::AppState;
use crate::util::{toast, Icon};
use base64::Engine;
use leptos::prelude::*;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Idle,
    Starting,
    Recording,
    Working,
}

struct Recorder {
    ctx: web_sys::AudioContext,
    stream: web_sys::MediaStream,
    _node: web_sys::ScriptProcessorNode,
    _source: web_sys::MediaStreamAudioSourceNode,
    _cb: Closure<dyn FnMut(web_sys::AudioProcessingEvent)>,
    rate: f32,
}

thread_local! {
    static REC: RefCell<Option<Recorder>> = const { RefCell::new(None) };
    static BUF: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy)]
pub struct Dictation {
    pub phase: RwSignal<Phase>,
    pub level: RwSignal<f32>,
    pub started: RwSignal<f64>,
    /// Which control owns the current recording.
    pub owner: RwSignal<u64>,
}

impl Dictation {
    pub fn new() -> Self {
        Dictation { phase: RwSignal::new(Phase::Idle), level: RwSignal::new(0.0), started: RwSignal::new(0.0), owner: RwSignal::new(0) }
    }
}

async fn start(d: Dictation) -> Result<(), String> {
    let nav = web_sys::window().unwrap().navigator();
    let md = nav.media_devices().map_err(|_| "no-mic".to_string())?;
    let c = web_sys::MediaStreamConstraints::new();
    c.set_audio(&JsValue::TRUE);
    let p = md.get_user_media_with_constraints(&c).map_err(|_| "no-mic".to_string())?;
    let stream: web_sys::MediaStream = wasm_bindgen_futures::JsFuture::from(p).await.map_err(|_| "mic-denied".to_string())?.unchecked_into();
    let ctx = web_sys::AudioContext::new().map_err(|_| "no-audio".to_string())?;
    let source = ctx.create_media_stream_source(&stream).map_err(|_| "no-audio".to_string())?;
    let node = ctx
        .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(4096, 1, 1)
        .map_err(|_| "no-audio".to_string())?;
    BUF.with(|b| b.borrow_mut().clear());
    let level = d.level;
    let cb = Closure::<dyn FnMut(web_sys::AudioProcessingEvent)>::new(move |ev: web_sys::AudioProcessingEvent| {
        if let Ok(buf) = ev.input_buffer() {
            if let Ok(data) = buf.get_channel_data(0) {
                let rms = (data.iter().map(|x| x * x).sum::<f32>() / data.len().max(1) as f32).sqrt();
                level.set((rms * 6.0).min(1.0));
                BUF.with(|b| b.borrow_mut().extend_from_slice(&data));
            }
        }
    });
    node.set_onaudioprocess(Some(cb.as_ref().unchecked_ref()));
    source.connect_with_audio_node(&node).map_err(|_| "no-audio".to_string())?;
    node.connect_with_audio_node(&ctx.destination()).map_err(|_| "no-audio".to_string())?;
    let rate = ctx.sample_rate();
    REC.with(|r| *r.borrow_mut() = Some(Recorder { ctx, stream, _node: node, _source: source, _cb: cb, rate }));
    Ok(())
}

fn stop() -> Option<Vec<f32>> {
    let rec = REC.with(|r| r.borrow_mut().take())?;
    rec._node.set_onaudioprocess(None);
    let _ = rec.ctx.close();
    for t in rec.stream.get_tracks().iter() {
        if let Ok(track) = t.dyn_into::<web_sys::MediaStreamTrack>() {
            track.stop();
        }
    }
    let raw = BUF.with(|b| std::mem::take(&mut *b.borrow_mut()));
    Some(resample(&raw, rec.rate, 16_000.0))
}

/// Linear resampling with a small box filter when downsampling.
fn resample(x: &[f32], from: f32, to: f32) -> Vec<f32> {
    if (from - to).abs() < 1.0 || x.is_empty() {
        return x.to_vec();
    }
    let ratio = from / to;
    let n = (x.len() as f32 / ratio) as usize;
    let half = (ratio / 2.0).floor() as isize;
    (0..n)
        .map(|i| {
            let pos = i as f32 * ratio;
            let c = pos as isize;
            if half >= 1 {
                let (a, b) = ((c - half).max(0) as usize, ((c + half) as usize).min(x.len() - 1));
                x[a..=b].iter().sum::<f32>() / (b - a + 1) as f32
            } else {
                let f = pos - c as f32;
                let a = x[c as usize];
                let b = x[(c as usize + 1).min(x.len() - 1)];
                a + (b - a) * f
            }
        })
        .collect()
}

fn encode(pcm: &[f32]) -> String {
    let mut bytes = Vec::with_capacity(pcm.len() * 4);
    for s in pcm {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn error_text(e: &str) -> String {
    match e {
        "mic-denied" | "no-mic" => t("dictation.mic_denied"),
        "model-missing" => t("dictation.no_model"),
        "native-only" => t("common.native_only"),
        _ => format!("{}: {e}", t("dictation.failed")),
    }
}

/// Microphone toggle. Recognised text is passed to `on_text`.
#[component]
pub fn DictateButton(on_text: Callback<String>, #[prop(optional)] label: bool) -> impl IntoView {
    let app = expect_context::<AppState>();
    let d = app.dictation;
    let my_id = crate::util::now() as u64 ^ (js_sys::Math::random() * 1e9) as u64;
    let mine = move || d.owner.get() == my_id;
    let click = move |_| {
        match d.phase.get_untracked() {
            Phase::Idle => {
                let Some(model) = app.stt_model.get_untracked() else {
                    toast(t("dictation.no_model"), Some((t("nav.settings"), "settings".into())));
                    return;
                };
                d.owner.set(my_id);
                d.phase.set(Phase::Starting);
                leptos::task::spawn_local(async move {
                    match start(d).await {
                        Ok(()) => {
                            d.started.set(crate::util::now());
                            d.phase.set(Phase::Recording);
                        }
                        Err(e) => {
                            d.phase.set(Phase::Idle);
                            toast(error_text(&e), None);
                        }
                    }
                    let _ = model;
                });
            }
            Phase::Recording if d.owner.get_untracked() == my_id => {
                let Some(pcm) = stop() else {
                    d.phase.set(Phase::Idle);
                    return;
                };
                d.phase.set(Phase::Working);
                d.level.set(0.0);
                let model = app.stt_model.get_untracked().unwrap_or_default();
                let lang = app.stt_language();
                leptos::task::spawn_local(async move {
                    let res = api::stt_transcribe(&model, encode(&pcm), lang).await;
                    d.phase.set(Phase::Idle);
                    match res {
                        Ok(text) if !text.trim().is_empty() => on_text.run(text),
                        Ok(_) => toast(t("dictation.nothing_heard"), None),
                        Err(e) => toast(error_text(&e), None),
                    }
                });
            }
            _ => {}
        }
    };
    let elapsed = move || {
        let _ = d.level.get();
        let s = ((crate::util::now() - d.started.get_untracked()) / 1000.0).max(0.0) as u32;
        format!("{}:{:02}", s / 60, s % 60)
    };
    view! {
        <button
            class="btn icon dictate"
            class:rec=move || mine() && d.phase.get() == Phase::Recording
            class:busy=move || mine() && matches!(d.phase.get(), Phase::Working | Phase::Starting)
            disabled=move || !mine() && d.phase.get() != Phase::Idle
            title=move || t("dictation.button")
            on:click=click
        >
            {move || {
                if mine() && d.phase.get() == Phase::Recording {
                    view! {
                        <span class="meter" style=move || format!("--lvl:{:.2}", d.level.get())></span>
                        <span class="mono time">{elapsed}</span>
                    }
                        .into_any()
                } else if mine() && matches!(d.phase.get(), Phase::Working | Phase::Starting) {
                    view! { <Icon name="progress_activity"/> <span class="lbl">{move || t("dictation.working")}</span> }.into_any()
                } else {
                    view! {
                        <Icon name="mic"/>
                        {label.then(|| view! { <span class="lbl">{move || t("dictation.button")}</span> })}
                    }
                        .into_any()
                }
            }}
        </button>
    }
}

#[cfg(test)]
mod tests {
    use super::resample;
    #[test]
    fn resample_len() {
        let x = vec![0.0; 48_000];
        assert_eq!(resample(&x, 48_000.0, 16_000.0).len(), 16_000);
    }
}
