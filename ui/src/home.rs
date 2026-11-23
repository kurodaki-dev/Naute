//! The library: every document, filterable by kind, plus creation and import.

use crate::api;
use crate::i18n::t;
use crate::state::{AppState, Route};
use crate::util::{fmt_date, now, toast, Icon};
use leptos::prelude::*;
use naute_core::model::*;

pub fn kind_icon(k: DocKind) -> &'static str {
    match k {
        DocKind::Note => "description",
        DocKind::Sheet => "table_chart",
        DocKind::Design => "design_services",
        DocKind::Slides => "slideshow",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Kind(DocKind),
}

#[component]
pub fn Brand() -> impl IntoView {
    view! {
        <div class="brand">
            <img src="logo.svg" alt="" width="28" height="28"/>
            <span class="word">"Naute"</span>
        </div>
    }
}

#[component]
pub fn Home() -> impl IntoView {
    let app = expect_context::<AppState>();
    let filter = RwSignal::new(Filter::All);
    let query = RwSignal::new(String::new());
    let creating = RwSignal::new(false);
    let menu_for = RwSignal::new(None::<String>);
    let confirm_delete = RwSignal::new(None::<String>);

    let visible = Memo::new(move |_| {
        let q = query.get().to_lowercase();
        let f = filter.get();
        app.docs.with(|docs| {
            docs.iter()
                .filter(|d| match f {
                    Filter::All => true,
                    Filter::Kind(k) => d.kind == k,
                })
                .filter(|d| q.is_empty() || d.title.to_lowercase().contains(&q) || d.excerpt.to_lowercase().contains(&q))
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let count = move |k: Option<DocKind>| app.docs.with(|d| d.iter().filter(|x| k.map(|k| x.kind == k).unwrap_or(true)).count());

    let import = move |_| {
        leptos::task::spawn_local(async move {
            match api::import_document(now()).await {
                Ok(Some(meta)) => {
                    app.reload_docs();
                    app.route.set(Route::Edit(meta.id));
                }
                Ok(None) => {}
                Err(e) if e == "native-only" => toast(t("common.native_only"), None),
                Err(e) if e == "not-a-naute-file" => toast(t("import.invalid"), None),
                Err(e) => toast(e, None),
            }
        });
    };

    let duplicate = move |id: String| {
        leptos::task::spawn_local(async move {
            if let Ok(mut d) = api::load_document(&id).await {
                d.meta.id = new_id();
                d.meta.title = format!("{} ({})", d.meta.title, t("common.copy"));
                d.meta.created = now();
                d.meta.updated = now();
                d.meta.pinned = false;
                let _ = api::save_document(&d).await;
                app.reload_docs();
            }
        });
    };
    let toggle_pin = move |id: String| {
        leptos::task::spawn_local(async move {
            if let Ok(mut d) = api::load_document(&id).await {
                d.meta.pinned = !d.meta.pinned;
                let _ = api::save_document(&d).await;
                app.reload_docs();
            }
        });
    };
    let delete = move |id: String| {
        leptos::task::spawn_local(async move {
            let _ = api::delete_document(&id).await;
            confirm_delete.set(None);
            app.reload_docs();
        });
    };

    let nav_item = move |f: Filter, icon: &'static str, key: &'static str, k: Option<DocKind>| {
        view! {
            <button class="nav-item" class:active=move || filter.get() == f on:click=move |_| filter.set(f)>
                <Icon name=icon/>
                <span class="grow">{move || t(key)}</span>
                <span class="mono count">{move || count(k)}</span>
            </button>
        }
    };

    let title = move || match filter.get() {
        Filter::All => t("nav.all"),
        Filter::Kind(k) => t(&format!("kind.{}.plural", k.key())),
    };

    view! {
        <div class="shell" on:click=move |_| menu_for.set(None)>
            <aside class="sidebar">
                <Brand/>
                <nav>
                    {nav_item(Filter::All, "grid_view", "nav.all", None)}
                    <div class="nav-rule"></div>
                    {nav_item(Filter::Kind(DocKind::Note), "description", "kind.note.plural", Some(DocKind::Note))}
                    {nav_item(Filter::Kind(DocKind::Sheet), "table_chart", "kind.sheet.plural", Some(DocKind::Sheet))}
                    {nav_item(Filter::Kind(DocKind::Design), "design_services", "kind.design.plural", Some(DocKind::Design))}
                    {nav_item(Filter::Kind(DocKind::Slides), "slideshow", "kind.slides.plural", Some(DocKind::Slides))}
                </nav>
                <div class="grow"></div>
                <button class="nav-item" on:click=import>
                    <Icon name="file_open"/>
                    <span class="grow">{move || t("nav.import")}</span>
                </button>
                <button class="nav-item" on:click=move |_| app.route.set(Route::Settings)>
                    <Icon name="settings"/>
                    <span class="grow">{move || t("nav.settings")}</span>
                </button>
                <p class="legal">"© 2026 Kurodaki"</p>
            </aside>

            <main class="library">
                <header class="lib-head">
                    <div class="mobile-only row between">
                        <Brand/>
                        <div class="row">
                            <button class="btn icon" title=move || t("nav.import") on:click=import><Icon name="file_open"/></button>
                            <button class="btn icon" title=move || t("nav.settings") on:click=move |_| app.route.set(Route::Settings)>
                                <Icon name="settings"/>
                            </button>
                        </div>
                    </div>
                    <div class="row between wrap gap">
                        <h1 class="display">{title}</h1>
                        <div class="row gap">
                            <label class="search">
                                <Icon name="search"/>
                                <input
                                    type="search"
                                    placeholder=move || t("home.search")
                                    prop:value=move || query.get()
                                    on:input=move |ev| query.set(event_target_value(&ev))
                                />
                            </label>
                            <button class="btn primary desktop-only" on:click=move |_| creating.set(true)>
                                <Icon name="add"/>
                                <span>{move || t("home.new")}</span>
                            </button>
                        </div>
                    </div>
                    <div class="tabs mobile-only">
                        <button class:active=move || filter.get() == Filter::All on:click=move |_| filter.set(Filter::All)>{move || t("nav.all")}</button>
                        {DocKind::ALL.into_iter().map(|k| view! {
                            <button class:active=move || filter.get() == Filter::Kind(k) on:click=move |_| filter.set(Filter::Kind(k))>
                                {move || t(&format!("kind.{}.plural", k.key()))}
                            </button>
                        }).collect_view()}
                    </div>
                </header>

                <Show
                    when=move || !visible.get().is_empty()
                    fallback=move || view! {
                        <div class="empty">
                            <p class="display small">{move || if app.docs.with(|d| d.is_empty()) { t("home.empty_title") } else { t("home.no_results") }}</p>
                            <p class="muted">{move || t("home.empty_body")}</p>
                            <button class="btn primary" on:click=move |_| creating.set(true)><Icon name="add"/><span>{move || t("home.new")}</span></button>
                        </div>
                    }
                >
                    <ol class="ledger">
                        <li class="ledger-head" aria-hidden="true">
                            <span></span>
                            <span>{move || t("home.col.title")}</span>
                            <span class="desktop-only">{move || t("home.col.kind")}</span>
                            <span class="right">{move || t("home.col.updated")}</span>
                            <span></span>
                        </li>
                        <For each=move || visible.get() key=|d| (d.id.clone(), d.updated as u64, d.pinned, d.title.clone()) let:doc>
                            {
                                let id = doc.id.clone();
                                let id_open = id.clone();
                                let id_menu = id.clone();
                                let id_menu2 = id.clone();
                                let id_pin = id.clone();
                                let id_dup = id.clone();
                                let id_del = id.clone();
                                let id_del2 = id.clone();
                                let id_del3 = id.clone();
                                let pinned = doc.pinned;
                                let kind = doc.kind;
                                view! {
                                    <li class="ledger-row" on:click=move |_| app.route.set(Route::Edit(id_open.clone()))>
                                        <span class=format!("glyph k-{}", kind.key())><Icon name=kind_icon(kind)/></span>
                                        <span class="cell-title">
                                            <span class="title">
                                                {pinned.then(|| view! { <Icon name="push_pin" fill=true/> })}
                                                {if doc.title.trim().is_empty() { t("common.untitled") } else { doc.title.clone() }}
                                            </span>
                                            <span class="excerpt">{doc.excerpt.clone()}</span>
                                        </span>
                                        <span class="kind mono desktop-only">{move || t(&format!("kind.{}", kind.key()))}</span>
                                        <span class="date mono right">{fmt_date(doc.updated)}</span>
                                        <span class="menu-wrap">
                                            <button
                                                class="btn icon ghost"
                                                title=move || t("common.more")
                                                on:click=move |ev| {
                                                    ev.stop_propagation();
                                                    let cur = menu_for.get_untracked();
                                                    menu_for.set(if cur.as_deref() == Some(id_menu.as_str()) { None } else { Some(id_menu.clone()) });
                                                    confirm_delete.set(None);
                                                }
                                            >
                                                <Icon name="more_horiz"/>
                                            </button>
                                            <Show when=move || menu_for.get().as_deref() == Some(id_menu2.as_str())>
                                                {
                                                    let (id_pin, id_dup, id_del, id_del2, id_del3) = (id_pin.clone(), id_dup.clone(), id_del.clone(), id_del2.clone(), id_del3.clone());
                                                    view! {
                                                        <div class="menu" on:click=|ev| ev.stop_propagation()>
                                                            <button on:click=move |_| { toggle_pin(id_pin.clone()); menu_for.set(None); }>
                                                                <Icon name="push_pin"/>
                                                                {move || if pinned { t("home.unpin") } else { t("home.pin") }}
                                                            </button>
                                                            <button on:click=move |_| { duplicate(id_dup.clone()); menu_for.set(None); }>
                                                                <Icon name="content_copy"/>
                                                                {move || t("common.duplicate")}
                                                            </button>
                                                            <Show
                                                                when=move || confirm_delete.get().as_deref() == Some(id_del.as_str())
                                                                fallback=move || {
                                                                    let i = id_del2.clone();
                                                                    view! {
                                                                        <button class="danger" on:click=move |_| confirm_delete.set(Some(i.clone()))>
                                                                            <Icon name="delete"/>
                                                                            {move || t("common.delete")}
                                                                        </button>
                                                                    }
                                                                }
                                                            >
                                                                {
                                                                    let i = id_del3.clone();
                                                                    view! {
                                                                        <button class="danger strong" on:click=move |_| delete(i.clone())>
                                                                            <Icon name="delete"/>
                                                                            {move || t("home.confirm_delete")}
                                                                        </button>
                                                                    }
                                                                }
                                                            </Show>
                                                        </div>
                                                    }
                                                }
                                            </Show>
                                        </span>
                                    </li>
                                }
                            }
                        </For>
                    </ol>
                </Show>
                <button class="fab mobile-only" on:click=move |_| creating.set(true)>
                    <Icon name="add"/>
                    <span>{move || t("home.new")}</span>
                </button>
            </main>
        </div>
        <Show when=move || creating.get()>
            <NewDialog close=Callback::new(move |_| creating.set(false))/>
        </Show>
    }
}

fn fmt_dim(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

#[component]
fn NewDialog(close: Callback<()>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let step = RwSignal::new(None::<DocKind>);
    let cw = RwSignal::new(1200.0_f32);
    let ch = RwSignal::new(800.0_f32);
    let cu = RwSignal::new(Unit::Px);

    let create = move |kind: DocKind, size: Option<(f32, f32, Unit)>| {
        let title = String::new();
        let doc = Document::new(kind, title, now(), size);
        let id = doc.meta.id.clone();
        leptos::task::spawn_local(async move {
            if api::save_document(&doc).await.is_ok() {
                app.reload_docs();
                close.run(());
                app.route.set(Route::Edit(id));
            }
        });
    };

    let pick_kind = move |k: DocKind| match k {
        DocKind::Note | DocKind::Sheet => create(k, None),
        _ => step.set(Some(k)),
    };

    view! {
        <div class="scrim" on:click=move |_| close.run(())>
            <div class="dialog" role="dialog" aria-modal="true" on:click=|ev| ev.stop_propagation()>
                <header class="row between">
                    <h2 class="display small">
                        {move || match step.get() {
                            None => t("new.title"),
                            Some(k) => t(&format!("new.size.{}", k.key())),
                        }}
                    </h2>
                    <button class="btn icon ghost" title=move || t("common.close") on:click=move |_| close.run(())><Icon name="close"/></button>
                </header>
                {move || match step.get() {
                    None => view! {
                        <ul class="choice-list">
                            {DocKind::ALL.into_iter().map(|k| view! {
                                <li>
                                    <button on:click=move |_| pick_kind(k)>
                                        <span class=format!("glyph k-{}", k.key())><Icon name=kind_icon(k)/></span>
                                        <span class="grow">
                                            <strong>{move || t(&format!("kind.{}", k.key()))}</strong>
                                            <span class="muted">{move || t(&format!("new.desc.{}", k.key()))}</span>
                                        </span>
                                        <Icon name="chevron_right"/>
                                    </button>
                                </li>
                            }).collect_view()}
                        </ul>
                    }.into_any(),
                    Some(k) => {
                        let presets = if k == DocKind::Slides { SLIDE_PRESETS } else { DESIGN_PRESETS };
                        view! {
                            <ul class="preset-list">
                                {presets.iter().map(|p| {
                                    let (w, h, u, key) = (p.width, p.height, p.unit, p.key);
                                    view! {
                                        <li>
                                            <button on:click=move |_| create(k, Some((w, h, u)))>
                                                <span class="ratio"><span style=format!("aspect-ratio:{w}/{h}")></span></span>
                                                <span class="grow">{move || t(&format!("preset.{key}"))}</span>
                                                <span class="mono muted">{format!("{} × {} {}", fmt_dim(w), fmt_dim(h), u.label())}</span>
                                            </button>
                                        </li>
                                    }
                                }).collect_view()}
                            </ul>
                            <fieldset class="custom-size">
                                <legend>{move || t("new.custom")}</legend>
                                <div class="row gap wrap">
                                    <label class="field">
                                        <span>{move || t("canvas.width")}</span>
                                        <input type="number" min="1" step="any" prop:value=move || cw.get().to_string()
                                            on:input=move |ev| if let Ok(v) = event_target_value(&ev).parse() { cw.set(v) }/>
                                    </label>
                                    <span class="times">"×"</span>
                                    <label class="field">
                                        <span>{move || t("canvas.height")}</span>
                                        <input type="number" min="1" step="any" prop:value=move || ch.get().to_string()
                                            on:input=move |ev| if let Ok(v) = event_target_value(&ev).parse() { ch.set(v) }/>
                                    </label>
                                    <label class="field">
                                        <span>{move || t("canvas.unit")}</span>
                                        <select on:change=move |ev| {
                                            cu.set(match event_target_value(&ev).as_str() { "mm" => Unit::Mm, "cm" => Unit::Cm, "in" => Unit::In, _ => Unit::Px })
                                        }>
                                            {Unit::ALL.into_iter().map(|u| view! { <option value=u.label() selected=move || cu.get() == u>{u.label()}</option> }).collect_view()}
                                        </select>
                                    </label>
                                    <button class="btn primary" on:click=move |_| {
                                        let (w, h, u) = (cw.get_untracked(), ch.get_untracked(), cu.get_untracked());
                                        let px = u.px();
                                        if w * px >= 16.0 && h * px >= 16.0 && w * px <= 20_000.0 && h * px <= 20_000.0 {
                                            create(k, Some((w, h, u)));
                                        } else {
                                            toast(t("new.size_invalid"), None);
                                        }
                                    }>{move || t("new.create")}</button>
                                </div>
                            </fieldset>
                            <button class="btn text" on:click=move |_| step.set(None)><Icon name="arrow_back"/>{move || t("common.back")}</button>
                        }.into_any()
                    }
                }}
            </div>
        </div>
    }
}
