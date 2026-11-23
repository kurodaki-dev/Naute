//! Spreadsheet: a formula bar, a grid with keyboard navigation and
//! resizable columns. Formulas are evaluated by naute-core.

use crate::dictate::DictateButton;
use crate::i18n::t;
use crate::util::Icon;
use leptos::html::{Div, Input};
use leptos::prelude::*;
use naute_core::model::*;
use naute_core::sheet::{compute, Value};
use std::collections::HashMap;

const DEFAULT_W: f32 = 112.0;
const FILLS: &[(&str, &str)] = &[("", "none"), ("#F4F3EE", "paper"), ("#E9E6DD", "sand"), ("#F3DDD3", "clay"), ("#C15F3C", "terracotta")];

#[component]
pub fn SheetEditor(doc: RwSignal<Option<Document>>, rev: RwSignal<u64>) -> impl IntoView {
    let sheet = RwSignal::new(doc.with_untracked(|d| match d.as_ref().map(|d| &d.body) {
        Some(Body::Sheet(s)) => s.clone(),
        _ => SheetBody::default(),
    }));
    let values = Memo::new(move |_| sheet.with(|s| compute(s).into_iter().collect::<HashMap<String, Value>>()));
    let sel = RwSignal::new((0u32, 0u32));
    let editing = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let grid = NodeRef::<Div>::new();
    let cell_input = NodeRef::<Input>::new();
    let bar_input = NodeRef::<Input>::new();
    let resizing = StoredValue::new(None::<(u32, f64, f32)>);

    let commit_sheet = move |f: &mut dyn FnMut(&mut SheetBody)| {
        sheet.update(|s| f(s));
        let s = sheet.get_untracked();
        doc.update(|d| {
            if let Some(d) = d {
                d.body = Body::Sheet(s);
            }
        });
        rev.update(|r| *r += 1);
    };

    let input_at = move |r: u32, c: u32| sheet.with(|s| s.cells.get(&cell_ref(r, c)).map(|c| c.input.clone()).unwrap_or_default());

    let write_cell = move |r: u32, c: u32, text: String| {
        commit_sheet(&mut |s: &mut SheetBody| {
            let key = cell_ref(r, c);
            let cell = s.cells.entry(key.clone()).or_default();
            cell.input = text.clone();
            if cell.input.is_empty() && !cell.bold && cell.align.is_none() && cell.fill.is_none() {
                s.cells.remove(&key);
            }
            // Grow the grid when typing near its edge.
            if r + 3 >= s.rows {
                s.rows = r + 10;
            }
            if c + 2 >= s.cols {
                s.cols = c + 4;
            }
        });
    };

    let focus_grid = move || {
        if let Some(g) = grid.get_untracked() {
            let _ = g.focus();
        }
    };
    let begin_edit = move |initial: Option<String>| {
        let (r, c) = sel.get_untracked();
        draft.set(initial.unwrap_or_else(|| input_at(r, c)));
        editing.set(true);
    };
    let finish_edit = move |save: bool| {
        if editing.get_untracked() {
            if save {
                let (r, c) = sel.get_untracked();
                write_cell(r, c, draft.get_untracked());
            }
            editing.set(false);
        }
    };
    let move_sel = move |dr: i32, dc: i32| {
        let (r, c) = sel.get_untracked();
        let (rows, cols) = sheet.with_untracked(|s| (s.rows, s.cols));
        let nr = (r as i32 + dr).clamp(0, rows as i32 - 1) as u32;
        let nc = (c as i32 + dc).clamp(0, cols as i32 - 1) as u32;
        sel.set((nr, nc));
        if let Some(g) = grid.get_untracked() {
            if let Ok(Some(el)) = g.query_selector(&format!("[data-cell='{}']", cell_ref(nr, nc))) {
                let opts = web_sys::ScrollIntoViewOptions::new();
                opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
                opts.set_inline(web_sys::ScrollLogicalPosition::Nearest);
                el.scroll_into_view_with_scroll_into_view_options(&opts);
            }
        }
    };

    // Keep the formula bar in sync with the selection.
    Effect::new(move |_| {
        let (r, c) = sel.get();
        if !editing.get_untracked() {
            draft.set(input_at(r, c));
        }
    });
    Effect::new(move |_| {
        if editing.get() {
            if let Some(i) = cell_input.get() {
                let _ = i.focus();
                let len = i.value().len() as u32;
                let _ = i.set_selection_range(len, len);
            }
        }
    });

    let on_key = move |ev: web_sys::KeyboardEvent| {
        let key = ev.key();
        let ctrl = ev.ctrl_key() || ev.meta_key();
        if editing.get_untracked() {
            match key.as_str() {
                "Enter" => {
                    ev.prevent_default();
                    finish_edit(true);
                    move_sel(if ev.shift_key() { -1 } else { 1 }, 0);
                    focus_grid();
                }
                "Tab" => {
                    ev.prevent_default();
                    finish_edit(true);
                    move_sel(0, if ev.shift_key() { -1 } else { 1 });
                    focus_grid();
                }
                "Escape" => {
                    finish_edit(false);
                    focus_grid();
                }
                _ => {}
            }
            return;
        }
        match key.as_str() {
            "ArrowUp" => move_sel(-1, 0),
            "ArrowDown" => move_sel(1, 0),
            "ArrowLeft" => move_sel(0, -1),
            "ArrowRight" => move_sel(0, 1),
            "Tab" => move_sel(0, if ev.shift_key() { -1 } else { 1 }),
            "Enter" | "F2" => begin_edit(None),
            "Delete" | "Backspace" => {
                let (r, c) = sel.get_untracked();
                write_cell(r, c, String::new());
                draft.set(String::new());
            }
            "b" | "B" if ctrl => toggle_bold(sel, commit_sheet),
            k if !ctrl && !ev.alt_key() && k.chars().count() == 1 => {
                begin_edit(Some(k.to_string()));
            }
            _ => return,
        }
        ev.prevent_default();
    };

    let set_align = move |a: Option<Align>| {
        let (r, c) = sel.get_untracked();
        commit_sheet(&mut |s: &mut SheetBody| {
            s.cells.entry(cell_ref(r, c)).or_default().align = a;
        });
    };
    let set_fill = move |f: &'static str| {
        let (r, c) = sel.get_untracked();
        commit_sheet(&mut |s: &mut SheetBody| {
            s.cells.entry(cell_ref(r, c)).or_default().fill = if f.is_empty() { None } else { Some(f.into()) };
        });
    };
    let auto_sum = move |_| {
        let (r, c) = sel.get_untracked();
        // Sum the contiguous numbers above, like most spreadsheets do.
        let mut top = r;
        values.with_untracked(|v| {
            while top > 0 && v.get(&cell_ref(top - 1, c)).map(|x| x.is_number()).unwrap_or(false) {
                top -= 1;
            }
        });
        let f = if top < r { format!("=SUM({}:{})", cell_ref(top, c), cell_ref(r - 1, c)) } else { "=SUM()".into() };
        write_cell(r, c, f.clone());
        draft.set(f);
    };
    let add_row = move |_| commit_sheet(&mut |s: &mut SheetBody| s.rows += 1);
    let add_col = move |_| commit_sheet(&mut |s: &mut SheetBody| s.cols += 1);
    let dictated = Callback::new(move |text: String| {
        let (r, c) = sel.get_untracked();
        let cur = input_at(r, c);
        let v = if cur.is_empty() { text } else { format!("{cur} {text}") };
        write_cell(r, c, v.clone());
        draft.set(v);
    });

    let col_w = move |c: u32| sheet.with(|s| s.col_widths.get(&c).copied().unwrap_or(DEFAULT_W));
    // Memos so the grid is only rebuilt when its shape changes.
    let rows_m = Memo::new(move |_| sheet.with(|s| s.rows));
    let cols_m = Memo::new(move |_| sheet.with(|s| s.cols));
    let rows = move || rows_m.get();
    let cols = move || cols_m.get();
    let cur = move |f: fn(&Cell) -> bool| {
        let (r, c) = sel.get();
        sheet.with(|s| s.cells.get(&cell_ref(r, c)).map(f).unwrap_or(false))
    };

    view! {
        <div class="toolbar sheet-bar" role="toolbar">
            <span class="cell-name mono">{move || { let (r, c) = sel.get(); cell_ref(r, c) }}</span>
            <label class="formula">
                <Icon name="functions"/>
                <input
                    node_ref=bar_input
                    class="mono"
                    placeholder=move || t("sheet.formula_hint")
                    prop:value=move || draft.get()
                    on:focus=move |_| editing.set(true)
                    on:input=move |ev| { editing.set(true); draft.set(event_target_value(&ev)); }
                    on:keydown=on_key
                    on:blur=move |_| finish_edit(true)
                />
            </label>
            <div class="tool-group">
                <button class="btn icon ghost" class:on=move || cur(|c| c.bold) title=move || t("note.bold")
                    on:click=move |_| toggle_bold(sel, commit_sheet)><Icon name="format_bold"/></button>
                <button class="btn icon ghost" class:on=move || cur(|c| c.align == Some(Align::Left)) title=move || t("align.left")
                    on:click=move |_| set_align(Some(Align::Left))><Icon name="format_align_left"/></button>
                <button class="btn icon ghost" class:on=move || cur(|c| c.align == Some(Align::Center)) title=move || t("align.center")
                    on:click=move |_| set_align(Some(Align::Center))><Icon name="format_align_center"/></button>
                <button class="btn icon ghost" class:on=move || cur(|c| c.align == Some(Align::Right)) title=move || t("align.right")
                    on:click=move |_| set_align(Some(Align::Right))><Icon name="format_align_right"/></button>
            </div>
            <div class="tool-group swatches">
                {FILLS.iter().map(|(color, name)| {
                    let color: &'static str = color;
                    view! {
                        <button class="swatch" class:none=color.is_empty() style=format!("--c:{}", if color.is_empty() { "#fff" } else { color })
                            title=move || t(&format!("color.{name}")) on:click=move |_| set_fill(color)></button>
                    }
                }).collect_view()}
            </div>
            <div class="tool-group">
                <button class="btn icon ghost" title=move || t("sheet.autosum") on:click=auto_sum><span class="sigma">"Σ"</span></button>
                <button class="btn icon ghost" title=move || t("note.add_row") on:click=add_row><Icon name="add_row_below"/></button>
                <button class="btn icon ghost" title=move || t("note.add_column") on:click=add_col><Icon name="add_column_right"/></button>
            </div>
            <DictateButton on_text=dictated/>
        </div>
        <div class="grid-scroll" tabindex="0" node_ref=grid on:keydown=on_key>
            <table class="grid" style=move || format!("width:{}px", 44.0 + (0..cols()).map(|c| col_w(c)).sum::<f32>())>
                <colgroup>
                    <col style="width:44px"/>
                    {move || (0..cols()).map(|c| view! { <col style=move || format!("width:{}px", col_w(c))/> }).collect_view()}
                </colgroup>
                <thead>
                    <tr>
                        <th class="corner"></th>
                        {move || (0..cols()).map(|c| view! {
                            <th class:hl=move || sel.get().1 == c>
                                {col_name(c)}
                                <span
                                    class="col-resize"
                                    on:pointerdown=move |ev: web_sys::PointerEvent| {
                                        ev.prevent_default();
                                        if let Some(t) = ev.target() {
                                            use wasm_bindgen::JsCast;
                                            if let Ok(el) = t.dyn_into::<web_sys::Element>() { let _ = el.set_pointer_capture(ev.pointer_id()); }
                                        }
                                        resizing.set_value(Some((c, ev.client_x() as f64, col_w(c))));
                                    }
                                    on:pointermove=move |ev: web_sys::PointerEvent| {
                                        if let Some((col, x0, w0)) = resizing.get_value() {
                                            let w = (w0 + (ev.client_x() as f64 - x0) as f32).clamp(40.0, 600.0);
                                            sheet.update(|s| { s.col_widths.insert(col, w.round()); });
                                        }
                                    }
                                    on:pointerup=move |_| {
                                        if resizing.get_value().is_some() {
                                            resizing.set_value(None);
                                            commit_sheet(&mut |_s: &mut SheetBody| {});
                                        }
                                    }
                                ></span>
                            </th>
                        }).collect_view()}
                    </tr>
                </thead>
                <tbody>
                    {move || (0..rows()).map(|r| view! {
                        <tr>
                            <th class:hl=move || sel.get().0 == r>{r + 1}</th>
                            {move || (0..cols()).map(|c| {
                                let key = cell_ref(r, c);
                                let k1 = key.clone();
                                let k2 = key.clone();
                                let k3 = key.clone();
                                let k4 = key.clone();
                                let is_sel = move || sel.get() == (r, c);
                                view! {
                                    <td
                                        data-cell=key.clone()
                                        class:sel=is_sel
                                        class:bold=move || sheet.with(|s| s.cells.get(&k1).map(|c| c.bold).unwrap_or(false))
                                        class:err=move || values.with(|v| matches!(v.get(&k3), Some(Value::Err(_))))
                                        style=move || {
                                            let (fill, align) = sheet.with(|s| s.cells.get(&k2).map(|c| (c.fill.clone(), c.align)).unwrap_or((None, None)));
                                            let num = values.with(|v| v.get(&k2).map(|x| x.is_number()).unwrap_or(false));
                                            let a = align.map(|a| a.key()).unwrap_or(if num { "right" } else { "left" });
                                            let light = fill.as_deref() == Some("#C15F3C");
                                            format!("text-align:{a};{}{}", fill.map(|f| format!("background:{f};")).unwrap_or_default(), if light { "color:#fff;" } else { "" })
                                        }
                                        on:mousedown=move |_| {
                                            if !is_sel() { finish_edit(true); }
                                            sel.set((r, c));
                                        }
                                        on:click=move |_| {}
                                        on:dblclick=move |_| begin_edit(None)
                                    >
                                        {move || if is_sel() && editing.get() {
                                            view! {
                                                <input
                                                    class="cell-edit"
                                                    node_ref=cell_input
                                                    prop:value=move || draft.get()
                                                    on:input=move |ev| draft.set(event_target_value(&ev))
                                                    on:blur=move |_| finish_edit(true)
                                                />
                                            }.into_any()
                                        } else {
                                            let k = k4.clone();
                                            view! { <span>{move || values.with(|v| v.get(&k).map(|x| x.display()).unwrap_or_default())}</span> }.into_any()
                                        }}
                                    </td>
                                }
                            }).collect_view()}
                        </tr>
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }
}

fn toggle_bold(sel: RwSignal<(u32, u32)>, commit: impl Fn(&mut dyn FnMut(&mut SheetBody))) {
    let (r, c) = sel.get_untracked();
    commit(&mut |s: &mut SheetBody| {
        let cell = s.cells.entry(cell_ref(r, c)).or_default();
        cell.bold = !cell.bold;
    });
}
