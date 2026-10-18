//! Executes a [`CommandAction`] against the workspace (the `run` closures of
//! `buildAppCommands`). The overlay is already closed and focus restored.

use gpui_kit::{Action, AppContext, Context, Window};

use super::commands::CommandAction;
use super::{PaletteKind, open};
use crate::actions::{GoToLine, Save, ToggleTheme, editor_action_for};
use crate::settings::{PaletteId, SettingsStore};
use crate::workspace::Workspace;

pub fn run(ws: &mut Workspace, action: CommandAction, window: &mut Window, cx: &mut Context<Workspace>) {
    use CommandAction as A;
    match action {
        A::SetView(v) => ws.set_view(v, window, cx),
        A::EditorAction(id) => {
            if let Some(a) = editor_action_for(id) {
                to_editor(ws, a, window, cx);
            }
        }
        A::GoToLine(line) => to_editor(ws, GoToLine { line }.boxed_clone(), window, cx),
        A::Save => to_editor(ws, Save.boxed_clone(), window, cx),
        A::SaveAndClose => {
            let active = ws.tabs().active.clone();
            to_editor(ws, Save.boxed_clone(), window, cx);
            // After the (deferred) save dispatch.
            cx.defer_in(window, move |ws, window, cx| {
                if let Some(p) = active {
                    ws.close_tab(&p, window, cx);
                }
            });
        }
        A::CloseActiveTab => {
            if let Some(p) = ws.tabs().active.clone() {
                ws.close_tab(&p, window, cx);
            }
        }
        A::CloseAllTabs => ws.close_all_tabs(window, cx),
        A::OpenSettings(section) => ws.open_settings(section, window, cx),
        A::OpenFolder => ws.open_folder(window, cx),
        A::QuickOpen => open(ws, PaletteKind::QuickOpen, window, cx),
        A::OpenDailyNote => {
            let Some(root) = ws.root().map(str::to_string) else { return };
            let s = SettingsStore::get(cx);
            let (daily, templates) = (s.daily_notes_folder.clone(), s.templates_folder.clone());
            create_and_open(window, cx, move || {
                let now = chrono::Local::now().naive_local();
                fuchico_core::templates::open_daily_note(&root, &daily, &templates, &now, &*crate::bus::sink())
            });
        }
        A::NewFromTemplate(template) => {
            let Some(root) = ws.root().map(str::to_string) else { return };
            create_and_open(window, cx, move || {
                let now = chrono::Local::now().naive_local();
                fuchico_core::templates::create_from_template(&root, &template, &now, &*crate::bus::sink())
            });
        }
        A::OpenInNewWindow => {
            if let Some(p) = ws.tabs().active.clone() {
                crate::windows::open_editor_window(p, ws.root().map(str::to_string), cx);
            }
        }
        A::ToggleBacklinks => ws.toggle_backlinks(window, cx),
        A::ExportHtml(path) => crate::export::export_html(&path, window, cx),
        A::ExportPdf(path) => crate::export::export_pdf(&path, window, cx),
        A::ToggleTheme => cx.dispatch_action(&ToggleTheme),
        A::SelectPalette(p) => SettingsStore::update(cx, |s| s.palette = p),
        A::SelectCustomTheme(id) => SettingsStore::update(cx, |s| {
            s.palette = PaletteId::Custom;
            s.custom_theme_id = Some(id);
        }),
    }
}

/// Focuses the active pane, then dispatches `action` from there, so the
/// editor's `.on_action` handlers see it. Without a pane it is a no-op
/// (the original's `editorRef.current?.…`).
fn to_editor(ws: &mut Workspace, action: Box<dyn Action>, window: &mut Window, cx: &mut Context<Workspace>) {
    if ws.focus_active_pane(window, cx) {
        window.dispatch_action(action, cx);
    }
}

/// Runs a note-creating core call off the UI thread, then opens the note.
fn create_and_open(
    window: &mut Window,
    cx: &mut Context<Workspace>,
    f: impl FnOnce() -> Result<String, String> + Send + 'static,
) {
    let task = cx.background_spawn(async move { f() });
    cx.spawn_in(window, async move |ws, cx| {
        let res = task.await;
        ws.update_in(cx, |ws, window, cx| match res {
            Ok(path) => ws.open_file(path, None, window, cx),
            Err(e) => {
                log::error!("create note: {e}");
                crate::ui::toast::toast_error(window, cx, e);
            }
        })
        .ok();
    })
    .detach();
}
