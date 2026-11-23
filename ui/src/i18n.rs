//! Interface translations: the 24 official EU languages, other European
//! languages, and Chinese. Files are `key = value` lines in `ui/locales/`;
//! anything missing falls back to English.

use leptos::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

/// (code, name in its own language)
pub const LANGS: &[(&str, &str)] = &[
    ("en", "English"),
    ("fr", "Français"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("it", "Italiano"),
    ("pt", "Português"),
    ("nl", "Nederlands"),
    ("pl", "Polski"),
    ("ro", "Română"),
    ("cs", "Čeština"),
    ("sk", "Slovenčina"),
    ("sl", "Slovenščina"),
    ("hr", "Hrvatski"),
    ("hu", "Magyar"),
    ("bg", "Български"),
    ("el", "Ελληνικά"),
    ("sv", "Svenska"),
    ("da", "Dansk"),
    ("fi", "Suomi"),
    ("et", "Eesti"),
    ("lv", "Latviešu"),
    ("lt", "Lietuvių"),
    ("mt", "Malti"),
    ("ga", "Gaeilge"),
    ("nb", "Norsk bokmål"),
    ("is", "Íslenska"),
    ("ca", "Català"),
    ("uk", "Українська"),
    ("ru", "Русский"),
    ("sr", "Srpski"),
    ("sq", "Shqip"),
    ("mk", "Македонски"),
    ("zh", "中文（简体）"),
];

macro_rules! locales {
    ($($code:literal),*) => {
        &[$(($code, include_str!(concat!("../locales/", $code, ".txt")))),*]
    };
}

const FILES: &[(&str, &str)] = locales!(
    "en", "fr", "de", "es", "it", "pt", "nl", "pl", "ro", "cs", "sk", "sl", "hr", "hu", "bg", "el", "sv", "da",
    "fi", "et", "lv", "lt", "mt", "ga", "nb", "is", "ca", "uk", "ru", "sr", "sq", "mk", "zh"
);

fn parse(src: &'static str) -> HashMap<&'static str, &'static str> {
    src.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect()
}

thread_local! {
    static TABLES: RefCell<HashMap<&'static str, HashMap<&'static str, &'static str>>> = RefCell::new(HashMap::new());
    static LANG: RefCell<Option<RwSignal<String>>> = const { RefCell::new(None) };
}

pub fn init(initial: &str) -> RwSignal<String> {
    let s = RwSignal::new(initial.to_string());
    LANG.with(|l| *l.borrow_mut() = Some(s));
    s
}

pub fn lang() -> RwSignal<String> {
    LANG.with(|l| l.borrow().expect("i18n::init not called"))
}

fn lookup(code: &str, key: &str) -> Option<&'static str> {
    TABLES.with(|t| {
        let mut t = t.borrow_mut();
        if !t.contains_key(code) {
            let file = FILES.iter().find(|(c, _)| *c == code)?.1;
            let static_code: &'static str = FILES.iter().find(|(c, _)| *c == code)?.0;
            t.insert(static_code, parse(file));
        }
        t.get(code)?.get(key).copied()
    })
}

/// Translate `key` in the current language. Reactive when called inside a
/// closure in a view.
pub fn t(key: &str) -> String {
    let code = lang().get();
    lookup(&code, key).or_else(|| lookup("en", key)).unwrap_or(key).to_string()
}

/// Translate and fill `{0}`, `{1}`… placeholders.
pub fn tf(key: &str, args: &[&str]) -> String {
    let mut s = t(key);
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), a);
    }
    s
}

/// Best language for this device from the browser's preferences.
pub fn detect() -> String {
    let nav = web_sys::window().unwrap().navigator();
    let mut prefs: Vec<String> = nav.languages().iter().filter_map(|v| v.as_string()).collect();
    if let Some(l) = nav.language() {
        prefs.push(l);
    }
    for p in prefs {
        let p = p.to_lowercase();
        let base = p.split('-').next().unwrap_or("");
        let base = match base {
            "no" | "nn" => "nb",
            "hr" | "bs" | "sh" => if base == "hr" { "hr" } else { "sr" },
            b => b,
        };
        if LANGS.iter().any(|(c, _)| *c == base) {
            return base.to_string();
        }
    }
    "en".into()
}

/// BCP-47 tag for date formatting.
pub fn locale_tag() -> String {
    match lang().get_untracked().as_str() {
        "zh" => "zh-CN".into(),
        "nb" => "nb-NO".into(),
        c => c.to_string(),
    }
}
