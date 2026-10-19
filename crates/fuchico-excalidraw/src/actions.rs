//! Editor actions shared by the context menu, the properties panel and
//! keyboard shortcuts: flip, lock, styles clipboard, align/distribute,
//! grid/snap modes, image export/copy, library.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui_kit::*;
use serde_json::Value;

use crate::export::{self, ExportOptions};
use crate::geom::{self, P};
use crate::library;
use crate::model::{Element, Num};
use crate::scene::{self, Scene};
use crate::view::{ExcalidrawView, Tool};

/// What a colour picker edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerTarget {
    Stroke,
    Background,
    Canvas,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialog {
    Export,
    Help,
    /// "Text to diagram" → Mermaid. Fuchico stubs the converter, so (like the
    /// original app) the dialog only reports that it is unavailable.
    Mermaid,
    /// "Reset library" confirmation (library deletes are not undoable).
    ResetLibrary,
}

/// The error the original app's mermaid-to-excalidraw stub throws.
pub const MERMAID_UNAVAILABLE: &str =
    "Mermaid to Excalidraw conversion is not available in Fuchico.";

/// Export dialog settings (Excalidraw's image export options).
#[derive(Clone, Debug, PartialEq)]
pub struct ExportState {
    pub background: bool,
    pub dark: bool,
    pub embed_scene: bool,
    pub scale: f64,
    pub only_selected: bool,
}

impl Default for ExportState {
    fn default() -> Self {
        Self {
            background: true,
            dark: false,
            embed_scene: false,
            scale: 2.0,
            only_selected: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    CenterX,
    Right,
    Top,
    CenterY,
    Bottom,
}

/// Every context-menu / command action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Cut,
    Copy,
    Paste,
    CopyPng,
    CopySvg,
    CopyStyles,
    PasteStyles,
    AddToLibrary,
    SendBackward,
    BringForward,
    SendToBack,
    BringToFront,
    FlipH,
    FlipV,
    Group,
    Ungroup,
    Duplicate,
    Lock,
    Delete,
    SelectAll,
    ToggleGrid,
    ToggleObjectSnap,
    UnlockAll,
    Export,
    Help,
    ZenMode,
    ViewMode,
    Stats,
    Find,
    Link,
    Crop,
}

impl Action {
    pub fn id(self) -> &'static str {
        match self {
            Action::Cut => "cut",
            Action::Copy => "copy",
            Action::Paste => "paste",
            Action::CopyPng => "copy-png",
            Action::CopySvg => "copy-svg",
            Action::CopyStyles => "copy-styles",
            Action::PasteStyles => "paste-styles",
            Action::AddToLibrary => "add-library",
            Action::SendBackward => "send-backward",
            Action::BringForward => "bring-forward",
            Action::SendToBack => "send-back",
            Action::BringToFront => "bring-front",
            Action::FlipH => "flip-h",
            Action::FlipV => "flip-v",
            Action::Group => "group",
            Action::Ungroup => "ungroup",
            Action::Duplicate => "duplicate",
            Action::Lock => "lock",
            Action::Delete => "delete",
            Action::SelectAll => "select-all",
            Action::ToggleGrid => "grid",
            Action::ToggleObjectSnap => "obj-snap",
            Action::UnlockAll => "unlock-all",
            Action::Export => "export",
            Action::Help => "help",
            Action::ZenMode => "zen",
            Action::ViewMode => "view-mode",
            Action::Stats => "stats",
            Action::Find => "find",
            Action::Link => "link",
            Action::Crop => "crop",
        }
    }
}

impl ExcalidrawView {
    /// Context-menu entries for the current state, as (action, label,
    /// shortcut, checked); `None` is a separator.
    pub(crate) fn context_entries(&self) -> Vec<Option<(Action, String, &'static str, bool)>> {
        let mut v: Vec<Option<(Action, String, &'static str, bool)>> = Vec::new();
        let e = |a, l: &str, k| Some((a, l.to_string(), k, false));
        let modes = |v: &mut Vec<_>| {
            v.push(Some((Action::ZenMode, "Zen mode".into(), "⌥Z", self.zen)));
            v.push(Some((
                Action::ViewMode,
                "View mode".into(),
                "⌥R",
                self.view_mode,
            )));
            v.push(Some((
                Action::Stats,
                "Canvas & Shape properties".into(),
                "⌥/",
                self.stats_open,
            )));
        };
        if self.view_mode {
            // Stock: the canvas menu in view mode is just the view toggles.
            v.push(Some((
                Action::ToggleGrid,
                "Toggle grid".into(),
                "⌘'",
                self.scene.grid_enabled(),
            )));
            modes(&mut v);
            return v;
        }
        if self.selected.is_empty() {
            v.push(e(Action::Paste, "Paste", "⌘V"));
            v.push(None);
            v.push(e(Action::CopyPng, "Copy to clipboard as PNG", "⇧⌥C"));
            v.push(e(Action::CopySvg, "Copy to clipboard as SVG", ""));
            v.push(None);
            v.push(e(Action::SelectAll, "Select all", "⌘A"));
            if self.scene.elements().any(|e| e.locked()) {
                v.push(e(Action::UnlockAll, "Unlock all elements", ""));
            }
            v.push(None);
            v.push(Some((
                Action::ToggleGrid,
                "Toggle grid".into(),
                "⌘'",
                self.scene.grid_enabled(),
            )));
            v.push(Some((
                Action::ToggleObjectSnap,
                "Snap to objects".into(),
                "⌥S",
                self.scene.app_bool("objectsSnapModeEnabled"),
            )));
            modes(&mut v);
            return v;
        }
        let list = self.selection_list();
        v.push(e(Action::Cut, "Cut", "⌘X"));
        v.push(e(Action::Copy, "Copy", "⌘C"));
        v.push(e(Action::Paste, "Paste", "⌘V"));
        v.push(None);
        v.push(e(Action::CopyPng, "Copy to clipboard as PNG", "⇧⌥C"));
        v.push(e(Action::CopySvg, "Copy to clipboard as SVG", ""));
        v.push(None);
        v.push(e(Action::CopyStyles, "Copy styles", "⌘⌥C"));
        if self.copied_styles.is_some() {
            v.push(e(Action::PasteStyles, "Paste styles", "⌘⌥V"));
        }
        v.push(None);
        if list.len() == 1 && list[0].container_id().is_none() {
            let label = if list[0].link().is_empty() {
                "Add link"
            } else {
                "Edit link"
            };
            v.push(e(Action::Link, label, "⌘K"));
        }
        if list.len() == 1 && list[0].kind == "image" {
            v.push(e(Action::Crop, "Crop image", "↵"));
        }
        v.push(e(Action::AddToLibrary, "Add to library", ""));
        v.push(None);
        v.push(e(Action::SendBackward, "Send backward", "⌘["));
        v.push(e(Action::BringForward, "Bring forward", "⌘]"));
        v.push(e(Action::SendToBack, "Send to back", "⌘⌥["));
        v.push(e(Action::BringToFront, "Bring to front", "⌘⌥]"));
        v.push(None);
        v.push(e(Action::FlipH, "Flip horizontal", "⇧H"));
        v.push(e(Action::FlipV, "Flip vertical", "⇧V"));
        v.push(None);
        if list.len() > 1 {
            v.push(e(Action::Group, "Group selection", "⌘G"));
        }
        if list.iter().any(|e| !e.group_ids().is_empty()) {
            v.push(e(Action::Ungroup, "Ungroup selection", "⌘⇧G"));
        }
        v.push(e(Action::Duplicate, "Duplicate", "⌘D"));
        v.push(e(Action::Lock, "Lock", "⌘⇧L"));
        v.push(None);
        v.push(e(Action::Delete, "Delete", "⌫"));
        v
    }

    pub(crate) fn run_action(&mut self, a: Action, window: &mut Window, cx: &mut Context<Self>) {
        self.context_menu = None;
        let s = self.selected.clone();
        match a {
            Action::Cut => {
                self.copy(cx);
                self.delete_selection(cx);
            }
            Action::Copy => self.copy(cx),
            Action::Paste => self.paste(window, cx),
            Action::CopyPng => self.copy_as_png(cx),
            Action::CopySvg => self.copy_as_svg(cx),
            Action::CopyStyles => self.copy_styles(),
            Action::PasteStyles => self.paste_styles(cx),
            Action::AddToLibrary => self.add_selection_to_library(cx),
            Action::SendBackward
            | Action::BringForward
            | Action::SendToBack
            | Action::BringToFront => {
                let fwd = matches!(a, Action::BringForward | Action::BringToFront);
                let end = matches!(a, Action::SendToBack | Action::BringToFront);
                self.scene.reorder(&s, fwd, end);
                self.commit(cx);
            }
            Action::FlipH => self.flip(true, cx),
            Action::FlipV => self.flip(false, cx),
            Action::Group => {
                self.scene.group(&s);
                self.commit(cx);
            }
            Action::Ungroup => {
                self.scene.ungroup(&s);
                self.commit(cx);
            }
            Action::Duplicate => {
                self.duplicate_selection(10.0, cx);
                self.commit(cx);
            }
            Action::Lock => self.lock_selection(cx),
            Action::Delete => self.delete_selection(cx),
            Action::SelectAll => self.select_all(),
            Action::ToggleGrid => self.toggle_grid(cx),
            Action::ToggleObjectSnap => self.toggle_object_snap(cx),
            Action::UnlockAll => self.unlock_all(cx),
            Action::Export => self.open_dialog(Dialog::Export),
            Action::Help => self.open_dialog(Dialog::Help),
            Action::ZenMode => self.zen = !self.zen,
            Action::ViewMode => {
                let on = !self.view_mode;
                self.set_view_mode(on);
            }
            Action::Stats => self.stats_open = !self.stats_open,
            Action::Find => self.open_search(),
            Action::Link => self.start_link_edit(),
            Action::Crop => self.start_crop(),
        }
        cx.notify();
    }

    pub(crate) fn open_dialog(&mut self, d: Dialog) {
        self.context_menu = None;
        self.menu_open = false;
        self.picker = None;
        if d == Dialog::Export {
            self.export.only_selected = !self.selected.is_empty();
        }
        self.dialog = Some(d);
    }

    pub(crate) fn select_all(&mut self) {
        self.tool = Tool::Selection;
        self.selected = self
            .scene
            .elements()
            .filter(|e| !e.locked() && e.container_id().is_none())
            .map(|e| e.id.clone())
            .collect();
    }

    // ------------------------------------------------------------ flip

    /// Flips the selection about its bounding-box centre (Excalidraw's
    /// flipHorizontal / flipVertical).
    pub(crate) fn flip(&mut self, horizontal: bool, cx: &mut Context<Self>) {
        let mut ids = self.selected.clone();
        if ids.is_empty() {
            return;
        }
        for el in self.scene.elements() {
            if el.container_id().map(|c| ids.contains(c)).unwrap_or(false) {
                ids.insert(el.id.clone());
            }
        }
        let tops: Vec<Element> = self
            .scene
            .elements()
            .filter(|e| ids.contains(&e.id) && e.container_id().is_none())
            .cloned()
            .collect();
        let Some(bb) = tops.iter().map(geom::aabb).reduce(geom::union) else {
            return;
        };
        let axis = if horizontal {
            (bb[0] + bb[2]) / 2.0
        } else {
            (bb[1] + bb[3]) / 2.0
        };
        for o in &tops {
            let mut el = o.clone();
            let c = geom::center(o);
            let nc = if horizontal {
                [2.0 * axis - c[0], c[1]]
            } else {
                [c[0], 2.0 * axis - c[1]]
            };
            el.f.angle
                .set(Num((-o.angle()).rem_euclid(std::f64::consts::TAU)));
            if o.has_points() {
                let lc = geom::local_center(o);
                let pts: Vec<P> = o
                    .points()
                    .iter()
                    .map(|p| {
                        if horizontal {
                            [2.0 * lc[0] - p[0], p[1]]
                        } else {
                            [p[0], 2.0 * lc[1] - p[1]]
                        }
                    })
                    .collect();
                let shift = pts[0];
                let pts: Vec<P> = pts
                    .iter()
                    .map(|p| [p[0] - shift[0], p[1] - shift[1]])
                    .collect();
                el.set_points(&pts);
                // Arrowheads stay on their ends; bindings follow below.
            }
            if o.kind == "image" {
                let mut sc = o.f.scale.get().cloned().unwrap_or([Num(1.0), Num(1.0)]);
                if horizontal {
                    sc[0] = Num(-sc[0].0)
                } else {
                    sc[1] = Num(-sc[1].0)
                }
                el.f.scale.set(sc);
            }
            let lc = geom::local_center(&el);
            el.f.x.set(Num(nc[0] - lc[0]));
            el.f.y.set(Num(nc[1] - lc[1]));
            if let Some(t) = self.scene.get_mut(&o.id) {
                *t = el;
            }
        }
        // Rebind/relayout what moved.
        for o in &tops {
            if o.is_linear() {
                self.scene.reroute_arrow(&o.id);
            }
        }
        for c in self.scene.bound_containers_of(&ids) {
            self.scene.layout_bound_text(&c);
        }
        self.scene.update_bound_arrows(&ids);
        self.commit(cx);
    }

    // ------------------------------------------------------------ lock

    pub(crate) fn lock_selection(&mut self, cx: &mut Context<Self>) {
        let s = self.selected.clone();
        let all_locked = self.selection_list().iter().all(|e| e.locked());
        for id in &s {
            if let Some(e) = self.scene.get_mut(id) {
                e.f.locked.set(!all_locked);
            }
        }
        if !all_locked {
            self.selected.clear();
        }
        self.commit(cx);
    }

    /// Opens the hyperlink editor for the single selected element.
    pub(crate) fn start_link_edit(&mut self) {
        let list = self.selection_list();
        if list.len() != 1 || list[0].container_id().is_some() {
            return;
        }
        self.link_edit = Some((list[0].id.clone(), list[0].link().to_string()));
    }

    /// Applies the hyperlink editor's text (empty removes the link).
    pub(crate) fn apply_link(&mut self, cx: &mut Context<Self>) {
        let Some((id, text)) = self.link_edit.take() else {
            return;
        };
        self.set_link(&id, text.trim(), cx);
    }

    pub(crate) fn set_link(&mut self, id: &str, link: &str, cx: &mut Context<Self>) {
        let Some(e) = self.scene.get_mut(id) else {
            return;
        };
        if e.link() == link {
            return;
        }
        if link.is_empty() {
            // Stock stores a removed link as `null`.
            e.f.link.set_null();
        } else {
            e.f.link.set(link.to_string());
        }
        self.commit(cx);
    }

    /// Keyboard for the open hyperlink editor. Returns whether consumed.
    pub(crate) fn link_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        let Some((_, text)) = self.link_edit.as_mut() else {
            return false;
        };
        let m = &ks.modifiers;
        if m.platform || m.control {
            if ks.key == "v"
                && let Some(t) = cx.read_from_clipboard().and_then(|c| c.text())
            {
                text.push_str(t.trim());
                return true;
            }
            return false;
        }
        match ks.key.as_str() {
            "escape" => self.link_edit = None,
            "enter" => self.apply_link(cx),
            "backspace" => {
                text.pop();
            }
            _ => match ks.key_char.as_deref() {
                Some(c) if !c.chars().any(char::is_control) => text.push_str(c),
                _ => return false,
            },
        }
        true
    }

    pub(crate) fn unlock_all(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self
            .scene
            .elements()
            .filter(|e| e.locked())
            .map(|e| e.id.clone())
            .collect();
        for id in &ids {
            if let Some(e) = self.scene.get_mut(id) {
                e.f.locked.set(false);
            }
        }
        self.selected = ids.into_iter().collect();
        self.commit(cx);
    }

    // ------------------------------------------------------------ styles

    pub(crate) fn copy_styles(&mut self) {
        let list = self.selection_list();
        // Prefer a shape over its text for the shape styles; remember the
        // text styles of either.
        let Some(first) = list.first().map(|e| (*e).clone()) else {
            return;
        };
        let mut st = first.clone();
        let text = list
            .iter()
            .find(|e| e.kind == "text")
            .map(|e| (*e).clone())
            .or_else(|| {
                first
                    .bound_elements()
                    .iter()
                    .find(|b| b.kind == "text")
                    .and_then(|b| self.scene.get(&b.id))
                    .cloned()
            });
        if let Some(t) = text {
            st.f.font_size = t.f.font_size.clone();
            st.f.font_family = t.f.font_family.clone();
            st.f.text_align = t.f.text_align.clone();
            st.f.line_height = t.f.line_height.clone();
        }
        self.copied_styles = Some(st);
    }

    pub(crate) fn paste_styles(&mut self, cx: &mut Context<Self>) {
        let Some(st) = self.copied_styles.clone() else {
            return;
        };
        self.apply(
            move |e, _| {
                if e.kind.is_empty() {
                    return;
                }
                let s = &st.f;
                e.f.stroke_color = s.stroke_color.clone();
                if e.kind != "text" {
                    e.f.background_color = s.background_color.clone();
                    e.f.fill_style = s.fill_style.clone();
                    e.f.stroke_width = s.stroke_width.clone();
                    e.f.stroke_style = s.stroke_style.clone();
                    e.f.roughness = s.roughness.clone();
                }
                e.f.opacity = s.opacity.clone();
                if e.kind == "text" && s.font_size.get().is_some() {
                    e.f.font_size = s.font_size.clone();
                    e.f.font_family = s.font_family.clone();
                    e.f.text_align = s.text_align.clone();
                    e.f.line_height = s.line_height.clone();
                }
                if e.kind == "arrow" && st.kind == "arrow" {
                    e.f.start_arrowhead = s.start_arrowhead.clone();
                    e.f.end_arrowhead = s.end_arrowhead.clone();
                }
                if matches!(e.kind.as_str(), "rectangle" | "diamond" | "line" | "arrow")
                    && !s.roundness.is_absent()
                {
                    e.f.roundness = s.roundness.clone();
                }
            },
            cx,
        );
    }

    // ------------------------------------------------------------ align

    pub(crate) fn align(&mut self, a: Align, cx: &mut Context<Self>) {
        let groups = self.selection_units();
        if groups.len() < 2 {
            return;
        }
        let bb = groups.iter().map(|(_, b)| *b).reduce(geom::union).unwrap();
        let moves: Vec<(Vec<String>, [f64; 2])> = groups
            .iter()
            .map(|(ids, b)| {
                let d = match a {
                    Align::Left => [bb[0] - b[0], 0.0],
                    Align::Right => [bb[2] - b[2], 0.0],
                    Align::CenterX => [(bb[0] + bb[2]) / 2.0 - (b[0] + b[2]) / 2.0, 0.0],
                    Align::Top => [0.0, bb[1] - b[1]],
                    Align::Bottom => [0.0, bb[3] - b[3]],
                    Align::CenterY => [0.0, (bb[1] + bb[3]) / 2.0 - (b[1] + b[3]) / 2.0],
                };
                (ids.clone(), d)
            })
            .collect();
        self.translate_units(&moves, cx);
    }

    pub(crate) fn distribute(&mut self, horizontal: bool, cx: &mut Context<Self>) {
        let mut groups = self.selection_units();
        if groups.len() < 3 {
            return;
        }
        let k = if horizontal { 0 } else { 1 };
        groups.sort_by(|a, b| (a.1[k] + a.1[k + 2]).total_cmp(&(b.1[k] + b.1[k + 2])));
        let bb = groups.iter().map(|(_, b)| *b).reduce(geom::union).unwrap();
        let total: f64 = groups.iter().map(|(_, b)| b[k + 2] - b[k]).sum();
        let gap = ((bb[k + 2] - bb[k]) - total) / (groups.len() - 1) as f64;
        let mut pos = bb[k];
        let mut moves = Vec::new();
        for (ids, b) in &groups {
            let mut d = [0.0, 0.0];
            d[k] = pos - b[k];
            pos += b[k + 2] - b[k] + gap;
            moves.push((ids.clone(), d));
        }
        self.translate_units(&moves, cx);
    }

    /// The selection split into groups (outermost group id) and loose
    /// elements, each with its bounding box.
    fn selection_units(&self) -> Vec<(Vec<String>, [f64; 4])> {
        let mut by_group: HashMap<String, Vec<&Element>> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for el in self.selection_list() {
            if el.container_id().is_some() {
                continue;
            }
            let key = el
                .group_ids()
                .last()
                .cloned()
                .unwrap_or_else(|| format!("#{}", el.id));
            if !by_group.contains_key(&key) {
                order.push(key.clone());
            }
            by_group.entry(key).or_default().push(el);
        }
        order
            .into_iter()
            .map(|k| {
                let els = &by_group[&k];
                let b = els
                    .iter()
                    .map(|e| geom::aabb(e))
                    .reduce(geom::union)
                    .unwrap();
                (els.iter().map(|e| e.id.clone()).collect(), b)
            })
            .collect()
    }

    fn translate_units(&mut self, moves: &[(Vec<String>, [f64; 2])], cx: &mut Context<Self>) {
        let mut all = HashSet::new();
        for (ids, d) in moves {
            let mut set: HashSet<String> = ids.iter().cloned().collect();
            for el in self.scene.elements() {
                if el.container_id().map(|c| set.contains(c)).unwrap_or(false) {
                    set.insert(el.id.clone());
                }
            }
            for id in &set {
                if let Some(e) = self.scene.get_mut(id) {
                    e.f.x.set(Num(e.x() + d[0]));
                    e.f.y.set(Num(e.y() + d[1]));
                }
            }
            all.extend(set);
        }
        self.scene.update_bound_arrows(&all);
        self.commit(cx);
    }

    // ------------------------------------------------------------ modes

    pub(crate) fn toggle_grid(&mut self, cx: &mut Context<Self>) {
        let on = !self.scene.grid_enabled();
        self.scene.set_app_state("gridModeEnabled", Value::Bool(on));
        if self
            .scene
            .app_state()
            .and_then(|a| a.get("gridSize"))
            .map(|v| v.is_null())
            .unwrap_or(true)
        {
            self.scene.set_app_state("gridSize", Value::from(20));
        }
        self.commit(cx);
    }

    pub(crate) fn toggle_object_snap(&mut self, cx: &mut Context<Self>) {
        let on = !self.scene.app_bool("objectsSnapModeEnabled");
        self.scene
            .set_app_state("objectsSnapModeEnabled", Value::Bool(on));
        self.commit(cx);
    }

    /// Grid snapping for a world point (when grid mode is on and the
    /// platform modifier isn't held).
    pub(crate) fn grid_snap(&self, p: P, m: &Modifiers) -> P {
        if self.scene.grid_enabled() && !(m.platform || m.control) {
            crate::snap::snap_to_grid(p, self.scene.grid_size())
        } else {
            p
        }
    }

    pub(crate) fn object_snap_on(&self, m: &Modifiers) -> bool {
        self.scene.app_bool("objectsSnapModeEnabled") != (m.platform || m.control)
            && !self.scene.grid_enabled()
    }

    /// Bounding boxes other elements offer for object snapping.
    pub(crate) fn snap_targets(&self, exclude: &HashSet<String>) -> Vec<[f64; 4]> {
        let b = self.bounds.get();
        let tl = self.world(b.origin);
        let br = self.world(b.bottom_right());
        self.scene
            .elements()
            .filter(|e| !exclude.contains(&e.id) && e.container_id().is_none())
            .map(geom::aabb)
            .filter(|bb| bb[2] >= tl[0] && bb[0] <= br[0] && bb[3] >= tl[1] && bb[1] <= br[1])
            .collect()
    }

    // ------------------------------------------------------------ export

    pub(crate) fn export_options(&self) -> ExportOptions {
        let e = &self.export;
        ExportOptions {
            background: e.background,
            dark: e.dark,
            embed_scene: e.embed_scene,
            scale: e.scale,
            only: if e.only_selected && !self.selected.is_empty() {
                Some(self.selection_with_bound())
            } else {
                None
            },
            padding: 10.0,
        }
    }

    pub(crate) fn selection_with_bound(&self) -> HashSet<String> {
        let mut ids = self.selected.clone();
        for el in self.scene.elements() {
            if el.container_id().map(|c| ids.contains(c)).unwrap_or(false) {
                ids.insert(el.id.clone());
            }
        }
        ids
    }

    fn clipboard_opts(&self) -> ExportOptions {
        ExportOptions {
            only: if self.selected.is_empty() {
                None
            } else {
                Some(self.selection_with_bound())
            },
            scale: 2.0,
            ..Default::default()
        }
    }

    pub(crate) fn copy_as_png(&mut self, cx: &mut Context<Self>) {
        match export::to_png(&self.scene, &self.clipboard_opts()) {
            Ok(png) => {
                cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
                    ImageFormat::Png,
                    png,
                )));
                self.toast("Copied to clipboard as PNG");
            }
            Err(e) => self.toast(&format!("Couldn't export: {e}")),
        }
    }

    pub(crate) fn copy_as_svg(&mut self, cx: &mut Context<Self>) {
        let svg = export::to_svg(&self.scene, &self.clipboard_opts());
        cx.write_to_clipboard(ClipboardItem::new_string(svg));
        self.toast("Copied to clipboard as SVG");
    }

    pub(crate) fn toast(&mut self, msg: &str) {
        self.toast = Some(msg.to_string());
    }

    /// Saves the export as PNG or SVG through a save dialog.
    pub(crate) fn export_to_file(&mut self, svg: bool, cx: &mut Context<Self>) {
        let opts = self.export_options();
        let bytes: Result<Vec<u8>, String> = if svg {
            Ok(export::to_svg(&self.scene, &opts).into_bytes())
        } else {
            export::to_png(&self.scene, &opts)
        };
        let bytes = match bytes {
            Ok(b) => b,
            Err(e) => {
                self.toast(&format!("Couldn't export: {e}"));
                return;
            }
        };
        let stem = self
            .source_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| {
                n.to_string_lossy()
                    .trim_end_matches(".excalidraw")
                    .trim_end_matches(".md")
                    .to_string()
            })
            .unwrap_or_else(|| "drawing".into());
        let name = format!("{stem}{}", if svg { ".svg" } else { ".png" });
        let dir = self
            .source_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let rx = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let path = rx.await.ok().and_then(|r| r.ok()).flatten();
            let _ = this.update(cx, |v, cx| {
                if let Some(p) = path {
                    match std::fs::write(&p, &bytes) {
                        Ok(()) => v.toast("Exported"),
                        Err(e) => v.toast(&format!("Couldn't export: {e}")),
                    }
                    v.dialog = None;
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ------------------------------------------------------------ library

    pub(crate) fn add_selection_to_library(&mut self, cx: &mut Context<Self>) {
        let ids = self.selection_with_bound();
        if ids.is_empty() {
            return;
        }
        let els: Vec<Value> = self
            .scene
            .elements()
            .filter(|e| ids.contains(&e.id))
            .filter_map(|e| serde_json::to_value(e).ok())
            .collect();
        self.library.add(els);
        self.library_open = true;
        self.toast("Added to library");
        cx.notify();
    }

    /// Inserts a library item centred at `at` (world) or the viewport.
    pub(crate) fn insert_library_item(&mut self, id: &str, at: Option<P>, cx: &mut Context<Self>) {
        let Some(item) = self.library.get(id) else {
            return;
        };
        let mut els = library::item_elements(item);
        if els.is_empty() {
            return;
        }
        scene::remap_ids(&mut els);
        let at = at.unwrap_or_else(|| self.viewport_center());
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
    }

    /// Asks before clearing the personal library (stock confirms too).
    pub(crate) fn reset_library(&mut self, _cx: &mut Context<Self>) {
        self.dialog = Some(Dialog::ResetLibrary);
    }

    pub(crate) fn confirm_reset_library(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self
            .library
            .items
            .iter()
            .map(|i| library::item_id(i).to_string())
            .collect();
        for id in ids {
            self.library.remove(&id);
        }
        self.dialog = None;
        cx.notify();
    }

    pub(crate) fn import_library_prompt(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import library".into()),
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let paths = rx
                .await
                .ok()
                .and_then(|r| r.ok())
                .flatten()
                .unwrap_or_default();
            let _ = this.update(cx, |v, cx| {
                for p in paths {
                    v.import_library_file(&p);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn import_library_file(&mut self, p: &std::path::Path) -> bool {
        let Ok(s) = std::fs::read_to_string(p) else {
            return false;
        };
        match self.library.import(&s) {
            Ok(n) => {
                self.library_open = true;
                self.toast(&format!(
                    "Imported {n} library item{}",
                    if n == 1 { "" } else { "s" }
                ));
                true
            }
            Err(e) => {
                self.toast(&e);
                false
            }
        }
    }

    pub(crate) fn export_library_prompt(&mut self, cx: &mut Context<Self>) {
        let dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let rx = cx.prompt_for_new_path(&dir, Some("library.excalidrawlib"));
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let path = rx.await.ok().and_then(|r| r.ok()).flatten();
            let _ = this.update(cx, |v, cx| {
                if let Some(p) = path {
                    match v.library.export_to(&p) {
                        Ok(()) => v.toast("Library exported"),
                        Err(e) => v.toast(&e),
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// Excalidraw's colour palette (open-color shades 0/2/4/6/8 per hue), in
/// the stock picker's grid order (top picks, then `COMMON_ELEMENT_SHADES`).
pub const PALETTE: [(&str, [&str; 5]); 15] = [
    ("transparent", ["transparent"; 5]),
    ("white", ["#ffffff"; 5]),
    (
        "gray",
        ["#f8f9fa", "#e9ecef", "#ced4da", "#868e96", "#343a40"],
    ),
    ("black", ["#1e1e1e"; 5]),
    (
        "bronze",
        ["#f8f1ee", "#eaddd7", "#d2bab0", "#a18072", "#846358"],
    ),
    (
        "cyan",
        ["#e3fafc", "#99e9f2", "#3bc9db", "#15aabf", "#0c8599"],
    ),
    (
        "blue",
        ["#e7f5ff", "#a5d8ff", "#4dabf7", "#228be6", "#1971c2"],
    ),
    (
        "violet",
        ["#f3f0ff", "#d0bfff", "#9775fa", "#7950f2", "#6741d9"],
    ),
    (
        "grape",
        ["#f8f0fc", "#eebefa", "#da77f2", "#be4bdb", "#9c36b5"],
    ),
    (
        "pink",
        ["#fff0f6", "#fcc2d7", "#f783ac", "#e64980", "#c2255c"],
    ),
    (
        "green",
        ["#ebfbee", "#b2f2bb", "#69db7c", "#40c057", "#2f9e44"],
    ),
    (
        "teal",
        ["#e6fcf5", "#96f2d7", "#38d9a9", "#12b886", "#099268"],
    ),
    (
        "yellow",
        ["#fff9db", "#ffec99", "#ffd43b", "#fab005", "#f08c00"],
    ),
    (
        "orange",
        ["#fff4e6", "#ffd8a8", "#ffa94d", "#fd7e14", "#e8590c"],
    ),
    (
        "red",
        ["#fff5f5", "#ffc9c9", "#ff8787", "#fa5252", "#e03131"],
    ),
];

/// Keyboard keys of the palette grid (Excalidraw's colour-picker keys).
pub const PALETTE_KEYS: [&str; 15] = [
    "q", "w", "e", "r", "t", "a", "s", "d", "f", "g", "z", "x", "c", "v", "b",
];

/// Shade index Excalidraw picks for a hue in each picker.
pub fn default_shade(t: PickerTarget) -> usize {
    match t {
        PickerTarget::Stroke => 4,
        PickerTarget::Background => 1,
        PickerTarget::Canvas => 0,
    }
}

/// (hue index, shade index) of a colour in the palette.
pub fn palette_pos(c: &str) -> Option<(usize, Option<usize>)> {
    for (i, (_, shades)) in PALETTE.iter().enumerate() {
        if shades[0] == shades[4] {
            if shades[0].eq_ignore_ascii_case(c) {
                return Some((i, None));
            }
            continue;
        }
        if let Some(k) = shades.iter().position(|s| s.eq_ignore_ascii_case(c)) {
            return Some((i, Some(k)));
        }
    }
    None
}

/// Normalises hex input ("abc", "#AABBCC", …) to `#rrggbb`/`#rgb`.
pub fn normalize_hex(s: &str) -> Option<String> {
    let t = s.trim().trim_start_matches('#');
    if (t.len() == 3 || t.len() == 6 || t.len() == 8) && t.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("#{}", t.to_ascii_lowercase()))
    } else if t.eq_ignore_ascii_case("transparent") {
        Some("transparent".into())
    } else {
        None
    }
}

impl ExcalidrawView {
    /// Sets the arrow type (sharp / round / elbow) of selected arrows and
    /// the arrow tool's default; elbow arrows are re-routed.
    pub(crate) fn set_arrow_type(&mut self, k: &'static str, cx: &mut Context<Self>) {
        self.apply(
            |e, d| {
                d.arrow_type = k.into();
                if e.kind != "arrow" {
                    return;
                }
                if k == "round" {
                    e.f.roundness.set(crate::model::Roundness {
                        kind: Num(2.0),
                        value: Default::default(),
                        extra: Default::default(),
                    });
                } else {
                    e.f.roundness.set_null();
                }
                e.f.elbowed.set(k == "elbow");
                if k == "elbow" {
                    e.f.extra.entry("fixedSegments").or_insert(Value::Null);
                    e.f.extra
                        .entry("startIsSpecial")
                        .or_insert(Value::Bool(false));
                    e.f.extra
                        .entry("endIsSpecial")
                        .or_insert(Value::Bool(false));
                }
            },
            cx,
        );
        let arrows: Vec<String> = self
            .selection_list()
            .iter()
            .filter(|e| e.kind == "arrow")
            .map(|e| e.id.clone())
            .collect();
        for a in &arrows {
            if k == "elbow" {
                // Elbow bindings use fixed points instead of focus.
                for start in [true, false] {
                    let t = self.scene.get(a).and_then(|e| {
                        if start {
                            e.f.start_binding.get()
                        } else {
                            e.f.end_binding.get()
                        }
                        .map(|b| b.element_id.clone())
                    });
                    if let Some(t) = t {
                        self.scene.bind(a, start, Some(&t));
                    }
                }
            }
            self.scene.reroute_arrow(a);
        }
        if !arrows.is_empty() {
            self.commit(cx);
        }
        cx.notify();
    }

    /// The colour the picker for `t` currently shows.
    pub(crate) fn current_color(&self, t: PickerTarget) -> String {
        let first = self.selection_list().first().map(|e| (*e).clone());
        match t {
            PickerTarget::Stroke => first
                .map(|e| e.stroke_color().to_string())
                .unwrap_or(self.defaults.stroke_color.clone()),
            PickerTarget::Background => first
                .map(|e| e.background_color().to_string())
                .unwrap_or(self.defaults.background_color.clone()),
            PickerTarget::Canvas => self.scene.view_background(),
        }
    }

    /// Applies a colour from the picker / eyedropper / hex field.
    pub(crate) fn set_color(&mut self, t: PickerTarget, c: &str, cx: &mut Context<Self>) {
        let c = c.to_string();
        match t {
            PickerTarget::Stroke => self.apply(
                |e, d| {
                    e.f.stroke_color.set(c.clone());
                    d.stroke_color = c.clone();
                },
                cx,
            ),
            PickerTarget::Background => self.apply(
                |e, d| {
                    e.f.background_color.set(c.clone());
                    d.background_color = c.clone();
                },
                cx,
            ),
            PickerTarget::Canvas => self.set_background(&c, cx),
        }
        cx.notify();
    }

    pub(crate) fn toggle_picker(&mut self, t: PickerTarget) {
        self.eyedropper = None;
        self.hex = None;
        self.picker = if self.picker == Some(t) {
            None
        } else {
            Some(t)
        };
    }

    /// Picker keyboard: q..b pick a hue, 1-5 a shade, typing edits the hex
    /// field. Returns whether the key was consumed.
    pub(crate) fn picker_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(t) = self.picker else { return false };
        let m = &ks.modifiers;
        if m.platform || m.control || m.alt {
            return false;
        }
        let key = ks.key.as_str();
        if let Some(h) = self.hex.as_mut() {
            match key {
                "enter" => {
                    if let Some(c) = normalize_hex(h) {
                        self.hex = None;
                        self.set_color(t, &c, cx);
                    }
                }
                "backspace" => {
                    h.pop();
                }
                k if k.len() == 1 && k.chars().all(|c| c.is_ascii_hexdigit()) && h.len() < 8 => {
                    h.push_str(k)
                }
                _ => return key != "escape",
            }
            return true;
        }
        if let Some(i) = PALETTE_KEYS.iter().position(|k| *k == key) {
            self.set_color(t, PALETTE[i].1[default_shade(t)], cx);
            return true;
        }
        if let Some(n) = key.parse::<usize>().ok().filter(|n| (1..=5).contains(n)) {
            let cur = self.current_color(t);
            if let Some((hue, Some(_))) = palette_pos(&cur) {
                self.set_color(t, PALETTE[hue].1[n - 1], cx);
            }
            return true;
        }
        false
    }

    /// Eyedropper: picks the colour under `p` (world) into the active target.
    pub(crate) fn eyedrop(&mut self, p: P, dark: bool, cx: &mut Context<Self>) {
        if let Some(t) = self.eyedropper.take()
            && let Some(c) = export::sample_color(&self.scene, p, dark)
        {
            self.set_color(t, &c, cx);
        }
    }
}

#[allow(dead_code)]
fn _unused(_: &Scene) {}
