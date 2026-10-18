# Port decisions

- UI stack: `gpui-kit` 0.7 (gpui-pre 0.3.9 + gpui-component). All GPUI crates must use this same gpui-pre.
- Editor: custom GPUI element over `fuchico-helix` (modal engine, spec = codemirror-helix 0.5.7 behaviour, quirks copied) and `fuchico-markdown` (live-preview model).
- Vim mode: in scope; implemented as a second engine behind the same trait.
- Excalidraw: native, `gpui-whiteboard` + lossless `.excalidraw` adapter (unknown fields preserved). No webview.
- Mermaid: `mermaid-rs-renderer` → SVG → rasterized, themed via token mapping.
- PDF export: generated natively (no print webview).
- Persistence: same JSON shapes as the Tauri app's localStorage keys, stored as files in the app config dir (`fuchico_core::paths`).
- QUIRKs from PARITY.md: genuine bugs (data loss, UTC "today", double checkbox, `🔁 weekly`, second window not reloading, toggle-theme from system) are FIXED; behavioural deviations of codemirror-helix are COPIED.
- There is now a real file watcher (`fuchico_core::watcher`), superset of the old event-only refresh.
