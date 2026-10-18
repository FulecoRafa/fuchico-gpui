//! Port of `src/modules/editor/lib/fuzzyMatch.ts`.

#[derive(Debug, Clone, PartialEq)]
pub struct FuzzyResult {
    pub matched: bool,
    pub score: f64,
    /// Indices (in chars of the lowercased target) of matched characters.
    pub indices: Vec<usize>,
}

fn is_non_word(c: char) -> bool {
    // JS `/\W/` is ASCII-only.
    !(c.is_ascii_alphanumeric() || c == '_')
}

/// Subsequence fuzzy match, VSCode-symbol-search style.
pub fn fuzzy_match(query: &str, target: &str) -> FuzzyResult {
    if query.trim().is_empty() {
        return FuzzyResult { matched: true, score: 0.0, indices: vec![] };
    }
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let t: Vec<char> = target.to_lowercase().chars().collect();
    let mut indices = Vec::new();
    let mut qi = 0;
    let mut score = 0.0;
    let mut prev: Option<usize> = None;

    for ti in 0..t.len() {
        if qi >= q.len() {
            break;
        }
        if t[ti] != q[qi] {
            continue;
        }
        indices.push(ti);
        score += if prev.is_some_and(|p| p + 1 == ti) { 3.0 } else { 1.0 };
        if ti == 0 || is_non_word(t[ti - 1]) {
            score += 2.0;
        }
        prev = Some(ti);
        qi += 1;
    }
    if qi < q.len() {
        return FuzzyResult { matched: false, score: 0.0, indices: vec![] };
    }
    FuzzyResult { matched: true, score: score - t.len() as f64 * 0.01, indices }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_word_boundaries_higher() {
        let a = fuzzy_match("gts", "Getting Started");
        let b = fuzzy_match("gts", "Ghost");
        assert!(a.matched);
        assert!(!b.matched || a.score > b.score);
    }

    #[test]
    fn empty_query_matches_everything_and_misses_report_false() {
        assert_eq!(fuzzy_match("  ", "x"), FuzzyResult { matched: true, score: 0.0, indices: vec![] });
        assert!(!fuzzy_match("zz", "abc").matched);
    }

    #[test]
    fn consecutive_run_scores_three_each() {
        let r = fuzzy_match("ab", "ab");
        // 'a': 1 + 2 (start), 'b': 3 consecutive, minus 2 * 0.01
        assert!((r.score - (6.0 - 0.02)).abs() < 1e-9);
        assert_eq!(r.indices, vec![0, 1]);
    }
}
