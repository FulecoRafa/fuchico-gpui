//! Platform text input (IME / composed characters / dead keys) for the
//! in-canvas text editor, via GPUI's `EntityInputHandler`.
//!
//! Ranges handed to and from the platform are UTF-16 offsets into the
//! text being edited; the editor keeps `char` offsets internally.

use std::ops::Range;

use gpui_kit::*;

use crate::geom;
use crate::view::ExcalidrawView;

fn char_to_u16(s: &str, chars: usize) -> usize {
    s.chars().take(chars).map(char::len_utf16).sum()
}

fn u16_to_char(s: &str, u16: usize) -> usize {
    let mut n = 0;
    let mut acc = 0;
    for c in s.chars() {
        if acc >= u16 {
            break;
        }
        acc += c.len_utf16();
        n += 1;
    }
    n
}

fn range_to_chars(s: &str, r: &Range<usize>) -> Range<usize> {
    u16_to_char(s, r.start)..u16_to_char(s, r.end)
}

impl ExcalidrawView {
    /// Replaces `range` (chars; default: marked text, else the caret) with
    /// `text`, optionally marking the inserted text as composition.
    fn ime_replace(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        mark: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(s) = self.edit_string() else { return };
        let Some(caret) = self.editing.as_ref().map(|e| e.caret) else {
            return;
        };
        let chars: Vec<char> = s.chars().collect();
        let n = chars.len();
        let r = range.or(self.marked.clone()).unwrap_or(caret..caret);
        let (a, b) = (r.start.min(n), r.end.min(n).max(r.start.min(n)));
        let mut out: String = chars[..a].iter().collect();
        out.push_str(text);
        out.extend(chars[b..].iter());
        let len = text.chars().count();
        self.marked = if mark && len > 0 {
            Some(a..a + len)
        } else {
            None
        };
        self.set_edit_string(out, a + len);
        cx.notify();
    }
}

impl EntityInputHandler for ExcalidrawView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let s = self.edit_string()?;
        let r = range_to_chars(&s, &range);
        *adjusted = Some(char_to_u16(&s, r.start)..char_to_u16(&s, r.end));
        Some(s.chars().skip(r.start).take(r.end - r.start).collect())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let s = self.edit_string()?;
        let c = char_to_u16(&s, self.editing.as_ref()?.caret);
        Some(UTF16Selection {
            range: c..c,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let s = self.edit_string()?;
        let m = self.marked.clone()?;
        Some(char_to_u16(&s, m.start)..char_to_u16(&s, m.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let s = self.edit_string().unwrap_or_default();
        let r = range.map(|r| range_to_chars(&s, &r));
        self.ime_replace(r, text, false, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _sel: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let s = self.edit_string().unwrap_or_default();
        let r = range.map(|r| range_to_chars(&s, &r));
        self.ime_replace(r, text, true, cx);
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // The candidate window sits at the caret.
        let (x, y, lh) = self.caret_local()?;
        let el = self.scene.get(&self.editing.as_ref()?.id)?;
        let origin = self.bounds.get().origin;
        let a = self.cam.to_screen(origin, geom::to_world(el, [x, y]));
        let h = px((lh * self.cam.zoom) as f32);
        Some(Bounds::new(a, size(px(2.0), h)))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let s = self.edit_string()?;
        Some(char_to_u16(&s, self.editing.as_ref()?.caret))
    }
}

#[cfg(test)]
mod tests {
    use super::{char_to_u16, range_to_chars, u16_to_char};

    #[test]
    fn utf16_offsets() {
        let s = "aé😀b";
        assert_eq!(char_to_u16(s, 3), 4);
        assert_eq!(u16_to_char(s, 4), 3);
        assert_eq!(range_to_chars(s, &(2..4)), 2..3);
    }
}
