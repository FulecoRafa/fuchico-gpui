//! Export a note as HTML or PDF (parity §13–§15).
//!
//! * [`export_html`] — `fuchico_core::render_html` with the current theme
//!   variables, a save dialog defaulting to `<note dir>/<stem>.html`, a
//!   write with source `"export"` (emits `fs:file-written`) and a native
//!   outcome dialog (`export.saved` / `export.failed`).
//! * [`export_pdf`] — replaces the original's print window: the PDF is
//!   generated natively ([`pdf`], Typst) with embedded fonts, highlighted
//!   code, a single checkbox per task, tables, images and mermaid diagrams
//!   as vector graphics, then saved through a dialog (`<stem>.pdf`).
//! * [`print_pdf`] — renders the same PDF to a temporary file and hands it
//!   to the OS default PDF viewer, which offers printing.
//!
//! All three use the live editor buffer when the editor registered a
//! [`set_buffer_provider`]; otherwise the file is read off the UI thread.
//! The palette's `:export`, `:pdf` and `:print` aliases only call these.

pub mod pdf;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use fuchico_core::fs::file::{ReadResult, fs_read_file, fs_write_bytes, fs_write_file};
use fuchico_core::render_html::{RenderOptions, ThemeMode, render_html};
use gpui_kit::{App, AppContext, Global, PromptLevel, Task, Window};

use crate::bus;
use crate::t;
use crate::theme::{AppTheme, ColorTokens, hex};

// ----- live buffer --------------------------------------------------------

type Provider = Rc<dyn Fn(&str, &App) -> Option<String>>;

struct BufferProvider(Provider);
impl Global for BufferProvider {}

/// Lets the editor hand out the live (possibly unsaved) buffer of an open
/// note, as the original's `editorRef.getContent()`. Returning `None`
/// falls back to the file on disk.
pub fn set_buffer_provider(cx: &mut App, f: impl Fn(&str, &App) -> Option<String> + 'static) {
    cx.set_global(BufferProvider(Rc::new(f)));
}

pub(crate) fn note_content(path: &str, cx: &App) -> Task<Result<String, String>> {
    if let Some(p) = cx.try_global::<BufferProvider>()
        && let Some(text) = (p.0)(path, cx)
    {
        return Task::ready(Ok(text));
    }
    let p = path.to_string();
    cx.background_spawn(async move {
        match fs_read_file(p)? {
            ReadResult::Text { content, .. } => Ok(content),
            _ => Err("not a text file".to_string()),
        }
    })
}

// ----- pure helpers -------------------------------------------------------

/// `.md` / `.markdown`, case-insensitive (the palette's export condition).
pub fn is_markdown(path: &str) -> bool {
    let l = path.to_ascii_lowercase();
    l.ends_with(".md") || l.ends_with(".markdown")
}

/// Basename with `/\.(md|markdown)$/i` removed.
pub fn stem(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let l = base.to_ascii_lowercase();
    for ext in [".markdown", ".md"] {
        if l.ends_with(ext) {
            return base[..base.len() - ext.len()].to_string();
        }
    }
    base.to_string()
}

/// The save dialog's default file name: `<stem>.<ext>`.
pub fn suggested_name(path: &str, ext: &str) -> String {
    format!("{}.{ext}", stem(path))
}

fn note_dir(path: &str) -> PathBuf {
    Path::new(path)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Adds `ext` when the chosen name has no extension (the original's save
/// dialog filter did the same).
pub fn with_extension(p: PathBuf, ext: &str) -> PathBuf {
    if p.extension().is_some() {
        p
    } else {
        p.with_extension(ext)
    }
}

fn color_vars(th: &AppTheme) -> Vec<(String, String)> {
    ColorTokens::NAMES
        .iter()
        .filter_map(|n| th.colors.get(n).map(|c| (n.to_string(), hex(c))))
        .collect()
}

/// `THEME_VARIABLES` of the current theme (`currentThemeVars()`).
pub fn html_theme_vars(th: &AppTheme) -> Vec<(String, String)> {
    let mut v = color_vars(th);
    v.push(("--radius".into(), format!("{}px", th.radius.base)));
    v.push((
        "--font-sans".into(),
        format!(
            "\"{}\", -apple-system, BlinkMacSystemFont, \"Segoe UI\", sans-serif",
            th.ui_font
        ),
    ));
    v.push((
        "--font-mono".into(),
        format!(
            "\"{}\", ui-monospace, SFMono-Regular, monospace",
            th.mono_font
        ),
    ));
    v
}

pub fn pdf_style(th: &AppTheme, title: &str) -> pdf::PdfStyle {
    let locale = sys_locale::get_locale().unwrap_or_default();
    pdf::PdfStyle {
        title: title.to_string(),
        paper: pdf::PdfStyle::paper_for_locale(&locale),
        colors: color_vars(th),
        font_sans: th.ui_font.to_string(),
        font_mono: th.mono_font.to_string(),
        mermaid: crate::mermaid::theme::mermaid_theme(th),
    }
}

/// The HTML document written by [`export_html`].
pub fn export_html_document(markdown: &str, title: &str, th: &AppTheme) -> String {
    let vars = html_theme_vars(th);
    let mode = if th.appearance.is_dark() {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    render_html(
        markdown,
        &RenderOptions {
            title,
            resolve_href: None,
            body_extra: "",
            theme_vars: &vars,
            mode,
        },
    )
}

// ----- entry points -------------------------------------------------------

/// Native message dialog (`message(..., {kind})` in the original). A saved
/// PDF additionally offers "Open".
fn outcome(window: &mut Window, cx: &mut App, res: &Result<PathBuf, String>, offer_open: bool) {
    match res {
        Ok(path) => {
            let msg = t!(ExportSaved, path = path.display());
            if offer_open {
                let open = t!(CommonOpen);
                let rx = window.prompt(PromptLevel::Info, &msg, None, &["OK", open.as_str()], cx);
                let path = path.clone();
                cx.spawn(async move |cx| {
                    if rx.await == Ok(1) {
                        cx.update(|cx| cx.open_with_system(&path));
                    }
                })
                .detach();
            } else {
                // The answer is irrelevant; the dialog stays up until dismissed.
                drop(window.prompt(PromptLevel::Info, &msg, None, &["OK"], cx));
            }
        }
        Err(e) => {
            let msg = t!(ExportFailed, message = e);
            drop(window.prompt(PromptLevel::Critical, &msg, None, &["OK"], cx));
        }
    }
}

/// Checks the note, starts loading its content and asks for a target path;
/// `write` then runs off the UI thread and the outcome is reported.
fn run_export(
    path: &str,
    ext: &'static str,
    window: &mut Window,
    cx: &mut App,
    write: impl FnOnce(String, PathBuf) -> Result<(), String> + Send + 'static,
    offer_open: bool,
) {
    if !is_markdown(path) {
        crate::ui::toast_info(window, cx, t!(ExportNotMarkdown));
        return;
    }
    let content = note_content(path, cx);
    let dir = note_dir(path);
    let suggested = suggested_name(path, ext);
    let target = cx.prompt_for_new_path(&dir, Some(&suggested));
    window
        .spawn(cx, async move |cx| {
            let target = match target.await {
                Ok(Ok(Some(p))) => with_extension(p, ext),
                Ok(Ok(None)) | Err(_) => return, // cancelled
                Ok(Err(e)) => {
                    let res = Err(e.to_string());
                    let _ = cx.update(|window, cx| outcome(window, cx, &res, false));
                    return;
                }
            };
            let res = match content.await {
                Ok(text) => {
                    let t2 = target.clone();
                    cx.background_spawn(async move { write(text, t2) })
                        .await
                        .map(|_| target)
                }
                Err(e) => Err(e),
            };
            let _ = cx.update(|window, cx| outcome(window, cx, &res, offer_open));
        })
        .detach();
}

pub fn export_html(path: &str, window: &mut Window, cx: &mut App) {
    let th = AppTheme::get(cx).clone();
    let title = stem(path);
    run_export(
        path,
        "html",
        window,
        cx,
        move |text, target| {
            let html = export_html_document(&text, &title, &th);
            fs_write_file(
                target.to_string_lossy().into_owned(),
                html,
                Some("export".into()),
                &*bus::sink(),
            )
        },
        false,
    );
}

pub fn export_pdf(path: &str, window: &mut Window, cx: &mut App) {
    let style = pdf_style(AppTheme::get(cx), &stem(path));
    let dir = note_dir(path);
    // Scan system fonts while the save dialog is up.
    cx.background_spawn(async { pdf::world::prewarm() })
        .detach();
    run_export(
        path,
        "pdf",
        window,
        cx,
        move |text, target| {
            let bytes = pdf::render_pdf(&text, &style, &dir)?;
            fs_write_bytes(
                target.to_string_lossy().into_owned(),
                &bytes,
                Some("export".into()),
                &*bus::sink(),
            )
        },
        true,
    );
}

/// "Print…": renders the PDF to a temporary file and opens it with the
/// system's PDF viewer, whose print dialog does the rest.
pub fn print_pdf(path: &str, window: &mut Window, cx: &mut App) {
    if !is_markdown(path) {
        crate::ui::toast_info(window, cx, t!(ExportNotMarkdown));
        return;
    }
    let style = pdf_style(AppTheme::get(cx), &stem(path));
    let dir = note_dir(path);
    let name = format!("{}.pdf", stem(path));
    let content = note_content(path, cx);
    crate::ui::toast_info(window, cx, t!(ExportExporting));
    window
        .spawn(cx, async move |cx| {
            let res: Result<PathBuf, String> = match content.await {
                Ok(text) => {
                    cx.background_spawn(async move {
                        let bytes = pdf::render_pdf(&text, &style, &dir)?;
                        let out_dir = std::env::temp_dir().join("fuchico-print");
                        std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
                        let out = out_dir.join(name);
                        std::fs::write(&out, bytes).map_err(|e| e.to_string())?;
                        Ok(out)
                    })
                    .await
                }
                Err(e) => Err(e),
            };
            let _ = cx.update(|window, cx| match res {
                Ok(p) => cx.open_with_system(&p),
                Err(e) => {
                    crate::ui::toast::toast_error(window, cx, t!(ExportPrintFailed, message = e))
                }
            });
        })
        .detach();
}

#[cfg(test)]
mod ui_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_and_markdown_detection() {
        assert_eq!(stem("/v/a/Note.md"), "Note");
        assert_eq!(stem("/v/Note.MARKDOWN"), "Note");
        assert_eq!(stem("/v/x.txt"), "x.txt");
        assert!(is_markdown("/v/A.Md"));
        assert!(!is_markdown("/v/a.excalidraw"));
        assert_eq!(note_dir("/v/a/n.md"), PathBuf::from("/v/a"));
        assert_eq!(suggested_name("/v/a/My Note.md", "pdf"), "My Note.pdf");
    }

    #[test]
    fn missing_extension_is_added() {
        assert_eq!(
            with_extension("/x/a".into(), "html"),
            PathBuf::from("/x/a.html")
        );
        assert_eq!(
            with_extension("/x/a.htm".into(), "html"),
            PathBuf::from("/x/a.htm")
        );
    }
}
