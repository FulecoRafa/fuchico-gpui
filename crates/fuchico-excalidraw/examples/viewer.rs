//! Opens, edits and autosaves the `.excalidraw` file given on argv.
//!
//!     cargo run -p fuchico-excalidraw --example viewer -- path/to/file.excalidraw
//!
//! `EXCALIDRAW_DARK=1` / `EXCALIDRAW_LIGHT=1` force a theme; otherwise the
//! window appearance is followed. The host (this example) owns all IO:
//! `Save` events write the file, and external edits are picked up by
//! polling the file's mtime and calling `reload`.
//!
//! `EXCALIDRAW_DRIVE=<dir>` enables the scripted-input driver
//! (`support/driver.rs`) and leaves the app inactive (no focus stealing).

#[path = "support/driver.rs"]
mod driver;

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use fuchico_excalidraw::{ExcalidrawEvent, ExcalidrawStyle, ExcalidrawView, OnChange, StyleFn};
use gpui_kit::*;

fn mtime(p: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn main() {
    let path = PathBuf::from(std::env::args().nth(1).expect("usage: viewer <file.excalidraw>"));
    let forced = if std::env::var("EXCALIDRAW_DARK").is_ok() {
        Some(true)
    } else if std::env::var("EXCALIDRAW_LIGHT").is_ok() {
        Some(false)
    } else {
        None
    };
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        let title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1280.0), px(800.0)), cx))),
            titlebar: Some(TitlebarOptions { title: Some(title.into()), ..Default::default() }),
            ..Default::default()
        };
        let dark = Rc::new(Cell::new(forced.unwrap_or(false)));
        let path2 = path.clone();
        let drive = std::env::var_os("EXCALIDRAW_DRIVE").map(PathBuf::from);
        let (handle, view) = gpui_kit::open_window(opts, cx, move |window, cx| {
            if forced.is_none() {
                dark.set(matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark));
            }
            let d = dark.clone();
            let style: StyleFn = Rc::new(move |_| if d.get() { ExcalidrawStyle::dark() } else { ExcalidrawStyle::light() });
            let save_path = path2.clone();
            let on_change: OnChange = Rc::new(move |ev, _cx| match ev {
                ExcalidrawEvent::Dirty(d) => {
                    log::info!("dirty: {d}");
                    Ok(())
                }
                ExcalidrawEvent::Save(json) => std::fs::write(&save_path, json).map_err(|e| e.to_string()),
            });
            let view = cx.new(|cx| ExcalidrawView::new(path2.clone(), style, on_change, window, cx));
            let fh = view.read(cx).focus_handle().clone();
            window.focus(&fh, cx);
            if forced.is_none() {
                let v = view.clone();
                let d = dark.clone();
                window
                    .observe_window_appearance(move |window, cx| {
                        d.set(matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark));
                        v.update(cx, |_, cx| cx.notify());
                    })
                    .detach();
            }
            view
        })
        .expect("window");

        if let Some(dir) = drive.clone() {
            driver::start(dir, handle, view.clone(), cx);
        }

        // Reload on external change.
        let watch = path.clone();
        let view = view.downgrade();
        cx.spawn(async move |cx: &mut AsyncApp| {
            let mut last = mtime(&watch);
            loop {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                let now = mtime(&watch);
                if now != last {
                    last = now;
                    if let Ok(s) = std::fs::read_to_string(&watch)
                        && view.update(cx, |v, cx| v.reload(&s, cx)).is_err() {
                            break;
                        }
                }
            }
        })
        .detach();
        if drive.is_none()
            && std::env::var_os("FUCHICO_BACKGROUND").is_none() { cx.activate(true); }
    });
}
