//! The Naute document model. Everything a user creates is a [`Document`]:
//! a note, a sheet, a free-size design or a slide deck. Documents are plain
//! serde structures so the same JSON travels between the UI (WASM), the
//! native backend and the `.naute` files people share.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const FILE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum DocKind {
    Note,
    Sheet,
    Design,
    Slides,
}

impl DocKind {
    pub const ALL: [DocKind; 4] = [DocKind::Note, DocKind::Sheet, DocKind::Design, DocKind::Slides];

    pub fn key(self) -> &'static str {
        match self {
            DocKind::Note => "note",
            DocKind::Sheet => "sheet",
            DocKind::Design => "design",
            DocKind::Slides => "slides",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DocMeta {
    pub id: String,
    pub title: String,
    pub kind: DocKind,
    /// Unix milliseconds.
    pub created: f64,
    pub updated: f64,
    #[serde(default)]
    pub pinned: bool,
    /// Short plain-text excerpt for lists (notes, sheets).
    #[serde(default)]
    pub excerpt: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Document {
    #[serde(default = "file_version")]
    pub version: u32,
    pub meta: DocMeta,
    pub body: Body,
}

fn file_version() -> u32 {
    FILE_VERSION
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Body {
    Note(NoteBody),
    Sheet(SheetBody),
    Canvas(CanvasBody),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct NoteBody {
    /// Sanitised HTML produced by the editor.
    pub html: String,
}

// ---------------------------------------------------------------- sheets

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SheetBody {
    pub rows: u32,
    pub cols: u32,
    /// Keyed by A1 reference ("B12").
    #[serde(default)]
    pub cells: BTreeMap<String, Cell>,
    /// Column widths in px, keyed by zero-based column index.
    #[serde(default)]
    pub col_widths: BTreeMap<u32, f32>,
}

impl Default for SheetBody {
    fn default() -> Self {
        SheetBody { rows: 40, cols: 12, cells: BTreeMap::new(), col_widths: BTreeMap::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Cell {
    /// What the user typed: a value or a formula starting with '='.
    pub input: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

// ---------------------------------------------------------------- canvas

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    Px,
    Mm,
    Cm,
    In,
}

impl Unit {
    /// Size of one unit in CSS pixels (96 dpi).
    pub fn px(self) -> f32 {
        match self {
            Unit::Px => 1.0,
            Unit::Mm => 96.0 / 25.4,
            Unit::Cm => 96.0 / 2.54,
            Unit::In => 96.0,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Unit::Px => "px",
            Unit::Mm => "mm",
            Unit::Cm => "cm",
            Unit::In => "in",
        }
    }
    pub const ALL: [Unit; 4] = [Unit::Px, Unit::Mm, Unit::Cm, Unit::In];
}

/// A canvas is a stack of same-size pages. Designs usually have one page,
/// slide decks many. Coordinates are always in CSS pixels; `unit` only
/// changes how sizes are displayed and how PDFs are sized.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CanvasBody {
    pub width: f32,
    pub height: f32,
    pub unit: Unit,
    pub pages: Vec<Page>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Page {
    pub id: String,
    pub background: String,
    pub elements: Vec<Element>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Element {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub rotation: f32,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default)]
    pub locked: bool,
    pub kind: ElementKind,
}

fn one() -> f32 {
    1.0
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    pub fn key(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum FontFamily {
    #[default]
    Sans,
    Serif,
    Mono,
}

impl FontFamily {
    pub fn css(self) -> &'static str {
        match self {
            FontFamily::Sans => "'IBM Plex Sans', sans-serif",
            FontFamily::Serif => "Literata, serif",
            FontFamily::Mono => "'IBM Plex Mono', monospace",
        }
    }
    pub const ALL: [FontFamily; 3] = [FontFamily::Sans, FontFamily::Serif, FontFamily::Mono];
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub font: FontFamily,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: String,
    pub align: Align,
    pub line_height: f32,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            font: FontFamily::Sans,
            size: 32.0,
            bold: false,
            italic: false,
            color: "#26241F".into(),
            align: Align::Left,
            line_height: 1.3,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ElementKind {
    Text {
        text: String,
        style: TextStyle,
    },
    Rect {
        fill: String,
        stroke: String,
        stroke_width: f32,
        radius: f32,
    },
    Ellipse {
        fill: String,
        stroke: String,
        stroke_width: f32,
    },
    Line {
        stroke: String,
        stroke_width: f32,
    },
    Image {
        /// data: URL, so documents stay self-contained when shared.
        src: String,
    },
    Table {
        cells: Vec<Vec<String>>,
        header: bool,
        style: TextStyle,
        stroke: String,
        header_fill: String,
    },
}

impl ElementKind {
    pub fn key(&self) -> &'static str {
        match self {
            ElementKind::Text { .. } => "text",
            ElementKind::Rect { .. } => "rect",
            ElementKind::Ellipse { .. } => "ellipse",
            ElementKind::Line { .. } => "line",
            ElementKind::Image { .. } => "image",
            ElementKind::Table { .. } => "table",
        }
    }
}

// ---------------------------------------------------------------- presets

pub struct SizePreset {
    pub key: &'static str,
    pub width: f32,
    pub height: f32,
    pub unit: Unit,
}

/// Values are in the preset's own unit.
pub const DESIGN_PRESETS: &[SizePreset] = &[
    SizePreset { key: "a4", width: 210.0, height: 297.0, unit: Unit::Mm },
    SizePreset { key: "a5", width: 148.0, height: 210.0, unit: Unit::Mm },
    SizePreset { key: "a3", width: 297.0, height: 420.0, unit: Unit::Mm },
    SizePreset { key: "letter", width: 8.5, height: 11.0, unit: Unit::In },
    SizePreset { key: "square", width: 1080.0, height: 1080.0, unit: Unit::Px },
    SizePreset { key: "portrait_post", width: 1080.0, height: 1350.0, unit: Unit::Px },
    SizePreset { key: "story", width: 1080.0, height: 1920.0, unit: Unit::Px },
    SizePreset { key: "banner", width: 1500.0, height: 500.0, unit: Unit::Px },
    SizePreset { key: "poster", width: 18.0, height: 24.0, unit: Unit::In },
    SizePreset { key: "card", width: 85.0, height: 55.0, unit: Unit::Mm },
    SizePreset { key: "video", width: 1920.0, height: 1080.0, unit: Unit::Px },
];

pub const SLIDE_PRESETS: &[SizePreset] = &[
    SizePreset { key: "wide", width: 1920.0, height: 1080.0, unit: Unit::Px },
    SizePreset { key: "standard", width: 1440.0, height: 1080.0, unit: Unit::Px },
    SizePreset { key: "square", width: 1080.0, height: 1080.0, unit: Unit::Px },
    SizePreset { key: "story", width: 1080.0, height: 1920.0, unit: Unit::Px },
];

// ---------------------------------------------------------------- constructors

pub fn new_id() -> String {
    // Unique enough for local documents: time + a counter + a little entropy
    // derived from the address of a stack value. No RNG dependency needed.
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    let probe = 0u8;
    let addr = &probe as *const u8 as u64;
    let mut x = c.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ addr.rotate_left(17) ^ seed();
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    format!("{:016x}", x)
}

static SEED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Called once at startup with the current time so ids differ between runs.
pub fn seed_ids(now_ms: f64) {
    SEED.store(now_ms as u64, std::sync::atomic::Ordering::Relaxed);
}

fn seed() -> u64 {
    SEED.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn blank_page(background: &str) -> Page {
    Page { id: new_id(), background: background.into(), elements: vec![], notes: String::new() }
}

impl Document {
    pub fn new(kind: DocKind, title: String, now: f64, canvas: Option<(f32, f32, Unit)>) -> Document {
        let body = match kind {
            DocKind::Note => Body::Note(NoteBody::default()),
            DocKind::Sheet => Body::Sheet(SheetBody::default()),
            DocKind::Design | DocKind::Slides => {
                let (w, h, unit) = canvas.unwrap_or(match kind {
                    DocKind::Slides => (1920.0, 1080.0, Unit::Px),
                    _ => (1080.0, 1080.0, Unit::Px),
                });
                Body::Canvas(CanvasBody { width: w, height: h, unit, pages: vec![blank_page("#FFFFFF")] })
            }
        };
        Document {
            version: FILE_VERSION,
            meta: DocMeta {
                id: new_id(),
                title,
                kind,
                created: now,
                updated: now,
                pinned: false,
                excerpt: String::new(),
            },
            body,
        }
    }
}

/// Column index (0-based) to spreadsheet letters: 0 -> A, 26 -> AA.
pub fn col_name(mut c: u32) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (c % 26) as u8);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

pub fn cell_ref(row: u32, col: u32) -> String {
    format!("{}{}", col_name(col), row + 1)
}

/// "B12" -> (row 11, col 1). Accepts `$` anchors.
pub fn parse_ref(s: &str) -> Option<(u32, u32)> {
    let s = s.replace('$', "");
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = s.split_at(split);
    if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut col: u32 = 0;
    for ch in letters.chars() {
        col = col.checked_mul(26)? + (ch.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
    }
    let row: u32 = digits.parse().ok()?;
    if row == 0 {
        return None;
    }
    Some((row - 1, col - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_roundtrip() {
        for (r, c) in [(0, 0), (11, 1), (99, 25), (3, 26), (0, 701), (0, 702)] {
            assert_eq!(parse_ref(&cell_ref(r, c)), Some((r, c)));
        }
        assert_eq!(cell_ref(0, 27), "AB1");
        assert_eq!(parse_ref("$C$3"), Some((2, 2)));
        assert_eq!(parse_ref("A0"), None);
    }

    #[test]
    fn json_roundtrip() {
        let d = Document::new(DocKind::Slides, "Deck".into(), 1.0, None);
        let s = serde_json::to_string(&d).unwrap();
        let back: Document = serde_json::from_str(&s).unwrap();
        assert_eq!(d, back);
    }
}
