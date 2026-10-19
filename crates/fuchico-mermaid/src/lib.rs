//! Native Mermaid support for Fuchico: the original's renderer (pele, run in
//! an embedded QuickJS engine) -> SVG -> resvg raster -> GPUI, with the
//! original's pan/zoom, toolbar and error UX.

pub mod color;
pub mod fonts;
pub mod icons;
pub mod measure;
pub mod panzoom;
pub mod pele;
pub mod raster;
pub mod render;
pub mod samples;
pub mod svgvars;
pub mod theme;
pub mod view;

pub use fonts::add_fonts;
pub use panzoom::{DragState, PanZoom};
pub use pele::warm_up;
pub use raster::{Raster, rasterize, rasterize_cached};
pub use render::{Diagram, MermaidError, render, render_svg};
pub use theme::MermaidTheme;
pub use view::{BlockStatus, Chrome, MermaidEvent, MermaidStrings, MermaidView};
