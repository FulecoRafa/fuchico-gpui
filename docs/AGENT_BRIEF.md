# Brief for feature engineers (wave 2)

Native GPUI port of Fuchico (original: Tauri+React at /Users/fuleco/Documents/Dev/obsidian-helix — read its source freely, never modify it).
Acceptance criterion: FEATURE PARITY with the original, as specified in docs/PARITY.md / docs/parity/*.md. Decisions: docs/DECISIONS.md.

- You work in your own git worktree (path given in your task) on your own branch. Commit your work there in small logical commits
  (message style: "<area>: <what>", end with a blank line + `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`). Never push, never touch main or other worktrees.
- Own ONLY the module dirs named in your task. Touch shared files (workspace/mod.rs, actions.rs, keymap.rs, bus.rs, i18n catalogs, Cargo.toml) minimally and
  additively (append new variants/keys; don't reorder/reformat) — other engineers edit them in parallel and the integrator merges.
- Use the existing extension points (PaneItem/PaneHandle, MainView, Dock, bus, SettingsStore, UiState, AppTheme, t!, ui::* helpers). Backend logic lives in
  crates/fuchico-core (see its README) — reuse, don't duplicate. Live-preview/editor logic: crates/fuchico-markdown, crates/fuchico-helix.
- Every user-visible string goes through i18n (en + pt-BR), including ones the original hardcoded in English.
- UI stack: gpui-kit 0.7 (gpui-pre 0.3.9 + gpui-component 0.7.3). Read sources in ~/.cargo/registry/src/index.crates.io-*/ instead of guessing APIs.
  Match the original's look (docs/screenshots/*.png in the original repo, src/styles/globals.css) — sizes, fonts, spacing, colors via theme tokens.
- No blocking IO on the UI thread. Unit-test pure logic.
- Verify: `cargo build`, `cargo test --workspace`, `cargo clippy -p fuchico`. Then RUN the app and exercise your features with real input events:
  `FUCHICO_CONFIG_DIR=<your scratch dir> cargo run -- <vault>`; use a COPY of /Users/fuleco/Documents/Dev/obsidian-helix/demo-notes as vault.
  Drive it with python Quartz CGEvent / cliclick aimed at the app window; screenshot ONLY the app window
  (window id via CGWindowListCopyWindowInfo — the owner name may show as "Zed"; `screencapture -x -o -l <id>`). NEVER capture the full screen —
  the user is using this machine. Kill only the processes you started.
- Final reply: what's done (map to parity checklist bullets), what's verified interactively vs only compiled, known gaps, commits on your branch.
