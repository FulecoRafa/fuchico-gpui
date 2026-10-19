`pele.global.js` is the unmodified IIFE build of [pele](https://github.com/obsidianmd/pele) 1.5.2
(npm `pele@1.5.2`, `dist/pele.global.js`), the renderer the original Tauri app uses. MIT, see LICENSE.
It runs inside an embedded QuickJS engine (`src/pele.rs`). To upgrade, replace the file and update
`PELE_VERSION` in `src/pele.rs`.
