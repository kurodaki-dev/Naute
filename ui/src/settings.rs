//! Settings: interface language, dictation models, about.

use crate::api::{self, ModelInfo};
use crate::i18n::{t, tf, LANGS};
use crate::state::{AppState, Route};
use crate::util::{fmt_size, Icon};
use leptos::prelude::*;

#[component]
pub fn Settings() -> impl IntoView {
    let app = expect_context::<AppState>();
    let models = RwSignal::new(Vec::<ModelInfo>::new());
    let progress = app.stt_progress;
    let available = RwSignal::new(None::<bool>);

    Effect::new(move |_| {
        let _ = app.stt_rev.get();
        leptos::task::spawn_local(async move {
            let a = api::stt_available().await;
            let list = api::stt_models().await;
            let _ = available.try_set(Some(a));
            let _ = models.try_set(list);
        });
    });
    let refresh = move || app.stt_rev.update(|r| *r += 1);

    let download = move |id: String| {
        progress.update(|m| {
            m.insert(id.clone(), (0, 1));
        });
        models.update(|ms| ms.iter_mut().filter(|m| m.id == id).for_each(|m| m.downloading = true));
        leptos::task::spawn_local(async move {
            let _ = api::stt_download(&id).await;
        });
    };

    let lang_name = |code: &str| LANGS.iter().find(|(c, _)| *c == code).map(|x| x.1.to_string()).unwrap_or_else(|| code.to_uppercase());

    view! {
        <div class="page settings">
            <header class="topbar">
                <button class="btn icon ghost" title=move || t("common.back") on:click=move |_| app.route.set(Route::Home)>
                    <Icon name="arrow_back"/>
                </button>
                <h1 class="display small grow">{move || t("nav.settings")}</h1>
            </header>
            <div class="settings-body">
                <section>
                    <h2 class="eyebrow">{move || t("settings.language")}</h2>
                    <p class="muted">{move || t("settings.language_hint")}</p>
                    <div class="lang-grid" role="radiogroup">
                        {LANGS.iter().map(|(code, name)| {
                            let code = code.to_string();
                            let c1 = code.clone();
                            let c2 = code.clone();
                            view! {
                                <button
                                    role="radio"
                                    class="lang"
                                    class:active=move || app.lang.get() == c1
                                    aria-checked=move || (app.lang.get() == c2).to_string()
                                    on:click={
                                        let code = code.clone();
                                        move |_| { app.lang.set(code.clone()); app.persist(); }
                                    }
                                >
                                    <span class="mono code">{code.to_uppercase()}</span>
                                    <span>{*name}</span>
                                </button>
                            }
                        }).collect_view()}
                    </div>
                </section>

                <section>
                    <h2 class="eyebrow">{move || t("settings.dictation")}</h2>
                    <p class="muted">{move || t("settings.dictation_hint")}</p>
                    <Show when=move || available.get() == Some(false)>
                        <p class="notice"><Icon name="info"/>{move || t("common.native_only")}</p>
                    </Show>
                    <label class="field inline">
                        <span>{move || t("settings.dictation_language")}</span>
                        <select on:change=move |ev| { app.stt_lang.set(event_target_value(&ev)); app.persist(); }>
                            <option value="app" selected=move || app.stt_lang.get() == "app">
                                {move || tf("settings.follow_app", &[&lang_name(&app.lang.get())])}
                            </option>
                            <option value="auto" selected=move || app.stt_lang.get() == "auto">{move || t("settings.autodetect")}</option>
                            {LANGS.iter().map(|(c, n)| {
                                let c = c.to_string();
                                let c2 = c.clone();
                                view! { <option value=c selected=move || app.stt_lang.get() == c2>{*n}</option> }
                            }).collect_view()}
                        </select>
                    </label>
                    <ol class="ledger models">
                        <For each=move || models.get() key=|m| (m.id.clone(), m.downloaded, m.downloading) let:m>
                            {
                                let id = m.id.clone();
                                let id_use = id.clone();
                                let id_active = id.clone();
                                let id_active2 = id.clone();
                                let id_prog = id.clone();
                                let id_prog2 = id.clone();
                                let id_dl = id.clone();
                                let id_cancel = id.clone();
                                let id_del = id.clone();
                                let langs = if m.languages.is_empty() {
                                    t("settings.langs_many")
                                } else if m.languages.len() == 1 {
                                    lang_name(&m.languages[0])
                                } else {
                                    tf("settings.langs_n", &[&m.languages.len().to_string()])
                                };
                                let tier = m.tier.clone();
                                view! {
                                    <li class="model-row" class:active=move || app.stt_model.get().as_deref() == Some(id_active.as_str())>
                                        <span class="cell-title">
                                            <span class="title">{m.name.clone()}</span>
                                            <span class="excerpt">
                                                {move || t(&format!("tier.{tier}"))}
                                                " · "
                                                {langs.clone()}
                                            </span>
                                        </span>
                                        <span class="mono muted right">{fmt_size(m.size)}</span>
                                        <span class="actions">
                                            {move || {
                                                if let Some((r, total)) = progress.get().get(&id_prog).copied() {
                                                    let pct = if total > 0 { r as f64 / total as f64 * 100.0 } else { 0.0 };
                                                    let idc = id_cancel.clone();
                                                    view! {
                                                        <span class="progress" title=format!("{pct:.0}%")><span style=format!("width:{pct:.1}%")></span></span>
                                                        <span class="mono small">{format!("{pct:.0}%")}</span>
                                                        <button class="btn icon ghost" title=move || t("common.cancel")
                                                            on:click=move |_| { let i = idc.clone(); leptos::task::spawn_local(async move { api::stt_cancel(&i).await }); }>
                                                            <Icon name="close"/>
                                                        </button>
                                                    }.into_any()
                                                } else if m.downloaded {
                                                    let iu = id_use.clone();
                                                    let idd = id_del.clone();
                                                    let ia = id_active2.clone();
                                                    view! {
                                                        <Show when=move || app.stt_model.get().as_deref() != Some(ia.as_str())
                                                            fallback=move || view! { <span class="badge"><Icon name="check"/>{move || t("settings.in_use")}</span> }>
                                                            {
                                                                let iu = iu.clone();
                                                                view! { <button class="btn" on:click=move |_| { app.stt_model.set(Some(iu.clone())); app.persist(); }>{move || t("settings.use")}</button> }
                                                            }
                                                        </Show>
                                                        <button class="btn icon ghost" title=move || t("common.delete") on:click=move |_| {
                                                            let i = idd.clone();
                                                            leptos::task::spawn_local(async move {
                                                                let _ = api::stt_delete(&i).await;
                                                                if app.stt_model.get_untracked().as_deref() == Some(i.as_str()) {
                                                                    app.stt_model.set(None);
                                                                    app.persist();
                                                                }
                                                                refresh();
                                                            });
                                                        }><Icon name="delete"/></button>
                                                    }.into_any()
                                                } else {
                                                    let i = id_dl.clone();
                                                    view! {
                                                        <button class="btn" on:click=move |_| download(i.clone())>
                                                            <Icon name="download"/>{move || t("settings.download")}
                                                        </button>
                                                    }.into_any()
                                                }
                                            }}
                                        </span>
                                        <span class="hidden">{id_prog2.clone()}</span>
                                    </li>
                                }
                            }
                        </For>
                    </ol>
                    <p class="fineprint">{move || t("settings.models_credit")}</p>
                </section>

                <section class="about">
                    <h2 class="eyebrow">{move || t("settings.about")}</h2>
                    <div class="about-card">
                        <img src="logo-copyright.svg" alt="Naute ©" width="88"/>
                        <div>
                            <p class="display small">"Naute"</p>
                            <p class="mono muted">{move || format!("v{} · {}", app.info.get().version, app.info.get().platform)}</p>
                            <p>"© 2026 Kurodaki. " {move || t("settings.rights")}</p>
                            <p class="muted">{move || t("settings.privacy")}</p>
                        </div>
                    </div>
                </section>
            </div>
        </div>
    }
}
