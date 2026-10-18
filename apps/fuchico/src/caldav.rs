//! CalDAV in the app: the shared [`CalDavCtx`] and the background sync
//! triggers of the original backend (spec 5 §3.14):
//!
//! * launch-time `sync_all`, then every 15 min (`start_background_sync`);
//! * per linked folder, 2 s after a save (`on_file_written`, fed from the
//!   bus' `fs:file-written`; writes tagged `source: "caldav"` are ignored).
//!
//! Sync results reach views through the bus (`caldav:sync-complete`).

use std::path::PathBuf;

use fuchico_core::caldav::commands::{self, CalDavCtx, DebounceState};
use fuchico_core::events::CoreEvent;
use gpui_kit::{App, Global};

use crate::bus::{self, AppEvent};

struct CaldavGlobal(CalDavCtx);

impl Global for CaldavGlobal {}

/// Where `config.json` lives: `FUCHICO_CONFIG_DIR` when set (so dev and
/// automated runs never touch the real accounts), else the Tauri app data dir.
#[cfg(test)]
pub fn data_dir() -> Option<PathBuf> {
    crate::persist::config_dir()
}

#[cfg(not(test))]
pub fn data_dir() -> Option<PathBuf> {
    match std::env::var_os("FUCHICO_CONFIG_DIR") {
        Some(dir) => Some(PathBuf::from(dir)),
        None => fuchico_core::paths::caldav_data_dir(),
    }
}

/// The shared context, once [`init`] ran.
pub fn ctx(cx: &App) -> Option<CalDavCtx> {
    cx.try_global::<CaldavGlobal>().map(|g| g.0.clone())
}

/// Creates the context and starts the triggers. Call after `bus::init`.
pub fn init(cx: &mut App) {
    let Some(dir) = data_dir() else {
        log::warn!("caldav: no data directory; sync disabled");
        return;
    };
    let ctx = CalDavCtx::new(dir, bus::sink());
    cx.set_global(CaldavGlobal(ctx.clone()));

    // Launch sync + 15-minute poll; runs for the app's lifetime.
    drop(commands::start_background_sync(ctx.clone()));

    let debounce = DebounceState::default();
    bus::subscribe_app(cx, move |ev, _| {
        if let AppEvent::Core(CoreEvent::FileWritten { path, source }) = ev {
            commands::on_file_written(&ctx, debounce.clone(), path.clone(), source.clone());
        }
    })
    .detach();
}
