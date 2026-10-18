//! The Quick Switcher overlay (spec 4 §5, `QuickSwitcher.tsx`).
//!
//! The vault is walked off the UI thread on every open (like the original,
//! whose cache dies with the unmounted component), so new files always show.

use gpui_kit::component::IconName;
use gpui_kit::component::input::{Enter, Escape, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, EventEmitter, FocusHandle, Focusable, ScrollHandle, Subscription, Window, div};

use super::OverlayEvent;
use super::files::{FileRow, rank_files, walk_vault};
use super::overlay;
use crate::settings::SettingsStore;

/// `useVaultFiles` state.
#[derive(Clone, Debug, PartialEq)]
pub enum FilesState {
    Loading,
    Loaded(Vec<String>),
    Error(String),
}

pub struct QuickSwitcher {
    input: Entity<InputState>,
    root: Option<String>,
    files: FilesState,
    results: Vec<FileRow>,
    active: usize,
    scroll: ScrollHandle,
    _subs: Vec<Subscription>,
}

impl EventEmitter<OverlayEvent> for QuickSwitcher {}

impl Focusable for QuickSwitcher {
    fn focus_handle(&self, cx: &gpui_kit::App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }
}

impl QuickSwitcher {
    pub fn new(root: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(crate::t!(PaletteGoToFile)));
        input.update(cx, |s, cx| s.focus(window, cx));
        let subs = vec![
            cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.recompute(cx);
                }
            }),
            cx.observe_global_in::<SettingsStore>(window, |this, window, cx| {
                this.input.update(cx, |s, cx| s.set_placeholder(crate::t!(PaletteGoToFile), window, cx));
                cx.notify();
            }),
        ];
        if let Some(root) = root.clone() {
            let walk = cx.background_spawn(async move { walk_vault(&root) });
            cx.spawn(async move |this, cx| {
                let res = walk.await;
                this.update(cx, |this, cx| {
                    this.files = match res {
                        Ok(files) => FilesState::Loaded(files),
                        Err(e) => FilesState::Error(e),
                    };
                    this.recompute(cx);
                })
                .ok();
            })
            .detach();
        }
        QuickSwitcher {
            input,
            root,
            files: FilesState::Loading,
            results: vec![],
            active: 0,
            scroll: ScrollHandle::new(),
            _subs: subs,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |s, cx| s.focus(window, cx));
    }

    fn recompute(&mut self, cx: &mut Context<Self>) {
        let results = match (&self.root, &self.files) {
            (Some(root), FilesState::Loaded(files)) => rank_files(root, files, &self.input.read(cx).value()),
            _ => vec![],
        };
        if results != self.results {
            self.results = results;
            self.active = 0;
            self.scroll.scroll_to_item(0);
        }
        cx.notify();
    }

    fn move_active(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.results.is_empty() {
            return;
        }
        let max = self.results.len() as isize - 1;
        self.active = (self.active as isize + delta).clamp(0, max) as usize;
        self.scroll.scroll_to_item(self.active);
        cx.notify();
    }

    /// Enter / click: open and close. Nothing happens without a result.
    fn open(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(r) = self.results.get(ix) {
            cx.emit(OverlayEvent::OpenFile(r.path.clone()));
        }
    }

    fn status(&self) -> Option<String> {
        if self.root.is_none() {
            return Some(crate::t!(PaletteOpenFolderFirst));
        }
        match &self.files {
            FilesState::Loading => Some(crate::t!(PaletteScanningVault)),
            FilesState::Error(e) => Some(crate::t!(PaletteCantReadVault, message = e)),
            FilesState::Loaded(_) if self.results.is_empty() => Some(crate::t!(PaletteNoFilesFound)),
            FilesState::Loaded(_) => None,
        }
    }

    #[cfg(test)]
    pub fn results(&self) -> &[FileRow] {
        &self.results
    }
}

impl Render for QuickSwitcher {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<gpui_kit::AnyElement> = match self.status() {
            Some(msg) => vec![overlay::empty(msg, cx)],
            None => self
                .results
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    overlay::item(("switcher-file", i), IconName::FileText, &r.rel, &r.indices, i == self.active, cx)
                        .debug_selector(move || format!("switcher-row-{i}"))
                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                            if this.active != i {
                                this.active = i;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| this.open(i, cx)))
                        .into_any_element()
                })
                .collect(),
        };
        let backdrop = cx.listener(|_, _, _, cx| cx.emit(OverlayEvent::Dismiss));
        div()
            .absolute()
            .inset_0()
            .key_context("QuickSwitcher")
            .capture_action(cx.listener(|_, _: &Escape, _, cx| {
                cx.stop_propagation();
                cx.emit(OverlayEvent::Dismiss);
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                cx.stop_propagation();
                this.move_active(1, cx);
            }))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                cx.stop_propagation();
                this.move_active(-1, cx);
            }))
            .capture_action(cx.listener(|this, _: &Enter, _, cx| {
                cx.stop_propagation();
                this.open(this.active, cx);
            }))
            .child(overlay::frame("quick-switcher", &self.input, rows, &self.scroll, backdrop, cx))
    }
}
