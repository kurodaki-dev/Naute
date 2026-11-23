//! Small helpers shared by the views.

use leptos::prelude::*;
use naute_core::model::FontFamily;
use naute_core::text::Measure;
use std::cell::RefCell;
use wasm_bindgen::JsCast;

pub fn now() -> f64 {
    js_sys::Date::now()
}

pub fn window() -> web_sys::Window {
    web_sys::window().unwrap()
}

pub fn document() -> web_sys::Document {
    window().document().unwrap()
}

/// Material Symbols glyph.
#[component]
pub fn Icon(#[prop(into)] name: String, #[prop(optional)] fill: bool) -> impl IntoView {
    view! { <span class="ms" class:fill=fill aria-hidden="true">{name}</span> }
}

// ---------------------------------------------------------------- toasts

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub text: String,
    pub action: Option<(String, String)>,
}

thread_local! {
    static TOASTS: RefCell<Option<RwSignal<Vec<Toast>>>> = const { RefCell::new(None) };
}

pub fn toasts() -> RwSignal<Vec<Toast>> {
    TOASTS.with(|t| {
        let mut t = t.borrow_mut();
        if t.is_none() {
            *t = Some(RwSignal::new(vec![]));
        }
        t.unwrap()
    })
}

/// Show a short message. `action` is (label, path to reveal).
pub fn toast(text: impl Into<String>, action: Option<(String, String)>) {
    let id = now() as u64;
    let list = toasts();
    list.update(|l| l.push(Toast { id, text: text.into(), action }));
    set_timeout(move || list.update(|l| l.retain(|t| t.id != id)), std::time::Duration::from_millis(4200));
}

// ---------------------------------------------------------------- measuring

thread_local! {
    static CTX: RefCell<Option<web_sys::CanvasRenderingContext2d>> = const { RefCell::new(None) };
}

/// Measures with the browser's canvas, using the same bundled fonts the
/// exporter uses, so line breaks match between screen and file.
pub struct CanvasMeasure;

impl Measure for CanvasMeasure {
    fn width(&self, text: &str, font: FontFamily, size: f32, bold: bool, italic: bool) -> f32 {
        CTX.with(|c| {
            let mut c = c.borrow_mut();
            if c.is_none() {
                let canvas: web_sys::HtmlCanvasElement = document().create_element("canvas").unwrap().unchecked_into();
                *c = canvas.get_context("2d").ok().flatten().map(|x| x.unchecked_into());
            }
            let Some(ctx) = c.as_ref() else {
                return naute_core::text::ApproxMeasure.width(text, font, size, bold, italic);
            };
            // Measure at 100px and scale: avoids rounding at small sizes.
            ctx.set_font(&format!(
                "{} {} 100px {}",
                if italic { "italic" } else { "normal" },
                if bold { 600 } else { 400 },
                font.css()
            ));
            ctx.measure_text(text).map(|m| m.width() as f32 * size / 100.0).unwrap_or(0.0)
        })
    }
}

pub fn fmt_date(ms: f64) -> String {
    let d = js_sys::Date::new(&ms.into());
    let today = js_sys::Date::new_0();
    let same_day = d.get_full_year() == today.get_full_year() && d.get_month() == today.get_month() && d.get_date() == today.get_date();
    let opts = js_sys::Object::new();
    let set = |k: &str, v: &str| {
        let _ = js_sys::Reflect::set(&opts, &k.into(), &v.into());
    };
    if same_day {
        set("hour", "2-digit");
        set("minute", "2-digit");
    } else {
        set("day", "numeric");
        set("month", "short");
        if d.get_full_year() != today.get_full_year() {
            set("year", "numeric");
        }
    }
    d.to_locale_string(&crate::i18n::locale_tag(), &opts).as_string().unwrap_or_default()
}

pub fn fmt_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1e9)
    } else {
        format!("{} MB", (bytes as f64 / 1e6).round())
    }
}

/// Read a picked image as a data URL, downscaled so documents stay light.
pub async fn read_image(file: web_sys::File) -> Option<(String, f32, f32)> {
    let url = web_sys::Url::create_object_url_with_blob(&file).ok()?;
    let img = web_sys::HtmlImageElement::new().ok()?;
    let (tx, rx) = futures_channel();
    let tx = std::rc::Rc::new(RefCell::new(Some(tx)));
    let ok = {
        let tx = tx.clone();
        wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            if let Some(t) = tx.borrow_mut().take() {
                t(true)
            }
        })
    };
    let err = {
        let tx = tx.clone();
        wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            if let Some(t) = tx.borrow_mut().take() {
                t(false)
            }
        })
    };
    img.set_onload(Some(ok.as_ref().unchecked_ref()));
    img.set_onerror(Some(err.as_ref().unchecked_ref()));
    img.set_src(&url);
    let loaded = rx.await;
    let _ = web_sys::Url::revoke_object_url(&url);
    if !loaded {
        return None;
    }
    let (w, h) = (img.natural_width() as f64, img.natural_height() as f64);
    let max = 2400.0;
    let s = (max / w.max(h)).min(1.0);
    let (tw, th) = ((w * s).round().max(1.0), (h * s).round().max(1.0));
    let canvas: web_sys::HtmlCanvasElement = document().create_element("canvas").ok()?.unchecked_into();
    canvas.set_width(tw as u32);
    canvas.set_height(th as u32);
    let ctx: web_sys::CanvasRenderingContext2d = canvas.get_context("2d").ok()??.unchecked_into();
    ctx.draw_image_with_html_image_element_and_dw_and_dh(&img, 0.0, 0.0, tw, th).ok()?;
    let mime = if file.type_() == "image/png" || file.type_() == "image/gif" || file.type_() == "image/webp" { "image/png" } else { "image/jpeg" };
    let data = canvas.to_data_url_with_type_and_encoder_options(mime, &0.9.into()).ok()?;
    Some((data, tw as f32, th as f32))
}

/// A one-shot callback turned into a future, without extra crates.
fn futures_channel() -> (impl FnOnce(bool), impl std::future::Future<Output = bool>) {
    let state: std::rc::Rc<RefCell<(Option<bool>, Option<std::task::Waker>)>> = Default::default();
    let s2 = state.clone();
    let send = move |v: bool| {
        let mut s = s2.borrow_mut();
        s.0 = Some(v);
        if let Some(w) = s.1.take() {
            w.wake();
        }
    };
    let fut = std::future::poll_fn(move |cx| {
        let mut s = state.borrow_mut();
        match s.0 {
            Some(v) => std::task::Poll::Ready(v),
            None => {
                s.1 = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        }
    });
    (send, fut)
}

/// Debounced saver: call `bump()` on every change; `f` runs once things settle.
pub fn debounce(ms: u64, f: impl Fn() + 'static) -> impl Fn() + 'static {
    let gen = std::rc::Rc::new(std::cell::Cell::new(0u64));
    let f = std::rc::Rc::new(f);
    move || {
        let g = gen.get() + 1;
        gen.set(g);
        let gen = gen.clone();
        let f = f.clone();
        let cb = wasm_bindgen::closure::Closure::once_into_js(move || {
            if gen.get() == g {
                f();
            }
        });
        let _ = window().set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), ms as i32);
    }
}

