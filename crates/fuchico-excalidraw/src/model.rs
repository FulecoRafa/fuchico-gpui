//! Lossless `.excalidraw` (v2) document model.
//!
//! Every struct carries a `#[serde(flatten)] extra` map so fields this crate
//! does not know about survive a load/save cycle. Every known field is a
//! [`Field`], which distinguishes *absent* from *null* so a parse → serialize
//! cycle reproduces exactly the keys the source had. Numbers go through
//! [`Num`], which serializes integral values without a trailing `.0` (the way
//! `JSON.stringify` does), so integers written by Excalidraw stay integers.
//!
//! Elements or files whose shape doesn't match the typed model (a future
//! element kind with a conflicting field type, garbage) are kept verbatim as
//! [`ElementEntry::Raw`] / [`FileEntry::Raw`].

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// A JSON object field that remembers whether it was absent, `null`, or set.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Field<T> {
    #[default]
    Absent,
    Null,
    Value(T),
}

impl<T> Field<T> {
    pub fn is_absent(&self) -> bool {
        matches!(self, Field::Absent)
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Field::Null)
    }
    pub fn get(&self) -> Option<&T> {
        match self {
            Field::Value(v) => Some(v),
            _ => None,
        }
    }
    pub fn get_mut(&mut self) -> Option<&mut T> {
        match self {
            Field::Value(v) => Some(v),
            _ => None,
        }
    }
    pub fn set(&mut self, v: T) {
        *self = Field::Value(v);
    }
    pub fn set_null(&mut self) {
        *self = Field::Null;
    }
    /// `Some(v)` → value, `None` → `null`.
    pub fn set_opt(&mut self, v: Option<T>) {
        *self = match v {
            Some(v) => Field::Value(v),
            None => Field::Null,
        };
    }
}

impl<T> From<Option<T>> for Field<T> {
    fn from(v: Option<T>) -> Self {
        match v {
            Some(v) => Field::Value(v),
            None => Field::Null,
        }
    }
}

impl<T: Serialize> Serialize for Field<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Field::Value(v) => v.serialize(s),
            _ => s.serialize_none(),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Field<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(d)? {
            Some(v) => Field::Value(v),
            None => Field::Null,
        })
    }
}

/// A JSON number serialized the way JavaScript does: integral values have no
/// fractional part (`100`, not `100.0`), non-finite values become `null`.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Default)]
pub struct Num(pub f64);

impl Num {
    pub fn get(self) -> f64 {
        self.0
    }
}

impl From<f64> for Num {
    fn from(v: f64) -> Self {
        Num(v)
    }
}

impl Serialize for Num {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let v = self.0;
        if !v.is_finite() {
            s.serialize_none()
        } else if v.fract() == 0.0 && v.abs() < 9.0e15 {
            s.serialize_i64(v as i64)
        } else {
            s.serialize_f64(v)
        }
    }
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        f64::deserialize(d).map(Num)
    }
}

pub type Point = [Num; 2];

/// `{ "type": 1|2|3, "value"?: number }`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Roundness {
    #[serde(rename = "type")]
    pub kind: Num,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub value: Field<Num>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// An entry of `boundElements`: `{ "id": "...", "type": "arrow" | "text" }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoundElement {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// An arrow's `startBinding` / `endBinding`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub element_id: String,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub focus: Field<Num>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub gap: Field<Num>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub fixed_point: Field<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

macro_rules! fields {
    ($(#[$m:meta])* pub struct $name:ident { $($(#[$fm:meta])* $f:ident : $t:ty,)* } ) => {
        $(#[$m])*
        pub struct $name {
            $(
                $(#[$fm])*
                #[serde(default, skip_serializing_if = "Field::is_absent")]
                pub $f: Field<$t>,
            )*
            #[serde(flatten)]
            pub extra: Map<String, Value>,
        }
    };
}

fields! {
    /// One Excalidraw element (any `type`). Type-specific fields are simply
    /// absent on other types.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
    #[serde(rename_all = "camelCase")]
    pub struct ElementFields {
        x: Num,
        y: Num,
        width: Num,
        height: Num,
        angle: Num,
        stroke_color: String,
        background_color: String,
        fill_style: String,
        stroke_width: Num,
        stroke_style: String,
        roughness: Num,
        opacity: Num,
        group_ids: Vec<String>,
        frame_id: String,
        index: String,
        roundness: Roundness,
        seed: Num,
        version: Num,
        version_nonce: Num,
        is_deleted: bool,
        bound_elements: Vec<BoundElement>,
        updated: Num,
        link: String,
        locked: bool,
        // text
        text: String,
        font_size: Num,
        font_family: Num,
        text_align: String,
        vertical_align: String,
        container_id: String,
        original_text: String,
        auto_resize: bool,
        line_height: Num,
        // linear + freedraw
        points: Vec<Point>,
        pressures: Vec<Num>,
        simulate_pressure: bool,
        last_committed_point: Value,
        start_binding: Binding,
        end_binding: Binding,
        start_arrowhead: String,
        end_arrowhead: String,
        elbowed: bool,
        // image
        status: String,
        file_id: String,
        scale: Point,
        crop: Value,
        // frame
        name: String,
    }
}

/// An element: `id` + `type` + every other field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub f: ElementFields,
}

/// An element as stored in the file: typed when it matches the model,
/// verbatim JSON otherwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ElementEntry {
    Element(Box<Element>),
    Raw(Value),
}

/// A `files` entry: an embedded binary (images) as a data URL.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileData {
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub mime_type: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub id: Field<String>,
    #[serde(rename = "dataURL", default, skip_serializing_if = "Field::is_absent")]
    pub data_url: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub created: Field<Num>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub last_retrieved: Field<Num>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileEntry {
    File(FileData),
    Raw(Value),
}

/// The whole `.excalidraw` document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExcalidrawFile {
    #[serde(rename = "type", default, skip_serializing_if = "Field::is_absent")]
    pub kind: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub version: Field<Num>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub source: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub elements: Field<Vec<ElementEntry>>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub app_state: Field<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub files: Field<IndexMap<String, FileEntry>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Errors surfaced to the user exactly like the Tauri app's pane.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadError {
    Malformed,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Malformed => f.write_str("Malformed drawing file"),
        }
    }
}

impl std::error::Error for LoadError {}

impl ExcalidrawFile {
    /// Parses file content. Empty / whitespace-only content is an empty
    /// drawing (the app creates `.excalidraw` files empty).
    pub fn parse(content: &str) -> Result<Self, LoadError> {
        let t = content.trim();
        if t.is_empty() {
            return Ok(Self::default());
        }
        let v: Value = serde_json::from_str(t).map_err(|_| LoadError::Malformed)?;
        if !v.is_object() {
            return Err(LoadError::Malformed);
        }
        serde_json::from_value(v).map_err(|_| LoadError::Malformed)
    }

    /// Pretty-printed, 2-space indented — what `serializeAsJSON` produces.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }

    pub fn app_state_str(&self, key: &str) -> Option<&str> {
        self.app_state.get()?.get(key)?.as_str()
    }

    pub fn app_state_f64(&self, key: &str) -> Option<f64> {
        self.app_state.get()?.get(key)?.as_f64()
    }
}

// ---------------------------------------------------------------- helpers

impl Element {
    pub fn num(f: &Field<Num>, default: f64) -> f64 {
        f.get().map(|n| n.0).unwrap_or(default)
    }
    pub fn x(&self) -> f64 {
        Self::num(&self.f.x, 0.0)
    }
    pub fn y(&self) -> f64 {
        Self::num(&self.f.y, 0.0)
    }
    pub fn width(&self) -> f64 {
        Self::num(&self.f.width, 0.0)
    }
    pub fn height(&self) -> f64 {
        Self::num(&self.f.height, 0.0)
    }
    pub fn angle(&self) -> f64 {
        Self::num(&self.f.angle, 0.0)
    }
    pub fn stroke_color(&self) -> &str {
        self.f
            .stroke_color
            .get()
            .map(|s| s.as_str())
            .unwrap_or("#1e1e1e")
    }
    pub fn background_color(&self) -> &str {
        self.f
            .background_color
            .get()
            .map(|s| s.as_str())
            .unwrap_or("transparent")
    }
    pub fn fill_style(&self) -> &str {
        self.f
            .fill_style
            .get()
            .map(|s| s.as_str())
            .unwrap_or("solid")
    }
    pub fn stroke_width(&self) -> f64 {
        Self::num(&self.f.stroke_width, 2.0)
    }
    pub fn stroke_style(&self) -> &str {
        self.f
            .stroke_style
            .get()
            .map(|s| s.as_str())
            .unwrap_or("solid")
    }
    pub fn roughness(&self) -> f64 {
        Self::num(&self.f.roughness, 1.0)
    }
    pub fn opacity(&self) -> f64 {
        Self::num(&self.f.opacity, 100.0)
    }
    pub fn seed(&self) -> u64 {
        Self::num(&self.f.seed, 1.0).abs() as u64
    }
    pub fn is_deleted(&self) -> bool {
        self.f.is_deleted.get().copied().unwrap_or(false)
    }
    pub fn locked(&self) -> bool {
        self.f.locked.get().copied().unwrap_or(false)
    }
    pub fn group_ids(&self) -> &[String] {
        self.f.group_ids.get().map(|v| v.as_slice()).unwrap_or(&[])
    }
    pub fn points(&self) -> Vec<[f64; 2]> {
        self.f
            .points
            .get()
            .map(|v| v.iter().map(|p| [p[0].0, p[1].0]).collect())
            .unwrap_or_default()
    }
    pub fn set_points(&mut self, pts: &[[f64; 2]]) {
        self.f
            .points
            .set(pts.iter().map(|p| [Num(p[0]), Num(p[1])]).collect());
    }
    pub fn font_size(&self) -> f64 {
        Self::num(&self.f.font_size, 20.0)
    }
    pub fn font_family(&self) -> u32 {
        Self::num(&self.f.font_family, 5.0) as u32
    }
    pub fn line_height(&self) -> f64 {
        Self::num(&self.f.line_height, 1.25)
    }
    pub fn link(&self) -> &str {
        self.f.link.get().map(|s| s.as_str()).unwrap_or("")
    }
    pub fn text(&self) -> &str {
        self.f.text.get().map(|s| s.as_str()).unwrap_or("")
    }
    pub fn text_align(&self) -> &str {
        self.f
            .text_align
            .get()
            .map(|s| s.as_str())
            .unwrap_or("left")
    }
    pub fn vertical_align(&self) -> &str {
        self.f
            .vertical_align
            .get()
            .map(|s| s.as_str())
            .unwrap_or("top")
    }
    pub fn container_id(&self) -> Option<&str> {
        self.f.container_id.get().map(|s| s.as_str())
    }
    pub fn bound_elements(&self) -> &[BoundElement] {
        self.f
            .bound_elements
            .get()
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
    pub fn is_linear(&self) -> bool {
        matches!(self.kind.as_str(), "line" | "arrow")
    }
    pub fn has_points(&self) -> bool {
        matches!(self.kind.as_str(), "line" | "arrow" | "freedraw")
    }
}
