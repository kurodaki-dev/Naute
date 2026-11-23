//! Free-size designs and slide decks: place text, shapes, lines, images and
//! tables on pages of any dimension; move, resize, rotate, layer; snap to
//! the page's centre and edges; present full screen.

use crate::dictate::DictateButton;
use crate::i18n::{t, tf};
use crate::util::{read_image, CanvasMeasure, Icon};
use leptos::ev;
use leptos::html::{Div, Input};
use leptos::prelude::*;
use naute_core::model::*;
use naute_core::render::{canvas_px, default_shape, default_table, default_text, element_svg, page_svg, text_height};
use wasm_bindgen::JsCast;

const SWATCHES: &[&str] = &["#26241F", "#55524A", "#B1ADA1", "#E9E6DD", "#F4F3EE", "#FFFFFF", "#C15F3C", "#8C4A30", "#F3DDD3"];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Move,
    Resize(i8, i8),
}

#[derive(Clone, Debug)]
struct Drag {
    mode: Mode,
    x0: f32,
    y0: f32,
    orig: Element,
    moved: bool,
}

#[derive(Clone)]
struct History {
    undo: Vec<CanvasBody>,
    redo: Vec<CanvasBody>,
    last_tag: String,
    last_at: f64,
}

fn target_el(ev: &web_sys::Event) -> Option<web_sys::Element> {
    ev.target()?.dyn_into::<web_sys::Element>().ok()
}

fn in_text_field(ev: &web_sys::Event) -> bool {
    target_el(ev)
        .map(|e| matches!(e.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT") || e.closest("[contenteditable=true]").ok().flatten().is_some())
        .unwrap_or(false)
}

fn fmt_num(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

#[component]
pub fn CanvasEditor(doc: RwSignal<Option<Document>>, rev: RwSignal<u64>, page: RwSignal<usize>) -> impl IntoView {
    let is_slides = doc.with_untracked(|d| d.as_ref().map(|d| d.meta.kind == DocKind::Slides).unwrap_or(false));
    let canvas = RwSignal::new(doc.with_untracked(|d| match d.as_ref().map(|d| &d.body) {
        Some(Body::Canvas(c)) => c.clone(),
        _ => CanvasBody { width: 1080.0, height: 1080.0, unit: Unit::Px, pages: vec![blank_page("#FFFFFF")] },
    }));
    let selected = RwSignal::new(None::<String>);
    let editing = RwSignal::new(None::<String>);
    let presenting = RwSignal::new(false);
    let user_zoom = RwSignal::new(1.0_f32);
    let fit = RwSignal::new(0.5_f32);
    let guides = RwSignal::new(Vec::<(f32, f32, f32, f32)>::new());
    let offset = RwSignal::new((0.0_f32, 0.0_f32));
    let drag = StoredValue::new(None::<Drag>);
    let history = StoredValue::new(History { undo: vec![], redo: vec![], last_tag: String::new(), last_at: 0.0 });
    let stage = NodeRef::<Div>::new();
    let board = NodeRef::<Div>::new();
    let file_input = NodeRef::<Input>::new();

    let size = Memo::new(move |_| canvas.with(canvas_px));
    let scale = Memo::new(move |_| fit.get() * user_zoom.get());
    let pg = move || page.get().min(canvas.with(|c| c.pages.len().saturating_sub(1)));

    // ------------------------------------------------------------ changes

    let save = move || {
        let c = canvas.get_untracked();
        doc.update(|d| {
            if let Some(d) = d {
                d.body = Body::Canvas(c);
            }
        });
        rev.update(|r| *r += 1);
    };
    // Snapshot for undo. Consecutive edits with the same tag (typing in one
    // field, nudging) collapse into one step.
    let snapshot = move |tag: &str| {
        let now = crate::util::now();
        let cur = canvas.get_untracked();
        history.update_value(|h| {
            if !(tag == h.last_tag && !tag.is_empty() && now - h.last_at < 800.0) {
                h.undo.push(cur);
                if h.undo.len() > 100 {
                    h.undo.remove(0);
                }
            }
            h.redo.clear();
            h.last_tag = tag.to_string();
            h.last_at = now;
        });
    };
    let change = move |tag: &str, f: &mut dyn FnMut(&mut CanvasBody)| {
        snapshot(tag);
        canvas.update(|c| f(c));
        save();
    };
    let change_el = move |tag: &str, id: &str, f: &mut dyn FnMut(&mut Element)| {
        let p = pg();
        let id = id.to_string();
        change(tag, &mut |c: &mut CanvasBody| {
            if let Some(e) = c.pages.get_mut(p).and_then(|pg| pg.elements.iter_mut().find(|e| e.id == id)) {
                f(e);
                fit_text(e);
            }
        });
    };
    let change_sel = move |tag: &str, f: &mut dyn FnMut(&mut Element)| {
        if let Some(id) = selected.get_untracked() {
            change_el(tag, &id, f);
        }
    };
    let undo = move || {
        let prev = history.try_update_value(|h| {
            let p = h.undo.pop();
            if p.is_some() {
                h.redo.push(canvas.get_untracked());
                h.last_tag.clear();
            }
            p
        });
        if let Some(Some(p)) = prev {
            canvas.set(p);
            save();
        }
    };
    let redo = move || {
        let next = history.try_update_value(|h| {
            let n = h.redo.pop();
            if n.is_some() {
                h.undo.push(canvas.get_untracked());
                h.last_tag.clear();
            }
            n
        });
        if let Some(Some(n)) = next {
            canvas.set(n);
            save();
        }
    };

    let sel_el = Memo::new(move |_| {
        let id = selected.get()?;
        let p = pg();
        canvas.with(|c| c.pages.get(p)?.elements.iter().find(|e| e.id == id).cloned())
    });

    let add = move |mut el: Element| {
        let (cw, ch) = size.get_untracked();
        el.x = ((cw - el.w) / 2.0).round();
        el.y = ((ch - el.h) / 2.0).round();
        fit_text(&mut el);
        let id = el.id.clone();
        let p = pg();
        change("", &mut |c: &mut CanvasBody| {
            if let Some(pg) = c.pages.get_mut(p) {
                pg.elements.push(el.clone());
            }
        });
        selected.set(Some(id));
    };
    let add_kind = move |k: &'static str| {
        let (cw, _) = size.get_untracked();
        match k {
            "text" => add(default_text(0.0, 0.0, cw, &t("canvas.sample_text"))),
            "table" => add(default_table(0.0, 0.0, cw)),
            k => add(default_shape(k, 0.0, 0.0, cw)),
        }
    };
    let remove_sel = move || {
        if let Some(id) = selected.get_untracked() {
            let p = pg();
            change("", &mut |c: &mut CanvasBody| {
                if let Some(pg) = c.pages.get_mut(p) {
                    pg.elements.retain(|e| e.id != id);
                }
            });
            selected.set(None);
            editing.set(None);
        }
    };
    let duplicate_sel = move || {
        if let Some(mut e) = sel_el.get_untracked() {
            let (cw, _) = size.get_untracked();
            e.id = new_id();
            e.x += cw / 40.0;
            e.y += cw / 40.0;
            let id = e.id.clone();
            let p = pg();
            change("", &mut |c: &mut CanvasBody| {
                if let Some(pg) = c.pages.get_mut(p) {
                    pg.elements.push(e.clone());
                }
            });
            selected.set(Some(id));
        }
    };
    let reorder = move |to: i32| {
        let Some(id) = selected.get_untracked() else { return };
        let p = pg();
        change("", &mut |c: &mut CanvasBody| {
            let Some(pg) = c.pages.get_mut(p) else { return };
            let Some(i) = pg.elements.iter().position(|e| e.id == id) else { return };
            let e = pg.elements.remove(i);
            let n = pg.elements.len();
            let j = match to {
                2 => n,
                -2 => 0,
                d => (i as i32 + d).clamp(0, n as i32) as usize,
            };
            pg.elements.insert(j, e);
        });
    };

    // ------------------------------------------------------------ pages

    let add_page = move |dup: bool| {
        let p = pg();
        change("", &mut |c: &mut CanvasBody| {
            let mut np = if dup { c.pages[p].clone() } else { blank_page(&c.pages[p].background) };
            np.id = new_id();
            if dup {
                for e in &mut np.elements {
                    e.id = new_id();
                }
            }
            c.pages.insert(p + 1, np);
        });
        page.set(p + 1);
        selected.set(None);
    };
    let delete_page = move || {
        let p = pg();
        if canvas.with_untracked(|c| c.pages.len()) <= 1 {
            return;
        }
        change("", &mut |c: &mut CanvasBody| {
            c.pages.remove(p);
        });
        page.set(p.saturating_sub(1));
        selected.set(None);
    };
    let move_page = move |d: i32| {
        let p = pg();
        let n = canvas.with_untracked(|c| c.pages.len());
        let q = p as i32 + d;
        if q < 0 || q >= n as i32 {
            return;
        }
        change("", &mut |c: &mut CanvasBody| c.pages.swap(p, q as usize));
        page.set(q as usize);
    };

    // ------------------------------------------------------------ zoom

    let refit = move || {
        let Some(st) = stage.get_untracked() else { return };
        let (cw, ch) = size.get_untracked();
        let (sw, sh) = (st.client_width() as f32, st.client_height() as f32);
        if sw > 0.0 && sh > 0.0 {
            let pad = if sw < 600.0 { 24.0 } else { 64.0 };
            fit.set(((sw - pad) / cw).min((sh - pad) / ch).max(0.02));
        }
    };
    Effect::new(move |_| {
        let _ = size.get();
        if stage.get().is_some() {
            refit();
            // Layout may settle a frame later (fonts, side panels).
            set_timeout(refit, std::time::Duration::from_millis(60));
        }
    });
    let resize_handle = window_event_listener(ev::resize, move |_| refit());
    on_cleanup(move || resize_handle.remove());

    // ------------------------------------------------------------ pointer

    let to_canvas = move |x: i32, y: i32| -> (f32, f32) {
        let Some(b) = board.get_untracked() else { return (0.0, 0.0) };
        let r = b.get_bounding_client_rect();
        let (cw, ch) = size.get_untracked();
        (((x as f64 - r.left()) / r.width()) as f32 * cw, ((y as f64 - r.top()) / r.height()) as f32 * ch)
    };

    let on_down = move |ev: web_sys::PointerEvent| {
        if editing.get_untracked().is_some() {
            return;
        }
        let Some(t) = target_el(&ev) else { return };
        let (x, y) = to_canvas(ev.client_x(), ev.client_y());
        let handle = t.closest("[data-handle]").ok().flatten().and_then(|h| h.get_attribute("data-handle"));
        let hit = t.closest("[data-id]").ok().flatten().and_then(|h| h.get_attribute("data-id"));
        let mode = if let Some(h) = handle {
            let mut it = h.split(',').map(|v| v.parse::<i8>().unwrap_or(0));
            Some(Mode::Resize(it.next().unwrap_or(0), it.next().unwrap_or(0)))
        } else if let Some(id) = hit {
            selected.set(Some(id));
            Some(Mode::Move)
        } else {
            selected.set(None);
            None
        };
        if let (Some(mode), Some(orig)) = (mode, sel_el.get_untracked()) {
            if orig.locked {
                return;
            }
            if let Some(b) = board.get_untracked() {
                let _ = b.set_pointer_capture(ev.pointer_id());
            }
            ev.prevent_default();
            drag.set_value(Some(Drag { mode, x0: x, y0: y, orig, moved: false }));
        }
    };

    let on_move = move |ev: web_sys::PointerEvent| {
        let Some(mut d) = drag.get_value() else { return };
        let (x, y) = to_canvas(ev.client_x(), ev.client_y());
        let (dx, dy) = (x - d.x0, y - d.y0);
        let s = scale.get_untracked();
        if !d.moved && (dx * s).abs() < 2.0 && (dy * s).abs() < 2.0 {
            return;
        }
        if !d.moved {
            d.moved = true;
            snapshot("");
            drag.set_value(Some(d.clone()));
        }
        let o = &d.orig;
        match d.mode {
            Mode::Move => {
                // Snap the element's edges and centre to the page's.
                let (cw, ch) = size.get_untracked();
                let tol = 6.0 / s;
                let mut nx = o.x + dx;
                let mut ny = o.y + dy;
                let mut g = vec![];
                if !ev.alt_key() {
                    for (target, at) in [(0.0, 0.0), (cw / 2.0, 0.5), (cw, 1.0)] {
                        let edge = nx + o.w * at;
                        if (edge - target).abs() < tol {
                            nx = target - o.w * at;
                            g.push((target, 0.0, target, ch));
                            break;
                        }
                    }
                    for (target, at) in [(0.0, 0.0), (ch / 2.0, 0.5), (ch, 1.0)] {
                        let edge = ny + o.h * at;
                        if (edge - target).abs() < tol {
                            ny = target - o.h * at;
                            g.push((0.0, target, cw, target));
                            break;
                        }
                    }
                }
                guides.set(g);
                offset.set((nx - o.x, ny - o.y));
            }
            Mode::Resize(hx, hy) => {
                // Work in the element's own (unrotated) frame.
                let a = -o.rotation.to_radians();
                let (lx, ly) = (dx * a.cos() - dy * a.sin(), dx * a.sin() + dy * a.cos());
                let min = 8.0;
                let mut e = o.clone();
                let keep_ratio = ev.shift_key() || matches!(o.kind, ElementKind::Image { .. }) && hx != 0 && hy != 0;
                let mut w = (o.w + lx * hx as f32).max(min);
                let mut h = (o.h + ly * hy as f32).max(min);
                if keep_ratio && hx != 0 && hy != 0 {
                    let r = o.w / o.h;
                    if w / h > r {
                        h = w / r;
                    } else {
                        w = h * r;
                    }
                }
                if hx != 0 {
                    e.w = w;
                    if hx < 0 {
                        e.x = o.x + o.w - w;
                    }
                }
                if hy != 0 {
                    e.h = h;
                    if hy < 0 {
                        e.y = o.y + o.h - h;
                    }
                }
                // Text boxes: dragging a corner scales the type.
                if let ElementKind::Text { style, .. } = &mut e.kind {
                    if hx != 0 && hy != 0 {
                        if let ElementKind::Text { style: os, .. } = &o.kind {
                            style.size = (os.size * e.w / o.w).clamp(4.0, 800.0).round();
                        }
                    }
                }
                fit_text(&mut e);
                let p = pg();
                canvas.update(|c| {
                    if let Some(t) = c.pages.get_mut(p).and_then(|pg| pg.elements.iter_mut().find(|x| x.id == e.id)) {
                        *t = e.clone();
                    }
                });
            }
        }
    };

    let on_up = move |_ev: web_sys::PointerEvent| {
        let Some(d) = drag.get_value() else { return };
        drag.set_value(None);
        guides.set(vec![]);
        if !d.moved {
            return;
        }
        if d.mode == Mode::Move {
            let (ox, oy) = offset.get_untracked();
            offset.set((0.0, 0.0));
            let p = pg();
            let id = d.orig.id.clone();
            canvas.update(|c| {
                if let Some(t) = c.pages.get_mut(p).and_then(|pg| pg.elements.iter_mut().find(|x| x.id == id)) {
                    t.x = (d.orig.x + ox).round();
                    t.y = (d.orig.y + oy).round();
                }
            });
        }
        save();
    };

    let on_dbl = move |ev: web_sys::MouseEvent| {
        if let Some(e) = sel_el.get_untracked() {
            if matches!(e.kind, ElementKind::Text { .. }) && !e.locked {
                let hit = target_el(&ev).and_then(|t| t.closest("[data-id]").ok().flatten());
                if hit.is_some() {
                    snapshot("");
                    editing.set(Some(e.id));
                }
            }
        }
    };

    // ------------------------------------------------------------ keyboard

    let keys = window_event_listener(ev::keydown, move |ev| {
        if presenting.get_untracked() || in_text_field(&ev) {
            return;
        }
        let ctrl = ev.ctrl_key() || ev.meta_key();
        let step = if ev.shift_key() { 10.0 } else { 1.0 };
        let nudge = |dx: f32, dy: f32| {
            change_sel("nudge", &mut |e: &mut Element| {
                e.x += dx;
                e.y += dy;
            })
        };
        match ev.key().as_str() {
            "z" | "Z" if ctrl && ev.shift_key() => redo(),
            "z" | "Z" if ctrl => undo(),
            "y" | "Y" if ctrl => redo(),
            "d" | "D" if ctrl => duplicate_sel(),
            "Delete" | "Backspace" if selected.get_untracked().is_some() => remove_sel(),
            "Escape" => selected.set(None),
            "ArrowLeft" if selected.get_untracked().is_some() => nudge(-step, 0.0),
            "ArrowRight" if selected.get_untracked().is_some() => nudge(step, 0.0),
            "ArrowUp" if selected.get_untracked().is_some() => nudge(0.0, -step),
            "ArrowDown" if selected.get_untracked().is_some() => nudge(0.0, step),
            "Enter" => {
                if let Some(e) = sel_el.get_untracked() {
                    if matches!(e.kind, ElementKind::Text { .. }) {
                        snapshot("");
                        editing.set(Some(e.id));
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
            _ => return,
        }
        ev.prevent_default();
    });
    on_cleanup(move || keys.remove());

    // ------------------------------------------------------------ images & dictation

    let on_file = move |_| {
        let Some(input) = file_input.get_untracked() else { return };
        let Some(file) = input.files().and_then(|f| f.get(0)) else { return };
        input.set_value("");
        leptos::task::spawn_local(async move {
            if let Some((src, w, h)) = read_image(file).await {
                let (cw, ch) = size.get_untracked();
                let s = (cw * 0.6 / w).min(ch * 0.6 / h);
                add(Element {
                    id: new_id(),
                    x: 0.0,
                    y: 0.0,
                    w: (w * s).round(),
                    h: (h * s).round(),
                    rotation: 0.0,
                    opacity: 1.0,
                    locked: false,
                    kind: ElementKind::Image { src },
                });
            }
        });
    };
    let dictated = Callback::new(move |text: String| {
        match sel_el.get_untracked() {
            Some(e) if matches!(e.kind, ElementKind::Text { .. }) => change_sel("", &mut |e: &mut Element| {
                if let ElementKind::Text { text: t, .. } = &mut e.kind {
                    if !t.is_empty() && !t.ends_with([' ', '\n']) {
                        t.push(' ');
                    }
                    t.push_str(&text);
                }
            }),
            _ => {
                let (cw, _) = size.get_untracked();
                add(default_text(0.0, 0.0, cw, &text));
            }
        }
    });

    // ------------------------------------------------------------ view

    let element_ids = Memo::new(move |_| {
        let p = pg();
        canvas.with(|c| c.pages.get(p).map(|pg| pg.elements.iter().map(|e| e.id.clone()).collect::<Vec<_>>()).unwrap_or_default())
    });
    let background = Memo::new(move |_| {
        let p = pg();
        canvas.with(|c| c.pages.get(p).map(|pg| pg.background.clone()).unwrap_or_default())
    });

    let tool = move |icon: &'static str, key: &'static str, k: &'static str| {
        view! {
            <button class="btn icon ghost" title=move || t(key) on:click=move |_| add_kind(k)><Icon name=icon/></button>
        }
    };

    view! {
        <div class="toolbar" role="toolbar">
            <div class="tool-group">
                {tool("text_fields", "canvas.text", "text")}
                {tool("rectangle", "canvas.rect", "rect")}
                {tool("circle", "canvas.ellipse", "ellipse")}
                {tool("horizontal_rule", "canvas.line", "line")}
                {tool("table", "canvas.table", "table")}
                <button class="btn icon ghost" title=move || t("canvas.image") on:click=move |_| if let Some(i) = file_input.get_untracked() { i.click() }>
                    <Icon name="image"/>
                </button>
            </div>
            <div class="tool-group">
                <button class="btn icon ghost" title=move || t("common.undo") on:click=move |_| undo()><Icon name="undo"/></button>
                <button class="btn icon ghost" title=move || t("common.redo") on:click=move |_| redo()><Icon name="redo"/></button>
            </div>
            <div class="tool-group zoom desktop-only">
                <button class="btn icon ghost" title=move || t("canvas.zoom_out") on:click=move |_| user_zoom.update(|z| *z = (*z / 1.25).max(0.1))><Icon name="zoom_out"/></button>
                <button class="btn ghost mono" title=move || t("canvas.fit") on:click=move |_| user_zoom.set(1.0)>{move || format!("{:.0}%", scale.get() * 100.0)}</button>
                <button class="btn icon ghost" title=move || t("canvas.zoom_in") on:click=move |_| user_zoom.update(|z| *z = (*z * 1.25).min(8.0))><Icon name="zoom_in"/></button>
            </div>
            <div class="grow"></div>
            <DictateButton on_text=dictated/>
            <button class="btn" on:click=move |_| { selected.set(None); presenting.set(true); }>
                <Icon name="play_arrow"/>
                <span class="desktop-only">{move || if is_slides { t("canvas.present") } else { t("canvas.preview") }}</span>
            </button>
            <input type="file" accept="image/*" class="hidden" node_ref=file_input on:change=on_file/>
        </div>
        <div class="canvas-main">
            <aside class="pages" aria-label=move || t("canvas.pages")>
                <ol>
                    <For each=move || canvas.with(|c| c.pages.iter().enumerate().map(|(i, p)| (i, p.id.clone())).collect::<Vec<_>>()) key=|(i, id)| (*i, id.clone()) let:item>
                        {
                            let (i, id) = item;
                            let thumb = Memo::new(move |_| canvas.with(|c| c.pages.iter().find(|p| p.id == id).map(|p| page_svg(c, p, &CanvasMeasure)).unwrap_or_default()));
                            view! {
                                <li class:active=move || pg() == i>
                                    <button on:click=move |_| { page.set(i); selected.set(None); }>
                                        <span class="num mono">{i + 1}</span>
                                        <span class="thumb" style=move || { let (w, h) = size.get(); format!("aspect-ratio:{w}/{h}") } inner_html=move || thumb.get()></span>
                                    </button>
                                </li>
                            }
                        }
                    </For>
                </ol>
                <div class="page-actions">
                    <button class="btn icon ghost" title=move || t("canvas.add_page") on:click=move |_| add_page(false)><Icon name="add"/></button>
                    <button class="btn icon ghost" title=move || t("common.duplicate") on:click=move |_| add_page(true)><Icon name="content_copy"/></button>
                    <button class="btn icon ghost" title=move || t("canvas.move_up") on:click=move |_| move_page(-1)><Icon name="arrow_upward"/></button>
                    <button class="btn icon ghost" title=move || t("canvas.move_down") on:click=move |_| move_page(1)><Icon name="arrow_downward"/></button>
                    <button class="btn icon ghost" title=move || t("canvas.delete_page") disabled=move || canvas.with(|c| c.pages.len() <= 1) on:click=move |_| delete_page()><Icon name="delete"/></button>
                </div>
            </aside>
            <div class="stage" node_ref=stage on:pointerdown=move |ev: web_sys::PointerEvent| {
                // Clicking the grey area around the page clears the selection.
                if target_el(&ev).map(|t| t.class_list().contains("stage") || t.class_list().contains("stage-inner")).unwrap_or(false) {
                    selected.set(None);
                    editing.set(None);
                }
            }>
                <div class="stage-inner">
                    <div
                        class="artboard"
                        node_ref=board
                        style=move || { let (w, h) = size.get(); let s = scale.get(); format!("width:{}px;height:{}px", w * s, h * s) }
                        on:pointerdown=on_down
                        on:pointermove=on_move
                        on:pointerup=on_up
                        on:pointercancel=on_up
                        on:dblclick=on_dbl
                    >
                        <svg
                            xmlns="http://www.w3.org/2000/svg"
                            viewBox=move || { let (w, h) = size.get(); format!("0 0 {w} {h}") }
                            width="100%"
                            height="100%"
                        >
                            <rect width="100%" height="100%" fill=move || background.get()></rect>
                            <For each=move || element_ids.get() key=|id| id.clone() let:id>
                                {
                                    let id2 = id.clone();
                                    let id3 = id.clone();
                                    let svg = Memo::new(move |_| {
                                        let p = pg();
                                        canvas.with(|c| c.pages.get(p).and_then(|pg| pg.elements.iter().find(|e| e.id == id2)).map(|e| {
                                            // Hide text while it is being edited in place.
                                            if editing.get().as_deref() == Some(e.id.as_str()) { String::new() } else { element_svg(e, &CanvasMeasure) }
                                        }).unwrap_or_default())
                                    });
                                    view! {
                                        <g
                                            class="el"
                                            data-id=id.clone()
                                            transform=move || {
                                                if selected.get().as_deref() == Some(id3.as_str()) {
                                                    let (x, y) = offset.get();
                                                    if x != 0.0 || y != 0.0 { return format!("translate({x} {y})"); }
                                                }
                                                String::new()
                                            }
                                            inner_html=move || svg.get()
                                        ></g>
                                    }
                                }
                            </For>
                            {move || guides.get().into_iter().map(|(x1, y1, x2, y2)| view! {
                                <line class="guide" x1=x1 y1=y1 x2=x2 y2=y2 vector-effect="non-scaling-stroke"></line>
                            }).collect_view()}
                            {move || sel_el.get().filter(|_| editing.get().is_none()).map(|e| {
                                let s = scale.get();
                                let (ox, oy) = offset.get();
                                let (x, y) = (e.x + ox, e.y + oy);
                                let hs = 10.0 / s;
                                let handles: Vec<(i8, i8)> = if matches!(e.kind, ElementKind::Line { .. } | ElementKind::Text { .. }) {
                                    if matches!(e.kind, ElementKind::Line { .. }) { vec![(-1, 0), (1, 0)] } else { vec![(-1, 0), (1, 0), (-1, -1), (1, -1), (-1, 1), (1, 1)] }
                                } else {
                                    vec![(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]
                                };
                                let cx = x + e.w / 2.0;
                                let cy = y + e.h / 2.0;
                                view! {
                                    <g class="selection" transform=format!("rotate({} {cx} {cy})", e.rotation)>
                                        <rect x=x y=y width=e.w height=e.h fill="none" vector-effect="non-scaling-stroke" class:locked=e.locked></rect>
                                        {(!e.locked).then(|| handles.into_iter().map(|(hx, hy)| {
                                            let px = x + e.w * (hx as f32 + 1.0) / 2.0 - hs / 2.0;
                                            let py = y + e.h * (hy as f32 + 1.0) / 2.0 - hs / 2.0;
                                            view! {
                                                <rect class="handle" data-handle=format!("{hx},{hy}") x=px y=py width=hs height=hs rx=hs / 5.0
                                                    vector-effect="non-scaling-stroke"
                                                    style=format!("cursor:{}", match (hx, hy) { (0, _) => "ns-resize", (_, 0) => "ew-resize", (a, b) if a == b => "nwse-resize", _ => "nesw-resize" })></rect>
                                            }
                                        }).collect_view())}
                                    </g>
                                }
                            })}
                        </svg>
                        {move || editing.get().and_then(|id| {
                            let e = canvas.with(|c| c.pages.get(pg())?.elements.iter().find(|x| x.id == id).cloned())?;
                            let ElementKind::Text { text, style } = &e.kind else { return None };
                            let s = scale.get_untracked();
                            let st = format!(
                                "left:{}px;top:{}px;width:{}px;min-height:{}px;font:{} {} {}px/{} {};color:{};text-align:{};transform:rotate({}deg)",
                                e.x * s, e.y * s, e.w * s, e.h * s,
                                if style.italic { "italic" } else { "normal" }, if style.bold { 600 } else { 400 },
                                style.size * s, style.line_height, style.font.css(), style.color, style.align.key(), e.rotation
                            );
                            let id2 = id.clone();
                            Some(view! {
                                <textarea
                                    class="text-edit"
                                    style=st
                                    prop:value=text.clone()
                                    on:input=move |ev| {
                                        let v = event_target_value(&ev);
                                        let p = pg();
                                        let id = id2.clone();
                                        canvas.update(|c| if let Some(e) = c.pages.get_mut(p).and_then(|pg| pg.elements.iter_mut().find(|x| x.id == id)) {
                                            if let ElementKind::Text { text, .. } = &mut e.kind { *text = v.clone(); }
                                            fit_text(e);
                                        });
                                        save();
                                    }
                                    on:blur=move |_| editing.set(None)
                                    on:keydown=move |ev| if ev.key() == "Escape" { editing.set(None); }
                                    autofocus=true
                                ></textarea>
                            })
                        })}
                    </div>
                </div>
            </div>
            <aside class="inspector" class:open=move || sel_el.get().is_some()>
                {move || match sel_el.get() {
                    Some(e) => view! { <ElementPanel el=e change=Callback::new(move |(tag, f): (String, ElFn)| change_sel(&tag, &mut |e: &mut Element| (f.0)(e)))
                        actions=Callback::new(move |a: &'static str| match a {
                            "delete" => remove_sel(),
                            "duplicate" => duplicate_sel(),
                            "front" => reorder(2),
                            "back" => reorder(-2),
                            "forward" => reorder(1),
                            "backward" => reorder(-1),
                            "edit" => { if let Some(e) = sel_el.get_untracked() { snapshot(""); editing.set(Some(e.id)); } }
                            _ => {}
                        })/> }.into_any(),
                    None => view! { <PagePanel canvas=canvas page=Signal::derive(pg) slides=is_slides change=Callback::new(move |(tag, f): (String, CanvasFn)| change(&tag, &mut |c: &mut CanvasBody| (f.0)(c)))/> }.into_any(),
                }}
            </aside>
        </div>
        <Show when=move || presenting.get()>
            <Presenter canvas=canvas start=pg() close=Callback::new(move |_| presenting.set(false))/>
        </Show>
    }
}

/// Text boxes grow to fit their content.
fn fit_text(e: &mut Element) {
    if let ElementKind::Text { text, style } = &e.kind {
        e.h = text_height(&CanvasMeasure, text, style, e.w).max(style.size * style.line_height);
    }
}

// Boxed edit functions passed through callbacks (callbacks must be Send).
#[derive(Clone)]
pub struct ElFn(std::sync::Arc<dyn Fn(&mut Element) + Send + Sync>);
#[derive(Clone)]
pub struct CanvasFn(std::sync::Arc<dyn Fn(&mut CanvasBody) + Send + Sync>);

fn elf(f: impl Fn(&mut Element) + Send + Sync + 'static) -> ElFn {
    ElFn(std::sync::Arc::new(f))
}

#[component]
fn ColorRow(#[prop(into)] value: String, allow_none: bool, on_pick: Callback<String>) -> impl IntoView {
    let v = value.clone();
    view! {
        <div class="colors">
            {allow_none.then(|| {
                let v = v.clone();
                view! { <button class="swatch none" class:on=v.is_empty() title=move || t("color.none") on:click=move |_| on_pick.run(String::new())></button> }
            })}
            {SWATCHES.iter().map(|c| {
                let c: &'static str = c;
                let on = v.eq_ignore_ascii_case(c);
                view! { <button class="swatch" class:on=on style=format!("--c:{c}") title=c on:click=move |_| on_pick.run(c.to_string())></button> }
            }).collect_view()}
            <label class="swatch custom" title=move || t("color.custom")>
                <input type="color" prop:value=if value.starts_with('#') && value.len() == 7 { value.clone() } else { "#000000".to_string() }
                    on:input=move |ev| on_pick.run(event_target_value(&ev))/>
                <Icon name="palette"/>
            </label>
        </div>
    }
}

#[component]
fn Num(#[prop(into)] label: String, value: f32, on_set: Callback<f32>, #[prop(optional)] step: Option<f32>) -> impl IntoView {
    view! {
        <label class="num-field">
            <span class="mono">{label}</span>
            <input type="number" step=step.unwrap_or(1.0) prop:value=fmt_num(value)
                on:input=move |ev| { if let Ok(v) = event_target_value(&ev).parse::<f32>() { if v.is_finite() { on_set.run(v) } } } />
        </label>
    }
}

#[component]
fn ElementPanel(el: Element, change: Callback<(String, ElFn)>, actions: Callback<&'static str>) -> impl IntoView {
    let set = move |tag: &str, f: ElFn| change.run((tag.to_string(), f));
    let num = move |tag: &'static str, f: fn(&mut Element, f32)| Callback::new(move |v: f32| set(tag, elf(move |e| f(e, v))));
    let kind_key = el.kind.key();

    let specific = match el.kind.clone() {
        ElementKind::Text { text, style } => {
            let st = style.clone();
            view! {
                <section>
                    <h3 class="eyebrow">{move || t("canvas.text")}</h3>
                    <textarea class="text-area" prop:value=text rows="3"
                        on:input=move |ev| { let v = event_target_value(&ev); set("text", elf(move |e| if let ElementKind::Text { text, .. } = &mut e.kind { *text = v.clone() })) }></textarea>
                    <div class="seg">
                        {FontFamily::ALL.into_iter().map(|f| view! {
                            <button class:on=st.font == f style=format!("font-family:{}", f.css())
                                on:click=move |_| set("", elf(move |e| if let ElementKind::Text { style, .. } = &mut e.kind { style.font = f }))>
                                {match f { FontFamily::Sans => "Sans", FontFamily::Serif => "Serif", FontFamily::Mono => "Mono" }}
                            </button>
                        }).collect_view()}
                    </div>
                    <div class="row gap">
                        <Num label="Aa" value=style.size step=1.0 on_set=Callback::new(move |v: f32| set("size", elf(move |e| if let ElementKind::Text { style, .. } = &mut e.kind { style.size = v.clamp(1.0, 2000.0) })))/>
                        <Num label="↕" value=style.line_height step=0.05 on_set=Callback::new(move |v: f32| set("lh", elf(move |e| if let ElementKind::Text { style, .. } = &mut e.kind { style.line_height = v.clamp(0.6, 4.0) })))/>
                    </div>
                    <div class="seg">
                        <button class:on=style.bold title=move || t("note.bold") on:click=move |_| set("", elf(|e| if let ElementKind::Text { style, .. } = &mut e.kind { style.bold = !style.bold }))><Icon name="format_bold"/></button>
                        <button class:on=style.italic title=move || t("note.italic") on:click=move |_| set("", elf(|e| if let ElementKind::Text { style, .. } = &mut e.kind { style.italic = !style.italic }))><Icon name="format_italic"/></button>
                        {[(Align::Left, "format_align_left"), (Align::Center, "format_align_center"), (Align::Right, "format_align_right")].into_iter().map(|(a, icon)| view! {
                            <button class:on=style.align == a title=move || t(&format!("align.{}", a.key()))
                                on:click=move |_| set("", elf(move |e| if let ElementKind::Text { style, .. } = &mut e.kind { style.align = a }))><Icon name=icon/></button>
                        }).collect_view()}
                    </div>
                    <ColorRow value=style.color.clone() allow_none=false on_pick=Callback::new(move |c: String| set("color", elf(move |e| if let ElementKind::Text { style, .. } = &mut e.kind { style.color = c.clone() })))/>
                </section>
            }.into_any()
        }
        ElementKind::Rect { fill, stroke, stroke_width, radius } => view! {
            <section>
                <h3 class="eyebrow">{move || t("canvas.fill")}</h3>
                <ColorRow value=fill allow_none=true on_pick=Callback::new(move |c: String| set("fill", elf(move |e| if let ElementKind::Rect { fill, .. } = &mut e.kind { *fill = c.clone() })))/>
                <h3 class="eyebrow">{move || t("canvas.stroke")}</h3>
                <ColorRow value=stroke allow_none=true on_pick=Callback::new(move |c: String| set("stroke", elf(move |e| if let ElementKind::Rect { stroke, stroke_width, .. } = &mut e.kind { *stroke = c.clone(); if *stroke_width == 0.0 { *stroke_width = 4.0 } })))/>
                <div class="row gap">
                    <Num label="▢" value=stroke_width on_set=Callback::new(move |v: f32| set("sw", elf(move |e| if let ElementKind::Rect { stroke_width, .. } = &mut e.kind { *stroke_width = v.max(0.0) })))/>
                    <Num label="◜" value=radius on_set=Callback::new(move |v: f32| set("radius", elf(move |e| if let ElementKind::Rect { radius, .. } = &mut e.kind { *radius = v.max(0.0) })))/>
                </div>
            </section>
        }.into_any(),
        ElementKind::Ellipse { fill, stroke, stroke_width } => view! {
            <section>
                <h3 class="eyebrow">{move || t("canvas.fill")}</h3>
                <ColorRow value=fill allow_none=true on_pick=Callback::new(move |c: String| set("fill", elf(move |e| if let ElementKind::Ellipse { fill, .. } = &mut e.kind { *fill = c.clone() })))/>
                <h3 class="eyebrow">{move || t("canvas.stroke")}</h3>
                <ColorRow value=stroke allow_none=true on_pick=Callback::new(move |c: String| set("stroke", elf(move |e| if let ElementKind::Ellipse { stroke, stroke_width, .. } = &mut e.kind { *stroke = c.clone(); if *stroke_width == 0.0 { *stroke_width = 4.0 } })))/>
                <Num label="▢" value=stroke_width on_set=Callback::new(move |v: f32| set("sw", elf(move |e| if let ElementKind::Ellipse { stroke_width, .. } = &mut e.kind { *stroke_width = v.max(0.0) })))/>
            </section>
        }.into_any(),
        ElementKind::Line { stroke, stroke_width } => view! {
            <section>
                <h3 class="eyebrow">{move || t("canvas.stroke")}</h3>
                <ColorRow value=stroke allow_none=false on_pick=Callback::new(move |c: String| set("stroke", elf(move |e| if let ElementKind::Line { stroke, .. } = &mut e.kind { *stroke = c.clone() })))/>
                <Num label="▢" value=stroke_width on_set=Callback::new(move |v: f32| set("sw", elf(move |e| if let ElementKind::Line { stroke_width, .. } = &mut e.kind { *stroke_width = v.max(0.5) })))/>
            </section>
        }.into_any(),
        ElementKind::Table { cells, header, style, stroke, header_fill } => {
            let rows = cells.len();
            let cols = cells.iter().map(|r| r.len()).max().unwrap_or(1);
            view! {
                <section>
                    <h3 class="eyebrow">{move || t("canvas.table")}</h3>
                    <div class="table-edit" style=format!("grid-template-columns:repeat({cols}, minmax(0,1fr))")>
                        {cells.iter().enumerate().flat_map(|(r, row)| (0..cols).map(move |c| (r, c, row.get(c).cloned().unwrap_or_default()))).map(|(r, c, v)| view! {
                            <input class:head=header && r == 0 prop:value=v
                                on:input=move |ev| { let v = event_target_value(&ev); set(&format!("cell{r}-{c}"), elf(move |e| if let ElementKind::Table { cells, .. } = &mut e.kind {
                                    if let Some(row) = cells.get_mut(r) { while row.len() <= c { row.push(String::new()) } row[c] = v.clone(); }
                                })) }/>
                        }).collect_view()}
                    </div>
                    <div class="seg">
                        <button title=move || t("note.add_row") on:click=move |_| set("", elf(move |e| if let ElementKind::Table { cells, .. } = &mut e.kind {
                            let n = cells.iter().map(|r| r.len()).max().unwrap_or(1); cells.push(vec![String::new(); n]); e.h *= (cells.len() as f32) / (cells.len() as f32 - 1.0);
                        }))><Icon name="add_row_below"/></button>
                        <button title=move || t("note.add_column") on:click=move |_| set("", elf(|e| if let ElementKind::Table { cells, .. } = &mut e.kind { for r in cells.iter_mut() { r.push(String::new()) } }))><Icon name="add_column_right"/></button>
                        <button title=move || t("canvas.remove_row") disabled=rows <= 1 on:click=move |_| set("", elf(|e| if let ElementKind::Table { cells, .. } = &mut e.kind { if cells.len() > 1 { cells.pop(); e.h *= cells.len() as f32 / (cells.len() as f32 + 1.0); } }))><Icon name="remove"/></button>
                        <button class:on=header title=move || t("canvas.header_row") on:click=move |_| set("", elf(|e| if let ElementKind::Table { header, .. } = &mut e.kind { *header = !*header }))><Icon name="title"/></button>
                    </div>
                    <Num label="Aa" value=style.size on_set=Callback::new(move |v: f32| set("size", elf(move |e| if let ElementKind::Table { style, .. } = &mut e.kind { style.size = v.clamp(4.0, 400.0) })))/>
                    <h3 class="eyebrow">{move || t("canvas.text_color")}</h3>
                    <ColorRow value=style.color.clone() allow_none=false on_pick=Callback::new(move |c: String| set("color", elf(move |e| if let ElementKind::Table { style, .. } = &mut e.kind { style.color = c.clone() })))/>
                    <h3 class="eyebrow">{move || t("canvas.header_fill")}</h3>
                    <ColorRow value=header_fill allow_none=true on_pick=Callback::new(move |c: String| set("hf", elf(move |e| if let ElementKind::Table { header_fill, .. } = &mut e.kind { *header_fill = c.clone() })))/>
                    <h3 class="eyebrow">{move || t("canvas.stroke")}</h3>
                    <ColorRow value=stroke allow_none=true on_pick=Callback::new(move |c: String| set("stroke", elf(move |e| if let ElementKind::Table { stroke, .. } = &mut e.kind { *stroke = c.clone() })))/>
                </section>
            }.into_any()
        }
        ElementKind::Image { .. } => view! { <section></section> }.into_any(),
    };

    let locked = el.locked;
    view! {
        <div class="panel">
            <header class="row between">
                <h2 class="eyebrow strong">{move || t(&format!("canvas.{kind_key}"))}</h2>
                <div class="row">
                    <button class="btn icon ghost" title=move || t("common.duplicate") on:click=move |_| actions.run("duplicate")><Icon name="content_copy"/></button>
                    <button class="btn icon ghost danger" title=move || t("common.delete") on:click=move |_| actions.run("delete")><Icon name="delete"/></button>
                </div>
            </header>
            {specific}
            <section>
                <h3 class="eyebrow">{move || t("canvas.layout")}</h3>
                <div class="grid2">
                    <Num label="X" value=el.x on_set=num("x", |e, v| e.x = v)/>
                    <Num label="Y" value=el.y on_set=num("y", |e, v| e.y = v)/>
                    <Num label="W" value=el.w on_set=num("w", |e, v| e.w = v.max(1.0))/>
                    <Num label="H" value=el.h on_set=num("h", |e, v| e.h = v.max(1.0))/>
                    <Num label="↻" value=el.rotation on_set=num("rot", |e, v| e.rotation = v % 360.0)/>
                    <label class="num-field">
                        <span class="mono"><Icon name="opacity"/></span>
                        <input type="range" min="0" max="1" step="0.05" prop:value=el.opacity.to_string()
                            on:input=move |ev| { if let Ok(v) = event_target_value(&ev).parse::<f32>() { set("opacity", elf(move |e| e.opacity = v)) } } />
                    </label>
                </div>
                <div class="seg">
                    <button title=move || t("canvas.to_front") on:click=move |_| actions.run("front")><Icon name="flip_to_front"/></button>
                    <button title=move || t("canvas.forward") on:click=move |_| actions.run("forward")><Icon name="arrow_upward"/></button>
                    <button title=move || t("canvas.backward") on:click=move |_| actions.run("backward")><Icon name="arrow_downward"/></button>
                    <button title=move || t("canvas.to_back") on:click=move |_| actions.run("back")><Icon name="flip_to_back"/></button>
                    <button class:on=locked title=move || t("canvas.lock") on:click=move |_| set("", elf(|e| e.locked = !e.locked))><Icon name="push_pin"/></button>
                </div>
            </section>
        </div>
    }
}

#[component]
fn PagePanel(canvas: RwSignal<CanvasBody>, page: Signal<usize>, slides: bool, change: Callback<(String, CanvasFn)>) -> impl IntoView {
    let set = move |tag: &str, f: CanvasFn| change.run((tag.to_string(), f));
    let unit = Memo::new(move |_| canvas.with(|c| c.unit));
    let dims = Memo::new(move |_| canvas.with(|c| (c.width, c.height)));
    view! {
        <div class="panel">
            <header><h2 class="eyebrow strong">{move || tf(if slides { "canvas.slide_n" } else { "canvas.page_n" }, &[&(page.get() + 1).to_string()])}</h2></header>
            <section>
                <h3 class="eyebrow">{move || t("canvas.background")}</h3>
                {move || {
                    let p = page.get();
                    let bg = canvas.with(|c| c.pages.get(p).map(|x| x.background.clone()).unwrap_or_default());
                    view! { <ColorRow value=bg allow_none=false on_pick=Callback::new(move |col: String| set("bg", CanvasFn(std::sync::Arc::new(move |c: &mut CanvasBody| if let Some(pg) = c.pages.get_mut(p) { pg.background = col.clone() }))))/> }
                }}
                <button class="btn text" on:click=move |_| {
                    let p = page.get_untracked();
                    set("", CanvasFn(std::sync::Arc::new(move |c: &mut CanvasBody| { let bg = c.pages[p].background.clone(); for pg in c.pages.iter_mut() { pg.background = bg.clone() } })))
                }>{move || t("canvas.apply_all")}</button>
            </section>
            <section>
                <h3 class="eyebrow">{move || t("canvas.size")}</h3>
                {move || {
                    let (w, h) = dims.get();
                    let u = unit.get();
                    view! {
                        <div class="size-row">
                            <Num label="W" value=w step=0.1 on_set=Callback::new(move |v: f32| set("cw", CanvasFn(std::sync::Arc::new(move |c: &mut CanvasBody| { if v * c.unit.px() >= 16.0 && v * c.unit.px() <= 20000.0 { c.width = v } }))))/>
                            <Num label="H" value=h step=0.1 on_set=Callback::new(move |v: f32| set("ch", CanvasFn(std::sync::Arc::new(move |c: &mut CanvasBody| { if v * c.unit.px() >= 16.0 && v * c.unit.px() <= 20000.0 { c.height = v } }))))/>
                            <select class="unit" on:change=move |ev| {
                                let nu = match event_target_value(&ev).as_str() { "mm" => Unit::Mm, "cm" => Unit::Cm, "in" => Unit::In, _ => Unit::Px };
                                // Keep the physical size; only the unit changes.
                                set("", CanvasFn(std::sync::Arc::new(move |c: &mut CanvasBody| { let f = c.unit.px() / nu.px(); c.width *= f; c.height *= f; c.unit = nu; })))
                            }>
                                {Unit::ALL.into_iter().map(|x| view! { <option value=x.label() selected=x == u>{x.label()}</option> }).collect_view()}
                            </select>
                        </div>
                    }
                }}
            </section>
            {slides.then(|| view! {
                <section>
                    <h3 class="eyebrow">{move || t("canvas.notes")}</h3>
                    {move || {
                        let p = page.get();
                        let notes = canvas.with(|c| c.pages.get(p).map(|x| x.notes.clone()).unwrap_or_default());
                        view! {
                            <textarea class="text-area" rows="5" placeholder=move || t("canvas.notes_hint") prop:value=notes
                                on:input=move |ev| { let v = event_target_value(&ev); set("notes", CanvasFn(std::sync::Arc::new(move |c: &mut CanvasBody| if let Some(pg) = c.pages.get_mut(p) { pg.notes = v.clone() }))) }></textarea>
                        }
                    }}
                </section>
            })}
            <p class="fineprint">{move || t("canvas.tips")}</p>
        </div>
    }
}

#[component]
fn Presenter(canvas: RwSignal<CanvasBody>, start: usize, close: Callback<()>) -> impl IntoView {
    let cur = RwSignal::new(start);
    let n = move || canvas.with(|c| c.pages.len());
    let next = move || cur.update(|c| *c = (*c + 1).min(n() - 1));
    let prev = move || cur.update(|c| *c = c.saturating_sub(1));
    let exit = move || {
        let d = crate::util::document();
        if d.fullscreen_element().is_some() {
            d.exit_fullscreen();
        }
        close.run(());
    };
    let keys = window_event_listener(ev::keydown, move |ev| {
        match ev.key().as_str() {
            "ArrowRight" | "ArrowDown" | "PageDown" | " " | "Enter" => next(),
            "ArrowLeft" | "ArrowUp" | "PageUp" | "Backspace" => prev(),
            "Home" => cur.set(0),
            "End" => cur.set(n() - 1),
            "Escape" => exit(),
            _ => return,
        }
        ev.prevent_default();
    });
    on_cleanup(move || keys.remove());
    if let Some(el) = crate::util::document().document_element() {
        let _ = el.request_fullscreen();
    }
    let svg = Memo::new(move |_| {
        let i = cur.get();
        canvas.with(|c| c.pages.get(i).map(|p| page_svg(c, p, &CanvasMeasure)).unwrap_or_default())
    });
    view! {
        <div class="presenter" on:click=move |ev: web_sys::MouseEvent| {
            let w = crate::util::window().inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1000.0);
            if (ev.client_x() as f64) < w / 3.0 { prev() } else { next() }
        }>
            <div class="slide" inner_html=move || svg.get()></div>
            <div class="presenter-bar" on:click=|ev| ev.stop_propagation()>
                <button class="btn icon ghost" on:click=move |_| prev()><Icon name="chevron_left"/></button>
                <span class="mono">{move || format!("{} / {}", cur.get() + 1, n())}</span>
                <button class="btn icon ghost" on:click=move |_| next()><Icon name="chevron_right"/></button>
                <button class="btn icon ghost" title=move || t("common.close") on:click=move |_| exit()><Icon name="fullscreen_exit"/></button>
            </div>
        </div>
    }
}
