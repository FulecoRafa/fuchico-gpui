//! Editor status bar (spec "Status bar"): 22px, 11px muted,
//! `{n} word(s) · {m} min read`. Hard-coded English in the original.

use fuchico_core::doc_stats::DocStats;
use gpui_kit::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};

use crate::theme::AppTheme;
use crate::ui::u;

/// `"{n} word" | "{n} words" · "{m} min read"`.
pub fn stats_text(stats: &DocStats) -> (String, String) {
    let words = if stats.words == 1 {
        "1 word".to_string()
    } else {
        format!("{} words", stats.words)
    };
    let read = if stats.words == 0 {
        "0 min read".to_string()
    } else {
        format!("{} min read", stats.reading_time_min)
    };
    (words, read)
}

#[derive(IntoElement)]
pub struct StatusBar {
    pub stats: DocStats,
}

impl RenderOnce for StatusBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let th = AppTheme::get(cx);
        let (words, read) = stats_text(&self.stats);
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(6.))
            .h(u(22.))
            .px(u(10.))
            .border_t_1()
            .border_color(th.colors.border)
            .bg(th.panel_bg())
            .text_size(u(11.))
            .text_color(th.colors.muted_foreground)
            .child(words)
            .child(div().opacity(0.6).child("·"))
            .child(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pluralizes_and_zero_reads() {
        let s = |words, reading_time_min| {
            stats_text(&DocStats {
                words,
                reading_time_min,
            })
        };
        assert_eq!(s(0, 0), ("0 words".into(), "0 min read".into()));
        assert_eq!(s(1, 1), ("1 word".into(), "1 min read".into()));
        assert_eq!(s(450, 2), ("450 words".into(), "2 min read".into()));
    }
}
