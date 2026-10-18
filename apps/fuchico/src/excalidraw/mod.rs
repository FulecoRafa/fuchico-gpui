//! `.excalidraw` tabs (parity §10): the `fuchico-excalidraw` canvas hosted
//! as a [`PaneItem`].
//!
//! * Load: `fs_read_file` off the UI thread; non-text content is
//!   "Not a valid drawing file", a parse failure "Malformed drawing file",
//!   an IO error its message.
//! * Autosave: the view debounces (1000 ms) and hands us the JSON; we
//!   write it off-thread with `fs_write_file(.., source "editor")`, which
//!   emits `fs:file-written`. The tab's dirty dot mirrors the view; a
//!   failed write keeps it set (and reports the error).
//! * External changes (`fs:changed` for the folder, `fs:file-written` for
//!   the file) reload the drawing unless it has unsaved edits. Our own
//!   writes are no-ops (the view ignores content equal to its baseline).
//! * Theme: dark canvas when the app's resolved appearance is dark (the
//!   preferred port behavior over the original's OS-only check).

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use fuchico_core::events::CoreEvent;
use fuchico_core::fs::file::{ReadResult, fs_read_file, fs_write_file};
use fuchico_excalidraw::{ExcalidrawEvent, ExcalidrawStyle, ExcalidrawView, Scene};
use gpui_kit::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement,
    ParentElement, Render, Styled, Subscription, Window, div,
};

use crate::bus::{self, AppEvent};
use crate::t;
use crate::theme::AppTheme;
use crate::ui::u;
use crate::workspace::pane::{PaneContext, PaneEvent, PaneItem};

enum State {
    Loading,
    Failed(String),
    Ready(Entity<ExcalidrawView>),
}

pub struct ExcalidrawPane {
    path: String,
    /// Same path, shared with the view's save callback so renames apply.
    save_path: Rc<RefCell<String>>,
    /// Folder strings an `fs:changed` for this file may carry.
    dirs: Vec<String>,
    #[allow(dead_code)]
    pcx: PaneContext,
    state: State,
    /// Mirrors the tab's dirty dot (view dirty, or the last write failed).
    dirty: bool,
    save_failed: bool,
    /// A reload read is in flight.
    reloading: bool,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
}

/// What a read of the drawing file means for the pane.
#[derive(Debug, PartialEq)]
pub enum Loaded {
    Json(String),
    Error(String),
}

pub fn interpret_read(res: Result<ReadResult, String>) -> Loaded {
    match res {
        Ok(ReadResult::Text { content, .. }) => Loaded::Json(content),
        Ok(_) => Loaded::Error(t!(ExcalidrawNotValid)),
        Err(e) => Loaded::Error(e),
    }
}

/// Parent folder strings an `fs:changed {dir}` may use for this file
/// (as given, and canonicalized: the watcher reports real paths).
/// Blocking (canonicalize); cheap.
fn parent_dirs(path: &str) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(p) = Path::new(path).parent() {
        v.push(p.to_string_lossy().into_owned());
        if let Ok(c) = std::fs::canonicalize(p) {
            let c = c.to_string_lossy().into_owned();
            if !v.contains(&c) {
                v.push(c);
            }
        }
    }
    v
}

fn same_dir(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

fn style_for(cx: &App) -> ExcalidrawStyle {
    if AppTheme::get(cx).appearance.is_dark() { ExcalidrawStyle::dark() } else { ExcalidrawStyle::light() }
}

impl ExcalidrawPane {
    pub fn new(path: String, pcx: PaneContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let p = path.clone();
        let read = cx.background_spawn(async move { (interpret_read(fs_read_file(p.clone())), parent_dirs(&p)) });
        cx.spawn_in(window, async move |this, cx| {
            let (loaded, dirs) = read.await;
            this.update_in(cx, |this, window, cx| {
                this.dirs = dirs;
                match loaded {
                    Loaded::Json(json) => this.mount(json, window, cx),
                    Loaded::Error(e) => this.state = State::Failed(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();

        let bus_sub = bus::subscribe(cx, |this: &mut Self, ev, cx| this.on_bus(ev, cx));
        let theme_sub = cx.observe_global::<AppTheme>(|this: &mut Self, cx| {
            if let State::Ready(v) = &this.state {
                v.update(cx, |_, cx| cx.notify());
            }
        });
        let dirs = Path::new(&path).parent().map(|p| vec![p.to_string_lossy().into_owned()]).unwrap_or_default();
        ExcalidrawPane {
            save_path: Rc::new(RefCell::new(path.clone())),
            path,
            dirs,
            pcx,
            state: State::Loading,
            dirty: false,
            save_failed: false,
            reloading: false,
            focus: cx.focus_handle(),
            _subs: vec![bus_sub, theme_sub],
        }
    }

    fn mount(&mut self, json: String, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        let save_path = self.save_path.clone();
        // Must not update the view synchronously: defer to the pane.
        let on_change: fuchico_excalidraw::OnChange = Rc::new(move |ev, cx: &mut App| {
            match ev {
                ExcalidrawEvent::Dirty(d) => {
                    let weak = weak.clone();
                    cx.defer(move |cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if d {
                                // A new edit retries a failed save.
                                this.save_failed = false;
                            }
                            this.set_dirty(d, cx)
                        });
                    });
                }
                ExcalidrawEvent::Save(content) => {
                    let target = save_path.borrow().clone();
                    let weak = weak.clone();
                    let write = cx.background_spawn(async move {
                        fs_write_file(target, content, Some("editor".into()), &*bus::sink())
                    });
                    cx.spawn(async move |cx| {
                        if let Err(e) = write.await {
                            log::warn!("excalidraw: save failed: {e}");
                            let _ = weak.update(cx, |this, cx| {
                                this.save_failed = true;
                                this.set_dirty(true, cx);
                            });
                            let _ = cx.update(|cx| {
                                if let Some(w) = cx.active_window() {
                                    let _ = w.update(cx, |_, window, cx| {
                                        crate::ui::toast::toast_error(
                                            window,
                                            cx,
                                            t!(ExcalidrawSaveFailed, message = e),
                                        )
                                    });
                                }
                            });
                        }
                    })
                    .detach();
                }
            }
            Ok(())
        });
        let style: fuchico_excalidraw::StyleFn = Rc::new(style_for);
        let had_focus = self.focus.is_focused(window);
        let view = cx.new(|cx| ExcalidrawView::new(json, style, on_change, window, cx));
        if had_focus {
            let h = view.read(cx).focus_handle().clone();
            window.focus(&h, cx);
        }
        self.state = State::Ready(view);
    }

    fn set_dirty(&mut self, d: bool, cx: &mut Context<Self>) {
        let d = d || self.save_failed;
        if self.dirty != d {
            self.dirty = d;
            cx.emit(PaneEvent::DirtyChanged(d));
            cx.notify();
        }
    }

    fn on_bus(&mut self, ev: &AppEvent, cx: &mut Context<Self>) {
        let AppEvent::Core(core) = ev else { return };
        let ours = match core {
            CoreEvent::FsChanged { dir, .. } => self.dirs.iter().any(|d| same_dir(d, dir)),
            CoreEvent::FileWritten { path, .. } => *path == self.path,
            _ => false,
        };
        if ours {
            self.reload_from_disk(cx);
        }
    }

    /// Re-reads the file and hands it to the view unless there are unsaved
    /// edits (checked again once the read is back).
    fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        if self.dirty || self.reloading || !matches!(self.state, State::Ready(_)) {
            return;
        }
        self.reloading = true;
        let p = self.path.clone();
        let read = cx.background_spawn(async move { interpret_read(fs_read_file(p)) });
        cx.spawn(async move |this, cx| {
            let loaded = read.await;
            this.update(cx, |this, cx| {
                this.reloading = false;
                if this.dirty {
                    return;
                }
                match (&this.state, loaded) {
                    (State::Ready(v), Loaded::Json(json)) if !v.read(cx).is_dirty() => {
                        v.update(cx, |v, cx| v.reload(&json, cx));
                    }
                    // Deleted / unreadable: keep what is on screen.
                    (_, Loaded::Error(e)) => log::debug!("excalidraw: reload skipped: {e}"),
                    _ => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    #[cfg(test)]
    pub fn view(&self) -> Option<&Entity<ExcalidrawView>> {
        match &self.state {
            State::Ready(v) => Some(v),
            _ => None,
        }
    }
}

impl EventEmitter<PaneEvent> for ExcalidrawPane {}

impl Focusable for ExcalidrawPane {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.state {
            State::Ready(v) => v.read(cx).focus_handle().clone(),
            _ => self.focus.clone(),
        }
    }
}

impl PaneItem for ExcalidrawPane {
    fn path(&self) -> &str {
        &self.path
    }

    fn set_path(&mut self, path: String, cx: &mut Context<Self>) {
        *self.save_path.borrow_mut() = path.clone();
        self.dirs = Path::new(&path).parent().map(|p| vec![p.to_string_lossy().into_owned()]).unwrap_or_default();
        self.path = path;
        cx.notify();
    }

    /// No manual save in the original (Cmd+S did nothing for drawings);
    /// here it writes pending edits right away.
    fn save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let State::Ready(v) = &self.state {
            v.update(cx, |v, cx| v.flush(cx));
        }
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }
}

impl Render for ExcalidrawPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx);
        let status = |text: String, error: bool| {
            div()
                .track_focus(&self.focus)
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(th.colors.background)
                .text_size(u(13.))
                .text_color(if error { th.colors.destructive } else { th.colors.muted_foreground })
                .child(text)
        };
        match &self.state {
            State::Loading => status(t!(CommonLoading), false),
            State::Failed(e) => status(e.clone(), true),
            State::Ready(v) if v.read(cx).error().is_some() => status(t!(ExcalidrawMalformed), true),
            State::Ready(v) => div().size_full().min_h_0().child(v.clone()),
        }
    }
}

/// The content of a new drawing: an empty Excalidraw v2 scene.
pub fn drawing_template() -> String {
    Scene::default().to_json()
}

/// File name for "New drawing": `.excalidraw` is appended when the entered
/// name has no `.` (the original's `defaultExt`).
pub fn drawing_file_name(name: &str) -> String {
    let name = name.trim();
    if name.contains('.') { name.to_string() } else { format!("{name}.excalidraw") }
}

/// Creates `<dir>/<name>` holding an empty drawing (emits
/// `fs:file-written`). Blocking: call off the UI thread. Fails when the
/// file exists. Returns the new path.
pub fn create_drawing(dir: &str, name: &str) -> Result<String, String> {
    let file = drawing_file_name(name);
    let path = Path::new(dir).join(&file).to_string_lossy().into_owned();
    if Path::new(&path).exists() {
        return Err(format!("{file} already exists"));
    }
    fs_write_file(path.clone(), drawing_template(), Some("template".into()), &*bus::sink())?;
    Ok(path)
}

#[cfg(test)]
mod ui_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_results() {
        let ok = Ok(ReadResult::Text { content: "{}".into(), size: 2 });
        assert_eq!(interpret_read(ok), Loaded::Json("{}".into()));
        assert_eq!(interpret_read(Ok(ReadResult::Binary { size: 3 })), Loaded::Error(t!(ExcalidrawNotValid)));
        assert_eq!(interpret_read(Err("boom".into())), Loaded::Error("boom".into()));
    }

    #[test]
    fn dirs_match_with_or_without_trailing_slash() {
        assert!(same_dir("/v/a", "/v/a/"));
        assert!(!same_dir("/v/a", "/v/ab"));
    }

    #[test]
    fn new_drawing_names_and_template() {
        assert_eq!(drawing_file_name("Sketch"), "Sketch.excalidraw");
        assert_eq!(drawing_file_name("a.json"), "a.json");
        let t = drawing_template();
        assert!(t.contains("excalidraw"), "{t}");
        assert!(Scene::parse(&t).is_ok());
    }

    #[test]
    fn create_drawing_writes_template() {
        let dir = std::env::temp_dir().join(format!("fuchico-exc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_string_lossy().into_owned();
        let p = create_drawing(&d, "One").unwrap();
        assert!(p.ends_with("One.excalidraw"));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), drawing_template());
        assert!(create_drawing(&d, "One").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
