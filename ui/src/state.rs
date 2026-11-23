//! App-wide state, provided as context.

use crate::api;
use crate::dictate::Dictation;
use leptos::prelude::*;
use naute_core::model::DocMeta;
use serde_json::json;

#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Home,
    Edit(String),
    Settings,
}

#[derive(Clone, Copy)]
pub struct AppState {
    pub route: RwSignal<Route>,
    pub lang: RwSignal<String>,
    pub stt_model: RwSignal<Option<String>>,
    /// "app" (follow interface language), "auto", or a language code.
    pub stt_lang: RwSignal<String>,
    pub docs: RwSignal<Vec<DocMeta>>,
    pub dictation: Dictation,
    pub info: RwSignal<api::AppInfo>,
    /// Model download progress (received, total) by model id.
    pub stt_progress: RwSignal<std::collections::HashMap<String, (u64, u64)>>,
    /// Bumped whenever the set of downloaded models changes.
    pub stt_rev: RwSignal<u32>,
}

impl AppState {
    pub fn stt_language(&self) -> Option<String> {
        match self.stt_lang.get_untracked().as_str() {
            "auto" => None,
            "app" | "" => Some(self.lang.get_untracked()),
            c => Some(c.to_string()),
        }
    }

    pub fn persist(&self) {
        let v = json!({
            "lang": self.lang.get_untracked(),
            "stt_model": self.stt_model.get_untracked(),
            "stt_lang": self.stt_lang.get_untracked(),
        });
        leptos::task::spawn_local(async move { api::set_settings(&v).await });
    }

    pub fn reload_docs(&self) {
        let docs = self.docs;
        leptos::task::spawn_local(async move {
            if let Ok(list) = api::list_documents().await {
                docs.set(list);
            }
        });
    }
}
