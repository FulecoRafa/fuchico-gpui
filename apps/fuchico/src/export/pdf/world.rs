//! A minimal in-memory Typst [`World`]: one main source plus the images /
//! diagrams the note references. Fonts are the app's bundled Inter and
//! JetBrains Mono, Typst's embedded fallbacks and the system fonts; they
//! are scanned once and cached for the process lifetime.

use std::collections::HashMap;
use std::sync::OnceLock;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::fonts::FontStore;

fn fonts() -> &'static FontStore {
    static STORE: OnceLock<FontStore> = OnceLock::new();
    STORE.get_or_init(|| {
        let mut store = FontStore::new();
        // App fonts first: they win family lookups over system copies.
        for data in crate::assets::FONTS {
            for font in Font::iter(Bytes::new(*data)) {
                let info = font.info().clone();
                store.push((font, info));
            }
        }
        store.extend(typst_kit::fonts::embedded());
        store.extend(typst_kit::fonts::system());
        store
    })
}

/// Warms the font cache (system font scan) off the UI thread.
pub fn prewarm() {
    let _ = fonts();
}

fn library() -> &'static LazyHash<Library> {
    static LIB: OnceLock<LazyHash<Library>> = OnceLock::new();
    LIB.get_or_init(|| LazyHash::new(Library::builder().build()))
}

/// A stable, interned id for a virtual path (ids are leaked globally, so
/// callers reuse a small set of names).
pub fn file_id(path: &str) -> Option<FileId> {
    let vpath = VirtualPath::new(path).ok()?;
    Some(RootedPath::new(VirtualRoot::Project, vpath).intern())
}

pub struct DocWorld {
    main: Source,
    files: HashMap<FileId, Bytes>,
}

impl DocWorld {
    pub fn new(
        source: String,
        files: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) -> Result<Self, String> {
        let main_id = file_id("/main.typ").ok_or("bad main path")?;
        let mut map = HashMap::new();
        for (name, bytes) in files {
            let id = file_id(&name).ok_or_else(|| format!("bad file path {name}"))?;
            map.insert(id, Bytes::new(bytes));
        }
        Ok(DocWorld {
            main: Source::new(main_id, source),
            files: map,
        })
    }
}

impl World for DocWorld {
    fn library(&self) -> &LazyHash<Library> {
        library()
    }

    fn book(&self) -> &LazyHash<FontBook> {
        fonts().book()
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            Ok(self.main.clone())
        } else {
            Err(FileError::NotSource)
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main.id() {
            return Ok(Bytes::from_string(self.main.text().to_string()));
        }
        self.files.get(&id).cloned().ok_or(FileError::AccessDenied)
    }

    fn font(&self, index: usize) -> Option<Font> {
        fonts().font(index)
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}
