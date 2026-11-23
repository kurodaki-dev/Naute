use crate::api;
use crate::dictate::Dictation;
use crate::editors::Editor;
use crate::home::Home;
use crate::i18n::{self, t};
use crate::settings::Settings;
use crate::state::{AppState, Route};
use crate::util::{toasts, Icon};
use leptos::prelude::*;

#[component]
pub fn App() -> impl IntoView {
    let lang = i18n::init(&i18n::detect());
    let state = AppState {
        route: RwSignal::new(Route::Home),
        lang,
        stt_model: RwSignal::new(None),
        stt_lang: RwSignal::new("app".into()),
        docs: RwSignal::new(vec![]),
        dictation: Dictation::new(),
        info: RwSignal::new(Default::default()),
        stt_progress: RwSignal::new(Default::default()),
        stt_rev: RwSignal::new(0),
    };
    provide_context(state);

    api::listen::<api::Progress>("stt-progress", move |p| {
        if p.state == "downloading" {
            state.stt_progress.update(|m| {
                m.insert(p.id.clone(), (p.received, p.total));
            });
            return;
        }
        state.stt_progress.update(|m| {
            m.remove(&p.id);
        });
        match p.state.as_str() {
            // The first model downloaded becomes the active one.
            "done" if state.stt_model.get_untracked().is_none() => {
                state.stt_model.set(Some(p.id.clone()));
                state.persist();
            }
            "error" => crate::util::toast(crate::i18n::tf("settings.download_failed", &[&p.error.unwrap_or_default()]), None),
            _ => {}
        }
        state.stt_rev.update(|r| *r += 1);
    });
    let ready = RwSignal::new(false);

    leptos::task::spawn_local(async move {
        naute_core::model::seed_ids(crate::util::now());
        let s = api::get_settings().await;
        if let Some(l) = s.get("lang").and_then(|v| v.as_str()) {
            if i18n::LANGS.iter().any(|(c, _)| *c == l) {
                state.lang.set(l.into());
            }
        }
        if let Some(m) = s.get("stt_model").and_then(|v| v.as_str()) {
            state.stt_model.set(Some(m.into()));
        }
        if let Some(m) = s.get("stt_lang").and_then(|v| v.as_str()) {
            state.stt_lang.set(m.into());
        }
        state.info.set(api::app_info().await);
        // Measure text only once the bundled fonts are in.
        if let Ok(fonts) = js_sys::Reflect::get(&crate::util::document(), &"fonts".into()) {
            if let Ok(p) = js_sys::Reflect::get(&fonts, &"ready".into()) {
                let _ = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(p)).await;
            }
        }
        if let Ok(list) = api::list_documents().await {
            state.docs.set(list);
        }
        ready.set(true);
    });

    // Keep <html lang> in sync for screen readers and hyphenation.
    Effect::new(move |_| {
        let l = state.lang.get();
        if let Some(el) = crate::util::document().document_element() {
            let _ = el.set_attribute("lang", &l);
        }
    });

    let on_toast_action = move |cmd: String| {
        if cmd == "settings" {
            state.route.set(Route::Settings);
        } else if let Some(path) = cmd.strip_prefix("reveal:") {
            let p = path.to_string();
            leptos::task::spawn_local(async move { api::reveal_file(&p).await });
        }
    };

    view! {
        <Show when=move || ready.get() fallback=|| view! { <div class="splash"><img src="logo.svg" alt="Naute"/></div> }>
            {move || match state.route.get() {
                Route::Home => view! { <Home/> }.into_any(),
                Route::Settings => view! { <Settings/> }.into_any(),
                Route::Edit(id) => view! { <Editor id=id/> }.into_any(),
            }}
        </Show>
        <div class="toasts" role="status" aria-live="polite">
            <For each=move || toasts().get() key=|t| t.id let:toast>
                <div class="toast">
                    <span>{toast.text.clone()}</span>
                    {toast.action.clone().map(|(label, cmd)| {
                        view! {
                            <button class="btn text" on:click=move |_| on_toast_action(cmd.clone())>
                                {label}
                                <Icon name="chevron_right"/>
                            </button>
                        }
                    })}
                </div>
            </For>
        </div>
        <noscript>{move || t("common.loading")}</noscript>
    }
}
