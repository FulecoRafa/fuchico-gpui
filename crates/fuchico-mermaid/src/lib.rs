//! Native Mermaid support for Fuchico: `mermaid-rs-renderer` -> SVG ->
//! resvg raster -> GPUI, with the original's pan/zoom, toolbar and error UX.

pub mod color;
pub mod icons;
pub mod panzoom;
pub mod raster;
pub mod render;
pub mod samples;
pub mod theme;
pub mod view;

pub use panzoom::{DragState, PanZoom};
pub use raster::{Raster, rasterize, rasterize_cached};
pub use render::{MermaidError, render_svg};
pub use theme::MermaidTheme;
pub use view::{BlockStatus, Chrome, MermaidEvent, MermaidStrings, MermaidView};
