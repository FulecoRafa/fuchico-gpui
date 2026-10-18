//! Settings search (spec 5 §3.2), as pure functions over visible text.

/// Lowercased whitespace-separated terms of a query (empty = not searching).
pub fn terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// Every term is a substring of `text` (case-insensitive).
pub fn matches(text: &str, terms: &[String]) -> bool {
    let hay = text.to_lowercase();
    terms.iter().all(|t| hay.contains(t.as_str()))
}

/// Which items of a section to show. `fields[i]` is `Some(text)` for a
/// filterable field and `None` for other content (previews, headers).
///
/// * head (title + desc) matches → the whole section;
/// * else each field shows iff it matches; the section shows iff ≥1 field
///   does, or it has no fields and its whole text matches.
///
/// `None` = section hidden. Other content follows the section.
pub fn visibility(terms: &[String], head: &str, fields: &[Option<&str>]) -> Option<Vec<bool>> {
    if terms.is_empty() || matches(head, terms) {
        return Some(vec![true; fields.len()]);
    }
    let shown: Vec<bool> = fields
        .iter()
        .map(|f| f.is_none_or(|t| matches(t, terms)))
        .collect();
    let any_field = fields.iter().zip(&shown).any(|(f, s)| f.is_some() && *s);
    let has_fields = fields.iter().any(Option::is_some);
    if any_field {
        return Some(shown);
    }
    if !has_fields {
        let all: String = std::iter::once(head)
            .chain(fields.iter().flatten().copied())
            .collect::<Vec<_>>()
            .join(" ");
        return matches(&all, terms).then(|| vec![true; fields.len()]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_split_and_lowercase() {
        assert_eq!(terms("  Tab  SIZE "), vec!["tab", "size"]);
        assert!(terms("   ").is_empty());
    }

    #[test]
    fn every_term_must_match() {
        let t = terms("tab size");
        assert!(matches("Tab size", &t));
        assert!(matches("size of the tab", &t));
        assert!(!matches("Tab width", &t));
    }

    #[test]
    fn head_match_shows_everything() {
        let t = terms("theme");
        assert_eq!(
            visibility(&t, "Theme A theme is", &[Some("Palette"), None]),
            Some(vec![true, true])
        );
    }

    #[test]
    fn fields_filter_individually() {
        let t = terms("rulers");
        let v = visibility(
            &t,
            "Editor Behavior Gutter",
            &[Some("Relative line numbers"), Some("Vertical rulers"), None],
        );
        assert_eq!(v, Some(vec![false, true, true]));
        assert_eq!(
            visibility(&terms("zzz"), "Editor Behavior", &[Some("Tab size"), None]),
            None
        );
    }

    #[test]
    fn empty_query_shows_all() {
        assert_eq!(visibility(&[], "x", &[Some("a")]), Some(vec![true]));
    }
}
