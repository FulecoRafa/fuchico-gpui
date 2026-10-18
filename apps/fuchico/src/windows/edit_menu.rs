//! The Edit menu (Undo, Redo, Cut, Copy, Paste, Select All), as in the
//! original's default Tauri macOS menu. Each item dispatches a `Menu*`
//! action. A global handler forwards it to whatever has focus in the active
//! window: the note editor, a text input, or (for views that read raw keys,
//! like the drawing canvas) the equivalent keystroke.

use gpui_kit::component::input;
use gpui_kit::{Action, App, KeyBinding, Keystroke, OsAction};

use crate::actions::{MenuCopy, MenuCut, MenuPaste, MenuRedo, MenuSelectAll, MenuUndo};

/// Key context no element uses: the bindings below exist only so the menu
/// shows the usual shortcuts; the real keys go to the focused element's
/// own bindings first.
const HINT_CONTEXT: &str = "FuchicoMenuHint";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditCommand {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

impl EditCommand {
    pub const ALL: [EditCommand; 6] = [
        Self::Undo,
        Self::Redo,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::SelectAll,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::SelectAll => "Select All",
        }
    }

    pub fn os_action(self) -> OsAction {
        match self {
            Self::Undo => OsAction::Undo,
            Self::Redo => OsAction::Redo,
            Self::Cut => OsAction::Cut,
            Self::Copy => OsAction::Copy,
            Self::Paste => OsAction::Paste,
            Self::SelectAll => OsAction::SelectAll,
        }
    }

    /// `secondary-*` keystroke (Cmd on macOS, Ctrl elsewhere).
    pub fn keys(self) -> &'static str {
        match self {
            Self::Undo => "secondary-z",
            Self::Redo => "secondary-shift-z",
            Self::Cut => "secondary-x",
            Self::Copy => "secondary-c",
            Self::Paste => "secondary-v",
            Self::SelectAll => "secondary-a",
        }
    }

    /// The focused element's own actions, most specific first: the note
    /// editor (`fuchico_editor`), then gpui-component text inputs.
    fn targets(self) -> [Box<dyn Action>; 2] {
        use fuchico_editor as ed;
        match self {
            Self::Undo => [Box::new(ed::Undo), Box::new(input::Undo)],
            Self::Redo => [Box::new(ed::Redo), Box::new(input::Redo)],
            Self::Cut => [Box::new(ed::Cut), Box::new(input::Cut)],
            Self::Copy => [Box::new(ed::Copy), Box::new(input::Copy)],
            Self::Paste => [Box::new(ed::Paste), Box::new(input::Paste)],
            Self::SelectAll => [Box::new(ed::SelectAll), Box::new(input::SelectAll)],
        }
    }

    fn keystroke(self) -> Keystroke {
        let k = self.keys().replace(
            "secondary",
            if cfg!(target_os = "macos") {
                "cmd"
            } else {
                "ctrl"
            },
        );
        Keystroke::parse(&k).expect("valid keystroke")
    }
}

/// Runs `cmd` on the active window's focused element. Deferred: the menu
/// action itself is dispatched inside that window's update, which leases it.
pub fn forward(cmd: EditCommand, cx: &mut App) {
    cx.defer(move |cx| forward_now(cmd, cx));
}

fn forward_now(cmd: EditCommand, cx: &mut App) {
    let Some(h) = cx.active_window() else { return };
    h.update(cx, |_, window, cx| {
        match cmd
            .targets()
            .into_iter()
            .find(|a| window.is_action_available(a.as_ref(), cx))
        {
            Some(a) => window.dispatch_action(a, cx),
            None => {
                window.dispatch_keystroke(cmd.keystroke(), cx);
            }
        }
    })
    .ok();
}

pub fn register(cx: &mut App) {
    cx.on_action(|_: &MenuUndo, cx| forward(EditCommand::Undo, cx));
    cx.on_action(|_: &MenuRedo, cx| forward(EditCommand::Redo, cx));
    cx.on_action(|_: &MenuCut, cx| forward(EditCommand::Cut, cx));
    cx.on_action(|_: &MenuCopy, cx| forward(EditCommand::Copy, cx));
    cx.on_action(|_: &MenuPaste, cx| forward(EditCommand::Paste, cx));
    cx.on_action(|_: &MenuSelectAll, cx| forward(EditCommand::SelectAll, cx));
    use EditCommand::*;
    let c = Some(HINT_CONTEXT);
    cx.bind_keys([
        KeyBinding::new(Undo.keys(), MenuUndo, c),
        KeyBinding::new(Redo.keys(), MenuRedo, c),
        KeyBinding::new(Cut.keys(), MenuCut, c),
        KeyBinding::new(Copy.keys(), MenuCopy, c),
        KeyBinding::new(Paste.keys(), MenuPaste, c),
        KeyBinding::new(SelectAll.keys(), MenuSelectAll, c),
    ]);
}

/// The Edit submenu.
pub fn menu() -> gpui_kit::Menu {
    use EditCommand::*;
    use gpui_kit::MenuItem;
    gpui_kit::Menu::new("Edit").items([
        MenuItem::os_action(Undo.label(), MenuUndo, Undo.os_action()),
        MenuItem::os_action(Redo.label(), MenuRedo, Redo.os_action()),
        MenuItem::separator(),
        MenuItem::os_action(Cut.label(), MenuCut, Cut.os_action()),
        MenuItem::os_action(Copy.label(), MenuCopy, Copy.os_action()),
        MenuItem::os_action(Paste.label(), MenuPaste, Paste.os_action()),
        MenuItem::os_action(SelectAll.label(), MenuSelectAll, SelectAll.os_action()),
    ])
}
