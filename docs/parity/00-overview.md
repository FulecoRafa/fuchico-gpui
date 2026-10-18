# Fuchico: Feature-Parity Specification (Tauri/React → native Rust + GPUI)

Source of truth: `/Users/fuleco/Documents/Dev/obsidian-helix` at commit `6fe6dd1` plus the uncommitted working tree as of 2026-10-18. That working tree replaces mermaid with `pele`, adds `demo-notes/templates/first_template.md` and moves `Desenho.excalidraw`. App version `0.1.0`, identifier `dev.fuleco.fuchico`.

**Acceptance criterion: feature parity.** Every `- [ ]` item below is a testable behavior. Paths are relative to the repo root unless they are absolute. Items marked **QUIRK** / "possible bug" describe current behavior that looks unintended. Port it faithfully, or fix it deliberately and record the decision. Do not drop it silently.

## Contents

0. Overview and cross-cutting indexes (this part)
   - 0.1 Architecture map
   - 0.2 Windows
   - 0.3 Persistence inventory
   - 0.4 IPC inventory
   - 0.5 Master keyboard index
   - 0.6 Palette command index
   - 0.7 Hardest areas / porting risks
1. App Shell, Layout, Windows, Tabs, Explorer
2. Editor Core and Display Extensions: extension order, gutters, modes, in-editor search, load/save, languages, live preview, tables, code blocks, shortcuts, scroll persistence, outline, folding/regions, rulers, stats
3. Editor Extensions: wikilinks, tags, completion, tasks, attachments, path drop, mermaid preview button, frontmatter, backlinks, tags view, vault search/replace
4. Command Palette, Quick Switcher, Templates and Daily Notes, Agenda, Excalidraw, Mermaid diagrams, Export
5. Settings, Themes and i18n: every setting key and default, every section, shortcuts, zoom, theme variables per theme, the full en string catalog
6. Backend: every Tauri command, events, FS semantics, search, tags/links/tasks parsing, CalDAV, fonts, open-files, app setup
7. Helix modal editing (codemirror-helix 0.5.7): full key and typable-command list, deviations, unsupported features

---

## 0.1 Architecture map (what replaces what)

| Today | Role | GPUI port equivalent |
|---|---|---|
| `src/main.tsx` URL routing (`?window=editor|print`, `?mermaidBlockKey`) | One HTML entry for 4 window kinds | Window kinds created natively: main, editor, mermaid, print/export |
| React state in `App.tsx` | Main view, tabs, dock, vault root | App model entity |
| CodeMirror 6 + `@codemirror/lang-markdown` + custom ViewPlugins | Editor, live preview, tables, folding, completion | Custom GPUI editor element: rope buffer, tree-sitter (or equivalent) markdown/code parsing, decoration layer |
| `codemirror-helix` 0.5.7 | Helix modal editing | Native modal engine (section 7) |
| `@replit/codemirror-vim` | Vim mode (setting `keybindingMode: "vim"`) | Native vim layer, or an explicit decision to drop it (it is a user-selectable setting today, so it is in parity scope) |
| `@excalidraw/excalidraw` 0.18 React component | Full drawing editor for `.excalidraw` files | **Largest single gap:** a native canvas editor, or an embedded webview for this one pane (see 0.7) |
| `pele` (mermaid-compatible SVG renderer) | Diagram rendering in the dock, window and export | Native mermaid renderer, or rasterized SVG via a Rust mermaid lib plus `resvg` |
| `marked` | Markdown → HTML for export | `pulldown-cmark`/`comrak` with the same output rules (section 4.13) |
| Webview `window.print()` via `export_print` | PDF export | Native print / PDF generation |
| `localStorage` | Settings, scroll positions, last vault, settings tab | Files in the app config dir, same JSON shapes (0.3) |
| Tauri events (`emit`/`listen`) | Cross-window bus | In-process event bus or entity subscriptions (all windows share one process) |
| Rust backend (`src-tauri/src`) | FS, search, tags, links, tasks, CalDAV, fonts, open-files | Reused largely as-is, as plain Rust modules called directly instead of through `invoke` |
| `tauri_plugin_single_instance`, `window_state`, `dialog`, `opener` | Single instance, window geometry, native dialogs, open/reveal | Equivalent crates or native code (`rfd`, `open`, etc.) |
| `keyring` | CalDAV passwords | Same crate |

## 0.2 Windows

| Kind | Label | Created by | Content | Notes |
|---|---|---|---|---|
| Main | `main` | `tauri.conf.json` | Full app | 1000×700 default; title always "Fuchico"; geometry persisted |
| Editor | `editor-*` | `src/lib/editorWindow.ts` (palette "Open in New Window", explorer "Open in new window") | Single-file `EditorPane`, no palette/sidebar | Geometry persisted; `:q` closes the window; zoom shortcuts work; Mod+P does not |
| Mermaid | `mermaid-<blockKey>` | `src/modules/mermaid/lib/mermaidWindow.ts` | Pop-out diagram, live-updated from the source editor | Can re-dock (`mermaid:dock-request`) |
| Print | `export-*` | `src/modules/export/lib/exportActions.ts` | Rendered HTML, then the native print dialog | Fetches HTML through `export:request` → `export:html` |

Window details are in sections 1 (Process/windows, Editor windows), 4 (sections 12 and 15) and 6 (sections 7.2–7.5).

## 0.3 Persistence inventory (everything that survives a restart)

| Store | Key / path | Shape | Owner / section |
|---|---|---|---|
| localStorage | `helix.editorSettings` | One JSON object with all 19 setting keys (exact defaults in 5 §1.2) | `src/modules/settings/lib/editorSettings.ts` |
| localStorage | `helix.lastRootPath` | Absolute vault path string; validated with `fs_stat` at launch and removed if missing | `src/App.tsx` (section 1) |
| localStorage | `helix.scrollPositions` | JSON of per-file scroll entries, capped at 500 entries, written with a 400 ms debounce (exact shape in 2 §12) | `src/modules/editor/lib/scrollPositions.ts` |
| localStorage | `helix.settingsSection` | Last settings section id; default `"appearance"` | `src/modules/settings/lib/settingsNav.ts` |
| App data file | `{app_data_dir}/caldav/config.json` | `{accounts:[{id,serverUrl,username}], links:[{accountId,calendarHref,calendarDisplayName,folderPath}]}`; never holds passwords | 6 §6.1 |
| OS keyring | service `dev.fuleco.fuchico`, account = account UUID | password | 6 §6.2 |
| Vault file | `{linkedFolder}/.fuchico-sync.json` | CalDAV sidecar: task identity, etags, hashes | 6 §6.3 |
| Vault file | `{vault}/Synced Tasks.md` (inside the linked folder) | New remote tasks appended under `## {calendar name}` | 6 §6.7 |
| Vault files | `{vault}/attachments/…` | Pasted/dropped images (naming rules in 3) | 3 "Image paste / drop" |
| Vault files | `{dailyNotesFolder}/YYYY-MM-DD.md` (default `daily/`) | Daily notes, seeded from `{templatesFolder}/daily.md` | 4 §6 |
| Vault files | `{templatesFolder}/*.md` (default `templates/`) | Templates with `{{…}}` variables | 4 §6 |
| User file (export/import) | custom theme CSS files | See 5 §3.6 | `themeFiles.ts` |
| Plugin state | window geometry per label (`tauri_plugin_window_state`) | size and position | 1, 6 §7.3 |
| **Not persisted** | open tabs, active tab, expanded folders, main view, dock open state and width, agenda month/selection, Helix registers and undo history, Excalidraw library | in memory only | — |

Settings multi-window sync: every window observes `helix.editorSettings` changes (the `storage` event) and live-applies them. The port needs one shared settings model.

## 0.4 IPC inventory

**Tauri commands (31):** full table with args, returns and errors in section 6 §1. Frontend-used:
`fs_read_dir`, `fs_list_markdown_files`, `fs_read_file`, `fs_write_file`, `fs_write_binary`, `fs_stat`, `fs_create_file`, `fs_create_dir`, `fs_rename`, `fs_delete`, `fs_canonicalize`, `search_files`, `search_replace_files`, `tags_scan`, `links_scan`, `tasks_scan`, `tasks_toggle`, `fonts_list_system`, `export_print`, `open_requests_take`, `caldav_test_connection`, `caldav_discover_calendars`, `caldav_discover_calendars_for_account`, `caldav_save_account`, `caldav_list_accounts`, `caldav_remove_account`, `caldav_link_folder`, `caldav_unlink_folder`, `caldav_list_links`, `caldav_sync_now`, `caldav_get_sync_status`.

**Events** (full payloads in 6 §2 and 4 §16):

| Event | Payload | Emitted by → consumed by |
|---|---|---|
| `fs:file-written` | `{path, source?}` (`source` ∈ `editor`, `template`, `export`, `replace`, `caldav`, …) | `fs_write_file`, replace, CalDAV → editors (reload unless they wrote it), agenda, tags, backlinks, CalDAV on-save trigger (Rust) |
| `fs:changed` | `{dir, from: windowLabel}` | explorer mutations → other windows' explorers, vault file list, backlinks |
| DOM `helix:fs-changed` | `detail: dir` | same-window explorer refresh |
| `app:open-files` | `string[]` | OS open / second launch → main window (plus the `open_requests_take` queue) |
| `caldav:sync-progress` | `{folderPath}` | Rust; no listener |
| `caldav:sync-complete` | `{reports}` | Rust → settings sync status |
| `mermaid:update` | `{blockKey, text|null}` | source editor → dock/window |
| `mermaid:request` | `{blockKey}` | dock/window → source editor (asks it to resend) |
| `mermaid:closed` | `{blockKey}` | dock/window → source editor |
| `mermaid:dock-request` | `{blockKey, label}` | mermaid window → main (re-dock) |
| `export:request` / `export:html` | `{key}` / `{key, html}` | print window ↔ main |

There is **no filesystem watcher.** Every refresh comes from an app-initiated emit. External edits are seen only on re-read (see the conflict banner in 2 §6).

## 0.5 Master keyboard index

Details are in the referenced sections. Mod = Cmd on macOS, Ctrl elsewhere.

**Global (main window)**

| Keys | Action | Scope / notes | Ref |
|---|---|---|---|
| `Mod-p` | Quick Switcher | Window keydown (bubble); main window only | 1, 4 §1/§5 |
| `Mod-Shift-p` | Command Palette (empty query) | main window only | 1, 4 §1/§4 |
| `Mod-=` / `Mod--` / `Mod-0` | Editor font size +1 / −1 / reset to 13 (range 8–32) | Capture phase, by `e.code`; main and editor windows | 5 §2.4 |
| `Mod-Shift-=` / `Mod-Shift--` / `Mod-Shift-0` | UI scale +0.1 / −0.1 / reset to 1.0 (range 0.7–1.6) | same | 5 §2.4 |
| (none) | No tab-switching, Mod-W or Ctrl-Tab bindings exist | — | 1 |

**Editor: rebindable (defaults; stored in `shortcuts`)**

| Action id | Default | Ref |
|---|---|---|
| `openOutline` | `Mod-o` | 2 §13 |
| `toggleCheckboxAtCursor` | `Mod-Enter` | 2 §11 |
| `insertDate` | `Mod-Shift-d` (inserts `YYYY-MM-DD`) | 2 §11 |
| `insertDateTime` | `Mod-Alt-d` (inserts `YYYY-MM-DD HH:mm`) | 2 §11 |
| `insertRegion` | `Mod-Shift-r` | 2 §11/§14 |
| `insertTable` | `Mod-Alt-t` | 2 §9/§11 |
| `toggleTaskLine` | `Mod-Shift-Enter` | 3 Tasks |
| `pickDueDate` | `Mod-Shift-.` | 3 Tasks |
| `pickRecurrence` | `Mod-Shift-,` | 3 Tasks |

Precedence: `Prec.highest`, above the Helix/Vim keymaps. No conflict detection. Binding string format and capture rules: 5 §2.1.

**Editor: fixed**

| Keys | Action | Ref |
|---|---|---|
| `Mod-s` | Save (all keybinding modes) | 2 §6 |
| `Mod-f` | In-editor find/replace panel (`@codemirror/search`) | 2 §5 |
| `:` (Helix normal/select, focus in the editor content) | Opens the **app command palette** prefilled with `:` (main window only; editor windows open the native Helix prompt) | 2 §4, 4 §4.2 |
| `@` on a task line | Due/repeat completion (`@due`, `@today`, `@repeat`, 📅, 🔁) | 3 Tasks |
| `[[` | Wikilink completion; `[[Note#` / `[[#` heading completion | 3 |
| `#` | Tag completion | 3 |
| `Mod-v` / drop | Paste or drop an image into `attachments/` | 3 |
| Click | Follow wikilink / markdown link; tag pill → Tags view | 3 |
| `Ctrl-Shift-[` / `Ctrl-Shift-]` (mac `Cmd-Alt-[` / `Cmd-Alt-]`) | Fold / unfold | 2 §14 |
| `Ctrl-Alt-[` / `Ctrl-Alt-]` | Fold all / unfold all | 2 §14 |
| Completion popup keys | Up/Down/Enter/Tab/Escape (exact list in 3 "Completion aggregator") | 3 |
| Table keys | Tab/Shift-Tab/Enter cell navigation etc. (exact list in 2 §9) | 2 §9 |
| Outline overlay keys | Arrow/Enter/Escape (2 §13) | 2 §13 |
| Helix modal keys | Full list in section 7 | 7 |
| Vim mode | `@replit/codemirror-vim` defaults; `:w`/`:q` not wired | 2 §4 |

**Explorer (focused tree)**: `↑ ↓` move, `←` (moves the cursor only, see QUIRK in 1), `→` expand, `Enter` open/toggle, `F2` rename, `Delete`/`Backspace` delete (with confirm), `a–z` type-ahead (700 ms reset), `Mod-x` cut, `Mod-v` paste (move), `Mod-a` select all, `Escape` clear cut, `Shift+↑/↓` extend range; Shift-click range / Mod-click toggle (exact list in 1 "Selection, mouse and keyboard").

**Palette / quick switcher / pickers**: Up/Down, Enter, Escape, plus modifiers documented in 4 §2–§5.

**Mermaid view**: pan/zoom keys and wheel/pinch rules in 4 §12 (`usePanZoom`).

## 0.6 Palette command index

There are 22 registry rows, several of which expand to many commands. The full table with ids, en/pt-BR titles, conditions, keywords, binding hints and actions is in 4 §4.3. `:`-aliases (typed after `:` in the palette, Helix-style; `:<number>` = go to line):

| Alias(es) | Command |
|---|---|
| `:o` `:open` `:e` `:edit` | Quick Open |
| `:w` `:write` | Save File |
| `:wq` `:x` | Save and Close Tab |
| `:q` `:quit` `:bc` | Close Active Tab |
| `:qa` | Close All Tabs |
| `:backlinks` `:bl` | Toggle Backlinks |
| `:export` | Export as HTML… |
| `:pdf` `:print` | Export as PDF (Print)… |
| `:theme` | Toggle Theme |
| `:<n>` | Go to line n |

Other commands: Go to Editor / Tasks & Calendar / Search / Settings; `Editor: <action>` ×9; `Settings: <section>` ×6 (appearance, editor, shortcuts, vault, integrations, about); Open Folder…; Open Today's Daily Note; New Note from Template: `<stem>` (one per template); Keyboard Shortcuts; Open in New Window; Theme: Ayu / Dracula / Catppuccin / each custom theme.

## 0.7 Hardest areas / porting risks

1. **Live-preview markdown editor** (2 §8–§10, 3). The cursor-line-aware decorations hide and show markers per construct. Tables render as widgets with cell editing and navigation. Fenced code has a toolbar and per-language highlighting (143 languages with aliases). Frontmatter styling, tag pills, wikilink widgets, fold-region bars and pills, rulers, and a mermaid preview button all sit on the same decoration layer. GPUI has no CodeMirror, so this is a ground-up editor build.
2. **Helix modal engine** (7). The selection-first model, minor modes (g, m, space, [ ]), registers, plugin-owned snapshot undo, and the `:` prompt. Many package deviations from real Helix (`w`≈`e`, `h`/`l` wrap lines, non-linewise paste, text objects mostly no-ops) are now user-visible behavior. Decide per item whether to copy the deviation or fix it.
3. **Excalidraw** (4 §10). A full vector drawing app (`@excalidraw/excalidraw` 0.18). Natively that means reimplementing the editor, or hosting a webview for this pane only. The file format (`.excalidraw` JSON: `type`, `version`, `source`, `elements`, `appState`, `files`) must round-trip losslessly either way.
4. **Mermaid rendering** (4 §11–§12). `pele` produces themed SVG. The port needs a native renderer with equivalent diagram coverage and `--pele-*` → theme-token color mapping, plus pan/zoom and dock/pop-out sync.
5. **CalDAV two-way sync** (6 §6). Discovery, VTODO mapping, a three-stage fuzzy matcher (0.6 similarity / ±5 lines / 3-pass delete streak), etag and mtime conflict rules, sidecar identity, and triggers: launch, 15 min, 2 s on-save debounce, manual. This is mostly reusable Rust, but the triggers and the `fs:file-written` coupling must be re-plumbed.
6. **Vim mode.** It is a user-facing setting. Either port a vim layer or declare it out of scope explicitly.
7. **Export** (4 §13–§15). The markdown → HTML rules match `marked` output plus app-specific handling (wikilinks, tags, mermaid, task checkboxes, including the double-checkbox QUIRK). PDF goes through the platform print dialog.
8. **Theme system** (5 §4). 22 color tokens × 5 built-in theme variants (Ayu light/dark, Dracula, Catppuccin Latte/Mocha) plus non-color tokens. **Custom themes are user-authored CSS** overriding CSS variables, so the port needs a parser for at least `--var: value` declarations under the same selectors to keep existing custom themes working.
9. **i18n** (5 §5). 300 keys in en and pt-BR, with interpolation and plurals. Some strings are hardcoded English: the CalDAV settings UI and the status bar.
10. **Multi-window coherence.** Settings sync, `fs:file-written` reload rules, mermaid live sync, print handshake. Several current bugs live here (a second editor window does not reload on save; edits dropped on a tab switch within 800 ms; see 2 §6).

---
