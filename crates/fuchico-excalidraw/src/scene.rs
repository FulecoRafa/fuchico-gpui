//! The editable document: elements in z-order plus everything else from the
//! file, serialized losslessly.
//!
//! Losslessness: the top-level object (key order, unknown keys, `appState`)
//! is kept as parsed JSON; each element keeps the exact JSON it was parsed
//! from, which is written back verbatim while the typed element still equals
//! its parsed form. Elements that don't fit the typed model and `files`
//! entries stay raw JSON throughout.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use serde_json::{Map, Value};

use crate::fractional::key_between;
use crate::geom;
use crate::model::{Binding, BoundElement, Element, Field, LoadError, Num, Roundness};
use crate::text;

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    El(Box<Element>),
    Raw(Value),
}

/// Snapshot used for undo/redo.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    items: Vec<Item>,
    files: IndexMap<String, Value>,
    app_state: Option<Map<String, Value>>,
}

#[derive(Clone, Debug)]
pub struct Scene {
    /// The parsed top-level object (elements/files are re-inserted on output).
    top: Map<String, Value>,
    pub items: Vec<Item>,
    pub files: IndexMap<String, Value>,
    originals: HashMap<String, (Element, Value)>,
    /// Content was empty (a brand-new file): write canonical keys.
    fresh: bool,
}

/// The "current item" properties new elements are created with
/// (Excalidraw's `appState.currentItem*`).
#[derive(Clone, Debug, PartialEq)]
pub struct ItemDefaults {
    pub stroke_color: String,
    pub background_color: String,
    pub fill_style: String,
    pub stroke_width: f64,
    pub stroke_style: String,
    pub roughness: f64,
    pub opacity: f64,
    pub font_size: f64,
    pub font_family: u32,
    pub text_align: String,
    pub start_arrowhead: Option<String>,
    pub end_arrowhead: Option<String>,
    pub round: bool,
}

impl Default for ItemDefaults {
    fn default() -> Self {
        Self {
            stroke_color: "#1e1e1e".into(),
            background_color: "transparent".into(),
            fill_style: "solid".into(),
            stroke_width: 2.0,
            stroke_style: "solid".into(),
            roughness: 1.0,
            opacity: 100.0,
            font_size: 20.0,
            font_family: 5,
            text_align: "left".into(),
            start_arrowhead: None,
            end_arrowhead: Some("arrow".into()),
            round: true,
        }
    }
}

pub fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

pub fn random_id() -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
    (0..21).map(|_| A[(rand::random::<u32>() as usize) % A.len()] as char).collect()
}

fn random_int() -> f64 {
    (rand::random::<u32>() >> 1) as f64
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            top: Map::new(),
            items: Vec::new(),
            files: IndexMap::new(),
            originals: HashMap::new(),
            fresh: true,
        }
    }
}

impl Scene {
    /// Parses file content (empty content → empty drawing).
    pub fn parse(content: &str) -> Result<Self, LoadError> {
        let t = content.trim();
        if t.is_empty() {
            return Ok(Self::default());
        }
        let v: Value = serde_json::from_str(t).map_err(|_| LoadError::Malformed)?;
        let Value::Object(mut top) = v else { return Err(LoadError::Malformed) };
        let mut s = Self { fresh: false, ..Self::default() };
        match top.get_mut("elements") {
            Some(Value::Array(arr)) => {
                for v in std::mem::take(arr) {
                    match serde_json::from_value::<Element>(v.clone()) {
                        Ok(el) if v.is_object() => {
                            s.originals.insert(el.id.clone(), (el.clone(), v));
                            s.items.push(Item::El(Box::new(el)));
                        }
                        _ => s.items.push(Item::Raw(v)),
                    }
                }
            }
            Some(Value::Null) | None => {}
            Some(_) => return Err(LoadError::Malformed),
        }
        match top.get_mut("files") {
            Some(Value::Object(m)) => {
                for (k, v) in std::mem::take(m) {
                    s.files.insert(k, v);
                }
            }
            Some(Value::Null) | None => {}
            Some(_) => return Err(LoadError::Malformed),
        }
        s.top = top;
        Ok(s)
    }

    fn elements_json(&self) -> Value {
        Value::Array(
            self.items
                .iter()
                .map(|it| match it {
                    Item::Raw(v) => v.clone(),
                    Item::El(el) => match self.originals.get(&el.id) {
                        Some((orig, v)) if orig == el.as_ref() => v.clone(),
                        _ => serde_json::to_value(el.as_ref()).unwrap_or(Value::Null),
                    },
                })
                .collect(),
        )
    }

    fn files_json(&self) -> Value {
        Value::Object(self.files.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    pub fn to_value(&self) -> Value {
        let mut out = self.top.clone();
        let canonical: [(&str, Value); 3] = [
            ("type", Value::from("excalidraw")),
            ("version", Value::from(2)),
            ("source", Value::from("fuchico")),
        ];
        if self.fresh || out.is_empty() {
            let mut m = Map::new();
            for (k, v) in canonical {
                m.insert(k.into(), v);
            }
            m.insert("elements".into(), self.elements_json());
            let app = match out.remove("appState") {
                Some(Value::Object(a)) => a,
                _ => default_app_state(),
            };
            m.insert("appState".into(), Value::Object(app));
            m.insert("files".into(), self.files_json());
            return Value::Object(m);
        }
        for (k, v) in canonical {
            if !out.contains_key(k) {
                out.insert(k.into(), v);
            }
        }
        out.insert("elements".into(), self.elements_json());
        if !out.contains_key("appState") {
            out.insert("appState".into(), Value::Object(default_app_state()));
        }
        out.insert("files".into(), self.files_json());
        Value::Object(out)
    }

    /// `JSON.stringify(data, null, 2)`-style output.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.to_value()).unwrap_or_else(|_| "{}".into())
    }

    pub fn app_state(&self) -> Option<&Map<String, Value>> {
        self.top.get("appState").and_then(|v| v.as_object())
    }

    pub fn view_background(&self) -> String {
        self.app_state()
            .and_then(|a| a.get("viewBackgroundColor"))
            .and_then(|v| v.as_str())
            .unwrap_or("#ffffff")
            .to_string()
    }

    pub fn set_view_background(&mut self, c: &str) {
        let app = self
            .top
            .entry("appState")
            .or_insert_with(|| Value::Object(default_app_state()));
        if !app.is_object() {
            *app = Value::Object(default_app_state());
        }
        app.as_object_mut()
            .unwrap()
            .insert("viewBackgroundColor".into(), Value::from(c));
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            items: self.items.clone(),
            files: self.files.clone(),
            app_state: self.app_state().cloned(),
        }
    }

    pub fn restore(&mut self, s: &Snapshot) {
        self.items = s.items.clone();
        self.files = s.files.clone();
        match &s.app_state {
            Some(a) => {
                self.top.insert("appState".into(), Value::Object(a.clone()));
            }
            None => {
                self.top.remove("appState");
            }
        }
    }

    /// Bumps `version`/`versionNonce`/`updated` on every element that differs
    /// from `before` (Excalidraw's `mutateElement` bookkeeping, done once per
    /// committed change).
    pub fn bump_changed(&mut self, before: &Snapshot) {
        let prev: HashMap<&str, &Element> = before
            .items
            .iter()
            .filter_map(|i| match i {
                Item::El(e) => Some((e.id.as_str(), e.as_ref())),
                _ => None,
            })
            .collect();
        let now = now_ms();
        for it in &mut self.items {
            if let Item::El(el) = it {
                match prev.get(el.id.as_str()) {
                    Some(p) if *p == el.as_ref() => {}
                    Some(p) => {
                        let v = Element::num(&p.f.version, 1.0);
                        el.f.version.set(Num(v + 1.0));
                        el.f.version_nonce.set(Num(random_int()));
                        el.f.updated.set(Num(now));
                    }
                    None => {}
                }
            }
        }
    }

    // ------------------------------------------------------------ queries

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.items.iter().filter_map(|i| match i {
            Item::El(e) if !e.is_deleted() => Some(e.as_ref()),
            _ => None,
        })
    }

    pub fn position(&self, id: &str) -> Option<usize> {
        self.items
            .iter()
            .position(|i| matches!(i, Item::El(e) if e.id == id))
    }

    pub fn get(&self, id: &str) -> Option<&Element> {
        self.items.iter().find_map(|i| match i {
            Item::El(e) if e.id == id => Some(e.as_ref()),
            _ => None,
        })
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Element> {
        self.items.iter_mut().find_map(|i| match i {
            Item::El(e) if e.id == id => Some(e.as_mut()),
            _ => None,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.elements().next().is_none()
    }

    fn last_index(&self) -> Option<String> {
        self.items.iter().rev().find_map(|i| match i {
            Item::El(e) => e.f.index.get().cloned(),
            Item::Raw(v) => v.get("index").and_then(|v| v.as_str()).map(String::from),
        })
    }

    // ------------------------------------------------------------ creation

    /// A new element with Excalidraw's defaults for `kind`.
    pub fn new_element(kind: &str, x: f64, y: f64, d: &ItemDefaults) -> Element {
        let mut f = crate::model::ElementFields::default();
        f.x.set(Num(x));
        f.y.set(Num(y));
        f.width.set(Num(0.0));
        f.height.set(Num(0.0));
        f.angle.set(Num(0.0));
        f.stroke_color.set(d.stroke_color.clone());
        f.background_color.set(d.background_color.clone());
        f.fill_style.set(d.fill_style.clone());
        f.stroke_width.set(Num(d.stroke_width));
        f.stroke_style.set(d.stroke_style.clone());
        f.roughness.set(Num(d.roughness));
        f.opacity.set(Num(d.opacity));
        f.group_ids.set(Vec::new());
        f.frame_id.set_null();
        f.index.set_null();
        let roundness = |t: f64| Roundness { kind: Num(t), value: Field::Absent, extra: Map::new() };
        match kind {
            "rectangle" | "diamond" if d.round => f.roundness.set(roundness(3.0)),
            "line" | "arrow" if d.round => f.roundness.set(roundness(2.0)),
            _ => f.roundness.set_null(),
        }
        f.seed.set(Num(random_int()));
        f.version.set(Num(1.0));
        f.version_nonce.set(Num(random_int()));
        f.is_deleted.set(false);
        f.bound_elements.set_null();
        f.updated.set(Num(now_ms()));
        f.link.set_null();
        f.locked.set(false);
        match kind {
            "text" => {
                f.text.set(String::new());
                f.font_size.set(Num(d.font_size));
                f.font_family.set(Num(d.font_family as f64));
                f.text_align.set(d.text_align.clone());
                f.vertical_align.set("top".into());
                f.container_id.set_null();
                f.original_text.set(String::new());
                f.auto_resize.set(true);
                f.line_height.set(Num(text::default_line_height(d.font_family)));
            }
            "line" | "arrow" => {
                f.points.set(vec![[Num(0.0), Num(0.0)]]);
                f.last_committed_point.set_null();
                f.start_binding.set_null();
                f.end_binding.set_null();
                if kind == "arrow" {
                    f.start_arrowhead.set_opt(d.start_arrowhead.clone());
                    f.end_arrowhead.set_opt(d.end_arrowhead.clone());
                    f.elbowed.set(false);
                } else {
                    f.start_arrowhead.set_null();
                    f.end_arrowhead.set_null();
                }
            }
            "freedraw" => {
                f.points.set(vec![[Num(0.0), Num(0.0)]]);
                f.pressures.set(Vec::new());
                f.simulate_pressure.set(true);
                f.last_committed_point.set_null();
            }
            "image" => {
                f.status.set("saved".into());
                f.file_id.set_null();
                f.scale.set([Num(1.0), Num(1.0)]);
                f.crop.set_null();
            }
            "frame" => {
                f.name.set_null();
            }
            _ => {}
        }
        Element { id: random_id(), kind: kind.into(), f }
    }

    /// Appends an element on top, assigning a fractional index.
    pub fn add(&mut self, mut el: Element) {
        let idx = key_between(self.last_index().as_deref(), None);
        el.f.index.set(idx);
        self.items.push(Item::El(Box::new(el)));
    }

    /// Removes elements (and text bound to them), cleaning up references.
    pub fn remove(&mut self, ids: &HashSet<String>) {
        let mut all = ids.clone();
        for el in self.elements() {
            if let Some(c) = el.container_id() {
                if ids.contains(c) {
                    all.insert(el.id.clone());
                }
            }
        }
        self.items
            .retain(|i| !matches!(i, Item::El(e) if all.contains(&e.id)));
        for it in &mut self.items {
            let Item::El(el) = it else { continue };
            if let Some(b) = el.f.bound_elements.get_mut() {
                let before = b.len();
                b.retain(|be| !all.contains(&be.id));
                if b.len() != before && b.is_empty() {
                    // Excalidraw keeps an empty array here.
                }
            }
            for binding in [&mut el.f.start_binding, &mut el.f.end_binding] {
                if let Some(bd) = binding.get() {
                    if all.contains(&bd.element_id) {
                        binding.set_null();
                    }
                }
            }
            if let Some(c) = el.f.container_id.get() {
                if all.contains(c) {
                    el.f.container_id.set_null();
                }
            }
            if let Some(fr) = el.f.frame_id.get() {
                if all.contains(fr) {
                    el.f.frame_id.set_null();
                }
            }
        }
    }

    /// Adds a data-URL file entry; returns its id.
    pub fn add_file(&mut self, mime: &str, data_url: String) -> String {
        use sha2::Digest;
        let hash = sha2::Sha256::digest(data_url.as_bytes());
        let id: String = hash.iter().map(|b| format!("{b:02x}")).collect::<String>()[..40].to_string();
        if !self.files.contains_key(&id) {
            let now = now_ms();
            self.files.insert(
                id.clone(),
                serde_json::json!({
                    "mimeType": mime,
                    "id": id,
                    "dataURL": data_url,
                    "created": now as i64,
                    "lastRetrieved": now as i64,
                }),
            );
        }
        id
    }

    pub fn file_data_url(&self, id: &str) -> Option<&str> {
        self.files.get(id)?.get("dataURL")?.as_str()
    }

    // ------------------------------------------------------------ groups

    /// The ids selected when clicking `id`: its outermost group (unless we
    /// are already "inside" that group) plus bound text.
    pub fn expand_groups(&self, ids: &HashSet<String>) -> HashSet<String> {
        let mut out = ids.clone();
        let groups: HashSet<String> = ids
            .iter()
            .filter_map(|id| self.get(id))
            .filter_map(|e| e.group_ids().last().cloned())
            .collect();
        if !groups.is_empty() {
            for el in self.elements() {
                if el.group_ids().last().map(|g| groups.contains(g)).unwrap_or(false) {
                    out.insert(el.id.clone());
                }
            }
        }
        out
    }

    pub fn group(&mut self, ids: &HashSet<String>) {
        if ids.len() < 2 {
            return;
        }
        let gid = random_id();
        for id in ids {
            if let Some(el) = self.get_mut(id) {
                let mut g = el.group_ids().to_vec();
                g.push(gid.clone());
                el.f.group_ids.set(g);
            }
        }
    }

    pub fn ungroup(&mut self, ids: &HashSet<String>) {
        for id in ids {
            if let Some(el) = self.get_mut(id) {
                let mut g = el.group_ids().to_vec();
                if g.pop().is_some() {
                    el.f.group_ids.set(g);
                }
            }
        }
    }

    // ------------------------------------------------------------ z-order

    /// Moves `ids` one step (`forward`) or all the way (`to_end`).
    pub fn reorder(&mut self, ids: &HashSet<String>, forward: bool, to_end: bool) {
        let sel = |i: &Item| matches!(i, Item::El(e) if ids.contains(&e.id));
        if to_end {
            let (mut a, b): (Vec<Item>, Vec<Item>) =
                std::mem::take(&mut self.items).into_iter().partition(|i| !sel(i));
            if forward {
                a.extend(b);
                self.items = a;
            } else {
                let mut b = b;
                b.extend(a);
                self.items = b;
            }
        } else if forward {
            let n = self.items.len();
            for i in (0..n.saturating_sub(1)).rev() {
                if sel(&self.items[i]) && !sel(&self.items[i + 1]) {
                    self.items.swap(i, i + 1);
                }
            }
        } else {
            for i in 1..self.items.len() {
                if sel(&self.items[i]) && !sel(&self.items[i - 1]) {
                    self.items.swap(i, i - 1);
                }
            }
        }
        self.fix_indices(ids);
    }

    fn item_index(i: &Item) -> Option<String> {
        match i {
            Item::El(e) => e.f.index.get().cloned(),
            Item::Raw(v) => v.get("index").and_then(|v| v.as_str()).map(String::from),
        }
    }

    /// Re-assigns fractional indices of `ids` so they sort in array order.
    pub fn fix_indices(&mut self, ids: &HashSet<String>) {
        for i in 0..self.items.len() {
            let Item::El(e) = &self.items[i] else { continue };
            if !ids.contains(&e.id) {
                continue;
            }
            let prev = (0..i).rev().find_map(|j| Self::item_index(&self.items[j]));
            let next = (i + 1..self.items.len())
                .filter(|j| !matches!(&self.items[*j], Item::El(e) if ids.contains(&e.id)))
                .find_map(|j| Self::item_index(&self.items[j]));
            let ok = |k: &str| {
                prev.as_deref().map(|p| p < k).unwrap_or(true)
                    && next.as_deref().map(|n| k < n).unwrap_or(true)
            };
            if e.f.index.get().map(|k| ok(k)).unwrap_or(false) {
                continue;
            }
            let k = key_between(prev.as_deref(), next.as_deref());
            if let Item::El(e) = &mut self.items[i] {
                e.f.index.set(k);
            }
        }
    }

    // ------------------------------------------------------------ copy/paste

    /// Clones `ids` (plus bound text) with fresh ids, remapping every
    /// internal reference. Returns the clones, not yet added.
    pub fn clone_elements(&self, ids: &HashSet<String>) -> Vec<Element> {
        let mut src: Vec<Element> = self
            .elements()
            .filter(|e| {
                ids.contains(&e.id) || e.container_id().map(|c| ids.contains(c)).unwrap_or(false)
            })
            .cloned()
            .collect();
        remap_ids(&mut src);
        src
    }

    /// Updates arrows bound to `ids` after those shapes moved/resized.
    pub fn update_bound_arrows(&mut self, ids: &HashSet<String>) {
        let arrows: Vec<String> = self
            .elements()
            .filter(|e| e.kind == "arrow")
            .filter(|e| {
                [&e.f.start_binding, &e.f.end_binding].iter().any(|b| {
                    b.get().map(|b| ids.contains(&b.element_id)).unwrap_or(false)
                })
            })
            .map(|e| e.id.clone())
            .collect();
        for aid in arrows {
            let Some(arrow) = self.get(&aid).cloned() else { continue };
            let mut world: Vec<geom::P> = arrow.points().iter().map(|p| geom::to_world(&arrow, *p)).collect();
            if world.len() < 2 {
                continue;
            }
            let n = world.len();
            for (start, b) in [(true, arrow.f.start_binding.get()), (false, arrow.f.end_binding.get())] {
                let Some(b) = b else { continue };
                let Some(target) = self.get(&b.element_id) else { continue };
                let (i, j) = if start { (0, 1) } else { (n - 1, n - 2) };
                let adj = if n == 2 {
                    // Aim from the other end (or the other bound shape's centre).
                    let other = if start { arrow.f.end_binding.get() } else { arrow.f.start_binding.get() };
                    other
                        .and_then(|o| self.get(&o.element_id))
                        .map(geom::center)
                        .unwrap_or(world[j])
                } else {
                    world[j]
                };
                let gap = b.gap.get().map(|g| g.0).unwrap_or(4.0).clamp(0.0, 20.0);
                let p = geom::outline_toward_center(target, adj);
                let d = geom::dist(adj, p);
                world[i] = if d > 1e-6 {
                    [p[0] + (adj[0] - p[0]) / d * gap, p[1] + (adj[1] - p[1]) / d * gap]
                } else {
                    p
                };
            }
            if let Some(a) = self.get_mut(&aid) {
                set_world_points(a, &world);
            }
        }
    }

    /// Re-lays out text bound to `container` (wrap, centre, grow container).
    pub fn layout_bound_text(&mut self, container: &str) {
        let Some(c) = self.get(container).cloned() else { return };
        let Some(tid) = c
            .bound_elements()
            .iter()
            .find(|b| b.kind == "text")
            .map(|b| b.id.clone())
        else {
            return;
        };
        let Some(t) = self.get(&tid).cloned() else { return };
        let fam = t.font_family();
        let size = t.font_size();
        let lh = t.line_height();
        let original = t
            .f
            .original_text
            .get()
            .cloned()
            .unwrap_or_else(|| t.text().to_string());
        let pad = 5.0;
        let (cw, ch) = (c.width().abs(), c.height().abs());
        let max_w = match c.kind.as_str() {
            "ellipse" => ((cw / 2.0) * std::f64::consts::SQRT_2).round() - pad * 2.0,
            "diamond" => (cw / 2.0).round() - pad * 2.0,
            _ => cw - pad * 2.0,
        }
        .max(1.0);
        let wrapped = text::wrap(&original, fam, size, max_w);
        let (tw, th) = text::measure(&wrapped, fam, size, lh);
        let max_h = match c.kind.as_str() {
            "ellipse" => ((ch / 2.0) * std::f64::consts::SQRT_2).round() - pad * 2.0,
            "diamond" => (ch / 2.0).round() - pad * 2.0,
            _ => ch - pad * 2.0,
        };
        let mut container_h = ch;
        if th > max_h {
            container_h = match c.kind.as_str() {
                "ellipse" => ((th + pad * 2.0) / std::f64::consts::SQRT_2 * 2.0).round(),
                "diamond" => (th + pad * 2.0) * 2.0,
                _ => th + pad * 2.0,
            };
        }
        if (container_h - ch).abs() > 0.01 {
            if let Some(cm) = self.get_mut(container) {
                cm.f.height.set(Num(container_h));
            }
        }
        let c = self.get(container).cloned().unwrap();
        let cc = [c.x() + c.width() / 2.0, c.y() + c.height() / 2.0];
        let tw = tw.min(max_w.max(tw.min(max_w)));
        let y = match t.vertical_align() {
            "top" => c.y() + pad,
            "bottom" => c.y() + c.height() - th - pad,
            _ => cc[1] - th / 2.0,
        };
        let x = match t.text_align() {
            "left" => c.x() + pad,
            "right" => c.x() + c.width() - tw - pad,
            _ => cc[0] - tw / 2.0,
        };
        let angle = c.angle();
        if let Some(tm) = self.get_mut(&tid) {
            tm.f.text.set(wrapped);
            tm.f.width.set(Num(tw));
            tm.f.height.set(Num(th));
            tm.f.x.set(Num(x));
            tm.f.y.set(Num(y));
            tm.f.angle.set(Num(angle));
        }
    }

    /// Lays out every bound text in the scene (after load or reload this is
    /// *not* called, to keep untouched files byte-identical).
    pub fn bound_containers_of(&self, ids: &HashSet<String>) -> Vec<String> {
        self.elements()
            .filter(|e| ids.contains(&e.id))
            .filter(|e| e.bound_elements().iter().any(|b| b.kind == "text"))
            .map(|e| e.id.clone())
            .collect()
    }

    /// Binds an arrow end to `target` (`start` or end).
    pub fn bind(&mut self, arrow: &str, start: bool, target: Option<&str>) {
        let old = self.get(arrow).and_then(|a| {
            if start { a.f.start_binding.get() } else { a.f.end_binding.get() }
                .map(|b| b.element_id.clone())
        });
        if let Some(a) = self.get_mut(arrow) {
            let field = if start { &mut a.f.start_binding } else { &mut a.f.end_binding };
            match target {
                Some(t) => field.set(Binding {
                    element_id: t.to_string(),
                    focus: Field::Value(Num(0.0)),
                    gap: Field::Value(Num(4.0)),
                    fixed_point: Field::Null,
                    extra: Map::new(),
                }),
                None => field.set_null(),
            }
        }
        // Keep the other end's binding intact when checking whether the old
        // target is still referenced.
        let still = |s: &Scene, t: &str| {
            s.get(arrow)
                .map(|a| {
                    [&a.f.start_binding, &a.f.end_binding]
                        .iter()
                        .any(|b| b.get().map(|b| b.element_id == t).unwrap_or(false))
                })
                .unwrap_or(false)
        };
        if let Some(o) = old {
            if Some(o.as_str()) != target && !still(self, &o) {
                if let Some(t) = self.get_mut(&o) {
                    if let Some(b) = t.f.bound_elements.get_mut() {
                        b.retain(|be| be.id != arrow);
                    }
                }
            }
        }
        if let Some(t) = target {
            if let Some(te) = self.get_mut(t) {
                let mut b = te.bound_elements().to_vec();
                if !b.iter().any(|be| be.id == arrow) {
                    b.push(BoundElement { id: arrow.to_string(), kind: "arrow".into(), extra: Map::new() });
                    te.f.bound_elements.set(b);
                }
            }
        }
    }
}

pub fn default_app_state() -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("gridSize".into(), Value::from(20));
    m.insert("gridStep".into(), Value::from(5));
    m.insert("gridModeEnabled".into(), Value::from(false));
    m.insert("viewBackgroundColor".into(), Value::from("#ffffff"));
    m
}

/// Gives every element a fresh id (and fresh group ids), remapping
/// bindings, bound elements, containers and frames within the set.
pub fn remap_ids(els: &mut [Element]) {
    let mut map: HashMap<String, String> = HashMap::new();
    let mut gmap: HashMap<String, String> = HashMap::new();
    for e in els.iter() {
        map.insert(e.id.clone(), random_id());
    }
    for e in els.iter_mut() {
        e.id = map[&e.id].clone();
        e.f.seed.set(Num(random_int()));
        e.f.version.set(Num(1.0));
        e.f.version_nonce.set(Num(random_int()));
        e.f.updated.set(Num(now_ms()));
        if let Some(g) = e.f.group_ids.get_mut() {
            for gid in g.iter_mut() {
                *gid = gmap.entry(gid.clone()).or_insert_with(random_id).clone();
            }
        }
        if let Some(b) = e.f.bound_elements.get_mut() {
            b.retain(|be| map.contains_key(&be.id));
            for be in b.iter_mut() {
                be.id = map[&be.id].clone();
            }
        }
        for binding in [&mut e.f.start_binding, &mut e.f.end_binding] {
            let new = binding.get().map(|b| map.get(&b.element_id).cloned());
            match new {
                Some(Some(id)) => binding.get_mut().unwrap().element_id = id,
                Some(None) => binding.set_null(),
                None => {}
            }
        }
        let c = e.f.container_id.get().map(|c| map.get(c).cloned());
        match c {
            Some(Some(id)) => e.f.container_id.set(id),
            Some(None) => e.f.container_id.set_null(),
            None => {}
        }
        let fr = e.f.frame_id.get().map(|c| map.get(c).cloned());
        match fr {
            Some(Some(id)) => e.f.frame_id.set(id),
            Some(None) => e.f.frame_id.set_null(),
            None => {}
        }
    }
}

/// Sets a linear element's points from world positions, normalising so
/// `points[0] == [0, 0]` and keeping rotation consistent.
pub fn set_world_points(el: &mut Element, world: &[geom::P]) {
    let local: Vec<geom::P> = world.iter().map(|w| geom::to_local(el, *w)).collect();
    set_points_preserving(el, &local);
}

/// `local` are points in the element's current local frame (may be
/// unnormalised). Normalises and shifts `x`/`y` so the points keep their
/// world positions under the element's rotation.
pub fn set_points_preserving(el: &mut Element, local: &[geom::P]) {
    if local.is_empty() {
        return;
    }
    let a = el.angle();
    let cl_old = geom::local_center(el);
    let shift = local[0];
    let pts: Vec<geom::P> = local.iter().map(|p| [p[0] - shift[0], p[1] - shift[1]]).collect();
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for p in &pts {
        b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
    }
    let cl_new = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
    // o' = o + cl - cl' + R(shift + cl' - cl)
    let v = [shift[0] + cl_new[0] - cl_old[0], shift[1] + cl_new[1] - cl_old[1]];
    let rv = geom::rotate(v, [0.0, 0.0], a);
    let nx = el.x() + cl_old[0] - cl_new[0] + rv[0];
    let ny = el.y() + cl_old[1] - cl_new[1] + rv[1];
    el.f.x.set(Num(nx));
    el.f.y.set(Num(ny));
    el.set_points(&pts);
    el.f.width.set(Num(b[2] - b[0]));
    el.f.height.set(Num(b[3] - b[1]));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untouched_round_trip_is_exact() {
        let src = r##"{"type":"excalidraw","version":2,"source":"x","elements":[{"id":"a","type":"rectangle","x":1.5,"y":2,"width":10,"height":10,"weird":{"k":[1,2]}},{"id":5}],"appState":{"viewBackgroundColor":"#fff"},"files":{}}"##;
        let s = Scene::parse(src).unwrap();
        let a: Value = serde_json::from_str(src).unwrap();
        assert_eq!(s.to_value(), a);
    }

    #[test]
    fn rotated_points_keep_world_position() {
        let mut el: Element = serde_json::from_str(
            r#"{"id":"l","type":"line","x":10,"y":10,"width":100,"height":0,"angle":0.5,"points":[[0,0],[100,0]]}"#,
        )
        .unwrap();
        let w0 = geom::to_world(&el, [0.0, 0.0]);
        let w1 = geom::to_world(&el, [100.0, 0.0]);
        let w2 = [w1[0] + 30.0, w1[1] + 40.0];
        set_world_points(&mut el, &[w0, w1, w2]);
        let pts = el.points();
        for (p, w) in pts.iter().zip([w0, w1, w2]) {
            let q = geom::to_world(&el, *p);
            assert!(geom::dist(q, w) < 1e-6, "{q:?} vs {w:?}");
        }
        assert_eq!(pts[0], [0.0, 0.0]);
    }

    #[test]
    fn remove_cleans_references() {
        let mut s = Scene::default();
        let d = ItemDefaults::default();
        let r = Scene::new_element("rectangle", 0.0, 0.0, &d);
        let a = Scene::new_element("arrow", 0.0, 0.0, &d);
        let (rid, aid) = (r.id.clone(), a.id.clone());
        s.add(r);
        s.add(a);
        s.bind(&aid, false, Some(&rid));
        assert_eq!(s.get(&rid).unwrap().bound_elements().len(), 1);
        s.remove(&[rid.clone()].into_iter().collect());
        assert!(s.get(&aid).unwrap().f.end_binding.is_null());
    }
}
