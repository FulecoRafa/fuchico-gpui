//! Backend for the GPUI port of Fuchico: a tauri-free port of
//! `src-tauri/src/modules` plus the frontend's pure data-processing helpers.

pub mod agenda;
pub mod attachments;
pub mod backlinks;
pub mod caldav;
pub mod collate;
pub mod doc_stats;
pub mod editor_window;
pub mod events;
pub mod fonts;
pub mod frontmatter;
pub mod fs;
pub mod fuzzy;
pub mod links;
pub mod open_files;
pub mod outline;
pub mod parse_tags;
pub mod paths;
pub mod render_html;
pub mod runtime;
pub mod search;
pub mod shortcuts;
pub mod tags;
pub mod task_helpers;
pub mod tasks;
pub mod templates;
pub mod watcher;
pub mod wikilinks;

pub use events::{CoreEvent, EventSink};
