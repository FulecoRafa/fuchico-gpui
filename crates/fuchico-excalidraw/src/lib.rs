//! Native Excalidraw editor for GPUI: a lossless `.excalidraw` (v2) model,
//! a roughr-based renderer and an editable canvas view.

pub mod color;
pub mod fractional;
pub mod geom;
pub mod model;
pub mod render;
pub mod scene;
pub mod shape;
pub mod style;
pub mod text;
mod ui;
pub mod view;

pub use model::{ExcalidrawFile, LoadError};
pub use scene::Scene;
pub use style::ExcalidrawStyle;
pub use view::{DrawingSource, ExcalidrawEvent, ExcalidrawView, OnChange, StyleFn, Tool, SAVE_DEBOUNCE};
