//! `ExcalidrawView`: the editable canvas entity (state, interactions,
//! history, clipboard, autosave). UI chrome lives in `ui.rs`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::*;

use crate::color::Rgba;
use crate::geom::{self, P};
use crate::model::{BoundElement, Element, Num};
use crate::render::{Caches, Camera, Palette};
use crate::scene::{self, ItemDefaults, Scene, Snapshot};
use crate::style::ExcalidrawStyle;
use crate::text;

/// Autosave debounce (the Tauri pane's `SAVE_DEBOUNCE_MS`).
pub const SAVE_DEBOUNCE: Duration = Duration::from_millis(1000);
const HISTORY_LIMIT: usize = 100;

/// What the view reports to its host.
#[derive(Clone, Debug, PartialEq)]
pub enum ExcalidrawEvent {
    /// The unsaved-changes state changed.
    Dirty(bool),
    /// The debounced autosave fired: persist this content. Returning `Err`
    /// keeps the view dirty (it retries on the next change or flush).
    Save(String),
}

/// Host callback. Must not synchronously update the view entity.
pub type OnChange = Rc<dyn Fn(ExcalidrawEvent, &mut App) -> Result<(), String>>;
/// Theme provider, re-evaluated every render (follow the app theme).
pub type StyleFn = Rc<dyn Fn(&App) -> ExcalidrawStyle>;

/// Where the initial drawing comes from.
#[derive(Clone, Debug)]
pub enum DrawingSource {
    Path(PathBuf),
    Json(String),
}

impl From<PathBuf> for DrawingSource {
    fn from(p: PathBuf) -> Self {
        DrawingSource::Path(p)
    }
}
impl From<&std::path::Path> for DrawingSource {
    fn from(p: &std::path::Path) -> Self {
        DrawingSource::Path(p.to_path_buf())
    }
}
impl From<String> for DrawingSource {
    fn from(s: String) -> Self {
        DrawingSource::Json(s)
    }
}
impl From<&str> for DrawingSource {
    fn from(s: &str) -> Self {
        DrawingSource::Json(s.to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Hand,
    Selection,
    Rectangle,
    Diamond,
    Ellipse,
    Arrow,
    Line,
    Freedraw,
    Text,
    Image,
    Eraser,
    Frame,
}

impl Tool {
    pub fn element_kind(self) -> Option<&'static str> {
        Some(match self {
            Tool::Rectangle => "rectangle",
            Tool::Diamond => "diamond",
            Tool::Ellipse => "ellipse",
            Tool::Arrow => "arrow",
            Tool::Line => "line",
            Tool::Freedraw => "freedraw",
            Tool::Text => "text",
            Tool::Frame => "frame",
            _ => return None,
        })
    }
}

/// Chrome [`ExcalidrawView::debug_open`] can open.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DebugUi {
    Menu,
    Library,
    Help,
    Export,
    /// Context menu at a pane-local point.
    ContextMenu(f32, f32),
    StrokePicker,
}

/// Selection-box handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
    Rotate,
}

impl Handle {
    fn sides(self) -> (i8, i8) {
        match self {
            Handle::N => (0, -1),
            Handle::S => (0, 1),
            Handle::E => (1, 0),
            Handle::W => (-1, 0),
            Handle::NE => (1, -1),
            Handle::NW => (-1, -1),
            Handle::SE => (1, 1),
            Handle::SW => (-1, 1),
            Handle::Rotate => (0, 0),
        }
    }
    fn is_corner(self) -> bool {
        matches!(self, Handle::NE | Handle::NW | Handle::SE | Handle::SW)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Drag {
    Pan {
        start: Point<Pixels>,
        cam: Camera,
    },
    Marquee {
        start: P,
        current: P,
    },
    Move {
        start: P,
        ids: Vec<String>,
        orig: HashMap<String, Element>,
        moved: bool,
    },
    Resize {
        handle: Handle,
        orig: HashMap<String, Element>,
        bbox: [f64; 4],
        single: Option<String>,
    },
    Rotate {
        center: P,
        start_angle: f64,
        orig: HashMap<String, Element>,
    },
    Create {
        id: String,
        start: P,
    },
    Linear {
        id: String,
        start: P,
        moved: bool,
        start_bind: Option<String>,
    },
    Freedraw {
        id: String,
    },
    Point {
        id: String,
        index: usize,
    },
    Erase,
}

#[derive(Clone, Debug)]
pub(crate) struct TextEdit {
    pub id: String,
    pub caret: usize,
}

pub struct ExcalidrawView {
    pub(crate) scene: Scene,
    pub(crate) error: Option<String>,
    pub(crate) cam: Camera,
    pub(crate) tool: Tool,
    pub(crate) tool_locked: bool,
    pub(crate) selected: HashSet<String>,
    pub(crate) drag: Option<Drag>,
    pub(crate) multi: Option<String>,
    pub(crate) editing: Option<TextEdit>,
    pub(crate) erasing: HashSet<String>,
    pub(crate) bind_hint: Option<String>,
    pub(crate) defaults: ItemDefaults,
    pub(crate) pointer: Option<Point<Pixels>>,
    pub(crate) space_down: bool,
    pub(crate) menu_open: bool,
    pub(crate) focus: FocusHandle,
    pub(crate) bounds: Rc<Cell<Bounds<Pixels>>>,
    pub(crate) caches: Rc<RefCell<Caches>>,
    pub(crate) style_fn: StyleFn,
    on_change: OnChange,
    // history
    committed: Snapshot,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    // persistence
    baseline: String,
    dirty: bool,
    save_task: Option<Task<()>>,
    _release: Option<Subscription>,
    // Chrome state (see actions.rs).
    pub(crate) context_menu: Option<Point<Pixels>>,
    pub(crate) picker: Option<crate::actions::PickerTarget>,
    pub(crate) hex: Option<String>,
    pub(crate) eyedropper: Option<crate::actions::PickerTarget>,
    pub(crate) dialog: Option<crate::actions::Dialog>,
    pub(crate) export: crate::actions::ExportState,
    pub(crate) export_preview: Option<std::sync::Arc<RenderImage>>,
    pub(crate) copied_styles: Option<crate::model::Element>,
    pub(crate) library: crate::library::Library,
    pub(crate) library_open: bool,
    pub(crate) toast: Option<String>,
    pub(crate) guides: Vec<crate::snap::Guide>,
    pub(crate) opacity_track: Rc<Cell<Bounds<Pixels>>>,
    pub(crate) slider_drag: bool,
    /// IME composition range (chars) inside the edited text.
    pub(crate) marked: Option<std::ops::Range<usize>>,
    pub(crate) source_path: Option<std::path::PathBuf>,
}

fn load_source(src: &DrawingSource) -> Result<String, String> {
    match src {
        DrawingSource::Json(s) => Ok(s.clone()),
        DrawingSource::Path(p) => match std::fs::read(p) {
            Ok(bytes) => {
                String::from_utf8(bytes).map_err(|_| "Not a valid drawing file".to_string())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e.to_string()),
        },
    }
}

fn camera_from(scene: &Scene) -> Camera {
    let mut cam = Camera::default();
    if let Some(a) = scene.app_state() {
        if let Some(x) = a.get("scrollX").and_then(|v| v.as_f64()) {
            cam.scroll_x = x;
        }
        if let Some(y) = a.get("scrollY").and_then(|v| v.as_f64()) {
            cam.scroll_y = y;
        }
        if let Some(z) = a.get("zoom").and_then(|v| {
            v.get("value")
                .and_then(|v| v.as_f64())
                .or_else(|| v.as_f64())
        }) && z > 0.0
        {
            cam.zoom = z;
        }
    }
    cam
}

impl ExcalidrawView {
    pub fn new(
        source: impl Into<DrawingSource>,
        style_fn: StyleFn,
        on_change: OnChange,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let source = source.into();
        let source_path = match &source {
            DrawingSource::Path(p) => Some(p.clone()),
            DrawingSource::Json(_) => None,
        };
        let (scene, error, baseline) = match load_source(&source) {
            Ok(content) => match Scene::parse(&content) {
                Ok(s) => (s, None, content),
                Err(e) => (Scene::default(), Some(e.to_string()), content),
            },
            Err(e) => (Scene::default(), Some(e), String::new()),
        };
        let cam = camera_from(&scene);
        let committed = scene.snapshot();
        let release = cx.on_release(|this: &mut Self, cx: &mut App| this.flush_app(cx));
        Self {
            scene,
            error,
            cam,
            tool: Tool::Selection,
            tool_locked: false,
            selected: HashSet::new(),
            drag: None,
            multi: None,
            editing: None,
            erasing: HashSet::new(),
            bind_hint: None,
            defaults: ItemDefaults::default(),
            pointer: None,
            space_down: false,
            menu_open: false,
            focus: cx.focus_handle(),
            bounds: Rc::new(Cell::new(Bounds::default())),
            caches: Rc::new(RefCell::new(Caches::default())),
            style_fn,
            on_change,
            committed,
            undo: Vec::new(),
            redo: Vec::new(),
            baseline,
            dirty: false,
            save_task: None,
            _release: Some(release),
            context_menu: None,
            picker: None,
            hex: None,
            eyedropper: None,
            dialog: None,
            export: Default::default(),
            export_preview: None,
            copied_styles: None,
            library: crate::library::Library::load(crate::library::default_path()),
            library_open: false,
            toast: None,
            guides: Vec::new(),
            opacity_track: Rc::new(Cell::new(Bounds::default())),
            slider_drag: false,
            marked: None,
            source_path,
        }
    }

    // ------------------------------------------------------------ public API

    /// The current drawing serialized the way it would be saved.
    pub fn to_json(&self) -> String {
        self.scene.to_json()
    }

    /// The drawing (or `opts.only`) as SVG.
    pub fn export_svg(&self, opts: &crate::export::ExportOptions) -> String {
        crate::export::to_svg(&self.scene, opts)
    }

    /// The drawing (or `opts.only`) as PNG bytes.
    pub fn export_png(&self, opts: &crate::export::ExportOptions) -> Result<Vec<u8>, String> {
        crate::export::to_png(&self.scene, opts)
    }

    /// Points the personal library at `path` (tests use a temp file).
    pub fn set_library_path(&mut self, path: std::path::PathBuf) {
        self.library = crate::library::Library::load(Some(path));
    }

    /// Where the personal library is persisted.
    pub fn library_path(&self) -> Option<std::path::PathBuf> {
        self.library.path.clone()
    }

    /// The current selection's ids (sorted).
    pub fn selection(&self) -> Vec<String> {
        let mut v: Vec<String> = self.selected.iter().cloned().collect();
        v.sort();
        v
    }

    /// The scene being edited (read-only).
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Replaces the selection (unknown ids are ignored).
    pub fn select_ids(&mut self, ids: &[&str], cx: &mut Context<Self>) {
        self.selected = ids
            .iter()
            .filter(|id| self.scene.get(id).is_some())
            .map(|s| s.to_string())
            .collect();
        cx.notify();
    }

    /// Opens a piece of editor chrome, as clicking its button would
    /// (offscreen snapshots and tests).
    #[doc(hidden)]
    pub fn debug_open(&mut self, ui: DebugUi, cx: &mut Context<Self>) {
        use crate::actions::{Dialog, PickerTarget};
        match ui {
            DebugUi::Menu => self.menu_open = true,
            DebugUi::Library => self.library_open = true,
            DebugUi::Help => self.open_dialog(Dialog::Help),
            DebugUi::Export => self.open_dialog(Dialog::Export),
            DebugUi::ContextMenu(x, y) => {
                let o = self.bounds.get().origin;
                self.context_menu = Some(Point {
                    x: o.x + px(x),
                    y: o.y + px(y),
                });
            }
            DebugUi::StrokePicker => self.toggle_picker(PickerTarget::Stroke),
        }
        cx.notify();
    }

    /// The active tool.
    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// The text element being edited, if any.
    pub fn editing_text(&self) -> Option<&str> {
        self.editing.as_ref().map(|e| e.id.as_str())
    }

    /// Replaces the drawing with externally changed content, keeping the
    /// camera. A no-op when the content matches what the view already has.
    pub fn reload(&mut self, json: &str, cx: &mut Context<Self>) {
        if json == self.baseline && self.error.is_none() && !self.dirty {
            return;
        }
        if self.error.is_none() && json == self.scene.to_json() {
            self.baseline = json.to_string();
            self.set_dirty(false, cx);
            return;
        }
        match Scene::parse(json) {
            Ok(s) => {
                self.scene = s;
                self.error = None;
                self.selected.retain(|id| self.scene.get(id).is_some());
                self.editing = None;
                self.multi = None;
                self.drag = None;
                self.committed = self.scene.snapshot();
                self.undo.clear();
                self.redo.clear();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.baseline = json.to_string();
        self.save_task = None;
        self.set_dirty(false, cx);
        cx.notify();
    }

    /// Writes pending changes now (also done automatically on release).
    pub fn flush(&mut self, cx: &mut Context<Self>) {
        self.flush_app(cx);
        cx.notify();
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    pub fn set_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_multi(cx);
        self.commit_text(cx);
        self.tool = tool;
        if !matches!(tool, Tool::Selection) {
            self.selected.clear();
        }
        if tool == Tool::Image {
            self.prompt_image(window, cx);
        }
        cx.notify();
    }

    // ------------------------------------------------------------ persistence

    fn emit(&self, ev: ExcalidrawEvent, cx: &mut App) -> Result<(), String> {
        (self.on_change)(ev, cx)
    }

    fn set_dirty(&mut self, d: bool, cx: &mut App) {
        if self.dirty != d {
            self.dirty = d;
            let _ = self.emit(ExcalidrawEvent::Dirty(d), cx);
        }
    }

    fn flush_app(&mut self, cx: &mut App) {
        self.save_task = None;
        if !self.dirty || self.error.is_some() {
            return;
        }
        let json = self.scene.to_json();
        if json == self.baseline {
            self.set_dirty(false, cx);
            return;
        }
        match self.emit(ExcalidrawEvent::Save(json.clone()), cx) {
            Ok(()) => {
                self.baseline = json;
                self.set_dirty(false, cx);
            }
            Err(e) => log::warn!("excalidraw save failed: {e}"),
        }
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.set_dirty(true, cx);
        self.save_task = Some(
            cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                cx.background_executor().timer(SAVE_DEBOUNCE).await;
                let _ = this.update(cx, |v, cx| {
                    v.flush_app(cx);
                    cx.notify();
                });
            }),
        );
    }

    // ------------------------------------------------------------ history

    /// Records the current scene as one undoable step and schedules a save.
    pub(crate) fn commit(&mut self, cx: &mut Context<Self>) {
        let now = self.scene.snapshot();
        if now == self.committed {
            return;
        }
        self.scene.bump_changed(&self.committed);
        let prev = std::mem::replace(&mut self.committed, self.scene.snapshot());
        self.undo.push(prev);
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.schedule_save(cx);
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(crate) fn undo(&mut self, cx: &mut Context<Self>) {
        self.commit_text(cx);
        if let Some(s) = self.undo.pop() {
            let cur = std::mem::replace(&mut self.committed, s.clone());
            self.redo.push(cur);
            self.scene.restore(&s);
            self.after_history(cx);
        }
    }

    pub(crate) fn redo(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = self.redo.pop() {
            let cur = std::mem::replace(&mut self.committed, s.clone());
            self.undo.push(cur);
            self.scene.restore(&s);
            self.after_history(cx);
        }
    }

    fn after_history(&mut self, cx: &mut Context<Self>) {
        self.selected.retain(|id| self.scene.get(id).is_some());
        self.drag = None;
        self.multi = None;
        self.editing = None;
        self.schedule_save(cx);
        cx.notify();
    }

    // ------------------------------------------------------------ geometry helpers

    pub(crate) fn origin(&self) -> Point<Pixels> {
        self.bounds.get().origin
    }

    pub(crate) fn world(&self, p: Point<Pixels>) -> P {
        self.cam.to_world(self.origin(), p)
    }

    pub(crate) fn style(&self, cx: &App) -> ExcalidrawStyle {
        (self.style_fn)(cx)
    }

    pub(crate) fn palette(&self, cx: &App) -> Palette {
        let dark = self.style(cx).dark;
        let bg = crate::color::parse(&self.scene.view_background())
            .unwrap_or(Rgba::new(1.0, 1.0, 1.0, 1.0));
        Palette {
            dark,
            canvas: if dark { bg.dark_filter() } else { bg },
        }
    }

    fn tol(&self) -> f64 {
        10.0 / self.cam.zoom
    }

    /// Topmost element under a world point (bound text resolves to its
    /// container; locked elements are skipped).
    pub(crate) fn hit_element(&self, p: P) -> Option<String> {
        let tol = self.tol();
        let mut frames = None;
        for el in self.scene.elements().collect::<Vec<_>>().into_iter().rev() {
            if el.locked() {
                continue;
            }
            if el.kind == "frame" || el.kind == "magicframe" {
                if frames.is_none() && geom::hit(el, p, tol) {
                    frames = Some(el.id.clone());
                }
                continue;
            }
            if geom::hit(el, p, tol) {
                if let Some(c) = el.container_id()
                    && self.scene.get(c).is_some()
                {
                    return Some(c.to_string());
                }
                return Some(el.id.clone());
            }
            // A container's text area counts as the container.
            if el.bound_elements().iter().any(|b| b.kind == "text") {
                let q = geom::to_local(el, p);
                if q[0] >= 0.0 && q[1] >= 0.0 && q[0] <= el.width() && q[1] <= el.height() {
                    let has_text = el
                        .bound_elements()
                        .iter()
                        .filter(|b| b.kind == "text")
                        .filter_map(|b| self.scene.get(&b.id))
                        .any(|t| geom::hit(t, p, 0.0));
                    if has_text {
                        return Some(el.id.clone());
                    }
                }
            }
        }
        frames
    }

    /// Bindable shape under a world point (for arrow ends).
    fn bind_target(&self, p: P, exclude: &str) -> Option<String> {
        let tol = self.tol();
        for el in self.scene.elements().collect::<Vec<_>>().into_iter().rev() {
            if el.id == exclude || !geom::is_bindable(el) || el.kind == "frame" {
                continue;
            }
            let q = geom::to_local(el, p);
            let b = geom::local_bounds(el);
            if q[0] >= b[0] - tol && q[1] >= b[1] - tol && q[0] <= b[2] + tol && q[1] <= b[3] + tol
            {
                return Some(el.id.clone());
            }
        }
        None
    }

    pub(crate) fn selection_list(&self) -> Vec<&Element> {
        self.scene
            .elements()
            .filter(|e| self.selected.contains(&e.id))
            .collect()
    }

    pub(crate) fn selection_bbox(&self) -> Option<[f64; 4]> {
        self.selection_list()
            .iter()
            .map(|e| geom::aabb(e))
            .reduce(geom::union)
    }

    /// The single selected non-linear element, if any.
    pub(crate) fn single(&self) -> Option<&Element> {
        let l = self.selection_list();
        if l.len() == 1 { Some(l[0]) } else { None }
    }

    /// Handle positions in world coordinates for the current selection.
    pub(crate) fn handles(&self) -> Vec<(Handle, P)> {
        let pad = 4.0 / self.cam.zoom;
        let rot = 16.0 / self.cam.zoom;
        let mut out = Vec::new();
        if let Some(el) = self.single() {
            if el.is_linear() || el.locked() {
                return out;
            }
            let b = geom::local_bounds(el);
            let (x0, y0, x1, y1) = (b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad);
            let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            let small =
                (b[2] - b[0]) * self.cam.zoom < 30.0 || (b[3] - b[1]) * self.cam.zoom < 30.0;
            let mut hs = vec![
                (Handle::NW, [x0, y0]),
                (Handle::NE, [x1, y0]),
                (Handle::SE, [x1, y1]),
                (Handle::SW, [x0, y1]),
            ];
            if el.kind == "text" && el.container_id().is_none() {
                hs.extend([(Handle::E, [x1, my]), (Handle::W, [x0, my])]);
            }
            if !small && el.kind != "text" {
                hs.extend([
                    (Handle::N, [mx, y0]),
                    (Handle::S, [mx, y1]),
                    (Handle::E, [x1, my]),
                    (Handle::W, [x0, my]),
                ]);
            }
            if el.kind != "frame" {
                hs.push((Handle::Rotate, [mx, y0 - rot]));
            }
            for (h, p) in hs {
                out.push((h, geom::to_world(el, p)));
            }
        } else if self.selected.len() > 1
            && let Some(b) = self.selection_bbox()
        {
            let (x0, y0, x1, y1) = (b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad);
            out = vec![
                (Handle::NW, [x0, y0]),
                (Handle::NE, [x1, y0]),
                (Handle::SE, [x1, y1]),
                (Handle::SW, [x0, y1]),
                (Handle::Rotate, [(x0 + x1) / 2.0, y0 - rot]),
            ];
        }
        out
    }

    pub(crate) fn hit_handle(&self, p: P) -> Option<Handle> {
        let r = 7.0 / self.cam.zoom;
        self.handles()
            .into_iter()
            .find(|(_, q)| (p[0] - q[0]).abs() <= r && (p[1] - q[1]).abs() <= r)
            .map(|(h, _)| h)
    }

    /// Point handles of a single selected linear element: (index, world, is_midpoint).
    pub(crate) fn point_handles(&self) -> Vec<(usize, P, bool)> {
        let Some(el) = self.single() else {
            return Vec::new();
        };
        if !el.is_linear() || el.locked() || self.multi.is_some() {
            return Vec::new();
        }
        let pts = el.points();
        let mut out: Vec<(usize, P, bool)> = pts
            .iter()
            .enumerate()
            .map(|(i, p)| (i, geom::to_world(el, *p), false))
            .collect();
        for i in 0..pts.len().saturating_sub(1) {
            let a = geom::to_world(el, pts[i]);
            let b = geom::to_world(el, pts[i + 1]);
            if geom::dist(a, b) * self.cam.zoom > 40.0 {
                out.push((i + 1, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], true));
            }
        }
        out
    }

    // ------------------------------------------------------------ pointer

    pub(crate) fn on_down(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.menu_open = false;
        if self.error.is_some() {
            return;
        }
        let p = self.world(ev.position);
        self.pointer = Some(ev.position);
        if ev.button == MouseButton::Middle || self.space_down || self.tool == Tool::Hand {
            self.drag = Some(Drag::Pan {
                start: ev.position,
                cam: self.cam,
            });
            cx.notify();
            return;
        }
        self.context_menu = None;
        if self.eyedropper.is_some() && ev.button == MouseButton::Left {
            let dark = (self.style_fn)(cx).dark;
            self.eyedrop(p, dark, cx);
            return;
        }
        self.picker = None;
        self.hex = None;
        if ev.button == MouseButton::Right {
            if self.editing.is_some() {
                self.commit_text(cx);
            }
            if self.multi.is_some() {
                self.finish_multi(cx);
            }
            self.drag = None;
            match self.hit_element(p) {
                Some(id) if !self.selected.contains(&id) => {
                    self.selected = self.scene.expand_groups(&[id].into());
                }
                Some(_) => {}
                None => self.selected.clear(),
            }
            self.tool = Tool::Selection;
            self.context_menu = Some(ev.position);
            cx.notify();
            return;
        }
        if ev.button != MouseButton::Left {
            return;
        }
        // Clicking anywhere commits an in-progress text edit (unless inside it).
        if let Some(ed) = self.editing.clone() {
            let inside = self
                .scene
                .get(&ed.id)
                .map(|t| geom::hit(t, p, self.tol()))
                .unwrap_or(false);
            if inside {
                self.place_caret(p);
                cx.notify();
                return;
            }
            self.commit_text(cx);
            if self.tool == Tool::Selection {
                cx.notify();
                return;
            }
        }
        if let Some(id) = self.multi.clone() {
            self.multi_click(&id, p, cx);
            return;
        }
        match self.tool {
            Tool::Hand => {}
            Tool::Selection => self.selection_down(ev, p, window, cx),
            Tool::Eraser => {
                self.erasing.clear();
                self.erase_at(p);
                self.drag = Some(Drag::Erase);
            }
            Tool::Text => {
                if let Some(id) = self.hit_element(p)
                    && self.start_text_edit_on(&id, p, cx)
                {
                    return;
                }
                self.create_text_at(p, None, cx);
            }
            Tool::Image => self.prompt_image(window, cx),
            Tool::Arrow | Tool::Line => {
                let p = self.grid_snap(p, &ev.modifiers);
                let kind = if self.tool == Tool::Arrow {
                    "arrow"
                } else {
                    "line"
                };
                let mut el = Scene::new_element(kind, p[0], p[1], &self.defaults);
                el.set_points(&[[0.0, 0.0], [0.0, 0.0]]);
                let id = el.id.clone();
                self.scene.add(el);
                let start_bind = if kind == "arrow" {
                    self.bind_target(p, &id)
                } else {
                    None
                };
                self.selected = [id.clone()].into();
                self.drag = Some(Drag::Linear {
                    id,
                    start: p,
                    moved: false,
                    start_bind,
                });
            }
            Tool::Freedraw => {
                let el = Scene::new_element("freedraw", p[0], p[1], &self.defaults);
                let id = el.id.clone();
                self.scene.add(el);
                self.selected.clear();
                self.drag = Some(Drag::Freedraw { id });
            }
            Tool::Rectangle | Tool::Diamond | Tool::Ellipse | Tool::Frame => {
                let p = self.grid_snap(p, &ev.modifiers);
                let kind = self.tool.element_kind().unwrap();
                let mut d = self.defaults.clone();
                if kind == "frame" {
                    d.stroke_color = "#bbb".into();
                    d.background_color = "transparent".into();
                    d.round = false;
                    d.stroke_width = 2.0;
                    d.roughness = 0.0;
                }
                let el = Scene::new_element(kind, p[0], p[1], &d);
                let id = el.id.clone();
                if kind == "frame" {
                    // Frames go below everything they will contain.
                    self.scene.add(el);
                } else {
                    self.scene.add(el);
                }
                self.selected = [id.clone()].into();
                self.drag = Some(Drag::Create { id, start: p });
            }
        }
        cx.notify();
    }

    fn selection_down(
        &mut self,
        ev: &MouseDownEvent,
        p: P,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shift = ev.modifiers.shift;
        // Double-click: text editing.
        if ev.click_count >= 2 {
            match self.hit_element(p) {
                Some(id) => {
                    if self.start_text_edit_on(&id, p, cx) {
                        return;
                    }
                }
                None => {
                    self.create_text_at(p, None, cx);
                    return;
                }
            }
        }
        // Linear point handles.
        for (i, q, mid) in self.point_handles() {
            if geom::dist(p, q) <= 8.0 / self.cam.zoom {
                let id = self.single().unwrap().id.clone();
                if mid {
                    let el = self.scene.get_mut(&id).unwrap();
                    let mut pts = el.points();
                    let local = geom::to_local(el, q);
                    pts.insert(i, local);
                    el.set_points(&pts);
                }
                self.drag = Some(Drag::Point { id, index: i });
                return;
            }
        }
        if let Some(h) = self.hit_handle(p) {
            let orig: HashMap<String, Element> = self
                .selection_list()
                .into_iter()
                .map(|e| (e.id.clone(), e.clone()))
                .collect();
            // Bound text follows its container.
            let bbox = self.selection_bbox().unwrap_or([0.0; 4]);
            if h == Handle::Rotate {
                let center = match self.single() {
                    Some(e) => geom::center(e),
                    None => [(bbox[0] + bbox[2]) / 2.0, (bbox[1] + bbox[3]) / 2.0],
                };
                let start_angle = (p[1] - center[1]).atan2(p[0] - center[0]);
                self.drag = Some(Drag::Rotate {
                    center,
                    start_angle,
                    orig,
                });
            } else {
                let single = self.single().map(|e| e.id.clone());
                self.drag = Some(Drag::Resize {
                    handle: h,
                    orig,
                    bbox,
                    single,
                });
            }
            return;
        }
        let hit = self.hit_element(p);
        match hit {
            Some(id) => {
                let group = self.scene.expand_groups(&[id.clone()].into());
                if shift {
                    if self.selected.contains(&id) {
                        for g in &group {
                            self.selected.remove(g);
                        }
                        return;
                    }
                    self.selected.extend(group);
                } else if !self.selected.contains(&id) {
                    self.selected = group;
                }
                if ev.modifiers.alt {
                    self.duplicate_selection(0.0, cx);
                }
                self.begin_move(p);
            }
            None => {
                let inside_sel = self.selected.len() > 1
                    && self
                        .selection_bbox()
                        .map(|b| p[0] >= b[0] && p[0] <= b[2] && p[1] >= b[1] && p[1] <= b[3])
                        .unwrap_or(false);
                if inside_sel && !shift {
                    self.begin_move(p);
                } else {
                    if !shift {
                        self.selected.clear();
                    }
                    self.drag = Some(Drag::Marquee {
                        start: p,
                        current: p,
                    });
                }
            }
        }
    }

    fn begin_move(&mut self, p: P) {
        let mut ids: HashSet<String> = self.selected.clone();
        // Bound text and frame children move along.
        for el in self.scene.elements() {
            if let Some(c) = el.container_id()
                && ids.contains(c)
            {
                ids.insert(el.id.clone());
            }
        }
        let frames: HashSet<String> = ids
            .iter()
            .filter(|id| {
                self.scene
                    .get(id)
                    .map(|e| e.kind == "frame")
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        for el in self.scene.elements() {
            if el
                .f
                .frame_id
                .get()
                .map(|f| frames.contains(f))
                .unwrap_or(false)
            {
                ids.insert(el.id.clone());
            }
        }
        for el in self.scene.elements() {
            if let Some(c) = el.container_id()
                && ids.contains(c)
            {
                ids.insert(el.id.clone());
            }
        }
        let orig = ids
            .iter()
            .filter_map(|id| self.scene.get(id).map(|e| (id.clone(), e.clone())))
            .collect();
        self.drag = Some(Drag::Move {
            start: p,
            ids: ids.into_iter().collect(),
            orig,
            moved: false,
        });
    }

    pub(crate) fn on_move(
        &mut self,
        ev: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prev = self.pointer;
        self.pointer = Some(ev.position);
        let p = self.world(ev.position);
        let Some(drag) = self.drag.clone() else {
            if let Some(id) = self.multi.clone() {
                self.set_last_point(&id, p, ev.modifiers.shift);
                cx.notify();
            } else if prev != self.pointer
                && (self.tool == Tool::Eraser || !self.selected.is_empty())
            {
                cx.notify();
            }
            return;
        };
        match drag {
            Drag::Pan { start, cam } => {
                let dx = f64::from(f32::from(ev.position.x - start.x));
                let dy = f64::from(f32::from(ev.position.y - start.y));
                self.cam.scroll_x = cam.scroll_x + dx / self.cam.zoom;
                self.cam.scroll_y = cam.scroll_y + dy / self.cam.zoom;
            }
            Drag::Marquee { start, .. } => {
                self.drag = Some(Drag::Marquee { start, current: p });
                let r = [
                    start[0].min(p[0]),
                    start[1].min(p[1]),
                    start[0].max(p[0]),
                    start[1].max(p[1]),
                ];
                let ids: HashSet<String> = self
                    .scene
                    .elements()
                    .filter(|e| {
                        !e.locked() && e.container_id().is_none() && geom::inside_rect(e, r)
                    })
                    .map(|e| e.id.clone())
                    .collect();
                self.selected = self.scene.expand_groups(&ids);
            }
            Drag::Move {
                start, ids, orig, ..
            } => {
                let (mut dx, mut dy) = (p[0] - start[0], p[1] - start[1]);
                if ev.modifiers.shift {
                    if dx.abs() > dy.abs() {
                        dy = 0.0
                    } else {
                        dx = 0.0
                    }
                }
                self.guides.clear();
                let bbox = ids
                    .iter()
                    .filter_map(|id| orig.get(id))
                    .filter(|e| e.container_id().is_none())
                    .map(geom::aabb)
                    .reduce(|a, b| {
                        [
                            a[0].min(b[0]),
                            a[1].min(b[1]),
                            a[2].max(b[2]),
                            a[3].max(b[3]),
                        ]
                    });
                if let Some(bb) = bbox {
                    let moved_bb = [bb[0] + dx, bb[1] + dy, bb[2] + dx, bb[3] + dy];
                    if self.scene.grid_enabled() && !(ev.modifiers.platform || ev.modifiers.control)
                    {
                        let s = crate::snap::snap_to_grid(
                            [moved_bb[0], moved_bb[1]],
                            self.scene.grid_size(),
                        );
                        dx += s[0] - moved_bb[0];
                        dy += s[1] - moved_bb[1];
                    } else if self.object_snap_on(&ev.modifiers) {
                        let exclude: HashSet<String> = ids.iter().cloned().collect();
                        let targets = self.snap_targets(&exclude);
                        let (off, guides) =
                            crate::snap::snap_box(moved_bb, &targets, 8.0 / self.cam.zoom);
                        dx += off[0];
                        dy += off[1];
                        self.guides = guides;
                    }
                }
                for id in &ids {
                    if let (Some(o), Some(el)) = (orig.get(id), self.scene.get_mut(id)) {
                        el.f.x.set(Num(o.x() + dx));
                        el.f.y.set(Num(o.y() + dy));
                    }
                }
                let set: HashSet<String> = ids.iter().cloned().collect();
                self.scene.update_bound_arrows(&set);
                self.drag = Some(Drag::Move {
                    start,
                    ids,
                    orig,
                    moved: true,
                });
            }
            Drag::Resize {
                handle,
                orig,
                bbox,
                single,
            } => {
                self.resize(handle, &orig, bbox, single.as_deref(), p, ev.modifiers);
            }
            Drag::Rotate {
                center,
                start_angle,
                orig,
            } => {
                let mut delta = (p[1] - center[1]).atan2(p[0] - center[0]) - start_angle;
                let snap = ev.modifiers.shift;
                for (id, o) in &orig {
                    let mut a = o.angle() + delta;
                    if snap && orig.len() == 1 {
                        let step = std::f64::consts::PI / 12.0;
                        a = (a / step).round() * step;
                        delta = a - o.angle();
                    }
                    let a = a.rem_euclid(std::f64::consts::TAU);
                    let c0 = geom::center(o);
                    let c1 = geom::rotate(c0, center, delta);
                    if let Some(el) = self.scene.get_mut(id) {
                        el.f.angle.set(Num(a));
                        el.f.x.set(Num(o.x() + c1[0] - c0[0]));
                        el.f.y.set(Num(o.y() + c1[1] - c0[1]));
                    }
                }
                let ids: HashSet<String> = orig.keys().cloned().collect();
                for c in self.scene.bound_containers_of(&ids) {
                    self.scene.layout_bound_text(&c);
                }
                self.scene.update_bound_arrows(&ids);
            }
            Drag::Create { id, start } => {
                let p = self.grid_snap(p, &ev.modifiers);
                let (mut x0, mut y0) = (start[0], start[1]);
                let (mut w, mut h) = (p[0] - start[0], p[1] - start[1]);
                if ev.modifiers.shift {
                    let s = w.abs().max(h.abs());
                    w = s * w.signum();
                    h = s * h.signum();
                }
                if ev.modifiers.alt {
                    x0 -= w;
                    y0 -= h;
                    w *= 2.0;
                    h *= 2.0;
                }
                if let Some(el) = self.scene.get_mut(&id) {
                    el.f.x.set(Num(x0.min(x0 + w)));
                    el.f.y.set(Num(y0.min(y0 + h)));
                    el.f.width.set(Num(w.abs()));
                    el.f.height.set(Num(h.abs()));
                }
            }
            Drag::Linear {
                id,
                start,
                moved,
                start_bind,
            } => {
                let p = self.grid_snap(p, &ev.modifiers);
                let moved = moved || geom::dist(start, p) * self.cam.zoom > 5.0;
                self.set_last_point(&id, p, ev.modifiers.shift);
                self.bind_hint = if self
                    .scene
                    .get(&id)
                    .map(|e| e.kind == "arrow")
                    .unwrap_or(false)
                {
                    self.bind_target(p, &id)
                } else {
                    None
                };
                // Elbow arrows route orthogonally around their (prospective) targets.
                if self
                    .scene
                    .get(&id)
                    .is_some_and(|e| e.f.elbowed.get().copied().unwrap_or(false))
                {
                    let pts = self.scene.elbow_route(
                        start,
                        start_bind.as_deref(),
                        p,
                        self.bind_hint.as_deref(),
                    );
                    if let Some(el) = self.scene.get_mut(&id) {
                        crate::scene::set_world_points(el, &pts);
                    }
                }
                self.drag = Some(Drag::Linear {
                    id,
                    start,
                    moved,
                    start_bind,
                });
            }
            Drag::Freedraw { id } => {
                if let Some(el) = self.scene.get_mut(&id) {
                    let mut pts = el.points();
                    let q = [p[0] - el.x(), p[1] - el.y()];
                    if pts
                        .last()
                        .map(|l| geom::dist(*l, q) * self.cam.zoom > 0.75)
                        .unwrap_or(true)
                    {
                        pts.push(q);
                        el.set_points(&pts);
                        let b = geom::local_bounds(el);
                        el.f.width.set(Num(b[2] - b[0]));
                        el.f.height.set(Num(b[3] - b[1]));
                    }
                }
            }
            Drag::Point { id, index } => {
                if let Some(el) = self.scene.get(&id).cloned() {
                    let mut world: Vec<P> = el
                        .points()
                        .iter()
                        .map(|q| geom::to_world(&el, *q))
                        .collect();
                    let p = self.grid_snap(p, &ev.modifiers);
                    if index < world.len() {
                        world[index] = p;
                        let el = self.scene.get_mut(&id).unwrap();
                        scene::set_world_points(el, &world);
                        self.scene.layout_bound_text(&id);
                    }
                    let last = world.len() - 1;
                    self.bind_hint = if el.kind == "arrow" && (index == 0 || index == last) {
                        self.bind_target(p, &id)
                    } else {
                        None
                    };
                }
            }
            Drag::Erase => self.erase_at(p),
        }
        cx.notify();
    }

    pub(crate) fn on_up(&mut self, ev: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.guides.clear();
        let Some(drag) = self.drag.take() else { return };
        let p = self.world(ev.position);
        self.bind_hint = None;
        match drag {
            Drag::Pan { .. } | Drag::Marquee { .. } => {}
            Drag::Move { ids, moved, .. } => {
                if moved {
                    // Arrows moved without their targets come unbound.
                    let set: HashSet<String> = ids.iter().cloned().collect();
                    for id in &ids {
                        let Some(el) = self.scene.get(id) else {
                            continue;
                        };
                        if el.kind != "arrow" {
                            continue;
                        }
                        let ends = [
                            (true, el.f.start_binding.get().map(|b| b.element_id.clone())),
                            (false, el.f.end_binding.get().map(|b| b.element_id.clone())),
                        ];
                        for (start, t) in ends {
                            if let Some(t) = t
                                && !set.contains(&t)
                            {
                                self.scene.bind(id, start, None);
                            }
                        }
                    }
                    self.update_frames(&set);
                    self.commit(cx);
                }
            }
            Drag::Resize { .. } | Drag::Rotate { .. } => self.commit(cx),
            Drag::Create { id, .. } => {
                let tiny = self
                    .scene
                    .get(&id)
                    .map(|e| e.width() * self.cam.zoom < 2.0 && e.height() * self.cam.zoom < 2.0)
                    .unwrap_or(true);
                if tiny {
                    self.scene.remove(&[id.clone()].into());
                    self.selected.clear();
                } else {
                    if self
                        .scene
                        .get(&id)
                        .map(|e| e.kind == "frame")
                        .unwrap_or(false)
                    {
                        self.adopt_into_frame(&id);
                    } else {
                        self.update_frames(&[id.clone()].into());
                    }
                    self.commit(cx);
                    if !self.tool_locked {
                        self.tool = Tool::Selection;
                    }
                }
            }
            Drag::Linear {
                id,
                moved,
                start_bind,
                ..
            } => {
                if moved {
                    self.finish_linear(&id, start_bind, Some(p), cx);
                } else {
                    // Click without drag: multi-point mode.
                    if let Some(el) = self.scene.get_mut(&id) {
                        el.set_points(&[[0.0, 0.0], [0.0, 0.0]]);
                        if let Some(sb) = start_bind {
                            el.f.extra
                                .insert("__startBind".into(), serde_json::Value::from(sb));
                        }
                    }
                    self.multi = Some(id);
                }
            }
            Drag::Freedraw { id } => {
                self.update_frames(&[id].into());
                self.commit(cx);
            }
            Drag::Point { id, index } => {
                if let Some(el) = self.scene.get(&id).cloned() {
                    let n = el.points().len();
                    if el.kind == "arrow" && (index == 0 || index + 1 == n) {
                        let t = self.bind_target(p, &id);
                        self.scene.bind(&id, index == 0, t.as_deref());
                        self.scene.update_bound_arrows(&t.into_iter().collect());
                    }
                }
                self.commit(cx);
            }
            Drag::Erase => {
                let ids = std::mem::take(&mut self.erasing);
                if !ids.is_empty() {
                    self.scene.remove(&ids);
                    self.selected.retain(|i| !ids.contains(i));
                    self.commit(cx);
                }
            }
        }
        let _ = window;
        cx.notify();
    }

    fn set_last_point(&mut self, id: &str, p: P, shift: bool) {
        let Some(el) = self.scene.get(id).cloned() else {
            return;
        };
        let mut world: Vec<P> = el
            .points()
            .iter()
            .map(|q| geom::to_world(&el, *q))
            .collect();
        if world.len() < 2 {
            world.push(p);
        }
        let n = world.len();
        let mut q = p;
        if shift {
            let a = world[n - 2];
            let ang = (p[1] - a[1]).atan2(p[0] - a[0]);
            let step = std::f64::consts::PI / 12.0;
            let snapped = (ang / step).round() * step;
            let d = geom::dist(a, p);
            q = [a[0] + d * snapped.cos(), a[1] + d * snapped.sin()];
        }
        world[n - 1] = q;
        let el = self.scene.get_mut(id).unwrap();
        // Keep points[0] at the element origin while drawing.
        scene::set_world_points(el, &world);
    }

    fn multi_click(&mut self, id: &str, p: P, cx: &mut Context<Self>) {
        let Some(el) = self.scene.get(id).cloned() else {
            self.multi = None;
            return;
        };
        let pts = el.points();
        let n = pts.len();
        let last_committed = geom::to_world(&el, pts[n.saturating_sub(2)]);
        if geom::dist(last_committed, p) * self.cam.zoom < 10.0 {
            self.finish_multi(cx);
            return;
        }
        let mut world: Vec<P> = pts.iter().map(|q| geom::to_world(&el, *q)).collect();
        world[n - 1] = p;
        world.push(p);
        let elm = self.scene.get_mut(id).unwrap();
        scene::set_world_points(elm, &world);
        cx.notify();
    }

    /// Ends multi-point creation (drops the trailing preview point).
    pub(crate) fn finish_multi(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.multi.take() else { return };
        let Some(el) = self.scene.get(&id).cloned() else {
            return;
        };
        let mut world: Vec<P> = el
            .points()
            .iter()
            .map(|q| geom::to_world(&el, *q))
            .collect();
        world.pop();
        let start_bind =
            el.f.extra
                .get("__startBind")
                .and_then(|v| v.as_str())
                .map(String::from);
        if let Some(e) = self.scene.get_mut(&id) {
            e.f.extra.remove("__startBind");
        }
        if world.len() < 2 {
            self.scene.remove(&[id].into());
            self.selected.clear();
            cx.notify();
            return;
        }
        let end = *world.last().unwrap();
        let e = self.scene.get_mut(&id).unwrap();
        scene::set_world_points(e, &world);
        self.finish_linear(&id, start_bind, Some(end), cx);
    }

    fn finish_linear(
        &mut self,
        id: &str,
        start_bind: Option<String>,
        end: Option<P>,
        cx: &mut Context<Self>,
    ) {
        let is_arrow = self
            .scene
            .get(id)
            .map(|e| e.kind == "arrow")
            .unwrap_or(false);
        if is_arrow {
            let end_t = end.and_then(|p| self.bind_target(p, id));
            if let Some(s) = &start_bind {
                self.scene.bind(id, true, Some(s));
            }
            if let Some(t) = &end_t {
                self.scene.bind(id, false, Some(t));
            }
            let targets: HashSet<String> = start_bind.into_iter().chain(end_t).collect();
            self.scene.update_bound_arrows(&targets);
        }
        self.update_frames(&[id.to_string()].into());
        self.selected = [id.to_string()].into();
        self.commit(cx);
        if !self.tool_locked {
            self.tool = Tool::Selection;
        }
        cx.notify();
    }

    fn erase_at(&mut self, p: P) {
        let tol = self.tol();
        let hits: Vec<String> = self
            .scene
            .elements()
            .filter(|e| !e.locked() && geom::hit(e, p, tol))
            .map(|e| {
                e.container_id()
                    .map(String::from)
                    .unwrap_or_else(|| e.id.clone())
            })
            .collect();
        if hits.is_empty() {
            return;
        }
        let set = self.scene.expand_groups(&hits.into_iter().collect());
        self.erasing.extend(set);
    }

    fn resize(
        &mut self,
        handle: Handle,
        orig: &HashMap<String, Element>,
        bbox: [f64; 4],
        single: Option<&str>,
        p: P,
        m: Modifiers,
    ) {
        if let Some(id) = single {
            let Some(o) = orig.get(id) else { return };
            let b = geom::local_bounds(o);
            let q = geom::to_local(o, p);
            let (sx, sy) = handle.sides();
            let (mut x0, mut y0, mut x1, mut y1) = (b[0], b[1], b[2], b[3]);
            let (w0, h0) = ((b[2] - b[0]).max(1e-6), (b[3] - b[1]).max(1e-6));
            if sx < 0 {
                x0 = q[0]
            }
            if sx > 0 {
                x1 = q[0]
            }
            if sy < 0 {
                y0 = q[1]
            }
            if sy > 0 {
                y1 = q[1]
            }
            if o.kind == "text" && sy == 0 && sx != 0 {
                // Side handles fix the text width (autoResize off) and re-wrap.
                let original =
                    o.f.original_text
                        .get()
                        .cloned()
                        .unwrap_or_else(|| o.text().to_string());
                let min_w = o.font_size() * 0.6;
                let (l, r) = if sx < 0 {
                    (q[0].min(b[2] - min_w), b[2])
                } else {
                    (b[0], q[0].max(b[0] + min_w))
                };
                let w = r - l;
                let wrapped = text::wrap(&original, o.font_family(), o.font_size(), w);
                let (_, h) =
                    text::measure(&wrapped, o.font_family(), o.font_size(), o.line_height());
                let mut el = o.clone();
                let nw = geom::to_world(o, [l, b[1]]);
                let ow = geom::to_world(o, [b[0], b[1]]);
                el.f.x.set(Num(o.x() + nw[0] - ow[0]));
                el.f.y.set(Num(o.y() + nw[1] - ow[1]));
                el.f.width.set(Num(w));
                el.f.height.set(Num(h));
                el.f.text.set(wrapped);
                el.f.auto_resize.set(false);
                if let Some(t) = self.scene.get_mut(id) {
                    *t = el;
                }
                return;
            }
            let keep_aspect = (o.kind == "image" || o.kind == "text") != m.shift
                && handle.is_corner()
                || (o.kind == "text");
            if keep_aspect {
                let s = ((x1 - x0) / w0).abs().max(((y1 - y0) / h0).abs());
                let (nw, nh) = (w0 * s, h0 * s);
                if sx < 0 {
                    x0 = x1 - nw
                } else {
                    x1 = x0 + nw
                }
                if sy < 0 {
                    y0 = y1 - nh
                } else {
                    y1 = y0 + nh
                }
                if sy == 0 {
                    let cy = (b[1] + b[3]) / 2.0;
                    y0 = cy - nh / 2.0;
                    y1 = cy + nh / 2.0;
                }
                if sx == 0 {
                    let cx = (b[0] + b[2]) / 2.0;
                    x0 = cx - nw / 2.0;
                    x1 = cx + nw / 2.0;
                }
            }
            if m.alt {
                let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
                if sx != 0 {
                    let hw = (if sx < 0 { cx - x0 } else { x1 - cx }).abs();
                    x0 = cx - hw;
                    x1 = cx + hw;
                }
                if sy != 0 {
                    let hh = (if sy < 0 { cy - y0 } else { y1 - cy }).abs();
                    y0 = cy - hh;
                    y1 = cy + hh;
                }
            }
            let (nx0, nx1) = (x0.min(x1), x0.max(x1));
            let (ny0, ny1) = (y0.min(y1), y0.max(y1));
            let (nw, nh) = ((nx1 - nx0).max(1.0), (ny1 - ny0).max(1.0));
            let wc = geom::to_world(o, [(nx0 + nx1) / 2.0, (ny0 + ny1) / 2.0]);
            let mut el = o.clone();
            apply_scale(&mut el, o, nw / w0, nh / h0);
            let lc = geom::local_center(&el);
            el.f.x.set(Num(wc[0] - lc[0]));
            el.f.y.set(Num(wc[1] - lc[1]));
            if let Some(t) = self.scene.get_mut(id) {
                *t = el;
            }
            let ids: HashSet<String> = [id.to_string()].into();
            self.scene.layout_bound_text(id);
            self.scene.update_bound_arrows(&ids);
        } else {
            // Multi-element: proportional scale of the group box.
            let (w0, h0) = ((bbox[2] - bbox[0]).max(1e-6), (bbox[3] - bbox[1]).max(1e-6));
            let (sx, sy) = handle.sides();
            let anchor = [
                if sx < 0 { bbox[2] } else { bbox[0] },
                if sy < 0 { bbox[3] } else { bbox[1] },
            ];
            let s = (((p[0] - anchor[0]).abs() / w0).max((p[1] - anchor[1]).abs() / h0)).max(0.05);
            for (id, o) in orig {
                let c = geom::center(o);
                let nc = [
                    anchor[0] + (c[0] - anchor[0]) * s,
                    anchor[1] + (c[1] - anchor[1]) * s,
                ];
                let mut el = o.clone();
                apply_scale(&mut el, o, s, s);
                let lc = geom::local_center(&el);
                el.f.x.set(Num(nc[0] - lc[0]));
                el.f.y.set(Num(nc[1] - lc[1]));
                if let Some(t) = self.scene.get_mut(id) {
                    *t = el;
                }
            }
            let ids: HashSet<String> = orig.keys().cloned().collect();
            for c in self.scene.bound_containers_of(&ids) {
                self.scene.layout_bound_text(&c);
            }
            self.scene.update_bound_arrows(&ids);
        }
    }

    /// Sets `frameId` of `ids` to the innermost frame fully containing them.
    fn update_frames(&mut self, ids: &HashSet<String>) {
        let frames: Vec<Element> = self
            .scene
            .elements()
            .filter(|e| e.kind == "frame")
            .cloned()
            .collect();
        if frames.is_empty() {
            return;
        }
        for id in ids {
            let Some(el) = self.scene.get(id) else {
                continue;
            };
            if el.kind == "frame" || el.container_id().is_some() {
                continue;
            }
            let target = frames
                .iter()
                .rev()
                .find(|f| geom::inside_rect(el, geom::aabb(f)))
                .map(|f| f.id.clone());
            let cur = el.f.frame_id.get().cloned();
            if cur != target {
                let bound: Vec<String> = el
                    .bound_elements()
                    .iter()
                    .filter(|b| b.kind == "text")
                    .map(|b| b.id.clone())
                    .collect();
                for x in std::iter::once(id.clone()).chain(bound) {
                    if let Some(e) = self.scene.get_mut(&x) {
                        e.f.frame_id.set_opt(target.clone());
                    }
                }
            }
        }
    }

    fn adopt_into_frame(&mut self, frame: &str) {
        let Some(f) = self.scene.get(frame).cloned() else {
            return;
        };
        let r = geom::aabb(&f);
        let ids: Vec<String> = self
            .scene
            .elements()
            .filter(|e| e.id != frame && e.kind != "frame" && geom::inside_rect(e, r))
            .map(|e| e.id.clone())
            .collect();
        for id in ids {
            if let Some(e) = self.scene.get_mut(&id) {
                e.f.frame_id.set(frame.to_string());
            }
        }
        // Keep the frame below its children.
        let mut set = HashSet::new();
        set.insert(frame.to_string());
        self.scene.reorder(&set, false, true);
    }

    // ------------------------------------------------------------ text

    /// Starts editing `id` (a text element or a container's bound text).
    fn start_text_edit_on(&mut self, id: &str, p: P, cx: &mut Context<Self>) -> bool {
        let Some(el) = self.scene.get(id).cloned() else {
            return false;
        };
        if el.kind == "text" {
            self.selected = [id.to_string()].into();
            let caret =
                el.f.original_text
                    .get()
                    .map(|s| s.chars().count())
                    .unwrap_or(el.text().chars().count());
            self.editing = Some(TextEdit {
                id: id.to_string(),
                caret,
            });
            self.place_caret(p);
            cx.notify();
            return true;
        }
        if matches!(
            el.kind.as_str(),
            "rectangle" | "diamond" | "ellipse" | "arrow" | "line"
        ) {
            if let Some(t) = el.bound_elements().iter().find(|b| b.kind == "text")
                && let Some(te) = self.scene.get(&t.id)
            {
                let n =
                    te.f.original_text
                        .get()
                        .map(|s| s.chars().count())
                        .unwrap_or(0);
                self.editing = Some(TextEdit {
                    id: t.id.clone(),
                    caret: n,
                });
                self.selected = [t.id.clone()].into();
                cx.notify();
                return true;
            }
            if el.kind == "line" {
                return false;
            }
            // Arrows get a label at their midpoint (laid out by the scene).
            self.create_text_at(geom::center(&el), Some(id.to_string()), cx);
            return true;
        }
        false
    }

    pub(crate) fn create_text_at(
        &mut self,
        p: P,
        container: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let mut d = self.defaults.clone();
        if container.is_some() {
            d.text_align = "center".into();
        }
        let mut t = Scene::new_element("text", p[0], p[1], &d);
        let lh = t.font_size() * t.line_height();
        t.f.y.set(Num(p[1] - lh / 2.0));
        t.f.height.set(Num(lh));
        if let Some(c) = &container {
            t.f.container_id.set(c.clone());
            t.f.vertical_align.set("middle".into());
            if let Some(ce) = self.scene.get(c) {
                t.f.frame_id = ce.f.frame_id.clone();
            }
        }
        let id = t.id.clone();
        self.scene.add(t);
        if let Some(c) = &container {
            if let Some(ce) = self.scene.get_mut(c) {
                let mut b = ce.bound_elements().to_vec();
                b.push(BoundElement {
                    id: id.clone(),
                    kind: "text".into(),
                    extra: Default::default(),
                });
                ce.f.bound_elements.set(b);
            }
            self.scene.layout_bound_text(c);
        }
        self.editing = Some(TextEdit {
            id: id.clone(),
            caret: 0,
        });
        self.selected = [id].into();
        cx.notify();
    }

    pub(crate) fn edit_string(&self) -> Option<String> {
        let ed = self.editing.as_ref()?;
        let el = self.scene.get(&ed.id)?;
        Some(
            el.f.original_text
                .get()
                .cloned()
                .unwrap_or_else(|| el.text().to_string()),
        )
    }

    pub(crate) fn set_edit_string(&mut self, s: String, caret: usize) {
        let Some(ed) = self.editing.as_mut() else {
            return;
        };
        ed.caret = caret;
        let id = ed.id.clone();
        let Some(el) = self.scene.get_mut(&id) else {
            return;
        };
        el.f.original_text.set(s.clone());
        let container = el.container_id().map(String::from);
        match container {
            Some(c) => {
                el.f.text.set(s);
                self.scene.layout_bound_text(&c);
            }
            None if el.f.auto_resize.get() == Some(&false) => {
                // Fixed width: wrap, grow only vertically.
                let wrapped = text::wrap(&s, el.font_family(), el.font_size(), el.width());
                let (_, h) =
                    text::measure(&wrapped, el.font_family(), el.font_size(), el.line_height());
                el.f.text.set(wrapped);
                el.f.height.set(Num(h));
            }
            None => {
                let (w, h) = text::measure(&s, el.font_family(), el.font_size(), el.line_height());
                // Keep the alignment anchor fixed while typing.
                let old_w = el.width();
                let dx = match el.text_align() {
                    "center" => (old_w - w) / 2.0,
                    "right" => old_w - w,
                    _ => 0.0,
                };
                el.f.x.set(Num(el.x() + dx));
                el.f.text.set(s);
                el.f.width.set(Num(w));
                el.f.height.set(Num(h));
            }
        }
    }

    /// Caret (line, x offset, y offset) in element-local coordinates.
    pub(crate) fn caret_local(&self) -> Option<(f64, f64, f64)> {
        let ed = self.editing.as_ref()?;
        let el = self.scene.get(&ed.id)?;
        let original = self.edit_string()?;
        let display = el.text().to_string();
        let (line, col) = map_caret(&original, &display, ed.caret);
        let lines: Vec<&str> = display.split('\n').collect();
        let l = lines.get(line).copied().unwrap_or("");
        let size = el.font_size();
        let fam = el.font_family();
        let lw = text::line_width(l, fam, size);
        let stops = text::caret_stops(l, fam, size);
        let x0 = match el.text_align() {
            "center" => (el.width() - lw) / 2.0,
            "right" => el.width() - lw,
            _ => 0.0,
        };
        let lh = size * el.line_height();
        Some((
            x0 + stops.get(col).copied().unwrap_or(lw),
            line as f64 * lh,
            lh,
        ))
    }

    fn place_caret(&mut self, p: P) {
        let Some(ed) = self.editing.clone() else {
            return;
        };
        let Some(el) = self.scene.get(&ed.id).cloned() else {
            return;
        };
        let Some(original) = self.edit_string() else {
            return;
        };
        let q = geom::to_local(&el, p);
        let lh = el.font_size() * el.line_height();
        let display = el.text().to_string();
        let lines: Vec<&str> = display.split('\n').collect();
        let li = ((q[1] / lh).floor().max(0.0) as usize).min(lines.len().saturating_sub(1));
        let l = lines[li];
        let lw = text::line_width(l, el.font_family(), el.font_size());
        let x0 = match el.text_align() {
            "center" => (el.width() - lw) / 2.0,
            "right" => el.width() - lw,
            _ => 0.0,
        };
        let stops = text::caret_stops(l, el.font_family(), el.font_size());
        let col = stops
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 + x0 - q[0]).abs().total_cmp(&(b.1 + x0 - q[0]).abs()))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let caret = unmap_caret(&original, &display, li, col);
        if let Some(e) = self.editing.as_mut() {
            e.caret = caret;
        }
    }

    /// Finishes text editing: empty text is removed, otherwise committed.
    pub(crate) fn commit_text(&mut self, cx: &mut Context<Self>) {
        self.marked = None;
        let Some(ed) = self.editing.take() else {
            return;
        };
        let Some(el) = self.scene.get(&ed.id).cloned() else {
            return;
        };
        let s =
            el.f.original_text
                .get()
                .cloned()
                .unwrap_or_else(|| el.text().to_string());
        if s.trim().is_empty() {
            self.scene.remove(&[ed.id.clone()].into());
            if let Some(c) = el.container_id() {
                if let Some(ce) = self.scene.get_mut(c)
                    && let Some(b) = ce.f.bound_elements.get_mut()
                {
                    b.retain(|x| x.id != ed.id);
                }
                self.selected = [c.to_string()].into();
            } else {
                self.selected.clear();
            }
        } else if let Some(c) = el.container_id() {
            self.selected = [c.to_string()].into();
        }
        self.update_frames(&[ed.id].into());
        self.commit(cx);
        if !self.tool_locked {
            self.tool = Tool::Selection;
        }
        cx.notify();
    }

    fn text_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(ed) = self.editing.clone() else {
            return false;
        };
        let Some(s) = self.edit_string() else {
            return false;
        };
        let chars: Vec<char> = s.chars().collect();
        let m = &ks.modifiers;
        let primary = m.platform || m.control;
        let mut caret = ed.caret.min(chars.len());
        let insert = |chars: &[char], caret: usize, ins: &str| -> (String, usize) {
            let mut out: String = chars[..caret].iter().collect();
            out.push_str(ins);
            let n = ins.chars().count();
            out.extend(chars[caret..].iter());
            (out, caret + n)
        };
        match ks.key.as_str() {
            "escape" => {
                self.commit_text(cx);
                return true;
            }
            "enter" if primary => {
                self.commit_text(cx);
                return true;
            }
            "enter" => {
                let (o, c) = insert(&chars, caret, "\n");
                self.set_edit_string(o, c);
            }
            "backspace" => {
                if caret > 0 {
                    let start = if m.alt || primary {
                        let mut i = caret;
                        while i > 0 && chars[i - 1].is_whitespace() {
                            i -= 1;
                        }
                        while i > 0 && !chars[i - 1].is_whitespace() {
                            i -= 1;
                        }
                        if primary {
                            chars[..caret]
                                .iter()
                                .rposition(|c| *c == '\n')
                                .map(|x| x + 1)
                                .unwrap_or(0)
                        } else {
                            i
                        }
                    } else {
                        caret - 1
                    };
                    let mut o: String = chars[..start].iter().collect();
                    o.extend(chars[caret..].iter());
                    self.set_edit_string(o, start);
                }
            }
            "delete" => {
                if caret < chars.len() {
                    let mut o: String = chars[..caret].iter().collect();
                    o.extend(chars[caret + 1..].iter());
                    self.set_edit_string(o, caret);
                }
            }
            "left" => {
                caret = caret.saturating_sub(1);
                self.editing.as_mut().unwrap().caret = caret;
            }
            "right" => {
                caret = (caret + 1).min(chars.len());
                self.editing.as_mut().unwrap().caret = caret;
            }
            "home" => {
                caret = chars[..caret]
                    .iter()
                    .rposition(|c| *c == '\n')
                    .map(|x| x + 1)
                    .unwrap_or(0);
                self.editing.as_mut().unwrap().caret = caret;
            }
            "end" => {
                caret = chars[caret..]
                    .iter()
                    .position(|c| *c == '\n')
                    .map(|x| x + caret)
                    .unwrap_or(chars.len());
                self.editing.as_mut().unwrap().caret = caret;
            }
            "up" | "down" => {
                let line_start = chars[..caret]
                    .iter()
                    .rposition(|c| *c == '\n')
                    .map(|x| x + 1)
                    .unwrap_or(0);
                let col = caret - line_start;
                if ks.key == "up" {
                    if line_start > 0 {
                        let prev_start = chars[..line_start - 1]
                            .iter()
                            .rposition(|c| *c == '\n')
                            .map(|x| x + 1)
                            .unwrap_or(0);
                        caret = (prev_start + col).min(line_start - 1);
                    } else {
                        caret = 0;
                    }
                } else {
                    match chars[caret..].iter().position(|c| *c == '\n') {
                        Some(nl) => {
                            let next_start = caret + nl + 1;
                            let next_end = chars[next_start..]
                                .iter()
                                .position(|c| *c == '\n')
                                .map(|x| x + next_start)
                                .unwrap_or(chars.len());
                            caret = (next_start + col).min(next_end);
                        }
                        None => caret = chars.len(),
                    }
                }
                self.editing.as_mut().unwrap().caret = caret;
            }
            "a" if primary => {
                self.editing.as_mut().unwrap().caret = chars.len();
            }
            "v" if primary => {
                if let Some(t) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    let (o, c) = insert(&chars, caret, &t);
                    self.set_edit_string(o, c);
                }
            }
            "c" | "x" if primary => {
                cx.write_to_clipboard(ClipboardItem::new_string(s.clone()));
            }
            "z" if primary => {}
            "tab" => {
                let (o, c) = insert(&chars, caret, "    ");
                self.set_edit_string(o, c);
            }
            _ => {
                if primary {
                    return true;
                }
                // Printable input goes through the platform input handler
                // (`input.rs`), which also handles IME composition.
                if ks
                    .key_char
                    .as_ref()
                    .is_some_and(|kc| !kc.chars().any(|c| c.is_control()))
                {
                    return false;
                }
            }
        }
        cx.notify();
        true
    }

    // ------------------------------------------------------------ keyboard

    pub(crate) fn on_key_down(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.error.is_some() {
            return;
        }
        let ks = &ev.keystroke;
        if self.editing.is_some() {
            if self.text_key(ks, cx) {
                cx.stop_propagation();
            }
            return;
        }
        let m = &ks.modifiers;
        let primary = m.platform || m.control;
        let key = ks.key.as_str();
        let mut handled = true;
        if self.picker_key(ks, cx) {
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if key == "escape" && self.eyedropper.take().is_some() {
            cx.notify();
            return;
        }
        if key == "escape"
            && (self.context_menu.is_some() || self.dialog.is_some() || self.picker.is_some())
        {
            self.context_menu = None;
            self.dialog = None;
            self.picker = None;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        use crate::actions::{Action, Dialog};
        let action = match (primary, m.shift, m.alt, key) {
            (false, true, false, "h") => Some(Action::FlipH),
            (false, true, false, "v") => Some(Action::FlipV),
            (true, true, false, "l") => Some(Action::Lock),
            (true, _, false, "'") => Some(Action::ToggleGrid),
            (false, false, true, "s") => Some(Action::ToggleObjectSnap),
            (true, false, true, "c") => Some(Action::CopyStyles),
            (true, false, true, "v") => Some(Action::PasteStyles),
            (false, true, true, "c") => Some(Action::CopyPng),
            (true, true, false, "e") => Some(Action::Export),
            (false, _, false, "?") | (false, true, false, "/") => Some(Action::Help),
            _ => None,
        };
        if let Some(a) = action {
            if a == Action::Export {
                self.open_dialog(Dialog::Export);
            } else {
                self.run_action(a, window, cx);
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match (primary, m.shift, key) {
            (true, false, "z") => self.undo(cx),
            (true, true, "z") | (true, _, "y") => self.redo(cx),
            (true, _, "c") => self.copy(cx),
            (true, _, "x") => {
                self.copy(cx);
                self.delete_selection(cx);
            }
            (true, _, "v") => self.paste(window, cx),
            (true, _, "d") => {
                self.duplicate_selection(10.0, cx);
                self.commit(cx);
            }
            (true, _, "a") => {
                self.tool = Tool::Selection;
                self.selected = self
                    .scene
                    .elements()
                    .filter(|e| !e.locked() && e.container_id().is_none())
                    .map(|e| e.id.clone())
                    .collect();
            }
            (true, false, "g") => {
                let s = self.selected.clone();
                self.scene.group(&s);
                self.commit(cx);
            }
            (true, true, "g") => {
                let s = self.selected.clone();
                self.scene.ungroup(&s);
                self.commit(cx);
            }
            (true, shift, "]") | (true, shift, "}") => {
                let s = self.selected.clone();
                self.scene.reorder(&s, true, shift);
                self.commit(cx);
            }
            (true, shift, "[") | (true, shift, "{") => {
                let s = self.selected.clone();
                self.scene.reorder(&s, false, shift);
                self.commit(cx);
            }
            (true, _, "=") | (true, _, "+") => self.zoom_step(1.1, None),
            (true, _, "-") => self.zoom_step(1.0 / 1.1, None),
            (true, _, "0") => self.zoom_step(1.0 / self.cam.zoom, None),
            (false, true, "1") | (false, _, "!") => self.zoom_to_fit(),
            (false, _, "delete") | (false, _, "backspace") => self.delete_selection(cx),
            (false, _, "escape") => {
                if self.multi.is_some() {
                    self.finish_multi(cx);
                } else if self.drag.is_some() {
                    self.drag = None;
                } else {
                    self.selected.clear();
                    self.tool = Tool::Selection;
                }
            }
            (false, _, "enter") => {
                if self.multi.is_some() {
                    self.finish_multi(cx);
                } else if let Some(el) = self.single().cloned() {
                    let c = geom::center(&el);
                    self.start_text_edit_on(&el.id, c, cx);
                }
            }
            (false, shift, "left" | "right" | "up" | "down") => {
                let d = if shift { 10.0 } else { 1.0 };
                let (dx, dy) = match key {
                    "left" => (-d, 0.0),
                    "right" => (d, 0.0),
                    "up" => (0.0, -d),
                    _ => (0.0, d),
                };
                self.nudge(dx, dy, cx);
            }
            (false, _, " ") | (false, _, "space") => self.space_down = true,
            (false, false, k) => match tool_for_key(k) {
                Some(t) => self.set_tool(t, window, cx),
                None if k == "q" => self.tool_locked = !self.tool_locked,
                None => handled = false,
            },
            _ => handled = false,
        }
        if handled {
            cx.stop_propagation();
            cx.notify();
        }
    }

    pub(crate) fn on_key_up(
        &mut self,
        ev: &KeyUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ev.keystroke.key == "space" || ev.keystroke.key == " " {
            self.space_down = false;
            cx.notify();
        }
    }

    fn nudge(&mut self, dx: f64, dy: f64, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            return;
        }
        self.begin_move([0.0, 0.0]);
        if let Some(Drag::Move { ids, orig, .. }) = self.drag.take() {
            for id in &ids {
                if let (Some(o), Some(el)) = (orig.get(id), self.scene.get_mut(id)) {
                    el.f.x.set(Num(o.x() + dx));
                    el.f.y.set(Num(o.y() + dy));
                }
            }
            self.scene.update_bound_arrows(&ids.into_iter().collect());
            self.commit(cx);
        }
    }

    pub(crate) fn delete_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            return;
        }
        let s = std::mem::take(&mut self.selected);
        self.scene.remove(&s);
        self.commit(cx);
    }

    pub(crate) fn duplicate_selection(&mut self, offset: f64, _cx: &mut Context<Self>) {
        let clones = self.scene.clone_elements(&self.selected);
        let mut new_sel = HashSet::new();
        for mut c in clones {
            c.f.x.set(Num(c.x() + offset));
            c.f.y.set(Num(c.y() + offset));
            if c.container_id().is_none() {
                new_sel.insert(c.id.clone());
            }
            self.scene.add(c);
        }
        self.selected = new_sel;
    }

    // ------------------------------------------------------------ view

    pub(crate) fn zoom_step(&mut self, factor: f64, anchor: Option<[f64; 2]>) {
        let b = self.bounds.get();
        let a = anchor.unwrap_or([
            f64::from(f32::from(b.size.width)) / 2.0,
            f64::from(f32::from(b.size.height)) / 2.0,
        ]);
        let z = ((self.cam.zoom * factor) * 100.0).round() / 100.0;
        self.cam.zoom_at(z, a);
    }

    pub(crate) fn zoom_to_fit(&mut self) {
        let Some(bb) = self.scene.elements().map(geom::aabb).reduce(geom::union) else {
            return;
        };
        let b = self.bounds.get();
        let (vw, vh) = (
            f64::from(f32::from(b.size.width)),
            f64::from(f32::from(b.size.height)),
        );
        let (w, h) = ((bb[2] - bb[0]).max(1.0), (bb[3] - bb[1]).max(1.0));
        let z = ((vw - 100.0) / w).min((vh - 160.0) / h).clamp(0.1, 1.0);
        self.cam.zoom = z;
        self.cam.scroll_x = vw / 2.0 / z - (bb[0] + bb[2]) / 2.0;
        self.cam.scroll_y = vh / 2.0 / z - (bb[1] + bb[3]) / 2.0;
    }

    pub(crate) fn on_scroll(
        &mut self,
        ev: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let d = ev.delta.pixel_delta(px(20.0));
        let (dx, dy) = (f64::from(f32::from(d.x)), f64::from(f32::from(d.y)));
        if ev.modifiers.platform || ev.modifiers.control {
            let o = self.origin();
            let a = [
                f64::from(f32::from(ev.position.x - o.x)),
                f64::from(f32::from(ev.position.y - o.y)),
            ];
            let factor = (dy / 200.0).exp();
            let z = self.cam.zoom * factor;
            self.cam.zoom_at(z, a);
        } else if ev.modifiers.shift && dx == 0.0 {
            self.cam.scroll_x += dy / self.cam.zoom;
        } else {
            self.cam.scroll_x += dx / self.cam.zoom;
            self.cam.scroll_y += dy / self.cam.zoom;
        }
        cx.notify();
    }

    pub(crate) fn on_pinch(
        &mut self,
        ev: &PinchEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let o = self.origin();
        let a = [
            f64::from(f32::from(ev.position.x - o.x)),
            f64::from(f32::from(ev.position.y - o.y)),
        ];
        let z = self.cam.zoom * (1.0 + ev.delta as f64);
        self.cam.zoom_at(z, a);
        cx.notify();
    }

    // ------------------------------------------------------------ properties

    /// Applies a property to the selection (or the defaults when nothing
    /// is selected), as one undo step.
    pub(crate) fn apply(
        &mut self,
        f: impl Fn(&mut Element, &mut ItemDefaults),
        cx: &mut Context<Self>,
    ) {
        let mut d = self.defaults.clone();
        let mut ids: HashSet<String> = self.selected.clone();
        for el in self.scene.elements() {
            if let Some(c) = el.container_id()
                && ids.contains(c)
            {
                ids.insert(el.id.clone());
            }
        }
        for id in &ids {
            if let Some(el) = self.scene.get_mut(id) {
                f(el, &mut d);
            }
        }
        f(
            &mut Element {
                id: String::new(),
                kind: String::new(),
                f: Default::default(),
            },
            &mut d,
        );
        self.defaults = d;
        // Text metrics may have changed.
        let texts: Vec<String> = ids
            .iter()
            .filter(|id| {
                self.scene
                    .get(id)
                    .map(|e| e.kind == "text")
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        for t in texts {
            let el = self.scene.get(&t).cloned().unwrap();
            match el.container_id() {
                Some(c) => {
                    let c = c.to_string();
                    self.scene.layout_bound_text(&c);
                }
                None => {
                    let (w, h) = text::measure(
                        el.text(),
                        el.font_family(),
                        el.font_size(),
                        el.line_height(),
                    );
                    let e = self.scene.get_mut(&t).unwrap();
                    e.f.width.set(Num(w));
                    e.f.height.set(Num(h));
                }
            }
        }
        self.commit(cx);
        cx.notify();
    }

    // ------------------------------------------------------------ clipboard

    pub(crate) fn copy(&mut self, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            return;
        }
        let mut ids = self.selected.clone();
        for el in self.scene.elements() {
            if el.container_id().map(|c| ids.contains(c)).unwrap_or(false) {
                ids.insert(el.id.clone());
            }
        }
        let els: Vec<serde_json::Value> = self
            .scene
            .elements()
            .filter(|e| ids.contains(&e.id))
            .filter_map(|e| serde_json::to_value(e).ok())
            .collect();
        let mut files = serde_json::Map::new();
        for e in self.scene.elements().filter(|e| ids.contains(&e.id)) {
            if let Some(fid) = e.f.file_id.get()
                && let Some(f) = self.scene.files.get(fid)
            {
                files.insert(fid.clone(), f.clone());
            }
        }
        let v =
            serde_json::json!({ "type": "excalidraw/clipboard", "elements": els, "files": files });
        cx.write_to_clipboard(ClipboardItem::new_string(
            serde_json::to_string(&v).unwrap_or_default(),
        ));
    }

    pub(crate) fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        for entry in item.entries() {
            if let ClipboardEntry::Image(img) = entry {
                let bytes = img.bytes.clone();
                self.insert_image_bytes(&bytes, None, cx);
                return;
            }
        }
        let Some(text) = item.text() else { return };
        let at = self
            .pointer
            .map(|p| self.world(p))
            .unwrap_or_else(|| self.viewport_center());
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if ty == "excalidraw/clipboard" || ty == "excalidraw" {
                let mut els: Vec<Element> = v
                    .get("elements")
                    .and_then(|e| e.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| serde_json::from_value(x.clone()).ok())
                            .collect()
                    })
                    .unwrap_or_default();
                els.retain(|e| !e.is_deleted());
                if els.is_empty() {
                    return;
                }
                if let Some(files) = v.get("files").and_then(|f| f.as_object()) {
                    for (k, f) in files {
                        self.scene
                            .files
                            .entry(k.clone())
                            .or_insert_with(|| f.clone());
                    }
                }
                scene::remap_ids(&mut els);
                let bb = els.iter().map(geom::aabb).reduce(geom::union).unwrap();
                let (dx, dy) = (at[0] - (bb[0] + bb[2]) / 2.0, at[1] - (bb[1] + bb[3]) / 2.0);
                let mut sel = HashSet::new();
                for mut e in els {
                    e.f.x.set(Num(e.x() + dx));
                    e.f.y.set(Num(e.y() + dy));
                    e.f.frame_id.set_null();
                    if e.container_id().is_none() {
                        sel.insert(e.id.clone());
                    }
                    self.scene.add(e);
                }
                self.selected = sel;
                self.tool = Tool::Selection;
                self.commit(cx);
                cx.notify();
                return;
            }
        }
        // Plain text → a text element.
        let mut t = Scene::new_element("text", at[0], at[1], &self.defaults);
        let (w, h) = text::measure(&text, t.font_family(), t.font_size(), t.line_height());
        t.f.text.set(text.clone());
        t.f.original_text.set(text);
        t.f.width.set(Num(w));
        t.f.height.set(Num(h));
        t.f.x.set(Num(at[0] - w / 2.0));
        t.f.y.set(Num(at[1] - h / 2.0));
        self.selected = [t.id.clone()].into();
        self.scene.add(t);
        self.commit(cx);
        let _ = window;
        cx.notify();
    }

    pub(crate) fn viewport_center(&self) -> P {
        let b = self.bounds.get();
        self.world(b.center())
    }

    // ------------------------------------------------------------ images

    pub(crate) fn prompt_image(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Insert image".into()),
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let paths = rx.await.ok().and_then(|r| r.ok()).flatten();
            let _ = this.update(cx, |v, cx| {
                if let Some(p) = paths.and_then(|p| p.into_iter().next())
                    && let Ok(bytes) = std::fs::read(&p)
                {
                    v.insert_image_bytes(&bytes, None, cx);
                }
                if !v.tool_locked {
                    v.tool = Tool::Selection;
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn insert_image_bytes(
        &mut self,
        bytes: &[u8],
        at: Option<P>,
        cx: &mut Context<Self>,
    ) -> bool {
        use base64::Engine;
        let (mime, (iw, ih)) = if crate::render::looks_like_svg(bytes) {
            let Some(sz) = crate::render::svg_size(bytes) else {
                return false;
            };
            ("image/svg+xml", sz)
        } else {
            let Ok(fmt) = image::guess_format(bytes) else {
                return false;
            };
            let Ok(img) = image::load_from_memory_with_format(bytes, fmt) else {
                return false;
            };
            (
                fmt.to_mime_type(),
                (img.width() as f64, img.height() as f64),
            )
        };
        let url = format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
        let fid = self.scene.add_file(mime, url);
        let max = 500.0 / self.cam.zoom.max(0.1);
        let s = (max / iw.max(ih)).min(1.0);
        let (w, h) = (iw * s, ih * s);
        let c = at.unwrap_or_else(|| self.viewport_center());
        let mut el = Scene::new_element("image", c[0] - w / 2.0, c[1] - h / 2.0, &self.defaults);
        el.f.width.set(Num(w));
        el.f.height.set(Num(h));
        el.f.file_id.set(fid);
        el.f.background_color.set("transparent".into());
        el.f.stroke_color.set("transparent".into());
        self.selected = [el.id.clone()].into();
        self.scene.add(el);
        self.commit(cx);
        cx.notify();
        true
    }

    pub(crate) fn on_drop_paths(
        &mut self,
        paths: &ExternalPaths,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = self.pointer.map(|p| self.world(p));
        for p in paths.paths() {
            if p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("excalidrawlib"))
            {
                self.import_library_file(p);
                cx.notify();
                continue;
            }
            if let Ok(bytes) = std::fs::read(p) {
                self.insert_image_bytes(&bytes, at, cx);
            }
        }
    }

    pub(crate) fn reset_canvas(&mut self, cx: &mut Context<Self>) {
        let ids: HashSet<String> = self.scene.elements().map(|e| e.id.clone()).collect();
        self.scene.remove(&ids);
        self.selected.clear();
        self.commit(cx);
        cx.notify();
    }

    pub(crate) fn set_background(&mut self, c: &str, cx: &mut Context<Self>) {
        self.scene.set_view_background(c);
        self.commit(cx);
        cx.notify();
    }

    pub(crate) fn hint(&self) -> Option<&'static str> {
        if self.editing.is_some() {
            return Some("Press Escape or CtrlOrCmd+Enter to finish editing");
        }
        if self.multi.is_some() {
            return Some("Click on last point or press Escape or Enter to finish");
        }
        match &self.drag {
            Some(Drag::Resize { .. }) => {
                return Some("Hold SHIFT to keep aspect ratio, ALT to resize from the center");
            }
            Some(Drag::Rotate { .. }) => {
                return Some("Hold SHIFT while rotating to constrain angles");
            }
            Some(Drag::Freedraw { .. }) => {
                return Some("Click and drag, release when you're finished");
            }
            _ => {}
        }
        match self.tool {
            Tool::Arrow | Tool::Line => {
                return Some("Click to start multiple points, drag for single line");
            }
            Tool::Freedraw => return Some("Click and drag, release when you're finished"),
            Tool::Text => {
                return Some(
                    "Tip: you can also add text by double-clicking anywhere with the selection tool",
                );
            }
            Tool::Image => {
                return Some(
                    "Click to place the image, or click and drag to set its size manually",
                );
            }
            Tool::Eraser => return None,
            Tool::Hand | Tool::Rectangle | Tool::Diamond | Tool::Ellipse | Tool::Frame => {
                return None;
            }
            Tool::Selection => {}
        }
        if let Some(el) = self.single() {
            if el.is_linear() {
                return Some("Drag points to edit, drag a midpoint to add one");
            }
            if matches!(el.kind.as_str(), "rectangle" | "diamond" | "ellipse") {
                return Some("Double-click or press Enter to add text");
            }
            if el.kind == "text" {
                return Some("Double-click or press Enter to edit text");
            }
            return None;
        }
        if self.selected.is_empty() {
            return Some(
                "To move canvas, hold mouse wheel or spacebar while dragging, or use the hand tool",
            );
        }
        None
    }
}

fn apply_scale(el: &mut Element, o: &Element, sx: f64, sy: f64) {
    if o.has_points() {
        let pts: Vec<P> = o.points().iter().map(|p| [p[0] * sx, p[1] * sy]).collect();
        el.set_points(&pts);
        let b = geom::local_bounds(el);
        el.f.width.set(Num(b[2] - b[0]));
        el.f.height.set(Num(b[3] - b[1]));
    } else {
        el.f.width.set(Num(o.width() * sx));
        el.f.height.set(Num(o.height() * sy));
    }
    if o.kind == "text" && o.container_id().is_none() {
        let fs = (o.font_size() * sy).max(1.0);
        el.f.font_size.set(Num(fs));
        let (w, h) = text::measure(o.text(), o.font_family(), fs, o.line_height());
        el.f.width.set(Num(w));
        el.f.height.set(Num(h));
    }
}

/// Maps a caret index in `original` to (line, column) in the wrapped
/// `display` text (wrapping only drops whitespace at break points).
pub(crate) fn map_caret(original: &str, display: &str, caret: usize) -> (usize, usize) {
    let o: Vec<char> = original.chars().collect();
    let mut oi = 0;
    let lines: Vec<Vec<char>> = display.split('\n').map(|l| l.chars().collect()).collect();
    for (li, line) in lines.iter().enumerate() {
        for (col, dc) in line.iter().enumerate() {
            while oi < o.len() && o[oi] != *dc && o[oi].is_whitespace() {
                if oi == caret {
                    return (li, col);
                }
                oi += 1;
            }
            if oi >= caret {
                return (li, col);
            }
            oi += 1;
        }
        if oi >= caret {
            return (li, line.len());
        }
        // skip the break (newline or dropped whitespace) unless more lines follow and caret sits there
        let mut skipped = false;
        while oi < o.len() && (o[oi] == ' ' || (o[oi] == '\n' && !skipped)) {
            if oi == caret {
                return (li, line.len());
            }
            if o[oi] == '\n' {
                skipped = true;
            }
            oi += 1;
            if skipped {
                break;
            }
        }
        if oi >= caret && li + 1 < lines.len() {
            return (li + 1, 0);
        }
    }
    (
        lines.len().saturating_sub(1),
        lines.last().map(|l| l.len()).unwrap_or(0),
    )
}

/// Inverse of [`map_caret`].
pub(crate) fn unmap_caret(original: &str, display: &str, line: usize, col: usize) -> usize {
    let n = original.chars().count();
    (0..=n)
        .find(|&c| {
            let (l, k) = map_caret(original, display, c);
            l > line || (l == line && k >= col)
        })
        .unwrap_or(n)
}

fn tool_for_key(k: &str) -> Option<Tool> {
    Some(match k {
        "1" | "v" => Tool::Selection,
        "2" | "r" => Tool::Rectangle,
        "3" | "d" => Tool::Diamond,
        "4" | "o" => Tool::Ellipse,
        "5" | "a" => Tool::Arrow,
        "6" | "l" => Tool::Line,
        "7" | "p" | "x" => Tool::Freedraw,
        "8" | "t" => Tool::Text,
        "9" => Tool::Image,
        "0" | "e" => Tool::Eraser,
        "h" => Tool::Hand,
        "f" => Tool::Frame,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{map_caret, unmap_caret};

    #[test]
    fn caret_mapping() {
        assert_eq!(map_caret("ab\ncd", "ab\ncd", 0), (0, 0));
        assert_eq!(map_caret("ab\ncd", "ab\ncd", 2), (0, 2));
        assert_eq!(map_caret("ab\ncd", "ab\ncd", 3), (1, 0));
        assert_eq!(map_caret("ab\ncd", "ab\ncd", 5), (1, 2));
        assert_eq!(map_caret("hello world", "hello\nworld", 6), (1, 0));
        assert_eq!(map_caret("hello world", "hello\nworld", 11), (1, 5));
        assert_eq!(unmap_caret("hello world", "hello\nworld", 1, 2), 8);
    }
}
