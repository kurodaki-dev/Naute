//! Rich-text notes: headings, lists, checklists, quotes, tables, images and
//! dictation, on a contenteditable surface.

use crate::dictate::DictateButton;
use crate::i18n::t;
use crate::util::{document, read_image, Icon};
use leptos::html::Div;
use leptos::prelude::*;
use naute_core::model::*;
use naute_core::note::{Block, Run};
use naute_core::text::xml_escape;
use wasm_bindgen::JsCast;

fn exec(cmd: &str, value: &str) {
    if let Ok(d) = document().dyn_into::<web_sys::HtmlDocument>() {
        let _ = d.exec_command_with_show_ui_and_value(cmd, false, value);
    }
}

fn selection_element() -> Option<web_sys::Element> {
    let sel = document().get_selection().ok()??;
    let node = sel.anchor_node()?;
    if node.node_type() == web_sys::Node::ELEMENT_NODE {
        node.dyn_into::<web_sys::Element>().ok()
    } else {
        node.parent_element()
    }
}

fn closest(tag: &str) -> Option<web_sys::Element> {
    selection_element()?.closest(tag).ok()?
}

/// Current block format, for toolbar state.
fn current_block() -> String {
    if closest("ul[data-type=check]").is_some() {
        return "check".into();
    }
    for tag in ["h1", "h2", "h3", "blockquote", "ul", "ol"] {
        if closest(tag).is_some() {
            return tag.into();
        }
    }
    "p".into()
}

#[component]
pub fn NoteEditor(doc: RwSignal<Option<Document>>, rev: RwSignal<u64>, node: NodeRef<Div>) -> impl IntoView {
    let initial = doc.with_untracked(|d| match d.as_ref().map(|d| &d.body) {
        Some(Body::Note(n)) => n.html.clone(),
        _ => String::new(),
    });
    let block = RwSignal::new("p".to_string());
    let saved_range = StoredValue::new_local(None::<web_sys::Range>);

    let sync = move || {
        if let Some(el) = node.get_untracked() {
            let html = el.inner_html();
            doc.update(|d| {
                if let Some(d) = d {
                    d.body = Body::Note(NoteBody { html });
                }
            });
            rev.update(|r| *r += 1);
        }
    };
    let remember = move || {
        if let Some(sel) = document().get_selection().ok().flatten() {
            if sel.range_count() > 0 {
                if let (Ok(r), Some(el)) = (sel.get_range_at(0), node.get_untracked()) {
                    if el.contains(Some(&r.common_ancestor_container().unwrap())) {
                        saved_range.set_value(Some(r));
                    }
                }
            }
        }
        block.set(current_block());
    };
    let restore = move || {
        if let Some(el) = node.get_untracked() {
            let _ = el.focus();
        }
        if let (Some(r), Some(sel)) = (saved_range.get_value(), document().get_selection().ok().flatten()) {
            let _ = sel.remove_all_ranges();
            let _ = sel.add_range(&r);
        }
    };

    let cmd = move |c: &'static str, v: &'static str| {
        move |ev: web_sys::MouseEvent| {
            ev.prevent_default();
            restore();
            exec(c, v);
            sync();
            remember();
        }
    };
    let format_block = move |tag: &'static str| {
        move |ev: web_sys::MouseEvent| {
            ev.prevent_default();
            restore();
            let cur = current_block();
            exec("formatBlock", if cur == tag { "p" } else { tag });
            sync();
            remember();
        }
    };
    let checklist = move |ev: web_sys::MouseEvent| {
        ev.prevent_default();
        restore();
        if let Some(ul) = closest("ul[data-type=check]") {
            // Toggle off: turn the checklist back into a plain list.
            let _ = ul.remove_attribute("data-type");
            exec("insertUnorderedList", "");
        } else {
            if closest("ul").is_none() {
                exec("insertUnorderedList", "");
            }
            if let Some(ul) = closest("ul") {
                let _ = ul.set_attribute("data-type", "check");
            }
        }
        sync();
        remember();
    };
    let insert_table = move |ev: web_sys::MouseEvent| {
        ev.prevent_default();
        restore();
        let row = |cell: &str| format!("<tr>{}</tr>", (0..3).map(|_| format!("<{cell}><br></{cell}>")).collect::<String>());
        let html = format!("<table><tbody>{}{}{}</tbody></table><p><br></p>", row("th"), row("td"), row("td"));
        exec("insertHTML", &html);
        sync();
    };
    let table_add = move |what: &'static str| {
        move |ev: web_sys::MouseEvent| {
            ev.prevent_default();
            restore();
            let Some(table) = closest("table") else { return };
            let Ok(table) = table.dyn_into::<web_sys::HtmlTableElement>() else { return };
            let rows = table.rows();
            if what == "row" {
                let cols = rows.item(0).map(|r| r.unchecked_into::<web_sys::HtmlTableRowElement>().cells().length()).unwrap_or(1);
                if let Ok(r) = table.insert_row() {
                    let r: web_sys::HtmlTableRowElement = r.unchecked_into();
                    for _ in 0..cols {
                        if let Ok(c) = r.insert_cell() {
                            c.set_inner_html("<br>");
                        }
                    }
                }
            } else {
                for i in 0..rows.length() {
                    if let Some(r) = rows.item(i) {
                        let r: web_sys::HtmlTableRowElement = r.unchecked_into();
                        let tag = if i == 0 { "th" } else { "td" };
                        if let Ok(c) = document().create_element(tag) {
                            c.set_inner_html("<br>");
                            let _ = r.append_child(&c);
                        }
                    }
                }
            }
            sync();
        }
    };
    let file_input = NodeRef::<leptos::html::Input>::new();
    let on_image = move |_| {
        let Some(input) = file_input.get_untracked() else { return };
        let Some(file) = input.files().and_then(|f| f.get(0)) else { return };
        input.set_value("");
        leptos::task::spawn_local(async move {
            if let Some((data, _, _)) = read_image(file).await {
                restore();
                exec("insertHTML", &format!("<p><img src=\"{}\" alt=\"\"></p><p><br></p>", xml_escape(&data)));
                sync();
            }
        });
    };
    let dictated = Callback::new(move |text: String| {
        restore();
        // Add a space when continuing a sentence.
        let needs_space = saved_range
            .get_value()
            .and_then(|r| {
                let c = r.start_container().ok()?;
                let txt = c.text_content()?;
                let off = r.start_offset().ok()? as usize;
                txt.chars().take(off).last()
            })
            .map(|c| !c.is_whitespace())
            .unwrap_or(false);
        exec("insertText", &if needs_space { format!(" {text}") } else { text });
        sync();
        remember();
    });

    // Checklist items toggle when their box (the left gutter) is clicked.
    let on_click = move |ev: web_sys::MouseEvent| {
        if let Some(target) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
            if target.tag_name() == "LI" && target.closest("ul[data-type=check]").ok().flatten().is_some() {
                let rect = target.get_bounding_client_rect();
                if (ev.client_x() as f64) < rect.left() + 26.0 {
                    let on = target.get_attribute("data-checked").as_deref() == Some("true");
                    let _ = target.set_attribute("data-checked", if on { "false" } else { "true" });
                    ev.prevent_default();
                    sync();
                }
            }
        }
        remember();
    };

    Effect::new(move |_| {
        if let Some(el) = node.get() {
            el.set_inner_html(&initial);
            if initial.trim().is_empty() {
                let _ = el.focus();
            }
        }
    });

    let tb = move |icon: &'static str, key: &'static str, active: Option<&'static str>, f: Box<dyn FnMut(web_sys::MouseEvent)>| {
        let mut f = f;
        view! {
            <button
                class="btn icon ghost"
                class:on=move || active.map(|a| block.get() == a).unwrap_or(false)
                title=move || t(key)
                on:mousedown=|ev| ev.prevent_default()
                on:click=move |ev| f(ev)
            >
                <Icon name=icon/>
            </button>
        }
    };

    view! {
        <div class="toolbar" role="toolbar">
            <div class="tool-group">
                {tb("format_h1", "note.h1", Some("h1"), Box::new(format_block("h1")))}
                {tb("format_h2", "note.h2", Some("h2"), Box::new(format_block("h2")))}
            </div>
            <div class="tool-group">
                {tb("format_bold", "note.bold", None, Box::new(cmd("bold", "")))}
                {tb("format_italic", "note.italic", None, Box::new(cmd("italic", "")))}
                {tb("format_underlined", "note.underline", None, Box::new(cmd("underline", "")))}
            </div>
            <div class="tool-group">
                {tb("format_list_bulleted", "note.bullets", Some("ul"), Box::new(cmd("insertUnorderedList", "")))}
                {tb("format_list_numbered", "note.numbers", Some("ol"), Box::new(cmd("insertOrderedList", "")))}
                {tb("checklist", "note.checklist", Some("check"), Box::new(checklist))}
                {tb("format_quote", "note.quote", Some("blockquote"), Box::new(format_block("blockquote")))}
            </div>
            <div class="tool-group">
                {tb("table", "note.table", None, Box::new(insert_table))}
                {tb("add_row_below", "note.add_row", None, Box::new(table_add("row")))}
                {tb("add_column_right", "note.add_column", None, Box::new(table_add("col")))}
                {tb("image", "note.image", None, Box::new(move |ev: web_sys::MouseEvent| {
                    ev.prevent_default();
                    remember();
                    if let Some(i) = file_input.get_untracked() { i.click(); }
                }))}
                {tb("horizontal_rule", "note.rule", None, Box::new(cmd("insertHorizontalRule", "")))}
            </div>
            <div class="tool-group">
                {tb("undo", "common.undo", None, Box::new(cmd("undo", "")))}
                {tb("redo", "common.redo", None, Box::new(cmd("redo", "")))}
            </div>
            <div class="grow"></div>
            <DictateButton on_text=dictated label=true/>
            <input type="file" accept="image/*" class="hidden" node_ref=file_input on:change=on_image/>
        </div>
        <div class="note-scroll">
            <article class="note-paper">
                <div
                    class="note-body"
                    contenteditable="true"
                    spellcheck="true"
                    node_ref=node
                    data-placeholder=move || t("note.placeholder")
                    on:input=move |_| sync()
                    on:keyup=move |_| remember()
                    on:mouseup=move |_| remember()
                    on:blur=move |_| remember()
                    on:click=on_click
                ></div>
            </article>
        </div>
    }
}

// ---------------------------------------------------------------- DOM -> blocks

fn runs(node: &web_sys::Node, bold: bool, italic: bool, underline: bool, out: &mut Vec<Run>) {
    let children = node.child_nodes();
    for i in 0..children.length() {
        let Some(c) = children.item(i) else { continue };
        match c.node_type() {
            web_sys::Node::TEXT_NODE => {
                let text = c.text_content().unwrap_or_default().replace('\u{a0}', " ");
                if text.is_empty() {
                    continue;
                }
                match out.last_mut() {
                    Some(r) if r.bold == bold && r.italic == italic && r.underline == underline => r.text.push_str(&text),
                    _ => out.push(Run { text, bold, italic, underline }),
                }
            }
            web_sys::Node::ELEMENT_NODE => {
                let el: &web_sys::Element = c.unchecked_ref();
                match el.tag_name().as_str() {
                    "BR" => out.push(Run { text: "\n".into(), bold, italic, underline }),
                    "B" | "STRONG" => runs(&c, true, italic, underline, out),
                    "I" | "EM" => runs(&c, bold, true, underline, out),
                    "U" => runs(&c, bold, italic, true, out),
                    "UL" | "OL" => {}
                    _ => runs(&c, bold, italic, underline, out),
                }
            }
            _ => {}
        }
    }
}

fn inline(node: &web_sys::Node) -> Vec<Run> {
    let mut out = Vec::new();
    runs(node, false, false, false, &mut out);
    // Trailing <br> placeholders are not content.
    while out.last().map(|r| r.text.trim().is_empty()).unwrap_or(false) {
        out.pop();
    }
    out
}

fn walk(node: &web_sys::Node, out: &mut Vec<Block>) {
    let children = node.child_nodes();
    let mut pending: Vec<Run> = Vec::new();
    let flush = |pending: &mut Vec<Run>, out: &mut Vec<Block>| {
        if pending.iter().any(|r| !r.text.trim().is_empty()) {
            out.push(Block::Paragraph { runs: std::mem::take(pending) });
        }
        pending.clear();
    };
    for i in 0..children.length() {
        let Some(c) = children.item(i) else { continue };
        if c.node_type() == web_sys::Node::TEXT_NODE {
            let text = c.text_content().unwrap_or_default();
            if !text.trim().is_empty() {
                pending.push(Run { text, ..Default::default() });
            }
            continue;
        }
        if c.node_type() != web_sys::Node::ELEMENT_NODE {
            continue;
        }
        let el: &web_sys::Element = c.unchecked_ref();
        let tag = el.tag_name();
        match tag.as_str() {
            "B" | "STRONG" | "I" | "EM" | "U" | "SPAN" | "A" => {
                runs(&c, false, false, false, &mut pending);
                continue;
            }
            "BR" => {
                flush(&mut pending, out);
                continue;
            }
            _ => flush(&mut pending, out),
        }
        match tag.as_str() {
            "H1" => out.push(Block::Heading { level: 1, runs: inline(&c) }),
            "H2" => out.push(Block::Heading { level: 2, runs: inline(&c) }),
            "H3" | "H4" | "H5" | "H6" => out.push(Block::Heading { level: 3, runs: inline(&c) }),
            "BLOCKQUOTE" => out.push(Block::Quote { runs: inline(&c) }),
            "HR" => out.push(Block::Rule),
            "IMG" => {
                let img: &web_sys::HtmlImageElement = c.unchecked_ref();
                out.push(Block::Image { src: img.src(), w: img.natural_width().max(1) as f32, h: img.natural_height().max(1) as f32 });
            }
            "UL" | "OL" => {
                let check = el.get_attribute("data-type").as_deref() == Some("check");
                let items = el.children();
                let mut n = 0;
                for j in 0..items.length() {
                    let Some(li) = items.item(j) else { continue };
                    n += 1;
                    let r = inline(&li);
                    out.push(if check {
                        Block::Check { runs: r, checked: li.get_attribute("data-checked").as_deref() == Some("true") }
                    } else if tag == "OL" {
                        Block::Numbered { runs: r, n }
                    } else {
                        Block::Bullet { runs: r }
                    });
                }
            }
            "TABLE" => {
                let t: &web_sys::HtmlTableElement = c.unchecked_ref();
                let rows = t.rows();
                let mut data = Vec::new();
                for r in 0..rows.length() {
                    let Some(row) = rows.item(r) else { continue };
                    let row: web_sys::HtmlTableRowElement = row.unchecked_into();
                    let cells = row.cells();
                    data.push((0..cells.length()).filter_map(|i| cells.item(i)).map(|c| c.text_content().unwrap_or_default().trim().to_string()).collect());
                }
                out.push(Block::Table { rows: data });
            }
            _ => {
                // Paragraph-like containers; an image alone in a paragraph is an image.
                let has_img = el.query_selector("img").ok().flatten();
                if let (Some(img), true) = (has_img, el.text_content().unwrap_or_default().trim().is_empty()) {
                    let img: web_sys::HtmlImageElement = img.unchecked_into();
                    out.push(Block::Image { src: img.src(), w: img.natural_width().max(1) as f32, h: img.natural_height().max(1) as f32 });
                } else if el.query_selector("h1,h2,h3,ul,ol,table,blockquote,p,div").ok().flatten().is_some() {
                    walk(&c, out);
                } else {
                    let r = inline(&c);
                    if !r.is_empty() {
                        out.push(Block::Paragraph { runs: r });
                    }
                }
            }
        }
    }
    flush(&mut pending, out);
}

pub fn blocks_from_dom(el: &web_sys::HtmlDivElement) -> Vec<Block> {
    let mut out = Vec::new();
    walk(el.unchecked_ref(), &mut out);
    out
}
