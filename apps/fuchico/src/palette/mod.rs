//! Command Palette (Mod-Shift-P), Quick Switcher (Mod-P) and the Keyboard
//! Shortcuts list (spec 4 §1–§6).
//!
//! * The overlays are entities owned by the main window's [`Workspace`]
//!   ([`PaletteHost`]) and painted over it by [`render_overlays`]; each
//!   emits [`OverlayEvent`]s that [`open`] wires to the workspace.
//! * The registry ([`commands`]) and the ranking are pure; `run.rs` executes
//!   a chosen [`CommandAction`]. Editor-targeted commands (`Editor: …`,
//!   save, go to line) focus the active pane and dispatch the GPUI actions
//!   from `crate::actions` (`Editor*`, `Save`, `GoToLine`), which the editor
//!   pane handles with `.on_action`.
//! * [`ShortcutsList`] is the self-contained "Keyboard Shortcuts" panel for
//!   the settings view.

mod command_palette;
pub mod commands;
pub mod files;
mod overlay;
mod quick_switcher;
mod run;
mod shortcuts;
#[cfg(test)]
mod ui_tests;

use std::sync::Arc;

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, FocusHandle, Focusable, Subscription, Window};

pub use command_palette::CommandPalette;
pub use commands::{CommandAction, CommandContext};
pub use quick_switcher::QuickSwitcher;
pub use shortcuts::ShortcutsList;

use crate::workspace::Workspace;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteKind {
    Commands { query: String },
    QuickOpen,
}

/// What an overlay asks its host to do. Every variant closes the overlay.
#[derive(Clone, Debug, PartialEq)]
pub enum OverlayEvent {
    Dismiss,
    Run(CommandAction),
    OpenFile(String),
}

/// The workspace's overlay state.
#[derive(Default)]
pub struct PaletteHost {
    commands: Option<(Entity<CommandPalette>, Subscription)>,
    switcher: Option<(Entity<QuickSwitcher>, Subscription)>,
    /// The vault's Markdown files (template discovery), refreshed on every
    /// palette open, and the root they belong to.
    md_files: Option<(String, Arc<Vec<String>>)>,
    /// Focus before the first overlay opened, restored on dismiss.
    restore: Option<FocusHandle>,
}

impl PaletteHost {
    pub fn is_open(&self) -> bool {
        self.commands.is_some() || self.switcher.is_some()
    }

    pub fn command_palette(&self) -> Option<&Entity<CommandPalette>> {
        self.commands.as_ref().map(|(e, _)| e)
    }

    pub fn quick_switcher(&self) -> Option<&Entity<QuickSwitcher>> {
        self.switcher.as_ref().map(|(e, _)| e)
    }
}

/// The registry's view of the workspace.
fn context(ws: &Workspace) -> CommandContext {
    let root = ws.root().map(str::to_string);
    let md_files = match (&root, &ws.palette.md_files) {
        (Some(r), Some((cached, files))) if r == cached => files.clone(),
        _ => Arc::default(),
    };
    CommandContext {
        root,
        active_path: ws.tabs().active.clone(),
        has_open_tabs: !ws.tabs().is_empty(),
        backlinks_open: ws.backlinks_open(),
        md_files,
    }
}

/// Opens (or refocuses) an overlay. Both can be open at once, as in the
/// original; the palette paints on top.
pub fn open(
    ws: &mut Workspace,
    kind: PaletteKind,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if !ws.palette.is_open() {
        ws.palette.restore = window.focused(cx);
    }
    match kind {
        PaletteKind::Commands { query } => {
            if let Some((p, _)) = &ws.palette.commands {
                p.update(cx, |p, cx| p.focus(window, cx));
                return;
            }
            let ctx = context(ws);
            let view = cx.new(|cx| CommandPalette::new(query, ctx, window, cx));
            let sub = cx.subscribe_in(&view, window, |ws, _, ev: &OverlayEvent, window, cx| {
                ws.palette.commands = None;
                on_event(ws, ev.clone(), window, cx);
            });
            ws.palette.commands = Some((view, sub));
            refresh_md_files(ws, cx);
        }
        PaletteKind::QuickOpen => {
            if let Some((s, _)) = &ws.palette.switcher {
                s.update(cx, |s, cx| s.focus(window, cx));
                return;
            }
            let root = ws.root().map(str::to_string);
            let view = cx.new(|cx| QuickSwitcher::new(root, window, cx));
            let sub = cx.subscribe_in(&view, window, |ws, _, ev: &OverlayEvent, window, cx| {
                ws.palette.switcher = None;
                on_event(ws, ev.clone(), window, cx);
            });
            ws.palette.switcher = Some((view, sub));
        }
    }
    cx.notify();
}

fn on_event(
    ws: &mut Workspace,
    ev: OverlayEvent,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    cx.notify();
    // Hand focus back to the overlay still open, else to where it was.
    if let Some((p, _)) = &ws.palette.commands {
        p.update(cx, |p, cx| p.focus(window, cx));
    } else if let Some((s, _)) = &ws.palette.switcher {
        s.update(cx, |s, cx| s.focus(window, cx));
    } else {
        match ws.palette.restore.take() {
            Some(h) => window.focus(&h, cx),
            None => window.focus(&ws.focus_handle(cx), cx),
        }
    }
    match ev {
        OverlayEvent::Dismiss => {}
        OverlayEvent::OpenFile(path) => ws.open_file(path, None, window, cx),
        OverlayEvent::Run(action) => run::run(ws, action, window, cx),
    }
}

/// Re-lists the vault's Markdown files off the UI thread and feeds the
/// open palette (the template commands depend on them).
fn refresh_md_files(ws: &mut Workspace, cx: &mut Context<Workspace>) {
    let Some(root) = ws.root().map(str::to_string) else {
        return;
    };
    let r = root.clone();
    let task =
        cx.background_spawn(async move { fuchico_core::fs::tree::fs_list_markdown_files(r) });
    cx.spawn(async move |ws, cx| {
        let Ok(files) = task.await else { return };
        ws.update(cx, |ws, cx| {
            if ws.root() != Some(root.as_str()) {
                return;
            }
            ws.palette.md_files = Some((root, Arc::new(files)));
            let ctx = context(ws);
            if let Some((p, _)) = &ws.palette.commands {
                p.update(cx, |p, cx| p.set_context(ctx, cx));
            }
        })
        .ok();
    })
    .detach();
}

/// The open overlays, to append to the workspace root (which must be
/// `relative`). The palette comes last so it paints on top.
pub fn render_overlays(ws: &Workspace) -> Vec<AnyElement> {
    let mut out = Vec::new();
    if let Some((s, _)) = &ws.palette.switcher {
        out.push(s.clone().into_any_element());
    }
    if let Some((p, _)) = &ws.palette.commands {
        out.push(p.clone().into_any_element());
    }
    out
}
