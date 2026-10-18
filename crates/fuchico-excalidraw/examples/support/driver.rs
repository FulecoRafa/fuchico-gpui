//! Scripted input for exercising the viewer without touching the user's
//! real pointer or focus (`EXCALIDRAW_DRIVE=<dir>`).
//!
//! The viewer polls `<dir>/cmd`; each line is one command, dispatched into
//! the window through GPUI's normal event pipeline (`Window::dispatch_event`
//! / `dispatch_keystroke`, so hit-testing, focus, listeners and the input
//! handler all run exactly as for OS events). When the script finishes the
//! viewer writes `<dir>/ack`. Coordinates are window-content points.
//!
//!     move x y [mods]            down x y [mods] [clicks] [right]
//!     up x y [mods]              click x y [mods] [clicks]
//!     rclick x y                 drag x1 y1 x2 y2 [mods]
//!     key <keystroke>            type <text>
//!     scroll x y dx dy [mods]    wait <ms>
//!     export <file.png|svg>      json <file>
//!
//! `mods` is a comma list of cmd,shift,alt,ctrl (or `-`). After every
//! command the window is asked to redisplay (see `refresh`), which also
//! repaints while the window is occluded so `screencapture -l` is current.

use std::path::PathBuf;
use std::time::Duration;

use fuchico_excalidraw::{ExcalidrawView, ExportOptions};
use gpui_kit::*;

fn mods(s: &str) -> Modifiers {
    let mut m = Modifiers::default();
    for p in s.split(',') {
        match p {
            "cmd" => m.platform = true,
            "shift" => m.shift = true,
            "alt" => m.alt = true,
            "ctrl" => m.control = true,
            _ => {}
        }
    }
    m
}

fn pt(a: &[&str], i: usize) -> Point<Pixels> {
    let f = |k: usize| a.get(k).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    point(px(f(i)), px(f(i + 1)))
}

/// The window's NSView (as an address), for [`refresh`].
#[cfg(target_os = "macos")]
fn ns_view(window: &Window) -> usize {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        Ok(RawWindowHandle::AppKit(h)) => h.ns_view.as_ptr() as usize,
        _ => 0,
    }
}
#[cfg(not(target_os = "macos"))]
fn ns_view(_: &Window) -> usize {
    0
}

/// Asks AppKit to redisplay the view: its `displayLayer:` runs GPUI's frame
/// callback (draw + present) on the next run-loop turn, even while the window
/// is occluded and GPUI's display link is paused. Never called while the App
/// is borrowed (the callback re-enters it).
#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn refresh(view: usize) {
    use objc::{msg_send, sel, sel_impl};
    if view != 0 {
        let view = view as *mut objc::runtime::Object;
        unsafe {
            let _: () = msg_send![view, setNeedsDisplay: true];
        }
    }
}
#[cfg(not(target_os = "macos"))]
fn refresh(_: usize) {}

fn run_line(line: &str, view: &Entity<ExcalidrawView>, window: &mut Window, cx: &mut App) -> Option<u64> {
    let a: Vec<&str> = line.split_whitespace().collect();
    let cmd = a.first().copied()?;
    let m = |i: usize| mods(a.get(i).copied().unwrap_or("-"));
    let down = |window: &mut Window, cx: &mut App, p, m, clicks, button| {
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent { button, position: p, modifiers: m, click_count: clicks, first_mouse: false }),
            cx,
        );
    };
    let up = |window: &mut Window, cx: &mut App, p, m, clicks, button| {
        window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button, position: p, modifiers: m, click_count: clicks }), cx);
    };
    let mv = |window: &mut Window, cx: &mut App, p, m, pressed| {
        window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, pressed_button: pressed, modifiers: m }), cx);
    };
    match cmd {
        "move" => mv(window, cx, pt(&a, 1), m(3), None),
        "down" => {
            let clicks = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(1);
            let b = if a.get(5) == Some(&"right") { MouseButton::Right } else { MouseButton::Left };
            mv(window, cx, pt(&a, 1), m(3), None);
            down(window, cx, pt(&a, 1), m(3), clicks, b);
        }
        "up" => up(window, cx, pt(&a, 1), m(3), 1, MouseButton::Left),
        "click" => {
            let clicks = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(1);
            let p = pt(&a, 1);
            mv(window, cx, p, m(3), None);
            for k in 1..=clicks {
                down(window, cx, p, m(3), k, MouseButton::Left);
                up(window, cx, p, m(3), k, MouseButton::Left);
            }
        }
        "rclick" => {
            let p = pt(&a, 1);
            mv(window, cx, p, Modifiers::default(), None);
            down(window, cx, p, Modifiers::default(), 1, MouseButton::Right);
            up(window, cx, p, Modifiers::default(), 1, MouseButton::Right);
        }
        "drag" => {
            let (p0, p1) = (pt(&a, 1), pt(&a, 3));
            let md = m(5);
            mv(window, cx, p0, md, None);
            down(window, cx, p0, md, 1, MouseButton::Left);
            for k in 1..=12 {
                let t = k as f32 / 12.0;
                let p = point(p0.x + (p1.x - p0.x) * t, p0.y + (p1.y - p0.y) * t);
                mv(window, cx, p, md, Some(MouseButton::Left));
            }
            up(window, cx, p1, md, 1, MouseButton::Left);
        }
        "key" => {
            if let Ok(ks) = Keystroke::parse(a.get(1).copied().unwrap_or("")) {
                window.dispatch_keystroke(ks, cx);
            }
        }
        "type" => {
            let text = line.split_once(' ').map(|x| x.1).unwrap_or("");
            for c in text.chars() {
                let key = match c {
                    ' ' => "space".to_string(),
                    c => c.to_lowercase().to_string(),
                };
                if let Ok(mut ks) = Keystroke::parse(&key) {
                    ks.key_char = Some(c.to_string());
                    ks.modifiers.shift = c.is_uppercase();
                    window.dispatch_keystroke(ks, cx);
                }
            }
        }
        "scroll" => {
            let p = pt(&a, 1);
            let d = pt(&a, 3);
            window.dispatch_event(
                PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position: p,
                    delta: ScrollDelta::Pixels(d),
                    modifiers: m(5),
                    touch_phase: TouchPhase::Moved,
                }),
                cx,
            );
        }
        "wait" => return a.get(1).and_then(|s| s.parse().ok()),
        "export" => {
            let path = PathBuf::from(a.get(1).copied().unwrap_or("export.png"));
            let opts = ExportOptions { background: true, ..Default::default() };
            let v = view.read(cx);
            let bytes = if path.extension().map(|e| e == "svg").unwrap_or(false) {
                v.export_svg(&opts).into_bytes()
            } else {
                v.export_png(&opts).unwrap_or_default()
            };
            let _ = std::fs::write(path, bytes);
        }
        "json" => {
            let path = PathBuf::from(a.get(1).copied().unwrap_or("scene.json"));
            let _ = std::fs::write(path, view.read(cx).to_json());
        }
        _ => log::warn!("driver: unknown command {line:?}"),
    }
    None
}

pub fn start(dir: PathBuf, handle: AnyWindowHandle, view: Entity<ExcalidrawView>, cx: &mut App) {
    let cmd = dir.join("cmd");
    let ack = dir.join("ack");
    let nsview = cx.update_window(handle, |_, window, _| ns_view(window)).unwrap_or(0);
    cx.spawn(async move |cx: &mut AsyncApp| {
        let mut n = 0u64;
        loop {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            let Ok(script) = std::fs::read_to_string(&cmd) else { continue };
            let _ = std::fs::remove_file(&cmd);
            n += 1;
            for line in script.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
                let wait = cx
                    .update_window(handle, |_, window, cx| {
                        let w = run_line(line, &view, window, cx);
                        view.update(cx, |_, cx| cx.notify());
                        w
                    })
                    .ok()
                    .flatten();
                refresh(nsview);
                cx.background_executor().timer(Duration::from_millis(wait.unwrap_or(40))).await;
            }
            for _ in 0..3 {
                refresh(nsview);
                cx.background_executor().timer(Duration::from_millis(80)).await;
            }
            let _ = std::fs::write(&ack, format!("{n}\n"));
        }
    })
    .detach();
}
