//! Vault search and replace (spec 03 "Vault search and replace", original
//! `src/modules/search/SearchPanel.tsx` + `lib/useSearch.ts`).
//!
//! * Plain, case-insensitive substring search (no regex / case / whole-word
//!   options, exactly like the original), debounced 200ms, run off the UI
//!   thread with `fuchico_core::search::search_files` (one match per line,
//!   capped at 500). A newer query cancels the pending one.
//! * Results grouped by file with the first occurrence highlighted; clicking
//!   opens the file at that line ([`ViewEvent::OpenFile`] → the pane's
//!   `reveal_line`).
//! * Replace all (vault or one file) via `search_replace_files` after a
//!   confirm dialog; the core emits `fs:file-written` per changed file.
//! * Panel state is component-local in the original and lost when another
//!   main view is shown: [`MainView::on_hide`] resets it.

use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use fuchico_core::search::{ReplaceResult, SearchMatch, group_by_file, search_files, search_replace_files};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, EventEmitter, FontWeight, HighlightStyle, SharedString, StyledText, Subscription,
    Task, WeakEntity, Window, div,
};

use crate::bus;
use crate::i18n::{Msg, tn};
use crate::theme::{AppTheme, alpha};
use crate::ui::{ConfirmOptions, MenuEntry, build_menu, confirm, empty_state, file_row_menu, u, with_context_menu};
use crate::workspace::tabs::basename;
use crate::workspace::view::{MainView, ViewEvent};

/// `DEBOUNCE_MS`.
pub const DEBOUNCE: Duration = Duration::from_millis(200);
/// Longest line prefix laid out per result row (the row is ellipsized anyway).
const ROW_TEXT_LIMIT: usize = 1000;

type Groups = Rc<Vec<(String, Vec<SearchMatch>)>>;

/// `useSearch` state.
#[derive(Clone)]
pub enum SearchState {
    Idle,
    Loading,
    /// Grouped results and the total match count.
    Loaded(Groups, usize),
    Error(String),
}

pub struct SearchView {
    root: Option<String>,
    query_input: Entity<InputState>,
    replace_input: Entity<InputState>,
    query: String,
    replacement: String,
    replace_open: bool,
    state: SearchState,
    notice: Option<String>,
    busy: bool,
    /// The pending debounced search; replacing it cancels the old one.
    search_task: Option<Task<()>>,
    placeholder_locale: crate::i18n::Locale,
    _subs: Vec<Subscription>,
}

impl SearchView {
    pub fn new(root: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query_input = cx.new(|cx| InputState::new(window, cx).placeholder(crate::t!(SearchPlaceholder)));
        let replace_input = cx.new(|cx| InputState::new(window, cx).placeholder(crate::t!(SearchReplaceWith)));
        let subs = vec![
            cx.subscribe_in(&query_input, window, |this, input, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Change) {
                    let q = input.read(cx).value().to_string();
                    if q != this.query {
                        this.query = q;
                        this.notice = None;
                        this.run(cx);
                    }
                }
            }),
            cx.subscribe_in(&replace_input, window, |this, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::Change => {
                    this.replacement = input.read(cx).value().to_string();
                    cx.notify();
                }
                InputEvent::PressEnter { .. } if this.can_replace() => this.run_replace(None, window, cx),
                _ => {}
            }),
        ];
        SearchView {
            root,
            query_input,
            replace_input,
            query: String::new(),
            replacement: String::new(),
            replace_open: false,
            state: SearchState::Idle,
            notice: None,
            busy: false,
            search_task: None,
            placeholder_locale: crate::i18n::locale(),
            _subs: subs,
        }
    }

    pub fn state(&self) -> &SearchState {
        &self.state
    }

    /// Types `q` into the query box as the user would (snapshots).
    #[cfg(test)]
    pub fn set_query(&mut self, q: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.query_input.update(cx, |s, cx| s.set_value(q.to_string(), window, cx));
        self.query = q.to_string();
        self.run(cx);
    }

    /// Focus the query input (the panel autofocuses it on mount).
    pub fn focus_input(&self, window: &mut Window, cx: &mut App) {
        self.query_input.update(cx, |s, cx| s.focus(window, cx));
    }

    /// The `useSearch` effect: idle for an empty query or no vault,
    /// otherwise "loading" now and `search_files` after the debounce.
    fn run(&mut self, cx: &mut Context<Self>) {
        let (Some(root), false) = (self.root.clone(), self.query.trim().is_empty()) else {
            self.search_task = None;
            self.state = SearchState::Idle;
            cx.notify();
            return;
        };
        self.state = SearchState::Loading;
        let query = self.query.clone();
        self.search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            let res = cx.background_spawn(async move { search_files(root, query) }).await;
            this.update(cx, |this, cx| {
                this.state = match res {
                    Ok(matches) => {
                        let n = matches.len();
                        SearchState::Loaded(Rc::new(group_by_file(&matches)), n)
                    }
                    Err(e) => SearchState::Error(e),
                };
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn match_count(&self) -> usize {
        match &self.state {
            SearchState::Loaded(_, n) => *n,
            _ => 0,
        }
    }

    fn group_count(&self) -> usize {
        match &self.state {
            SearchState::Loaded(g, _) => g.len(),
            _ => 0,
        }
    }

    /// `canReplace`.
    fn can_replace(&self) -> bool {
        !self.query.trim().is_empty() && self.match_count() > 0 && !self.busy
    }

    /// `runReplace(files?)`: confirm, then replace off the UI thread.
    fn run_replace(&mut self, files: Option<Vec<String>>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else { return };
        if !self.can_replace() {
            return;
        }
        let scope = match &files {
            Some(f) => crate::t!(SearchScopeInFile, file = basename(f.first().map(String::as_str).unwrap_or(""))),
            None => crate::t!(
                SearchScopeAcrossFiles,
                files = tn(self.group_count() as i64, Msg::CountFileOne, Msg::CountFileMany)
            ),
        };
        let (query, replacement) = (self.query.clone(), self.replacement.clone());
        let msg = crate::t!(SearchConfirmReplace, query = query, replacement = replacement, scope = scope);
        let this = cx.entity().downgrade();
        confirm(window, cx, ConfirmOptions::new(msg).danger(), move |_, cx| {
            let (root, query, replacement, files) = (root.clone(), query.clone(), replacement.clone(), files.clone());
            this.update(cx, |this, cx| this.replace(root, query, replacement, files, cx)).ok();
        });
    }

    fn replace(&mut self, root: String, query: String, replacement: String, files: Option<Vec<String>>, cx: &mut Context<Self>) {
        self.busy = true;
        cx.notify();
        let task = cx.background_spawn(async move {
            let sink = bus::sink();
            search_replace_files(root, query, replacement, files, &*sink)
        });
        cx.spawn(async move |this, cx| {
            let res = task.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                this.notice = Some(replace_notice(&res));
                // `setRefreshToken(t => t + 1)`: re-run the same query.
                this.run(cx);
            })
            .ok();
        })
        .detach();
    }

    fn open(&self, file: &str, line: usize, cx: &mut Context<Self>) {
        cx.emit(ViewEvent::OpenFile { path: file.to_string(), line: Some(line as u32) });
    }

    /// `fileRowMenuItems(file, { onOpen, extra: [replace item] })`.
    fn row_menu(&self, file: &str, line: usize, cx: &mut Context<Self>) -> Vec<MenuEntry> {
        let this = cx.entity().downgrade();
        let open = {
            let (this, f) = (this.clone(), file.to_string());
            move |_: &mut Window, cx: &mut App| {
                this.update(cx, |this, cx| this.open(&f, line, cx)).ok();
            }
        };
        let extra = if self.replace_open {
            let f = file.to_string();
            MenuEntry::item(crate::t!(SearchReplaceAllInFile, file = basename(file)), move |window, cx| {
                this.update(cx, |this, cx| this.run_replace(Some(vec![f.clone()]), window, cx)).ok();
            })
            .disabled(!self.can_replace())
        } else {
            MenuEntry::item(crate::t!(SearchReplaceInThisFile), move |_, cx| {
                this.update(cx, |this, cx| {
                    this.replace_open = true;
                    cx.notify();
                })
                .ok();
            })
        };
        file_row_menu(file, open, vec![extra.icon(IconName::Replace)])
    }

    fn render_input_row(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let th = AppTheme::get(cx);
        let active = self.replace_open;
        let (muted, fg, hover_bg) = (th.colors.muted_foreground, th.colors.foreground, th.fg_alpha(0.08));
        let title: SharedString = crate::t!(SearchFindReplaceTitle).into();
        let toggle = div()
            .id("search-replace-toggle")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(24.))
            .rounded(u(th.radius.sm))
            .cursor_pointer()
            .text_color(if active { fg } else { muted })
            .when(active, |d| d.bg(hover_bg))
            .hover(move |s| s.bg(hover_bg).text_color(fg))
            .child(Icon::new(IconName::Replace).size(u(14.)))
            .tooltip(move |w, cx| Tooltip::new(title.clone()).build(w, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                this.replace_open = !this.replace_open;
                cx.notify();
            }))
            .debug_selector(|| "search-replace-toggle".into());
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(8.))
            .px(u(16.))
            .py(u(10.))
            .border_b_1()
            .border_color(th.colors.border)
            .child(Icon::new(IconName::Search).size(u(14.)).text_color(muted).flex_none())
            .child(div().flex_1().min_w_0().child(bare_input(&self.query_input, "search-input")))
            .child(toggle)
    }

    fn render_replace_row(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let th = AppTheme::get(cx);
        let can = self.can_replace();
        let n = self.match_count();
        let label = replace_label(n);
        let hover = th.fg_alpha(0.12);
        let button = div()
            .id("search-replace-btn")
            .flex_none()
            .px(u(10.))
            .py(u(3.))
            .border_1()
            .border_color(th.colors.border)
            .rounded(u(th.radius.sm))
            .bg(th.fg_alpha(0.05))
            .text_color(th.colors.foreground)
            .text_size(u(12.))
            .whitespace_nowrap()
            .child(label)
            .when(!can, |d| d.opacity(0.5))
            .when(can, |d| {
                d.cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(|this, _, window, cx| this.run_replace(None, window, cx)))
            })
            .debug_selector(|| "search-replace-btn".into());
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(8.))
            .pt(u(6.))
            .pb(u(6.))
            .pr(u(16.))
            .pl(u(38.))
            .border_b_1()
            .border_color(th.colors.border)
            .child(div().flex_1().min_w_0().child(bare_input(&self.replace_input, "search-replace-input")))
            .child(button)
    }

    fn render_results(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let th = AppTheme::get(cx).clone();
        let status =
            |text: String, color| div().px(u(4.)).py(u(8.)).text_size(u(12.)).text_color(color).child(text).into_any_element();
        let mut children: Vec<AnyElement> = Vec::new();
        if let Some(n) = &self.notice {
            children.push(
                div()
                    .mt(u(4.))
                    .mx(u(4.))
                    .mb(u(8.))
                    .px(u(8.))
                    .py(u(6.))
                    .rounded(u(th.radius.sm))
                    .bg(alpha(th.colors.syntax_function, 0.18))
                    .text_size(u(12.))
                    .text_color(th.colors.foreground)
                    .child(n.clone())
                    .into_any_element(),
            );
        }
        match &self.state {
            SearchState::Loading => children.push(status(crate::t!(SearchSearching), th.colors.muted_foreground)),
            SearchState::Error(e) => children.push(status(e.clone(), th.colors.destructive)),
            SearchState::Idle if self.query.trim().is_empty() => {
                children.push(status(crate::t!(SearchTypeToSearch), th.colors.muted_foreground))
            }
            SearchState::Loaded(_, 0) => children.push(status(crate::t!(SearchNoMatches), th.colors.muted_foreground)),
            SearchState::Loaded(groups, _) => {
                let view = cx.entity().downgrade();
                for (gi, (file, matches)) in groups.iter().enumerate() {
                    children.push(render_group(&view, gi, file, matches, &self.query, &th));
                }
            }
            SearchState::Idle => {}
        }
        div()
            .id("search-results")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(u(12.))
            .py(u(8.))
            .children(children)
    }
}

fn render_group(
    view: &WeakEntity<SearchView>,
    gi: usize,
    file: &str,
    matches: &[SearchMatch],
    query: &str,
    th: &AppTheme,
) -> AnyElement {
    let hover = th.fg_alpha(0.05);
    let radius = u(th.radius.sm);
    let first_line = matches.first().map(|m| m.line).unwrap_or(1);
    let tip: SharedString = file.to_string().into();
    let (f1, f2, v1, v2) = (file.to_string(), file.to_string(), view.clone(), view.clone());
    let header = div()
        .id(("search-file", gi))
        .flex()
        .items_center()
        .gap(u(8.))
        .w_full()
        .px(u(8.))
        .py(u(4.))
        .rounded(radius)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .text_size(u(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(th.colors.foreground)
        .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(basename(file).to_string()))
        .child(
            div()
                .ml_auto()
                .flex_none()
                .font_weight(FontWeight::NORMAL)
                .text_size(u(11.))
                .text_color(th.colors.muted_foreground)
                .child(matches.len().to_string()),
        )
        .tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx))
        .on_click(move |_, _, cx| {
            v1.update(cx, |this, cx| this.open(&f1, first_line, cx)).ok();
        })
        .debug_selector(|| format!("search-file:{gi}"));
    let header = with_context_menu(("search-file-menu", gi), header, move |menu, _, cx| {
        let items = v2.update(cx, |this, cx| this.row_menu(&f2, first_line, cx)).unwrap_or_default();
        build_menu(menu, items)
    });
    let highlight_bg = alpha(th.colors.syntax_function, 0.35);
    let rows = matches.iter().enumerate().map(|(ri, m)| {
        let (text, range) = row_text(&m.text, query);
        let styled = match range {
            Some(r) => StyledText::new(text)
                .with_highlights(vec![(r, HighlightStyle { background_color: Some(highlight_bg), ..Default::default() })]),
            None => StyledText::new(text),
        };
        let (line, f1, f2, v1, v2) = (m.line, m.file.clone(), m.file.clone(), view.clone(), view.clone());
        let row = div()
            .id(SharedString::from(format!("search-row-{gi}-{ri}")))
            .flex()
            .flex_col()
            .gap(u(2.))
            .w_full()
            .pl(u(16.))
            .pr(u(8.))
            .py(u(6.))
            .rounded(radius)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .text_size(u(12.))
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_family(th.mono_font.clone())
                    .text_color(th.colors.foreground)
                    .child(styled),
            )
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(u(11.))
                    .text_color(th.colors.muted_foreground)
                    .child(crate::t!(SearchLine, n = line)),
            )
            .on_click(move |_, _, cx| {
                v1.update(cx, |this, cx| this.open(&f1, line, cx)).ok();
            })
            .debug_selector(|| format!("search-row-{gi}-{ri}"));
        with_context_menu(SharedString::from(format!("search-row-menu-{gi}-{ri}")), row, move |menu, _, cx| {
            let items = v2.update(cx, |this, cx| this.row_menu(&f2, line, cx)).unwrap_or_default();
            build_menu(menu, items)
        })
    });
    div().mb(u(6.)).child(header).children(rows).into_any_element()
}

/// A borderless 13px input (`.search-input`).
fn bare_input(state: &Entity<InputState>, id: &'static str) -> Input {
    Input::new(state).id(id).appearance(false).px(u(0.)).py(u(0.)).h(u(20.)).text_size(u(13.))
}

/// "Replace all" / "Replace all (N)".
fn replace_label(n: usize) -> String {
    if n > 0 { format!("{} ({n})", crate::t!(SearchReplaceAll)) } else { crate::t!(SearchReplaceAll) }
}

/// The `search.replaced` / `search.replaceFailed` notice.
fn replace_notice(res: &Result<ReplaceResult, String>) -> String {
    match res {
        Ok(r) => crate::t!(
            SearchReplaced,
            occurrences = tn(r.replacements as i64, Msg::CountOccurrenceOne, Msg::CountOccurrenceMany),
            files = tn(r.files_changed as i64, Msg::CountFileOne, Msg::CountFileMany)
        ),
        Err(e) => crate::t!(SearchReplaceFailed, error = e),
    }
}

/// `highlight(text, query)`: the byte range of the FIRST case-insensitive
/// occurrence of `query` in `text`. Matching is per char (lowercased), so a
/// lowercase form of a different byte length (`İ`) still yields a range on
/// char boundaries.
pub fn highlight_range(text: &str, query: &str) -> Option<Range<usize>> {
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return None;
    }
    'starts: for (start, _) in text.char_indices() {
        let mut ni = 0;
        for (off, c) in text[start..].char_indices() {
            for l in c.to_lowercase() {
                if ni == needle.len() || l != needle[ni] {
                    continue 'starts;
                }
                ni += 1;
            }
            if ni == needle.len() {
                return Some(start..start + off + c.len_utf8());
            }
        }
        // Ran out of text mid-match: no later start can fit either.
        return None;
    }
    None
}

/// The row's displayed line (capped at [`ROW_TEXT_LIMIT`] chars) and the
/// highlight inside it.
fn row_text(text: &str, query: &str) -> (SharedString, Option<Range<usize>>) {
    let cut = text.char_indices().nth(ROW_TEXT_LIMIT).map(|(i, _)| i).unwrap_or(text.len());
    let range = highlight_range(text, query).filter(|r| r.end <= cut);
    (text[..cut].to_string().into(), range)
}

impl EventEmitter<ViewEvent> for SearchView {}

impl MainView for SearchView {
    fn set_root(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        if self.root != root {
            self.root = root;
            self.run(cx);
        }
    }

    /// Unmount in the original: query, replacement, toggle and notice reset.
    fn on_hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query_input.update(cx, |s, cx| s.set_value("", window, cx));
        self.replace_input.update(cx, |s, cx| s.set_value("", window, cx));
        self.query.clear();
        self.replacement.clear();
        self.replace_open = false;
        self.notice = None;
        self.run(cx);
    }
}

impl Render for SearchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.root.is_none() {
            return empty_state(crate::t!(SearchOpenFolderToSearch), cx).into_any_element();
        }
        // Follow language changes.
        let locale = crate::i18n::locale();
        if self.placeholder_locale != locale {
            self.placeholder_locale = locale;
            for (input, msg) in [(&self.query_input, Msg::SearchPlaceholder), (&self.replace_input, Msg::SearchReplaceWith)] {
                input.update(cx, |s, cx| s.set_placeholder(crate::i18n::tr(msg, &[]), window, cx));
            }
        }
        let th = AppTheme::get(cx);
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .text_color(th.colors.foreground)
            .child(self.render_input_row(cx))
            .when(self.replace_open, |d| d.child(self.render_replace_row(cx)))
            .child(self.render_results(cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_first_case_insensitive_occurrence() {
        assert_eq!(highlight_range("Hello hello", "HELLO"), Some(0..5));
        assert_eq!(highlight_range("say Hello hello", "hello"), Some(4..9));
        assert_eq!(highlight_range("abc", "x"), None);
        assert_eq!(highlight_range("abc", ""), None);
        assert_eq!(highlight_range("ab", "abc"), None);
        assert_eq!(highlight_range("aab", "ab"), Some(1..3));
    }

    #[test]
    fn highlight_is_multibyte_safe() {
        let t = "çà Ünïcode";
        let r = highlight_range(t, "ünï").unwrap();
        assert_eq!(&t[r], "Ünï");
        let t = "İstanbul istanbul";
        let r = highlight_range(t, "istanbul").unwrap();
        assert!(t.is_char_boundary(r.start) && t.is_char_boundary(r.end));
    }

    #[test]
    fn query_whitespace_is_kept() {
        assert_eq!(highlight_range("a  b a b", "a b"), Some(5..8));
    }

    #[test]
    fn row_text_caps_long_lines() {
        let long = "x".repeat(2000) + "needle";
        let (shown, range) = row_text(&long, "needle");
        assert_eq!(shown.len(), ROW_TEXT_LIMIT);
        assert_eq!(range, None);
        let (_, range) = row_text("a needle", "NEEDLE");
        assert_eq!(range, Some(2..8));
    }

    #[test]
    fn labels_and_notices() {
        use crate::i18n::{Locale, translate};
        // Locale is global; compare against the active catalog directly.
        let _ = translate(Locale::En, Msg::SearchReplaceAll, &[]);
        assert!(replace_label(3).ends_with(" (3)"));
        assert!(!replace_label(0).contains('('));
        let ok = Ok(ReplaceResult { files_changed: 1, replacements: 3 });
        let s = replace_notice(&ok);
        assert!(s.contains('3') && s.contains('1'));
        assert!(replace_notice(&Err("boom".into())).contains("boom"));
    }
}

#[cfg(test)]
mod ui_tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui_kit::component::WindowExt;
    use gpui_kit::TestAppContext;

    use super::{SearchState, SearchView};
    use crate::test_ui;
    use crate::workspace::view::ViewEvent;

    fn vault() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(root.join("a.md"), "intro\nHello World\nfoo hello\n").unwrap();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/b.md"), "say HELLO again\n").unwrap();
        std::fs::write(root.join("c.md"), "nothing here\n").unwrap();
        (dir, root.to_string_lossy().into_owned())
    }

    fn loaded(v: &SearchView) -> Option<(usize, usize)> {
        match v.state() {
            SearchState::Loaded(g, n) => Some((g.len(), *n)),
            _ => None,
        }
    }

    #[gpui_kit::test]
    async fn search_types_debounces_opens_and_replaces(cx: &mut TestAppContext) {
        cx.update(test_ui::init);
        let (_dir, root) = vault();
        let (mut ui, view) = test_ui::open(cx, (900., 640.), |window, cx| SearchView::new(Some(root.clone()), window, cx));
        let events: Rc<RefCell<Vec<ViewEvent>>> = Rc::default();
        let ev = events.clone();
        cx.update(|cx| cx.subscribe(&view, move |_, e: &ViewEvent, _| ev.borrow_mut().push(e.clone())).detach());

        // Idle placeholder, then typing goes Loading immediately.
        ui.frame();
        cx.update(|cx| assert!(matches!(view.read(cx).state(), SearchState::Idle)));
        let input = cx.update(|cx| view.read(cx).query_input.clone());
        ui.cx.update(|window, cx| input.update(cx, |s, cx| s.focus(window, cx)));
        ui.cx.simulate_input("hel");
        cx.update(|cx| {
            assert_eq!(view.read(cx).query_input.read(cx).value().as_ref(), "hel");
            assert_eq!(view.read(cx).query, "hel");
            assert!(matches!(view.read(cx).state(), SearchState::Loading));
        });
        ui.type_text("lo");
        let v = view.clone();
        ui.wait("results", move |cx| loaded(v.read(cx)).is_some()).await;
        // 3 matching lines in 2 files, grouped in walk order.
        cx.update(|cx| assert_eq!(loaded(view.read(cx)), Some((2, 3))));
        assert!(ui.has("search-file:0"));
        assert!(ui.has("search-row-0-0"));
        // Row click opens at that line.
        ui.click("search-row-0-0");
        {
            let evs = events.borrow();
            let ViewEvent::OpenFile { path, line } = evs.last().expect("open event") else { panic!() };
            assert!(path.ends_with(".md"));
            assert!(line.is_some());
        }

        // Whitespace-only query is idle.
        ui.cx.update(|window, cx| input.update(cx, |s, cx| s.focus(window, cx)));
        ui.press("cmd-a");
        ui.type_text("   ");
        cx.update(|cx| assert!(matches!(view.read(cx).state(), SearchState::Idle)));
        ui.press("cmd-a");
        ui.type_text("HeLLo");
        let v = view.clone();
        ui.wait("results again", move |cx| loaded(v.read(cx)) == Some((2, 3))).await;

        // Toggle replace row, type a replacement, confirm.
        assert!(!ui.has("search-replace-btn"));
        ui.click("search-replace-toggle");
        cx.update(|cx| assert!(view.read(cx).replace_open));
        assert!(ui.has("search-replace-btn"));
        let rinput = cx.update(|cx| view.read(cx).replace_input.clone());
        ui.cx.update(|window, cx| rinput.update(cx, |s, cx| s.focus(window, cx)));
        ui.type_text("Bye");
        cx.update(|cx| assert_eq!(view.read(cx).replacement, "Bye"));
        ui.click("search-replace-btn");
        assert!(ui.cx.update(|window, cx| window.has_active_dialog(cx)), "confirm dialog shown");
        ui.press("enter");
        let v = view.clone();
        ui.wait("replace notice", move |cx| v.read(cx).notice.is_some()).await;
        let v = view.clone();
        ui.wait("rerun", move |cx| loaded(v.read(cx)) == Some((0, 0))).await;
        let notice = cx.update(|cx| view.read(cx).notice.clone().unwrap());
        assert_eq!(notice, "Replaced 3 occurrences in 2 files.");
        let a = std::fs::read_to_string(format!("{root}/a.md")).unwrap();
        assert_eq!(a, "intro\nBye World\nfoo Bye\n");
        let b = std::fs::read_to_string(format!("{root}/sub/b.md")).unwrap();
        assert_eq!(b, "say Bye again\n");

        // Leaving the view resets component-local state.
        ui.cx.update(|window, cx| {
            view.update(cx, |v, cx| crate::workspace::view::MainView::on_hide(v, window, cx));
            let v = view.read(cx);
            assert!(v.query.is_empty() && v.notice.is_none() && !v.replace_open);
            assert!(matches!(v.state(), SearchState::Idle));
        });
    }
}
