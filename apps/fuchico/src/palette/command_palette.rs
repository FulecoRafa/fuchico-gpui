//! The Command Palette overlay (spec 4 §4, `CommandPalette.tsx`).

use gpui_kit::component::input::{Enter, Escape, InputEvent, InputState, MoveDown, MoveUp, Position};
use gpui_kit::prelude::*;
use gpui_kit::{Context, EventEmitter, FocusHandle, Focusable, ScrollHandle, Subscription, Window, div};

use super::OverlayEvent;
use super::commands::{CommandContext, Ranked, build_commands, rank};
use super::overlay;
use crate::assets::AppIcon;
use crate::keymap::display_binding;
use crate::settings::SettingsStore;

pub struct CommandPalette {
    input: Entity<InputState>,
    ctx: CommandContext,
    results: Vec<Ranked>,
    active: usize,
    scroll: ScrollHandle,
    _subs: Vec<Subscription>,
}

use gpui_kit::Entity;

impl EventEmitter<OverlayEvent> for CommandPalette {}

impl Focusable for CommandPalette {
    fn focus_handle(&self, cx: &gpui_kit::App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }
}

impl CommandPalette {
    pub fn new(query: String, ctx: CommandContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(crate::t!(PaletteRunCommand)));
        let len = query.chars().count() as u32;
        input.update(cx, |s, cx| {
            s.set_value(query, window, cx);
            // Caret after any pre-filled ":" so typing appends.
            s.set_cursor_position(Position { line: 0, character: len }, window, cx);
            s.focus(window, cx);
        });
        let subs = vec![
            cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.recompute(cx);
                }
            }),
            // Settings (shortcuts, custom themes, templates folder, language).
            cx.observe_global_in::<SettingsStore>(window, |this, window, cx| {
                this.input.update(cx, |s, cx| s.set_placeholder(crate::t!(PaletteRunCommand), window, cx));
                this.recompute(cx);
            }),
        ];
        let mut this = CommandPalette { input, ctx, results: vec![], active: 0, scroll: ScrollHandle::new(), _subs: subs };
        this.recompute(cx);
        this
    }

    /// New app state (e.g. the Markdown file list arrived).
    pub fn set_context(&mut self, ctx: CommandContext, cx: &mut Context<Self>) {
        self.ctx = ctx;
        self.recompute(cx);
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |s, cx| s.focus(window, cx));
    }

    /// Re-ranks; the active row resets to 0 whenever the list changes.
    fn recompute(&mut self, cx: &mut Context<Self>) {
        let settings = SettingsStore::get(cx);
        let query = self.input.read(cx).value();
        let results = rank(&build_commands(&self.ctx, &settings), &query);
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

    fn run(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(r) = self.results.get(ix) {
            cx.emit(OverlayEvent::Run(r.command.action.clone()));
        }
    }

    #[cfg(test)]
    pub fn results(&self) -> &[Ranked] {
        &self.results
    }

    #[cfg(test)]
    pub fn active(&self) -> usize {
        self.active
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<gpui_kit::AnyElement> = if self.results.is_empty() {
            vec![overlay::empty(crate::t!(PaletteNoMatchingCommands), cx)]
        } else {
            self.results
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let active = i == self.active;
                    overlay::item(("palette-cmd", i), AppIcon::Command, &r.command.title, &r.indices, active, cx)
                        .when_some(r.alias, |d, a| d.child(overlay::alias_chip(a, cx)))
                        .when_some(r.command.binding.as_ref(), |d, b| {
                            d.child(overlay::keys_chip(display_binding(b), active, r.alias.is_some(), cx))
                        })
                        .debug_selector(move || format!("palette-row-{i}"))
                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                            if this.active != i {
                                this.active = i;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| this.run(i, cx)))
                        .into_any_element()
                })
                .collect()
        };
        let backdrop = cx.listener(|_, _, _, cx| cx.emit(OverlayEvent::Dismiss));
        div()
            .absolute()
            .inset_0()
            .key_context("CommandPalette")
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
                this.run(this.active, cx);
            }))
            .child(overlay::frame("command-palette", &self.input, rows, &self.scroll, backdrop, cx))
    }
}
