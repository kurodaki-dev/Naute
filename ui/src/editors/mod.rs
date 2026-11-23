//! Editor shell: loads a document, autosaves it, and hosts the title bar
//! and the export sheet shared by every kind of document.

mod canvas;
mod note;
mod sheet;

use crate::api;
use crate::home::kind_icon;
use crate::i18n::t;
use crate::state::{AppState, Route};
use crate::util::{debounce, now, toast, Icon};
use leptos::html::Div;
use leptos::prelude::*;
use naute_core::model::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SaveState {
    Saved,
    Pending,
    Failed,
}

fn excerpt(doc: &Document) -> String {
    let raw = match &doc.body {
        Body::Note(n) => {
            // Strip tags; good enough for a one-line preview.
            let mut out = String::new();
            let mut in_tag = false;
            for c in n.html.chars() {
                match c {
                    '<' => {
                        in_tag = true;
                        out.push(' ');
                    }
                    '>' => in_tag = false,
                    c if !in_tag => out.push(c),
                    _ => {}
                }
            }
            out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
        }
        Body::Sheet(s) => naute_core::sheet::excerpt(s),
        Body::Canvas(c) => {
            let (w, h) = (c.width, c.height);
            let fmt = |v: f32| if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{v:.1}") };
            format!("{} × {} {} · {} p.", fmt(w), fmt(h), c.unit.label(), c.pages.len())
        }
    };
    raw.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(160).collect()
}

#[component]
pub fn Editor(id: String) -> impl IntoView {
    let app = expect_context::<AppState>();
    let doc = RwSignal::new(None::<Document>);
    let save_state = RwSignal::new(SaveState::Saved);
    let exporting = RwSignal::new(false);
    let note_ref = NodeRef::<Div>::new();
    let page = RwSignal::new(0usize);
    // Incremented by editors on every user change; drives autosave.
    let rev = RwSignal::new(0u64);

    {
        let id = id.clone();
        leptos::task::spawn_local(async move {
            match api::load_document(&id).await {
                Ok(d) => doc.set(Some(d)),
                Err(e) => {
                    toast(e, None);
                    app.route.set(Route::Home);
                }
            }
        });
    }

    let save_now = move || {
        let Some(mut d) = doc.try_get_untracked().flatten() else { return };
        d.meta.updated = now();
        d.meta.excerpt = excerpt(&d);
        leptos::task::spawn_local(async move {
            let ok = api::save_document(&d).await.is_ok();
            let _ = save_state.try_set(if ok { SaveState::Saved } else { SaveState::Failed });
            app.docs.update(|list| {
                if let Some(m) = list.iter_mut().find(|m| m.id == d.meta.id) {
                    *m = d.meta.clone();
                } else {
                    list.insert(0, d.meta.clone());
                }
                list.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated.partial_cmp(&a.updated).unwrap()));
            });
        });
    };
    let schedule = std::rc::Rc::new(debounce(700, save_now));
    let schedule2 = schedule.clone();
    Effect::new(move |prev: Option<u64>| {
        let r = rev.get();
        if prev.is_some() && Some(r) != prev {
            save_state.set(SaveState::Pending);
            schedule2();
        }
        r
    });

    let leave = move |_| {
        if save_state.get_untracked() == SaveState::Pending {
            save_now();
        }
        app.route.set(Route::Home);
    };

    // A memo, so editors are only rebuilt when the document kind changes,
    // not on every keystroke.
    let kind_m = Memo::new(move |_| doc.with(|d| d.as_ref().map(|d| d.meta.kind)));
    let kind = move || kind_m.get();

    view! {
        <div class="page editor">
            <header class="topbar">
                <button class="btn icon ghost" title=move || t("common.back") on:click=leave>
                    <Icon name="arrow_back"/>
                </button>
                {move || kind().map(|k| view! { <span class=format!("glyph small k-{}", k.key())><Icon name=kind_icon(k)/></span> })}
                <input
                    class="title-input"
                    placeholder=move || t("common.untitled")
                    prop:value=move || doc.with(|d| d.as_ref().map(|d| d.meta.title.clone()).unwrap_or_default())
                    on:input=move |ev| {
                        let v = event_target_value(&ev);
                        doc.update(|d| if let Some(d) = d { d.meta.title = v });
                        rev.update(|r| *r += 1);
                    }
                />
                <span class="save-state mono" class:failed=move || save_state.get() == SaveState::Failed>
                    {move || match save_state.get() {
                        SaveState::Saved => t("editor.saved"),
                        SaveState::Pending => t("editor.saving"),
                        SaveState::Failed => t("editor.save_failed"),
                    }}
                </span>
                <button class="btn" on:click=move |_| exporting.set(true)>
                    <Icon name="ios_share"/>
                    <span class="desktop-only">{move || t("export.button")}</span>
                </button>
            </header>
            {move || {
                let Some(k) = kind() else { return view! { <div class="loading"><Icon name="progress_activity"/></div> }.into_any() };
                match k {
                    DocKind::Note => view! { <note::NoteEditor doc=doc rev=rev node=note_ref/> }.into_any(),
                    DocKind::Sheet => view! { <sheet::SheetEditor doc=doc rev=rev/> }.into_any(),
                    DocKind::Design | DocKind::Slides => view! { <canvas::CanvasEditor doc=doc rev=rev page=page/> }.into_any(),
                }
            }}
        </div>
        <Show when=move || exporting.get()>
            <ExportSheet doc=doc note=note_ref page=page close=Callback::new(move |_| exporting.set(false))/>
        </Show>
    }
}

fn formats(kind: DocKind) -> &'static [(&'static str, &'static str)] {
    match kind {
        DocKind::Note => &[("pdf", "PDF"), ("docx", "Word"), ("md", "Markdown"), ("html", "HTML"), ("txt", "Text"), ("naute", "Naute")],
        DocKind::Sheet => &[("xlsx", "Excel"), ("csv", "CSV"), ("pdf", "PDF"), ("naute", "Naute")],
        DocKind::Design => &[("png", "PNG"), ("pdf", "PDF"), ("svg", "SVG"), ("naute", "Naute")],
        DocKind::Slides => &[("pptx", "PowerPoint"), ("pdf", "PDF"), ("png", "PNG"), ("svg", "SVG"), ("naute", "Naute")],
    }
}

#[component]
fn ExportSheet(doc: RwSignal<Option<Document>>, note: NodeRef<Div>, page: RwSignal<usize>, close: Callback<()>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let kind = doc.with_untracked(|d| d.as_ref().map(|d| d.meta.kind)).unwrap_or(DocKind::Note);
    let pages = doc.with_untracked(|d| match d.as_ref().map(|d| &d.body) {
        Some(Body::Canvas(c)) => c.pages.len(),
        _ => 1,
    });
    let only_page = RwSignal::new(false);
    let busy = RwSignal::new(None::<&'static str>);

    let run = move |fmt: &'static str| {
        let Some(d) = doc.get_untracked() else { return };
        let blocks = if kind == DocKind::Note { note.get_untracked().map(|el| note::blocks_from_dom(&el)) } else { None };
        let pg = if only_page.get_untracked() || fmt == "png" && pages == 1 { Some(page.get_untracked()) } else { None };
        let pg = if fmt == "pptx" || fmt == "naute" { None } else { pg };
        busy.set(Some(fmt));
        let lang = app.lang.get_untracked();
        leptos::task::spawn_local(async move {
            let res = api::export_document(&d, fmt, blocks, pg, &lang).await;
            busy.set(None);
            match res {
                Ok(Some(saved)) => {
                    close.run(());
                    let action = saved.can_reveal.then(|| (t("export.show"), format!("reveal:{}", saved.path)));
                    toast(t("export.done"), action);
                }
                Ok(None) => {}
                Err(e) if e == "native-only" => toast(t("common.native_only"), None),
                Err(e) => toast(format!("{} — {e}", t("export.failed")), None),
            }
        });
    };

    view! {
        <div class="scrim" on:click=move |_| close.run(())>
            <div class="dialog" role="dialog" aria-modal="true" on:click=|ev| ev.stop_propagation()>
                <header class="row between">
                    <h2 class="display small">{move || t("export.title")}</h2>
                    <button class="btn icon ghost" title=move || t("common.close") on:click=move |_| close.run(())><Icon name="close"/></button>
                </header>
                <p class="muted">{move || t("export.hint")}</p>
                {(pages > 1).then(|| view! {
                    <label class="check-row">
                        <input type="checkbox" prop:checked=move || only_page.get() on:change=move |ev| only_page.set(event_target_checked(&ev))/>
                        <span>{move || crate::i18n::tf("export.only_page", &[&(page.get() + 1).to_string()])}</span>
                    </label>
                })}
                <ul class="choice-list formats">
                    {formats(kind).iter().map(|(fmt, label)| {
                        let fmt: &'static str = fmt;
                        view! {
                            <li>
                                <button disabled=move || busy.get().is_some() on:click=move |_| run(fmt)>
                                    <span class="mono ext">{format!(".{fmt}")}</span>
                                    <span class="grow">
                                        <strong>{*label}</strong>
                                        <span class="muted">{move || t(&format!("export.desc.{fmt}"))}</span>
                                    </span>
                                    {move || if busy.get() == Some(fmt) {
                                        view! { <Icon name="progress_activity"/> }.into_any()
                                    } else {
                                        view! { <Icon name="download"/> }.into_any()
                                    }}
                                </button>
                            </li>
                        }
                    }).collect_view()}
                </ul>
            </div>
        </div>
    }
}
