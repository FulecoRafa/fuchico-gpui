//! JS `String.prototype.localeCompare` (no locale argument, no options).
//!
//! The original runs in a WebView, where `localeCompare` is ICU collation
//! with the default options: tertiary strength, punctuation not ignorable,
//! no numeric ordering. The UI locales (en, pt-BR) use the CLDR root order,
//! so this is the root collator from `icu_collator`. That gives, at the
//! primary level, spaces < punctuation < symbols < digits < letters, with
//! accents (secondary) and then case (tertiary, lowercase first) only
//! breaking ties.

use std::cmp::Ordering;
use std::sync::OnceLock;

use icu_collator::options::CollatorOptions;
use icu_collator::{CollatorBorrowed, CollatorPreferences};

fn collator() -> &'static CollatorBorrowed<'static> {
    static C: OnceLock<CollatorBorrowed<'static>> = OnceLock::new();
    C.get_or_init(|| {
        CollatorBorrowed::try_new(CollatorPreferences::default(), CollatorOptions::default())
            .expect("compiled root collation data")
    })
}

/// `a.localeCompare(b)` as an [`Ordering`].
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    collator().compare(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut v: Vec<&str>) -> Vec<&str> {
        v.sort_by(|a, b| locale_compare(a, b));
        v
    }

    #[test]
    fn matches_js_locale_compare() {
        // Lowercase sorts before uppercase only as a tiebreak.
        assert_eq!(locale_compare("a", "A"), Ordering::Less);
        assert_eq!(sorted(vec!["B", "a", "A", "b"]), vec!["a", "A", "b", "B"]);
        // Accents are secondary: "éa" < "eb" (a plain lowercase compare
        // puts U+00E9 after every ASCII letter).
        assert_eq!(
            sorted(vec!["eb", "éa", "f", "e"]),
            vec!["e", "éa", "eb", "f"]
        );
        // Space < punctuation < symbols < digits < letters.
        assert_eq!(
            sorted(vec!["a", "9", "~", "_", " "]),
            vec![" ", "_", "~", "9", "a"]
        );
        // No numeric collation.
        assert_eq!(locale_compare("10", "9"), Ordering::Less);
        assert_eq!(locale_compare("same", "same"), Ordering::Equal);
    }
}
