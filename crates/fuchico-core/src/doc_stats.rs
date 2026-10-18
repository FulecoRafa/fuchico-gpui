//! Port of `computeDocStats` from `src/modules/editor/lib/docStats.ts`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocStats {
    pub words: usize,
    pub reading_time_min: usize,
}

const WORDS_PER_MINUTE: f64 = 200.0;

pub fn compute_doc_stats(text: &str) -> DocStats {
    let words = text.split_whitespace().count();
    let reading_time_min = if words == 0 {
        0
    } else {
        ((words as f64 / WORDS_PER_MINUTE).round() as usize).max(1)
    };
    DocStats {
        words,
        reading_time_min,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_words_and_reading_time() {
        assert_eq!(
            compute_doc_stats("  \n "),
            DocStats {
                words: 0,
                reading_time_min: 0
            }
        );
        assert_eq!(
            compute_doc_stats("a b  c\nd"),
            DocStats {
                words: 4,
                reading_time_min: 1
            }
        );
        let long = "w ".repeat(500);
        assert_eq!(compute_doc_stats(&long).reading_time_min, 3); // 2.5 rounds up
    }
}
