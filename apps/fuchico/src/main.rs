//! Fuchico — native GPUI port of the Tauri + React notes app.
//!
//! Startup order: single-instance check (a second launch forwards its file
//! arguments and exits), synchronous reads of the persisted state, then the
//! GPUI app: globals, fonts, theme, key bindings, global actions, menus and
//! the main window. OS/CLI file paths go through
//! [`windows::OPEN_REQUESTS`] and are routed by the main workspace.

// Foundation APIs (keymap tables, i18n helpers, bus events, export) are
// consumed by modules still being ported.
#![allow(dead_code)]

mod actions;
mod agenda;
mod assets;
mod backlinks;
mod bus;
mod editor;
mod excalidraw;
mod explorer;
mod export;
mod i18n;
mod keymap;
mod mermaid;
mod palette;
mod persist;
mod search;
mod settings;
mod settings_view;
mod tags;
mod theme;
mod ui;
mod windows;
mod workspace;

use fuchico_core::open_files::paths_from_args;

use settings::SettingsStore;
use settings::ui_state::UiState;
use windows::single_instance::{self, Instance};

fn main() {
    let cwd = std::env::current_dir().ok();
    let argv = paths_from_args(std::env::args().skip(1), cwd.as_deref());
    let listener = match single_instance::acquire(&argv) {
        Instance::Forwarded => return,
        Instance::Primary(l) => l,
    };

    let settings = SettingsStore::load();
    let ui_state = UiState::load();
    let window_states = windows::geometry::WindowStates::load();

    let app = gpui_kit::application().with_assets(assets::Assets);
    app.on_open_urls(|urls| windows::push_open_requests(windows::paths_from_urls(&urls)));
    app.on_reopen(windows::reopen);
    app.run(move |cx| {
        gpui_kit::init(cx);
        assets::register_fonts(cx);
        SettingsStore::init(settings, cx);
        cx.set_global(ui_state);
        cx.set_global(window_states);
        bus::init(cx);
        theme::init(cx);
        keymap::bind_global_keys(cx);
        windows::register_global_actions(cx);
        windows::menu::install(cx);
        if let Some(l) = listener {
            single_instance::serve(l, windows::push_open_requests);
        }
        if !argv.is_empty() {
            windows::push_open_requests(argv);
        }
        windows::open_main_window(cx);
        cx.activate(true);
    });
}
