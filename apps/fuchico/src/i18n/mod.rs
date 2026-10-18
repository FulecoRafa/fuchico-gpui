//! UI string catalogs (port of `src/lib/i18n.ts`).
//!
//! * [`Msg`] is the closed set of message keys; [`catalog::en`] and
//!   [`catalog::pt_br`] are exhaustive matches, so a missing translation is a
//!   compile error.
//! * The active locale is process-global (like the original module-level
//!   `t()`), set from the `language` setting by [`set_language`]. Every window
//!   re-renders when settings change, so they all pick it up.
//! * [`t!`](crate::t) formats a message with `{name}` parameters; [`tn`] is
//!   the `{n}` plural helper.

pub mod catalog;

use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};

pub use catalog::Msg;
use serde::{Deserialize, Serialize};

/// The `language` setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en")]
    En,
    #[serde(rename = "pt-BR")]
    PtBr,
}

/// A concrete catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    En,
    PtBr,
}

impl Locale {
    pub const ALL: [Locale; 2] = [Locale::En, Locale::PtBr];

    /// BCP-47 tag as the original used it.
    pub fn tag(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::PtBr => "pt-BR",
        }
    }
}

/// `"system"` → pt-BR when the OS locale starts with `pt` (case-insensitive).
pub fn resolve(language: Language, os_locale: Option<&str>) -> Locale {
    match language {
        Language::En => Locale::En,
        Language::PtBr => Locale::PtBr,
        Language::System => match os_locale {
            Some(l) if l.to_ascii_lowercase().starts_with("pt") => Locale::PtBr,
            _ => Locale::En,
        },
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Applies the `language` setting (reads the OS locale for `System`).
pub fn set_language(language: Language) {
    let os = sys_locale::get_locale();
    set_locale(resolve(language, os.as_deref()));
}

pub fn set_locale(locale: Locale) {
    CURRENT.store(locale as u8, Ordering::Relaxed);
}

pub fn locale() -> Locale {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Locale::PtBr,
        _ => Locale::En,
    }
}

/// Raw catalog lookup. Both catalogs are complete by construction, so the
/// "fall back to en" rule of the original only matters for empty strings.
pub fn raw(locale: Locale, msg: Msg) -> &'static str {
    let s = match locale {
        Locale::En => catalog::en(msg),
        Locale::PtBr => catalog::pt_br(msg),
    };
    if s.is_empty() { catalog::en(msg) } else { s }
}

/// Replaces every `{name}` with its param. Unknown placeholders are kept.
pub fn interpolate(template: &str, params: &[(&str, &dyn Display)]) -> String {
    if params.is_empty() {
        return template.to_string();
    }
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                match params.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(&v.to_string()),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// `t(key, params)` in the given locale.
pub fn translate(locale: Locale, msg: Msg, params: &[(&str, &dyn Display)]) -> String {
    interpolate(raw(locale, msg), params)
}

/// `t(key, params)` in the current locale. Prefer the [`t!`](crate::t) macro.
pub fn tr(msg: Msg, params: &[(&str, &dyn Display)]) -> String {
    translate(locale(), msg, params)
}

/// `tn(n, one, many)`: picks `one` when `n == 1` and fills `{n}`.
pub fn tn(n: i64, one: Msg, many: Msg) -> String {
    tr(if n == 1 { one } else { many }, &[("n", &n)])
}

/// Translate a message: `t!(AppFiles)`, `t!(SearchLine, n = 7)`.
/// The first argument is a [`Msg`] variant name.
#[macro_export]
macro_rules! t {
    ($msg:ident) => {
        $crate::i18n::tr($crate::i18n::Msg::$msg, &[])
    };
    ($msg:ident, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::i18n::tr(
            $crate::i18n::Msg::$msg,
            &[$((stringify!($name), &$val as &dyn ::std::fmt::Display)),+],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashSet};

    fn placeholders(s: &str) -> BTreeMap<String, usize> {
        let re = regex::Regex::new(r"\{(\w+)\}").unwrap();
        let mut m = BTreeMap::new();
        for c in re.captures_iter(s) {
            *m.entry(c[1].to_string()).or_insert(0) += 1;
        }
        m
    }

    #[test]
    fn key_sets_are_equal_and_unique() {
        let keys: HashSet<_> = Msg::ALL.iter().map(|m| m.key()).collect();
        assert_eq!(keys.len(), Msg::ALL.len());
        assert_eq!(Msg::ALL.len(), 302);
    }

    #[test]
    fn no_message_is_blank() {
        for &m in Msg::ALL {
            for l in Locale::ALL {
                assert!(!raw(l, m).trim().is_empty(), "{} blank in {}", m.key(), l.tag());
            }
        }
    }

    #[test]
    fn pt_br_keeps_placeholders() {
        for &m in Msg::ALL {
            assert_eq!(
                placeholders(catalog::en(m)),
                placeholders(catalog::pt_br(m)),
                "placeholder mismatch for {}",
                m.key()
            );
        }
    }

    #[test]
    fn interpolates_params() {
        assert_eq!(translate(Locale::En, Msg::SearchLine, &[("n", &7)]), "line 7");
        assert_eq!(
            translate(Locale::En, Msg::ExplorerConfirmDelete, &[("name", &"\"a.md\"")]),
            "Delete \"a.md\"?"
        );
        assert_eq!(interpolate("{a}-{b}-{a}", &[("a", &1)]), "1-{b}-1");
    }

    #[test]
    fn plural_helper() {
        set_locale(Locale::En);
        assert_eq!(tn(1, Msg::CountFileOne, Msg::CountFileMany), "1 file");
        assert_eq!(tn(3, Msg::CountFileOne, Msg::CountFileMany), "3 files");
        assert_eq!(t!(SearchLine, n = 7), "line 7");
    }

    #[test]
    fn system_language_resolution() {
        assert_eq!(resolve(Language::System, Some("pt-PT")), Locale::PtBr);
        assert_eq!(resolve(Language::System, Some("PT_br")), Locale::PtBr);
        assert_eq!(resolve(Language::System, Some("en-US")), Locale::En);
        assert_eq!(resolve(Language::System, None), Locale::En);
        assert_eq!(resolve(Language::PtBr, Some("en")), Locale::PtBr);
    }
}
