//! Excalidraw's personal library (`.excalidrawlib`, version 2), persisted
//! to the app data directory.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::model::Element;
use crate::scene;

pub const LIBRARY_ENV: &str = "FUCHICO_EXCALIDRAW_LIBRARY";

#[derive(Clone, Debug, Default)]
pub struct Library {
    /// Raw library items (`{status, elements, id, created, name?}`), kept
    /// verbatim so unknown keys survive a round trip.
    pub items: Vec<Value>,
    pub path: Option<PathBuf>,
}

/// `$FUCHICO_EXCALIDRAW_LIBRARY`, else
/// `<data dir>/fuchico/excalidraw/library.excalidrawlib`.
pub fn default_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(LIBRARY_ENV) {
        return Some(PathBuf::from(p));
    }
    dirs::data_dir().map(|d| {
        d.join("fuchico")
            .join("excalidraw")
            .join("library.excalidrawlib")
    })
}

/// Parses `.excalidrawlib` content (v2 `libraryItems`, or v1 `library`).
pub fn parse(content: &str) -> Result<Vec<Value>, String> {
    let v: Value = serde_json::from_str(content).map_err(|e| e.to_string())?;
    if v.get("type").and_then(|t| t.as_str()) != Some("excalidrawlib") {
        return Err("Not an Excalidraw library".into());
    }
    if let Some(items) = v.get("libraryItems").and_then(|i| i.as_array()) {
        return Ok(items
            .iter()
            .filter(|i| i.get("elements").map(|e| e.is_array()).unwrap_or(false))
            .cloned()
            .collect());
    }
    if let Some(items) = v.get("library").and_then(|i| i.as_array()) {
        return Ok(items
            .iter()
            .filter_map(|els| els.as_array())
            .map(|els| new_item(els.clone()))
            .collect());
    }
    Err("Library has no items".into())
}

fn new_item(elements: Vec<Value>) -> Value {
    json!({
        "status": "unpublished",
        "elements": elements,
        "id": scene::random_id(),
        "created": scene::now_ms(),
    })
}

pub fn item_id(item: &Value) -> &str {
    item.get("id").and_then(|i| i.as_str()).unwrap_or("")
}

pub fn item_name(item: &Value) -> Option<&str> {
    item.get("name").and_then(|i| i.as_str())
}

/// The item's elements, deleted ones dropped.
pub fn item_elements(item: &Value) -> Vec<Element> {
    item.get("elements")
        .and_then(|e| e.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| serde_json::from_value::<Element>(x.clone()).ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !e.is_deleted())
        .collect()
}

impl Library {
    pub fn load(path: Option<PathBuf>) -> Self {
        let items = path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| parse(&s).ok())
            .unwrap_or_default();
        Self { items, path }
    }

    pub fn to_json(&self) -> String {
        to_json(&self.items)
    }

    pub fn save(&self) -> Result<(), String> {
        let Some(p) = &self.path else { return Ok(()) };
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(p, self.to_json()).map_err(|e| e.to_string())
    }

    /// Adds an item made of `elements` at the top; returns its id.
    pub fn add(&mut self, elements: Vec<Value>) -> String {
        let item = new_item(elements);
        let id = item_id(&item).to_string();
        self.items.insert(0, item);
        if let Err(e) = self.save() {
            log::warn!("excalidraw library save failed: {e}");
        }
        id
    }

    pub fn remove(&mut self, id: &str) {
        self.items.retain(|i| item_id(i) != id);
        if let Err(e) = self.save() {
            log::warn!("excalidraw library save failed: {e}");
        }
    }

    /// Merges items from `.excalidrawlib` content (skipping ids already
    /// present); returns how many were added.
    pub fn import(&mut self, content: &str) -> Result<usize, String> {
        let items = parse(content)?;
        let mut n = 0;
        for it in items {
            if self
                .items
                .iter()
                .any(|x| item_id(x) == item_id(&it) && !item_id(&it).is_empty())
            {
                continue;
            }
            self.items.push(it);
            n += 1;
        }
        self.save()?;
        Ok(n)
    }

    pub fn export_to(&self, path: &Path) -> Result<(), String> {
        std::fs::write(path, self.to_json()).map_err(|e| e.to_string())
    }

    pub fn get(&self, id: &str) -> Option<&Value> {
        self.items.iter().find(|i| item_id(i) == id)
    }
}

pub fn to_json(items: &[Value]) -> String {
    let mut m = Map::new();
    m.insert("type".into(), Value::from("excalidrawlib"));
    m.insert("version".into(), Value::from(2));
    m.insert("source".into(), Value::from("https://excalidraw.com"));
    m.insert("libraryItems".into(), Value::Array(items.to_vec()));
    serde_json::to_string_pretty(&Value::Object(m)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_round_trip_and_v1_import() {
        let dir = std::env::temp_dir().join(format!("fxlib-{}", scene::random_id()));
        let path = dir.join("lib.excalidrawlib");
        let mut lib = Library::load(Some(path.clone()));
        assert!(lib.items.is_empty());
        let id = lib.add(vec![
            json!({"id":"a","type":"rectangle","x":0,"y":0,"width":10,"height":10}),
        ]);
        let back = Library::load(Some(path.clone()));
        assert_eq!(back.items.len(), 1);
        assert_eq!(item_id(&back.items[0]), id);
        assert_eq!(item_elements(&back.items[0]).len(), 1);
        let v1 = r#"{"type":"excalidrawlib","version":1,"library":[[{"id":"b","type":"ellipse","x":0,"y":0,"width":5,"height":5}]]}"#;
        let mut lib = back;
        assert_eq!(lib.import(v1).unwrap(), 1);
        assert_eq!(Library::load(Some(path)).items.len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
