//! Bundled typefaces. The same files are served to the UI, so text is
//! measured and broken identically on screen and in exports.

use naute_core::model::FontFamily;
use naute_core::text::{is_wide, Measure};
use resvg::usvg::{self, fontdb};
use std::sync::{Arc, OnceLock};

macro_rules! font {
    ($f:literal) => {
        include_bytes!(concat!("../../ui/assets/fonts/", $f)) as &'static [u8]
    };
}

const FONTS: &[(FontFamily, bool, bool, &[u8])] = &[
    (FontFamily::Sans, false, false, font!("IBMPlexSans-400.ttf")),
    (FontFamily::Sans, false, true, font!("IBMPlexSans-400i.ttf")),
    (FontFamily::Sans, true, false, font!("IBMPlexSans-600.ttf")),
    (FontFamily::Serif, false, false, font!("Literata-400.ttf")),
    (FontFamily::Serif, false, true, font!("Literata-400i.ttf")),
    (FontFamily::Serif, true, false, font!("Literata-600.ttf")),
    (FontFamily::Mono, false, false, font!("IBMPlexMono-400.ttf")),
];

const EXTRA: &[&[u8]] = &[font!("IBMPlexSans-500.ttf")];

struct Faces(Vec<(FontFamily, bool, bool, ttf_parser::Face<'static>)>);

fn faces() -> &'static Faces {
    static F: OnceLock<Faces> = OnceLock::new();
    F.get_or_init(|| {
        Faces(
            FONTS
                .iter()
                .filter_map(|(f, b, i, data)| ttf_parser::Face::parse(data, 0).ok().map(|face| (*f, *b, *i, face)))
                .collect(),
        )
    })
}

pub struct FontMeasure;

impl Measure for FontMeasure {
    fn width(&self, text: &str, font: FontFamily, size: f32, bold: bool, italic: bool) -> f32 {
        let fs = &faces().0;
        // Best match: exact style, then same family, then anything.
        let face = fs
            .iter()
            .find(|(f, b, i, _)| *f == font && *b == bold && *i == italic)
            .or_else(|| fs.iter().find(|(f, b, _, _)| *f == font && *b == bold))
            .or_else(|| fs.iter().find(|(f, _, _, _)| *f == font))
            .map(|x| &x.3);
        let Some(face) = face else {
            return naute_core::text::ApproxMeasure.width(text, font, size, bold, italic);
        };
        let upm = face.units_per_em() as f32;
        let mut w = 0.0;
        for c in text.chars() {
            let adv = face
                .glyph_index(c)
                .and_then(|g| face.glyph_hor_advance(g))
                .map(|a| a as f32 / upm)
                .unwrap_or(if is_wide(c) { 1.0 } else { 0.55 });
            w += adv;
        }
        w * size
    }
}

pub fn fontdb() -> Arc<fontdb::Database> {
    static DB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        for (_, _, _, data) in FONTS {
            db.load_font_data(data.to_vec());
        }
        for data in EXTRA {
            db.load_font_data(data.to_vec());
        }
        // System fonts cover scripts the bundled faces do not (CJK, …).
        db.load_system_fonts();
        db.set_sans_serif_family("IBM Plex Sans");
        db.set_serif_family("Literata");
        db.set_monospace_family("IBM Plex Mono");
        Arc::new(db)
    })
    .clone()
}

pub fn usvg_options() -> usvg::Options<'static> {
    let mut o = usvg::Options::default();
    o.fontdb = fontdb();
    o.font_family = "IBM Plex Sans".into();
    o
}
