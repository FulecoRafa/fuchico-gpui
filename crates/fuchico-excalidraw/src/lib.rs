//! Native Excalidraw editor for GPUI: a lossless `.excalidraw` (v2) model,
//! a roughr-based renderer and an editable canvas view.

mod actions;
pub mod color;
pub mod elbow;
pub mod export;
pub mod fractional;
pub mod geom;
mod input;
pub mod library;
pub mod model;
pub mod render;
pub mod scene;
pub mod shape;
pub mod snap;
pub mod style;
pub mod text;
mod ui;
pub mod view;

pub use model::{ExcalidrawFile, LoadError};
pub use scene::Scene;
pub use style::ExcalidrawStyle;
pub use view::{DrawingSource, ExcalidrawEvent, ExcalidrawView, OnChange, StyleFn, Tool, SAVE_DEBOUNCE};
pub use export::ExportOptions;
