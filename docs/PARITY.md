# Fuchico: Feature-Parity Specification (Tauri/React → native Rust + GPUI)

Source of truth: `/Users/fuleco/Documents/Dev/obsidian-helix` at commit `6fe6dd1` plus the uncommitted working tree as of 2026-10-18. That working tree replaces mermaid with `pele`, adds `demo-notes/templates/first_template.md` and moves `Desenho.excalidraw`. App version `0.1.0`, identifier `dev.fuleco.fuchico`.

**Acceptance criterion: feature parity.** Every `- [ ]` item below is a testable behavior. Paths are relative to the repo root unless they are absolute. Items marked **QUIRK** / "possible bug" describe current behavior that looks unintended. Port it faithfully, or fix it deliberately and record the decision. Do not drop it silently.

## 0. Overview and cross-cutting indexes

### Contents

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

### 0.1 Architecture map (what replaces what)

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

### 0.2 Windows

| Kind | Label | Created by | Content | Notes |
|---|---|---|---|---|
| Main | `main` | `tauri.conf.json` | Full app | 1000×700 default; title always "Fuchico"; geometry persisted |
| Editor | `editor-*` | `src/lib/editorWindow.ts` (palette "Open in New Window", explorer "Open in new window") | Single-file `EditorPane`, no palette/sidebar | Geometry persisted; `:q` closes the window; zoom shortcuts work; Mod+P does not |
| Mermaid | `mermaid-<blockKey>` | `src/modules/mermaid/lib/mermaidWindow.ts` | Pop-out diagram, live-updated from the source editor | Can re-dock (`mermaid:dock-request`) |
| Print | `export-*` | `src/modules/export/lib/exportActions.ts` | Rendered HTML, then the native print dialog | Fetches HTML through `export:request` → `export:html` |

Window details are in sections 1 (Process/windows, Editor windows), 4 (sections 12 and 15) and 6 (sections 7.2–7.5).

### 0.3 Persistence inventory (everything that survives a restart)

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

### 0.4 IPC inventory

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

### 0.5 Master keyboard index

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

### 0.6 Palette command index

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

### 0.7 Hardest areas / porting risks

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


---

## 1. App Shell, Layout, Windows, Tabs, Explorer

Sources: `src/main.tsx`, `src/App.tsx`, `src/lib/{ContextMenu.tsx,dragTypes.ts,editorWindow.ts,fileActions.ts,fileRowMenu.ts,useContextMenu.ts,useOpenRequests.ts,usePrefersDark.ts,i18n.ts}`, `src/modules/tabs/**`, `src/modules/statusbar/**`, `src/modules/explorer/**`, `src/modules/editor/EditorWindowApp.tsx`, `index.html`, `vite.config.ts`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/*.json`, `src-tauri/src/lib.rs`, `src-tauri/src/modules/open_files.rs`, `src/styles/globals.css`.

All user-visible strings go through `t(key, params)` (`src/lib/i18n.ts`). It looks up the key in the current locale catalog (`en` or `pt-BR`), falls back to `en`, and replaces every `{param}`. The locale comes from the `language` setting: `"system"` resolves to `pt-BR` when `navigator.language` starts with `pt` (case-insensitive), otherwise `en`. Pluralization uses `tn(n, oneKey, manyKey)`, which fills `{n}`. When the language changes, every open window re-renders through the settings store (including other windows, via the `storage` event). This section quotes the English strings, with pt-BR in parentheses where useful.

`src/App.css` is dead code. Nothing imports it, so don't port it.

### Process, windows and entry routing

- [ ] **One process per user (single instance).** `tauri_plugin_single_instance` is registered first. When the app is launched a second time, that launch's argv goes to the running instance (see "Open-file handling") and no new process starts.
- [ ] **Main window.** Label `main`, title `"Fuchico"` (from `index.html` `<title>` and `tauri.conf.json`), default size 1000×700, `dragDropEnabled: false` so HTML5 drag and drop works on macOS. **The main window title never changes**: it does not show the vault name, the active file or a dirty state.
- [ ] **Window geometry is remembered per window label** (`tauri_plugin_window_state`) for `main`, `editor-*` and `mermaid-*`. Size and position come back on the next open.
- [ ] **One HTML entry point for every window.** `index.html` → `src/main.tsx` reads `window.location.search` and routes in this order of precedence:
  1. `?window=print&key=<k>` → print/PDF preview window (`mountPrintWindow(key)`, no React; another section covers it).
  2. `?mermaidBlockKey=<k>[&title=<t>]` → popped-out diagram window `MermaidWindowApp` (title defaults to `"Diagram"`).
  3. `?window=editor&path=<abs>[&root=<abs>]` → single-file editor window (`EditorWindowApp`).
  4. Anything else → the full `App`.
  Windows other than the main app load only their own module (code-split).
- [ ] **Capabilities (window label → permissions).**
  - `main` (`default.json`): `core:default`, `core:webview:allow-create-webview-window`, `opener:default`, `dialog:allow-open`, `dialog:allow-save`, `dialog:allow-message`.
  - `editor-*` (`editor-window.json`): the same set as `main`.
  - `export-*` (`export-window.json`): `core:default` only.
  - `mermaid-*` (`mermaid-window.json`): `core:default` only.
  The port needs equivalent rules: only the main and editor windows may create windows, open dialogs, or reveal or open files.
- [ ] **Bundle and build metadata:** productName `Fuchico`, version `0.1.0`, identifier `dev.fuleco.fuchico`, category Productivity, macOS minimum 11.0. File association: extensions `md` and `markdown`, name "Markdown", mimeType `text/markdown`, role `Editor`. Dev server port `1423` (strict), HMR on `1421`. CSP: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost`. These are web-only; for GPUI only the association, identifier and version matter.

### Vault selection and startup

- [ ] **localStorage key `helix.lastRootPath`** (`LAST_ROOT_KEY`) holds the absolute path of the last vault folder.
- [ ] **Restore on launch.** If the key exists, call `invoke("fs_stat", { path })`.
  - On success, adopt it as `rootPath`.
  - On failure (folder moved or deleted), remove the key and show the no-vault state.
  - Until this check finishes, the window renders an empty layout (`<div class="app-layout"/>`, a blank screen with no activity bar).
  - The open-requests drain (below) waits for this restore.
- [ ] **Open Folder.** Shows the native directory picker (`plugin-dialog open({ directory: true, multiple: false })`). When it returns a string:
  - set `rootPath`,
  - **close all tabs**,
  - write `helix.lastRootPath`.
  If the user cancels or the result is not a string, nothing happens. The open dock panel (backlinks/mermaid) and the current main view are kept.
  Open Folder can be triggered from:
  - the no-vault sidebar button,
  - the explorer header button,
  - the palette command (`openFolder`).
- [ ] **No vault open.**
  - The sidebar shows only a centered primary button with a `FolderOpen` 14px icon and "Open Folder" ("Abrir pasta").
  - The activity bar and every main view stay available, and each view receives `rootPath = null`.
  - The editor area shows "No file open".
  - `FileExplorer` itself renders "No folder open" when `rootPath` is null. In practice it is never mounted without a root.
- [ ] **Vault switching is not otherwise persisted.** There is no recent-vaults list. Tabs, active tab, expanded folders, main view and dock are all in memory only and are lost on restart.

### Layout (main window)

Horizontal flex, top to bottom of the DOM, filling 100% height (`html, body, #root { height:100%; margin:0 }`, `box-sizing: border-box` everywhere):

- [ ] **Activity bar** (`.app-activitybar`): a fixed **40px** column with an 8px top padding, a 4px gap, a right border and the card background. It holds 5 icon buttons, each **30×30px** with a 17px lucide icon at stroke 1.75. Each button has a tooltip and sets `mainView`. In order:
  1. `Files` icon → `editor`, title "Files"
  2. `CalendarClock` → `agenda`, "Tasks & Calendar"
  3. `Search` → `search`, "Search"
  4. `Hash` → `tags`, "Tags"
  5. `Settings` → `settings`, "Settings"
  The button for the current view gets the active style (accent background). Clicking the active view again does nothing; it is not a toggle. Hover and color transitions take 160ms (`--dur-fast`, easing `cubic-bezier(0.16,1,0.3,1)`).
- [ ] **Sidebar** (`.app-sidebar`): fixed **260px** wide. It is **not resizable and not collapsible**, and it is always visible in every main view. It has a right border and shows the FileExplorer or the no-vault button.
- [ ] **Main** (`.app-main`): flex row holding the main content (flex 1) and an optional right **dock**.
- [ ] **Main content: exactly one `mainView` at a time** (`"editor" | "agenda" | "search" | "tags" | "settings"`, default `"editor"`):
  - `agenda` → `AgendaView{rootPath, onOpenItem: openFile}`
  - `search` → `SearchPanel{rootPath, onOpenMatch: openFile}`
  - `tags` → `TagsView{rootPath, onOpenFile, selectedTag, selectedTagToken}`
  - `settings` → `SettingsView{rootPath}`
  - `editor` → editor area: TabBar (only when at least 1 tab) plus the active tab's pane:
    - If `activeTab.path.toLowerCase().endsWith(".excalidraw")` → `ExcalidrawPane` (lazy; shows "Loading…" while loading).
    - Otherwise → `EditorPane`, which hosts the markdown editor and the status bar.
    - No tabs → centered muted text "No file open" ("Nenhum arquivo aberto").
  - **Only the active tab's pane is mounted.** It is keyed by path, so switching tabs remounts the editor.
- [ ] **`openFile(path, focusLine?)`** (used by every view, the quick switcher, wikilink navigation and the explorer) opens or activates the tab **and forces `mainView = "editor"`**.
- [ ] **Tag click from the editor** (`openTagFilter(tag)`): sets `selectedTag`, increments `selectedTagToken` so the same tag re-triggers, and switches to `tags`.
- [ ] **Settings deep link:** the palette `openSettings(section)` calls `settingsNav.set(section)` and then sets `mainView = "settings"`.

### Right dock panel (backlinks / mermaid)

- [ ] **One dock slot:** `Dock = {kind:"mermaid", blockKey, label, initialText?} | {kind:"backlinks"} | null`. Opening one kind replaces the other. The dock sits to the right of the main content in **every** main view, not only the editor.
- [ ] **Backlinks:** `toggleBacklinks` is exposed as a palette command (with a `backlinksOpen` flag). It opens the dock as `{kind:"backlinks"}`, or closes it if backlinks are already showing.
  - `BacklinksPane` receives `targetPath = activeTab.path`. It gets `null` when there is no tab or when the path `.endsWith(".excalidraw")`. That check is case-sensitive, unlike the pane switch.
  - Its own close button sets the dock to null.
- [ ] **Mermaid dock.** Two ways to open it:
  - `EditorPane.onOpenMermaid({blockKey, text})` → dock `{kind:"mermaid", blockKey, label: t("app.diagram") /* "Diagram" */, initialText: text}`.
  - Tauri event **`mermaid:dock-request`** with payload `{ blockKey: string, label: string }` (sent by a popped-out diagram window asking to re-dock; that window then closes itself) → dock `{kind:"mermaid", blockKey, label}` with no initialText. MermaidPane requests the current text itself.
  The pane is keyed by `blockKey`. Its close button sets the dock to null.
- [ ] **Dock resizing.**
  - A **5px** wide resizer sits on the dock's left edge (`col-resize` cursor, accent tint at 30% on hover).
  - On pointerdown it captures the pointer. On move: `width = clamp(240, 800, startWidth - (clientX - startX))`, so dragging left widens the dock.
  - The width is applied live. Default width is **380px** (`DEFAULT_DOCK_WIDTH`), min 240 (`MIN_DOCK_WIDTH`), max 800 (`MAX_DOCK_WIDTH`); CSS also sets min-width 240 and max-width 800.
  - The width is kept in memory across dock close and reopen within a session. It is **not persisted** across restarts.
  - The dock has a left border and the card background at 85%.

### Global keyboard handling (main window)

- [ ] **Mod+P / Mod+Shift+P** (`usePaletteShortcuts`): a window `keydown` listener in the bubble phase. When `(metaKey||ctrlKey) && key.toLowerCase()==="p"`, it calls preventDefault and then:
  - with Shift → opens the Command Palette with an empty query,
  - without Shift → opens the Quick Switcher.
  Works whatever has focus. These combos must not be bound inside the editor keymaps. Editor windows do NOT register these shortcuts.
- [ ] **Zoom** (`useZoomShortcuts`; registered in the main window AND in editor windows): a window `keydown` listener in the **capture** phase. It requires Mod and no Alt, and matches on `e.code` so it works across keyboard layouts:
  - `Equal`/`NumpadAdd` → +1, `Minus`/`NumpadSubtract` → −1, `Digit0`/`Numpad0` → reset.
  - Without Shift it changes `editorFontSize`: ±1, clamped by `clampEditorFontSize`; reset sets `EDITOR_FONT_SIZE_DEFAULT`.
  - With Shift it changes `uiScale`: ±0.1, clamped by `clampUiScale`; reset sets `UI_SCALE_DEFAULT`.
  - Calls preventDefault and stopPropagation, so it beats the editor keymaps and the webview's own zoom.
- [ ] **Command palette** can also be opened with a preset query: `EditorPane.onOpenCommandPalette(query)`. Its `onGoToLine(line)` calls `editorRef.goToLine(line)`.
- [ ] **Palette command context** passed by the shell (`buildAppCommands`): `setMainView`, `openFolder`, `openQuickSwitcher`, `closeActiveTab` (closes the active tab if there is one), `closeAllTabs`, `hasRootPath`, `hasActiveTab`, `hasOpenTabs`, `rootPath`, `vaultFiles`, `openFile`, `runEditorAction(action)`, `saveActiveFile` (`editorRef.save()`), `openActiveInNewWindow`, `toggleBacklinks`, `backlinksOpen`, `getActiveContent`, `activePath`, `openSettings(section)`. Commands are rebuilt when the locale changes. The palette section covers the commands themselves.
- [ ] The shell binds **no tab-switching keys** (no Ctrl+Tab, no Mod+W). Closing and switching tabs by keyboard goes through palette commands and the editor's own keymap.

### Tabs (`src/modules/tabs`)

- [ ] **Tab model:** `{ path: string; dirty: boolean; focusLine?: number; focusToken: number }`. Tabs are an ordered list in opening order, plus `activePath`. Tabs are not persisted.
- [ ] **Open:** if the path is already open, set its `focusLine` and increment `focusToken`; the editor uses the token to re-scroll to the line even when the line is unchanged. Otherwise append `{path, dirty:false, focusLine, focusToken:1}` at the **end**. Then make it active.
- [ ] **Close tab:** remove it. If it was active, activate the tab that now occupies the same index (`next[min(idx, len-1)]`, i.e. the right neighbor, or the last tab when the closed one was last). No tabs left → `null`. **Closing never prompts**, even when the tab is dirty.
- [ ] **Close others(path):** keep only that tab. If the active tab was removed, activate the kept tab.
- [ ] **Close all:** clear the list and set active to null. This also runs when the vault changes.
- [ ] **Rename/move propagation** (`handlePathRenamed(from,to)`): every tab path that equals `from` or starts with `from + "/"` is rewritten as `to + rest`; the active path is remapped the same way. The explorer's rename and move operations call it.
- [ ] **Delete propagation** (`handlePathDeleted(path)`): closes tabs whose path equals `path` or is under `path/`. If the active tab is closed, activate `next[min(firstRemovedIdx, len-1)]`.
- [ ] **Dirty flag:** set by the active pane's `onDirtyChange(dirty)`; a no-op when unchanged.
- [ ] **TabBar UI** (`.tab-bar`, rendered only when there is at least one tab):
  - The bar is a horizontal strip that **scrolls horizontally** on overflow (`overflow-x: auto`); tabs don't shrink.
  - Each tab is **32px high** and at most **200px** wide, with padding `0 8px 0 10px`, a 6px gap, 12px font, and a right separator border.
  - Each tab shows:
    - **Dirty dot:** a 6×6 circle in `--syntax-function` color before the name, shown only when the tab is dirty.
    - **Name:** the basename (split on `/` or `\`, ignoring empty parts), ellipsized.
    - **Close button:** 18×18 with an `X` icon (12px, stroke 2) and tooltip "Close"; stopPropagation so it doesn't also activate the tab.
  - The tab tooltip is the full path.
  - **Active tab:** background color plus an inset 2px bottom accent line (`box-shadow: inset 0 -2px 0 var(--ring)`).
- [ ] **Tab interactions:**
  - Click activates the tab.
  - Tabs are focusable (tabIndex 0); **Enter or Space** activates the focused tab.
  - **No middle-click close, no drag reordering, no pinning, no double-click action.**
- [ ] **Tab context menu** (right-click), in order:
  1. "Close"
  2. "Close others" (disabled when there are fewer than 2 tabs)
  3. "Close all"
  4. separator
  5. "Open in new window" (opens the path in an editor window; the tab stays open)
  pt-BR labels: Fechar / Fechar outras / Fechar todas / Abrir em nova janela.

### Status bar (`src/modules/statusbar/StatusBar.tsx`)

- [ ] Shown at the bottom of each `EditorPane`, both in main-window tabs and in editor windows; not shown for Excalidraw tabs or other views.
  - Height **22px**, padding `0 10px`, top border, 11px muted text.
  - Content: `"{n} word"` when n is 1, otherwise `"{n} words"`, then a separator `·` at 0.6 opacity, then `"0 min read"` when words is 0, otherwise `"{readingTimeMin} min read"`.
  - These strings are hard-coded English and are **not translated**.
- [ ] Stats (`computeDocStats`):
  - words = `trimmed.split(/\s+/).length`, or 0 for empty text.
  - readingTimeMin = `max(1, round(words/200))`, or 0 when words is 0.
  - Recomputed on document change, debounced by 200ms.

### Context menu component (`src/lib/ContextMenu.tsx`, `useContextMenu.ts`, `fileRowMenu.ts`)

- [ ] **Item types:**
  - `{label, icon?, danger?, disabled?, onSelect}`
  - `{kind:"separator"}`
- [ ] **Rendering:**
  - Fixed position at the mouse (clientX/clientY), rendered on top of everything (portal, z-index 1000).
  - Min width 160px, 4px padding, card background, border, shadow `0 8px 24px oklch(0 0 0/0.18)`.
  - Items: 12px text, padding 5px 8px, 13px icon at stroke 1.75, or a 13px spacer when there is no icon.
  - Separator: 1px line with margins 4px 6px.
  - Disabled items are muted at 0.6 opacity and can't be clicked.
  - Danger items hover with the destructive color.
- [ ] **Clamping:** after layout, x is clamped to `[4, innerWidth - menuWidth - 4]` and y to `[4, innerHeight - menuHeight - 4]`.
- [ ] **Closing:** pointerdown outside (capture phase), **Escape** (capture phase, stopPropagation so the editor and other views don't see it), window blur, or clicking an item (the menu closes first, then the action runs). Right-clicking the menu itself is suppressed.
- [ ] `useContextMenu<T>()`: the `open(e, data)` handler calls preventDefault and stopPropagation and stores `{x, y, data}`. Search, tags, agenda and backlinks use it.
- [ ] **Shared file-row menu** (`fileRowMenuItems(path, {onOpen, extra?})`), used by search results, tag file lists, agenda items and backlinks. In order:
  1. "Open" (`FileText` icon)
  2. any `extra` items
  3. separator
  4. "Reveal in file manager" (`Eye`)
  5. "Copy path" (`Copy`)

### File actions (`src/lib/fileActions.ts`)

- [ ] `revealInFileManager(path)`: `plugin-opener revealItemInDir(path)`. On failure it logs `console.warn("reveal failed", e)`; nothing is shown to the user.
- [ ] `openWithExternalTool(path, openWith?)`: `openPath(path, app)`, where app is the trimmed `externalTool` setting, or undefined to use the OS default handler. On failure it shows `window.alert("Could not open with {app || 'the default app'}: {e}")`, in English and untranslated.
- [ ] `copyPathToClipboard(text)`: writes the text to the clipboard; on failure, console.warn only.

### Drag-and-drop types (`src/lib/dragTypes.ts`)

- [ ] Custom MIME **`application/x-fuchico-paths`** (`PATH_DRAG_TYPE`): dragged vault absolute paths joined with `\n`. Explorer rows set it, together with `text/plain` = the row path.
  - `readDraggedPaths(dt)` splits on `\n` and drops empty entries.
- [ ] `hasExternalFiles(dt)` is true when `dt.types` includes `"Files"` and does **not** include `PATH_DRAG_TYPE`; that combination means an OS file drag.
- [ ] **Dropping vault paths into the editor** (`src/modules/editor/lib/pathDrop.ts`; the editor section owns the details): dragover accepts only `PATH_DRAG_TYPE` with dropEffect `copy`. On drop, one link per path, joined by `\n`, is inserted at the drop coordinates (falling back to the cursor):
  - a `.md` file (`/\.md$/i`) → `[[name-without-.md]]`
  - an image (`/\.(png|jpe?g|gif|webp|svg|bmp)$/i`) → `![label](relative)`
  - anything else → `[label](relative)`
  label = the name without its last extension; relative = path relative to the current note's folder. A space is prepended when the previous character is not whitespace. The cursor moves to the end of the insertion and the editor takes focus.
- [ ] Because native webview drag and drop is disabled (`dragDropEnabled:false`), OS file drops arrive as browser `File` objects. The bytes are copied over IPC (see Import below).

### Open-file handling (OS / CLI) — `open_files.rs`, `lib.rs`, `useOpenRequests.ts`

- [ ] **Backend queue:** `OpenRequests(Mutex<Vec<String>>)`. `push(app, paths)` does the following, and is a no-op when `paths` is empty:
  1. adds the paths to the queue,
  2. **focuses the `main` window**,
  3. emits the global event **`app:open-files`** with payload `string[]` (absolute paths).
- [ ] **Command `open_requests_take` → `string[]`:** returns all queued paths and clears the queue.
- [ ] **Sources:**
  - **macOS:** `RunEvent::Opened { urls }` (Finder double-click, `open -a Fuchico x.md`). Each URL is converted with `to_file_path()`; non-file URLs are dropped. This can fire before the webview exists, which is why paths are queued.
  - **Linux/Windows first launch:** in `setup`, `paths_from_args(env::args().skip(1), current_dir)`.
  - **Second launch on any platform** (single-instance callback): `paths_from_args(args.skip(1), cwd of the second process)`.
- [ ] **`paths_from_args` rules:**
  - Skip arguments that are empty or start with `-`.
  - Relative paths are joined to the cwd. With no cwd, they are dropped.
  - Keep a path only if `is_file()` is true. Directories and missing files are ignored.
  - Returns lossy UTF-8 strings.
- [ ] **Frontend drain** (`useOpenRequests(enabled = !restoring, handlers)`): once the vault restore finishes, call `open_requests_take` and handle the result. Errors are swallowed. Then listen for `app:open-files` and handle each payload. A batch is processed in order, and the root returned by each routing step is used for the next file.
- [ ] **Routing** (`routeOpenRequest`):
  - **No vault:**
    1. The vault becomes `dirname(path)` (`lastIndexOf("/")`, or `/` for index ≤ 0).
    2. All tabs close and `helix.lastRootPath` is written.
    3. The file opens as a tab.
  - **Vault open and `isInside(root, path)`:** the file opens as a tab and the view switches to editor. `isInside` is a prefix test on `root` with a trailing `/` added, so `/v` does NOT contain `/vault/a.md`.
  - **Outside the vault:** `openEditorWindow(path, currentRoot)`.
  Paths are not canonicalized; symlinked or case-variant paths are compared as plain strings.
- [ ] Quirk, not required: paths delivered by the event are never removed from the queue. Only the single startup drain clears it.

### Editor windows (`src/lib/editorWindow.ts`, `EditorWindowApp.tsx`)

- [ ] **Label:** `"editor-" + base36(h)` where `h = 0; for each UTF-16 code unit c: h = (h*31 + c) >>> 0` (unsigned 32-bit). The label is stable per path and matches `/^editor-[a-z0-9]+$/`.
- [ ] **URL:** `index.html?window=editor&path=<enc>[&root=<enc>]`, URLSearchParams-encoded so spaces become `+`. Example: `index.html?window=editor&path=%2Fv%2Fa+b.md&root=%2Fv`. `root` is omitted when there is no vault.
- [ ] **`openEditorWindow(path, rootPath)`:**
  - If a window with that label already exists, focus it and stop.
  - Otherwise create one with `{url, title: basename(path) /* after last "/" */, width: 900, height: 650, dragDropEnabled: false}`.
  So **each file gets at most one editor window**.
- [ ] **Triggers:**
  - tab context menu "Open in new window",
  - explorer file context menu "Open in new window",
  - palette `openActiveInNewWindow`,
  - OS open requests for files outside the vault.
- [ ] **EditorWindowApp:** a slim shell (`.editor-window`, a 100vh flex column) holding one `EditorPane` that fills the window. It has no activity bar, explorer, tabs, dock, palette or quick switcher. It applies the theme (`useTheme`) and the zoom shortcuts.
  - `vaultFiles` come from the `root` query parameter, so wikilink completion and resolution work.
  - **Following a wikilink in this window replaces the file shown in the same window**; no tab opens and no window is created. The window label keeps the original path's hash.
  - **Window title:** `"● " + basename` when dirty, otherwise `basename`. It updates whenever the path or dirty state changes.
  - The pane's close action (`onClose`) closes the OS window.
  - Settings stay in sync with the main window through the `storage` event.
- [ ] **Cross-window file-tree refresh:** see `fs:changed` under Explorer.

### File explorer (`src/modules/explorer`)

#### Data and loading

- [ ] **Lazy loading, one directory at a time:** `invoke("fs_read_dir", { path, showHidden: false })` → `DirEntry[] = { name, kind: "file"|"dir"|"symlink", size: number, mtime: number /* ms epoch, 0 if unknown */ }`. Only the root and the expanded folders are fetched.
- [ ] **Hidden files:** names starting with `.` are never shown, whether file or folder. No UI toggles this.
- [ ] **Filtering:** none. All file types are listed: md, images, `.excalidraw` and anything else.
- [ ] **Sorting** (done by the backend): directories first, then symlinks, then files; within each group, sorted by `to_lowercase()` name. Symlinks are followed: a symlink to a directory or file is reported as `dir` or `file`. Only broken symlinks come back as `symlink`, and the UI treats those as files (file icon; clicking opens them).
- [ ] **Per-directory state:** `idle | loading | loaded{entries} | error{message}`. A refetch only shows "loading" when the directory was not already loaded, so refreshes don't flicker.
- [ ] **What the user sees while loading or on error:**
  - Root: "Loading…" (`.explorer-status`, 11px muted, padding 8px 12px), or the error message in the destructive color.
  - Child folders: a status row under the folder, indented `6 + (depth+1)*12 + 18` px, 24px tall, 11px, ellipsized. It shows "Loading…" or the error text (destructive color).
- [ ] **Root change** resets all tree state: nodes, expanded set, pending create and rename. **Expansion is not persisted**; after a restart or a vault switch, every folder is collapsed.
- [ ] **External change notifications:**
  - `notifyFsChanged(dir)` dispatches a DOM `CustomEvent("helix:fs-changed", { detail: dir })` and emits the Tauri event **`fs:changed`** with `{ dir: string, from: <current window label, "main" on failure> }`.
  - Every mounted tree refetches `dir` when it receives either event. Tauri events whose `from` equals the receiving window's own label are ignored, because the DOM event already handled them.
  - Daily notes, templates, attachments and other code that creates files outside the explorer use this.

#### Layout and rows

- [ ] **Header** (`.explorer-header`): 32px tall, padding `0 6px`, gap 4, bottom border.
  - **Title:** the vault folder's basename (bold, 12px, ellipsized), with the full path as tooltip.
  - Then 5 icon buttons, each 24×24 with a 14px icon:
    1. `FolderOpen` "Open folder…" → Open Folder dialog
    2. `FilePlus` "New file" → create a file at the root
    3. `FolderPlus` "New folder" → create a folder at the root
    4. `PenTool` "New drawing" → create a file at the root with defaultExt `excalidraw` and placeholder "New drawing"
    5. `RefreshCw` "Refresh" → refetch the root **and** `notifyFsChanged(rootPath)`, which also re-indexes links and backlinks. Expanded subfolders are not refetched.
  - The header is also the **drop target for "move to vault root"**; it gets an accent background and a 1.5px ring outline while targeted.
- [ ] **Scroll area:** fills the remaining height with vertical scroll and no horizontal scroll. It contains, in order:
  - the notice banner (when shown),
  - the pending-create row for the root (when active),
  - the virtualized rows.
  Rows are **24px** each (`ROW_HEIGHT`), overscan 8.
- [ ] **Flattened rows:** a depth-first walk of loaded and expanded folders. Inside an expanded folder, the pending-create row (if any) comes **first**, followed by the loading/error status or the children.
- [ ] **Entry row:**
  - Height 24, gap 6, padding-right 6, **padding-left `6 + depth*12` px**, 13px text.
  - **Disclosure:** a 14px slot; for folders it holds a `ChevronRight` (12px, stroke 2.25) that **rotates 90° when expanded**, animated over 160ms.
  - **Icon** (14px): `Folder` for folders, `PenTool` for names ending in `.excalidraw` (case-insensitive), `File` for everything else. The folder icon does not change when open.
  - **Name:** ellipsized; the tooltip is the full path.
  - **States:**
    - hover: accent at 70%
    - selected: accent background with accent-foreground text
    - cut: opacity 0.55
    - drop target: accent background plus a 1.5px ring outline (offset −1.5)

#### Selection, mouse and keyboard

- [ ] **Selection model:**
  - `selectedPath` is the focused row (the keyboard cursor); `selectedPaths` is the set that actions apply to; `anchor` is where Shift ranges start.
  - **The active tab's file is auto-selected** (single selection, and it becomes the anchor) whenever the active file changes, provided its row is visible.
  - Selected and cut paths whose rows disappear (deleted, renamed, or **hidden because a folder collapsed**) are dropped from the selection and the cut list.
- [ ] **Click** (ignored while a rename or create input is open):
  - **Shift+click:** select the visible-row range between the anchor and the clicked row. The anchor stays put. Nothing opens.
  - **Cmd/Ctrl+click:** toggle the row in the selection. The last remaining selected row can't be removed. The anchor moves to this row. Nothing opens.
  - **Plain click:** select only this row and make it the anchor. A folder **toggles** expand/collapse; a file **opens in a tab** (and switches to the editor view).
- [ ] **Double-click:** starts inline rename. The two preceding clicks still fire, so a file opens first and a folder toggles twice.
- [ ] **Right-click:** if the row is not already in the selection, select it alone; otherwise keep the multi-selection. Then open the explorer context menu. Empty space and the header have no menu.
- [ ] **Keyboard** (the explorer container is focusable, tabIndex 0). Keys are ignored while renaming or creating, when an INPUT or contentEditable element is focused, or when there are no rows. "Visible entries" below means visible rows, excluding pending/status rows.
  - **ArrowDown / ArrowUp:**
    - Move to the next or previous entry, clamped at the ends.
    - With no current row, ArrowDown goes to the first entry and ArrowUp to the last.
    - **Shift** extends a range from the anchor.
    - The row scrolls into view (`align:auto`).
  - **ArrowRight:**
    - On a collapsed folder: expand it.
    - On an expanded folder: move to the next row, which is the first child.
    - On a file: nothing.
  - **ArrowLeft:**
    - On an expanded folder: collapse it.
    - Otherwise: move the focused row to the parent folder, unless the parent is the root. Quirk: only `selectedPath` changes and the highlighted selection stays where it was.
  - **Enter:** toggle a folder, or open a file.
  - **Delete / Backspace:** delete the selection (or the focused row) after confirmation.
  - **F2:** rename the focused row.
  - **Cmd/Ctrl+X** (no Alt or Shift): mark the selection (or the focused row) as cut.
  - **Cmd/Ctrl+V** (needs something cut): move the cut items into the focused folder, the focused file's parent folder, or the root when nothing is focused.
  - **Cmd/Ctrl+A:** select all visible entries. If nothing is focused, focus moves to the first entry.
  - **Escape:** clears the cut marking, if any.
  - **Type-ahead:** any single printable key without Cmd/Ctrl/Alt (Shift is allowed) is lowercased and added to a buffer. The buffer resets after **700ms** (`TYPE_AHEAD_RESET_MS`) without typing.
    - The target is the next visible entry, wrapping around, whose lowercased basename starts with the buffer.
    - A 1-character buffer, or one made of the same letter repeated ("aaa"), starts searching at the row after the current one and matches on the first letter. This cycles through items with that letter.
    - A longer, mixed buffer starts searching at the current row.
    - On a match, the cursor moves there (single selection).

#### Create, rename, delete

- [ ] **Create** (`beginCreate(parent, kind, {defaultExt?, placeholder?})`):
  - Cancels any rename in progress.
  - Expands the parent (unless it is the root) and fetches it if it isn't loaded yet.
  - Shows an inline input row with a Folder or File icon. Placeholder: "New folder" / "New file", or the custom one ("New drawing").
  - **Commit** (Enter **or blur**):
    1. Trim the name. Empty → cancel.
    2. If `defaultExt` is set and the name has no `.`, append `.ext`. Only "New drawing" sets one (`excalidraw`). **"New file" adds no extension**: typing `foo` creates `foo`, not `foo.md`.
    3. Call `fs_create_file {path}` or `fs_create_dir {path}`. Both fail with `"already exists: …"` when the target exists, and `create_dir_all` builds nested folders.
    4. Refetch the parent and remove the input.
  - **Errors** are only logged to the console; nothing is shown to the user.
  - The new file is **not** opened automatically.
  - Escape cancels.
- [ ] **Inline rename** (context menu, F2 or double-click):
  - Cancels any pending create. The row turns into an input prefilled with the **full name, all of it selected** (including the extension) and focused.
  - Enter or blur commits; Escape cancels. Keystrokes in the input are stopPropagated.
  - **Commit:**
    1. Trim the name. Empty or unchanged → cancel.
    2. Call `fs_rename {from, to: parent + "/" + newName}`, which fails if the target exists.
    3. Call `onPathRenamed(from,to)` so tabs are remapped, then refetch the parent.
  - **Errors** go to the console only.
  - Edge: a name containing `/` produces a nested target path.
- [ ] **Delete** (context menu, Delete or Backspace):
  - `window.confirm("Delete {name}?")`, where name is `"<basename>"` in quotes for a single item, or `"{count} items"` for several.
  - If confirmed, the items are deleted one after another with `fs_delete {path}`. This is **permanent** (remove_file / remove_dir_all), with **no trash**.
  - After each delete: `onPathDeleted(path)` closes the affected tabs, then the parent folder is refetched.
  - Errors go to the console.

#### Cut, paste and drag-and-drop moves

- [ ] **Move** (`movePath(from, toDir)`):
  - Target = `toDir/basename`.
  - **Skipped silently** when `toDir` is already the item's parent, or when `toDir + "/"` starts with `from + "/"` (moving into itself or a descendant).
  - Otherwise: `fs_rename {from, to}`, then `onPathRenamed`, then refetch the old and the new parent.
  - Fails when the target exists, so moves never overwrite.
  - A selection is moved one item at a time, in order. Errors go to the console only.
- [ ] **Cut and paste:** "Paste" moves the cut items that are allowed to move (the same skip rules as Move) and clears the cut list. There is no copy/duplicate operation.
- [ ] **Drag source** (row dragstart; not draggable while a rename or create input is open):
  - effectAllowed `copyMove`.
  - Data: `text/plain` = this row's path, plus `PATH_DRAG_TYPE` = the targets joined with `\n`. The targets are the whole selection if the row is in it, otherwise just the row.
  - Drag image: a small accent pill (`.tree-drag-ghost`: padding 2px 8px, radius 4, 12px) reading the basename or "{count} items", with offset (8,12).
- [ ] **Drop targets:**
  - folder row → that folder
  - file row → the file's parent folder; only folder rows highlight, so the parent row lights up if it is visible
  - header → root
  - empty scroll space → root
  Over a row the event is stopPropagated so the scroll container doesn't retarget the drop to the root.
- [ ] **Accepting a drop:**
  - OS files are always accepted (dropEffect `copy`).
  - An internal drag (dropEffect `move`) is accepted only when every dragged path passes the move rules for that folder; otherwise the cursor shows no-drop.
  - The highlight clears on dragend, on drop, or when the pointer leaves the header or scroll container itself.
- [ ] **Spring-loaded folders:** while dragging, hovering a collapsed non-root folder expands it after **600ms** (`DROP_EXPAND_MS`).
- [ ] **Drop action:** OS files are imported; otherwise the dragged paths are moved into the target folder.

#### Importing OS files (`lib/importFiles.ts`)

- [ ] Steps:
  1. For each dropped `File` that has a name, read its bytes, base64-encode them and call `fs_write_binary { path: toDir/name, dataBase64 }`.
  2. If the error text contains `"already exists"`, retry once with a timestamp suffix: `name-YYYYMMDD-HHMMSS.ext` (inserted before the last dot when the dot index is > 0), or `name-YYYYMMDD-HHMMSS` when there is no extension. Local time, zero-padded.
  3. Any other error is recorded as `"{file.name}: {error}"`.
- [ ] Afterwards the target folder is expanded if it is collapsed and not the root; otherwise it is refetched.
- [ ] **Notice banner** at the top of the scroll area (11px, padding 4px 8px, bottom border) **disappears after 4000ms** (`NOTICE_MS`):
  - If there was any error: "Import failed: {errors joined with '; '}" in the destructive color. This shows even when some files did import.
  - Otherwise: "Imported 1 file into {folder}", or "Imported {count} files into {folder}" when the count isn't 1. The folder is the target's basename.

#### Explorer context menu

- [ ] Items in exact order. Lucide icons in parentheses; "N" means more than one target.
  - **Folder rows:**
    1. "New file" (FilePlus)
    2. "New folder" (FolderPlus)
    3. "New drawing" (PenTool; defaultExt `excalidraw`)
    4. separator
    5. common items (below)
  - **File rows:**
    1. "Open in new window" (AppWindow)
    2. common items (below)
  - **Common items:**
    1. "Rename" (Pencil)
    2. "Cut", or "Cut {count} items" for N (Scissors)
    3. *only while something is cut:* "Paste {name} here" (Copy icon). name = the basename of the single cut item, or "{count} items". Pastes into the folder, or into the file's parent folder.
    4. separator
    5. "Reveal in file manager" (Eye)
    6. "Open with {tool}" when the `externalTool` setting is non-blank (trimmed), otherwise "Open with default app" (ExternalLink) → `openWithExternalTool(path, externalTool)`
    7. "Copy path", or "Copy {count} paths" for N (Copy). Copies the paths joined with `\n`.
    8. separator
    9. "Delete", or "Delete {count} items" for N (Trash2, **danger** style) → confirmation, then delete.
- [ ] The explorer menu has no "Open" item; a plain click opens. Rename, Reveal and Open-with act on the right-clicked row only. Cut, Copy path and Delete act on the whole selection.
- [ ] pt-BR labels:
  - Novo arquivo
  - Nova pasta
  - Novo desenho
  - Abrir em nova janela
  - Renomear
  - Recortar / Recortar {count} itens
  - Colar {name} aqui
  - Mostrar no gerenciador de arquivos
  - Abrir com {tool} / Abrir com o app padrão
  - Copiar caminho / Copiar {count} caminhos
  - Excluir / Excluir {count} itens
  - Excluir {name}?
  - {count} itens
  - Falha na importação: {errors}
  - 1 arquivo importado para {folder} / {count} arquivos importados para {folder}
  - Atualizar
  - Abrir pasta…
  - Nenhuma pasta aberta

### Theme helper

- [ ] `usePrefersDark()` returns whether the OS is in dark mode (`matchMedia("(prefers-color-scheme: dark)")`) and updates when that changes. The theme code (`useTheme`, ThemeSection) and ExcalidrawPane use it to follow the OS appearance. The port must react to OS appearance changes live.

### Invokes and events used by this section (summary)

- [ ] **Invokes:**
  - `fs_stat {path}` (vault restore; only success or failure matters)
  - `fs_read_dir {path, showHidden:false}` → `DirEntry[]`
  - `fs_create_file {path}`
  - `fs_create_dir {path}`
  - `fs_rename {from, to}`
  - `fs_delete {path}`
  - `fs_write_binary {path, dataBase64}`
  - `open_requests_take` → `string[]`
- [ ] **Plugins:**
  - dialog `open({directory:true, multiple:false})`
  - opener `revealItemInDir(path)`, `openPath(path, app?)`
  - webviewWindow `getByLabel`, `new WebviewWindow(label, opts)`, `setFocus`
  - window `getCurrentWindow().close()` and `.label`
- [ ] **Events:**
  - `app:open-files` (backend→frontend, `string[]`)
  - `mermaid:dock-request` (`{blockKey, label}`)
  - `fs:changed` (`{dir, from}`, between windows)
  - DOM `helix:fs-changed` (`detail: dir`, same window)
- [ ] **localStorage:** `helix.lastRootPath`. Settings keys (language, externalTool, uiScale, editorFontSize) belong to the settings section.


---

## 2. Editor Core and Display Extensions

Scope: the per-file text editor pane, how the editor is put together, keybinding modes, load/save, live preview of Markdown, tables, fenced code blocks, fold regions, rulers, outline overlay, document stats and scroll persistence.
Primary sources: `src/modules/editor/EditorPane.tsx`, `src/modules/editor/OutlineOverlay.tsx`, `src/modules/editor/EditorWindowApp.tsx`, `src/modules/editor/index.ts`, and `src/modules/editor/lib/{extensions,helix,useDocument,shortcuts,scrollPositions,markdownStyle,tableStyle,codeBlockStyle,languageResolver,languageDefinitions,docStats,outline,fuzzyMatch,rulers,regionFolding,regionDecorations,regionFoldWidget}.ts`. Styles are in `src/styles/globals.css` and in the `chromeTheme` inside `extensions.ts`.
No `*.test.ts` file covers these modules. The only editor tests are `imageAttachments.test.ts`, `pathDrop.test.ts` and `taskHelpers.test.ts`, which belong to other sections. The edge cases below come from reading the source.

### 1. Pane lifecycle, props and imperative handle

- [ ] One `EditorPane` instance per open file. The parent remounts it with `key={path}` whenever the path changes (`src/App.tsx`, `EditorWindowApp.tsx`), so every extension treats `path` as fixed for the life of the instance. Switching tabs destroys the editor and creates a new one (no editor state survives except scroll, see §12).
- [ ] `.excalidraw` paths go to `ExcalidrawPane`, not the editor. With no tab open, the pane shows `.editor-status` text `app.noFileOpen` (i18n).
- [ ] Props:
  - `path`
  - `onDirtyChange(dirty)`
  - `onSaved()`
  - `onClose()`
  - `onOpenMermaid(payload)`
  - `focusLine` (1-based) plus `focusToken` (a re-trigger counter)
  - `vaultFiles: string[]` (absolute .md paths)
  - `onNavigateFile(path, focusLine?)`
  - `getAllTags()`
  - `onTagClick(tag)`
  - `rootPath`
  - `onOpenCommandPalette(query)`

  All callbacks and values are read through refs, so changing them never rebuilds the editor (`extensions` is a `useMemo([], [])`; changing its identity would wipe the language compartment).
- [ ] Imperative handle `EditorPaneHandle`:
  - `focus()`
  - `runAction(action: ShortcutAction)`: rebuilds the command map from current settings (`foldStartMarker`/`foldEndMarker`), focuses the view, then runs the action. The command palette and menus use this.
  - `save()`: same as Mod-s (save, then `onSaved`).
  - `goToLine(n)`: clamps to `[1, doc.lines]`, puts a collapsed cursor at the line start, scrolls it to the vertical centre (`y:"center"`) and focuses. Used by the command palette `:42`.
  - `getContent()`: the live buffer including unsaved edits (used for export), or `null`.
  - `undo()` / `redo()`: CodeMirror `history` undo/redo, not Helix's own `u`/`U`.
- [ ] `focusLine` effect: runs when the doc is `ready` and `focusLine` is set, and again whenever `focusToken` changes even if the line number is the same. It clamps the line, **selects the whole line** (anchor = line.from, head = line.to), scrolls it to the centre and focuses the editor.
- [ ] Render states, driven by `useDocument` status:
  - `loading` → `<div class="editor-status">Loading…</div>`
  - `error` → `.editor-status.editor-status-error` showing the error message. Colour is `var(--destructive)`.
  - `binary` or `toolarge`:
    - If the extension (lower-cased) is one of `png, jpg, jpeg, gif, webp, svg, ico`, render `<img src=convertFileSrc(path) class="editor-image-preview">` (`max-width/max-height:100%; object-fit:contain`) with alt = basename.
    - Otherwise show the text `Binary file · <size>` or `File too large · <size>`.
    - `formatBytes`: `<1024` → `N B`; `<1 MiB` → `X.X KB`; else `X.X MB` (1 decimal place, base 1024).
  - `ready` → the editor.
- [ ] `.editor-status` style: flex, centred, `height:100%`, `padding:24px`, `font-size:13px`, colour `var(--muted-foreground)`.
- [ ] DOM layout:

  ```
  .editor-pane (flex column, 100%)
    [.editor-conflict banner]
    .editor-canvas (relative, flex:1)
      CodeMirror .editor-codemirror (flex:1, overflow hidden, font var(--font-mono))
      [.helix-mode-chip]
      [OutlineOverlay]
    StatusBar
  ```

- [ ] Secondary editor window (`EditorWindowApp.tsx`, `.editor-window`, `100vh`):
  - A single `EditorPane` with no sidebar.
  - `document.title` = `"● "` (only when dirty) + basename.
  - Wikilink navigation replaces the path inside the same window.
  - `onClose` closes the OS window (`getCurrentWindow().close()`).
  - `onOpenCommandPalette` is **not** passed, so Helix `:` opens Helix's own command line there (see §4).
  - It also calls `useTheme()` and `useZoomShortcuts()`.

### 2. Extension composition (exact order) and settings → extension map

The `extensions` array is passed to `@uiw/react-codemirror` with `theme="none"`, `height="100%"` and `className="editor-codemirror"`. `@uiw` puts its `basicSetup` **before** these user extensions.

`basicSetup` options (EditorPane.tsx):
- `lineNumbers:false` (supplied by a compartment instead)
- `highlightActiveLineGutter:true`
- `foldGutter:false` (supplied by the shared extensions instead)
- `bracketMatching:true`
- `closeBrackets:true`
- `autocompletion:false` (one shared instance is used instead)
- `highlightActiveLine:true`
- `highlightSelectionMatches:true`
- `searchKeymap:true`

Everything else stays at the @uiw defaults:
- `highlightSpecialChars`
- `history`
- `drawSelection`
- `dropCursor`
- `allowMultipleSelections(true)`
- `indentOnInput`
- `syntaxHighlighting(defaultHighlightStyle,{fallback:true})`
- `rectangularSelection` (Alt-drag)
- `crosshairCursor` off

Keymaps, in this order: `closeBracketsKeymap` (Backspace deletes an auto-inserted pair), `defaultKeymap`, `searchKeymap`, `historyKeymap`, `foldKeymap`, `completionKeymap`, `lintKeymap`.

Order of the user extensions:
1. `completionKeymap` (Prec.highest; owned by the completions section). It comes first so that Enter, arrows and Escape drive the completion popup before the modal keymap swallows them.
2. `lineNumberCompartment` ← `lineNumbersExtensionFor(settings.relativeLineNumbers)`
3. `indentCompartment` ← `indentExtensionFor(settings.tabSize)`
4. `rulersCompartment` ← `rulersExtension(settings.rulers)`
5. `keybindingCompartment` ← `keybindingExtensionFor(settings.keybindingMode)`
6. `shortcutsCompartment` ← `shortcutsExtension(settings.shortcuts, {start: foldStartMarker, end: foldEndMarker}, openOutline)`
7. `foldRegionCompartment` ← `foldRegionExtensionFor(foldStartMarker, foldEndMarker)`
8. `helixHandlersExtension` (typable `:w`/`:q`/`:wq`)
9. `helixModeReporterExtension`
10. `mermaidPreviewExtension` (other section)
11. `wikilinksExtension({getVaultFiles, currentPath, onNavigate})` (other section)
12. `tagsExtension({onTagClick})` (other section)
13. `pathDropExtension({currentPath})` (other section)
14. `imageAttachmentsExtension({currentPath, getAttachmentsDir: rootPath ? rootPath + "/attachments" : null})` (other section)
15. `editorCompletionExtension([...])`, the single shared autocomplete, with providers in this order: wikilink, heading (`[[Note#h]]`), tag, task, word. Other section.
16. `docStatsReporterExtension`
17. `scrollPersistenceExtension(path)`
18. `frontmatterExtension()` (other section)
19. `...buildSharedExtensions()`: `search({top:true})`, `lintGutter()`, `foldGutter({markerDOM: makeFoldMarker})`, `chromeTheme`, `syntaxHighlighting(syntaxHighlightStyle)`. This is a frozen singleton array, so the style modules are not duplicated across panes.
20. `languageCompartment.of([])`. It starts empty and is filled asynchronously by `resolveLanguage(path)` once the doc is `ready`. For Markdown, that is also what loads live preview, code-block style and tables (§7).
21. `keymap.of([{key:"Mod-s", preventDefault:true, run: save}])` at normal precedence.

Runtime reconfiguration: each setting below is applied with `view.dispatch({effects: compartment.reconfigure(...)})` in its own `useEffect`, so state, history and selection are kept.

- [ ] `keybindingMode` (`"helix" | "vim" | "normal"`, default `"helix"`) → `keybindingCompartment`:
  - `vim` → `Prec.highest(vim())`
  - `helix` → `Prec.highest(helix())`
  - `normal` → `[]` (plain CodeMirror default keymap)
- [ ] `relativeLineNumbers` (default `false`) → `lineNumberCompartment`.
- [ ] `tabSize` (default `2`) → `indentCompartment`: `indentUnit` = that many **spaces** and `EditorState.tabSize` = N. Indenting always inserts spaces, never tabs.
- [ ] `rulers` (`number[]`, default `[]`) → `rulersCompartment`.
- [ ] `shortcuts`, `foldStartMarker`, `foldEndMarker` → `shortcutsCompartment`. The markers matter here because `insertRegion` uses them.
- [ ] `foldStartMarker` (default `":::fold"`) and `foldEndMarker` (default `":::endfold"`) → `foldRegionCompartment`.
- [ ] Applied indirectly through CSS variables set by `useTheme` (settings section):
  - `editorFontSize` (default 13, clamped 8–32) sets `--editor-font-size` to `editorFontSize / uiScale` px.
  - `editorFont` sets `--font-mono`.
  - `uiScale` (0.7–1.6).
- [ ] Settings live in localStorage key `helix.editorSettings` (`editorSettingsStore`, a module-level pub-sub). Other OS windows pick up changes through the `storage` event, and the editor reconfigures live.
- [ ] Not present, so the port should also omit them (or treat as new features):
  - Soft line wrapping. `EditorView.lineWrapping` is not used in the main editor; `wrapCompartment` is declared in `extensions.ts` but never used. Long lines scroll horizontally.
  - A toggle to hide line numbers. The gutter is always shown.

### 3. Line numbers, gutters, cursor, selection, theme

- [ ] Gutter order, left to right: lint gutter (forced to `width:0px`; no linters are configured, so it is invisible), line numbers, fold gutter. Gutters use `background var(--background)`, `color var(--muted-foreground)` and no border.
- [ ] Absolute line numbers by default (`lineNumbers()`).
- [ ] Relative mode:
  - The active line (line of `selection.main.head`) shows its absolute number.
  - Every other line shows `|lineNo − current|`.
  - The gutter must repaint on every cursor move, not only on edits. The source does this with an updateListener that reconfigures the compartment whenever `selectionSet && !docChanged`.
- [ ] Active line background: `color-mix(in oklch, var(--foreground) 5%, transparent)`. Active gutter cell: `6%` foreground mix, text `var(--foreground)`.
- [ ] Selection background: `color-mix(in oklch, var(--primary) 30%, transparent) !important`, whether or not the editor is focused.
- [ ] Caret: `caretColor` and `.cm-cursor`/`.cm-dropCursor` border-left are `var(--foreground)`.
- [ ] Block "fat" cursor (`.cm-fat-cursor`, Vim/Helix normal mode):
  - Focused: background `color-mix(foreground 45%)`, outline `1px solid color-mix(foreground 65%)`, `border-radius:2px`.
  - Unfocused: transparent background, outline at 40%.
- [ ] Matching and non-matching brackets: background `foreground 12%` mix, no outline.
- [ ] Selection-match highlights and search matches (`.cm-searchMatch`): `--syntax-number` at 30%; the selected match at 55%.
- [ ] `.cm-foldPlaceholder`: background `var(--secondary)`, `1px solid var(--border)`, colour `var(--muted-foreground)`.
- [ ] Tooltips: `var(--card)` / `var(--card-foreground)`, `1px solid var(--border)`, radius `var(--radius-sm)`. The selected autocomplete item uses `var(--accent)` / `var(--accent-foreground)`.
- [ ] Editor root:
  - `height:100%`, background `var(--background)`, colour `var(--foreground)`, no focus outline.
  - Scroller font: `var(--font-mono)`, `font-size: var(--editor-font-size, 13px)`, `line-height:1.55`.
- [ ] Fold gutter markers (`makeFoldMarker`):
  - An SVG chevron 10×10, viewBox `0 0 24 24`, path `M9 6l6 6-6 6`, stroke currentColor, width 2.25, round caps and joins.
  - Wrapped in `span.cm-fold-marker`; adds `.cm-fold-marker-open` when the range is unfolded, which rotates it 90°.
  - Colour `var(--muted-foreground)`, becoming `var(--foreground)` on gutter hover.
  - Transition `transform/color var(--dur-fast) var(--ease-premium)`.
  - Gutter cells are flex-centred with `padding:0`.
- [ ] Syntax highlight style (`syntaxHighlightStyle`, applied to every language):
  - comment → `--syntax-comment`, italic
  - string / special(string) → `--syntax-string`
  - number, bool, null, atom → `--syntax-number`
  - keyword, controlKeyword, operatorKeyword, operator → `--syntax-keyword`
  - function(variableName), function(propertyName) → `--syntax-function`
  - className, typeName → `--syntax-type`
  - definition(variableName), propertyName → `--foreground`
  - punctuation → `--muted-foreground`
  - heading → `font-weight 700` only (colour comes from the `.cm-md-hN` line class)
  - link → `--syntax-function` + underline
  - url → `--syntax-function`
  - emphasis → italic
  - strong → 700
  - strikethrough → line-through
  - quote → `--muted-foreground`
  - monospace (inline code) → `--syntax-string` + `var(--font-mono)`
  - invalid → `--destructive`

### 4. Keybinding modes (Helix / Vim / Normal)

- [ ] **Helix** (default): `codemirror-helix@0.5.7` `helix()` with **no options**. That means:
  - `drawSelection` stays on.
  - Default config: no themes, default insert cursor shape, default yank register.
  - No `externalCommands`, no `pathRegister`, no `snapshot` or `globalInit`, so registers are per editor instance and are lost on remount.

  The port must reimplement the library's keymap. Notable bindings in the bundled version:

  | Group | Keys |
  |---|---|
  | Insert mode | Backspace/Delete, Enter (newline and indent), Escape → normal (commits a history checkpoint), Ctrl-r `<reg>` pastes a register, Tab/Shift-Tab indent/outdent |
  | Normal: motion | `h j k l` / arrows, `w e b`, `f F t T`, Home/End, `Ctrl-d`/`Ctrl-u` and PageDown/PageUp (half page), `%` select all |
  | Normal: mode switch | `v` select mode, `i a I A o O` enter insert mode, `:` command line, `/` search, `n N` next/previous match ("Wrapped around document" / "No more matches") |
  | Normal: selection | `s` select regex in selection, `Alt-s` split selection on newlines, `,` keep primary selection, `( )` rotate selections, `x` extend to line, `;` collapse, `Alt-;` flip, `Alt-:` ensure forward, `_` trim whitespace, `*` set search register |
  | Normal: syntax-tree | `Alt-o`/`Alt-ArrowUp` expand, `Alt-i`/`Alt-ArrowDown` shrink, `Alt-n`/`Alt-p` (Alt-Right/Left) sibling |
  | Normal: editing | `y`, `d`, `c`, `Alt-c`, `Alt-d`, `p`, `P`, `R`, `r<char>`, `u`/`U` undo/redo, `Ctrl-a`/`Ctrl-x` increment/decrement number, `Ctrl-c` toggle comment, `>` `<` indent, `` ` `` lowercase, `` Alt-` `` uppercase, `~` toggle case, `J` join lines, `C` copy selection to next line |
  | Normal: minor modes | `g` goto (`gg`, `ge`, `gh`, `gl`, `gs`, `gj`, `gk`, `gn`, `gp`); `m` match (`ms` surround, `mm` matching bracket, `ma`/`mi` text objects); `[` / `]` bracket motions; Space (`Space y/p/P/R` clipboard, `Space f`/`Space b` pickers which have no handler here because no external commands are configured, `Space /` global search) |

  Built-in typable commands: `goto` (`g`), `clipboard-yank`, `clear-register`, `echo`, `theme`, `buffer-close` (`bc`, `bclose`).
- [ ] App-added typable commands (`helix.ts`, through the `commands` facet):
  - `write` (alias `w`): save the file.
  - `quit` (alias `q`): `onClose()`, which closes the tab (main window) or the window (secondary).
  - `wq` (alias `x`): save, then close. The save is not awaited before close, so the port should save and then close.
- [ ] Mode chip:
  - `helixModeReporterExtension` reads `.cm-hx-status-panel > span:first-child` text on every update and maps `NOR→normal`, `INS→insert`, `SEL→select`.
  - The library's status panel is hidden (`display:none !important`), and the `.cm-panels-bottom` wrapper's border and background are stripped.
  - The chip renders only when `keybindingMode==="helix"`. It shows `<div class="helix-mode-chip" title="Helix mode: <mode>">` with the mode text; initial mode is `"normal"`.
  - Chip CSS: `position:absolute; bottom:12px; left:12px; z-index:10`; border `1px solid color-mix(var(--border) 60%)`; radius `var(--radius-md)`; background `color-mix(var(--card) 85%)`; `padding:2px 7px`; `font-size:10px`; `weight 600`; `letter-spacing:.05em`; uppercase; colour `color-mix(var(--foreground) 75%)`; `backdrop-filter: blur(6px)`; small shadow; `pointer-events:none`.
- [ ] `:` override (Zed style):
  - A window keydown listener in the **capture phase** handles `:`.
  - Conditions: no Meta, Ctrl or Alt; `onOpenCommandPalette` is provided; the mode setting is `helix`; the current Helix mode is not `insert`; the event target is inside `view.contentDOM` (so `:` still types normally in the `/` search input).
  - Action: `preventDefault`, `stopPropagation`, then `onOpenCommandPalette(":")` opens the app command palette prefilled with `:`.
  - In the secondary window no palette prop exists, so the native Helix command line opens.
- [ ] Helix command and search panel styling (globals.css):
  - `.cm-hx-command-panel`: background `var(--secondary)`, colour `var(--foreground)`, `border-top 1px var(--border)`, `padding:3px 8px`, `font-size:12px`.
  - `.cm-hx-command-input`: transparent background, foreground colour and caret.
  - `.cm-hx-command-popup`: secondary background, border, `radius-sm`, `padding:4px 8px`.
  - `.cm-hx-command-help`: border-color `var(--border)`.
  - `.cm-hx-selected-option`: accent background and accent-foreground text.
- [ ] **Vim**: `@replit/codemirror-vim` `vim()` with default options at `Prec.highest`.
  - No `Vim.defineEx` calls, so Vim `:w`/`:q` are **not** wired to app save or close.
  - No mode chip, and no `:` palette override.
  - The fat cursor uses `.cm-fat-cursor`.
- [ ] **Normal**: no modal extension; the CodeMirror `defaultKeymap` applies.
- [ ] Precedence:
  - completionKeymap, the modal keymap and the shortcuts keymap are all `Prec.highest`, in that array order.
  - basicSetup keymaps sit below them.
  - `Mod-s` works in every mode.
  - Changing the mode reconfigures live, without a remount.

### 5. Search inside the editor (`@codemirror/search`)

- [ ] `search({top:true})`: the find/replace panel docks at the **top** of the editor.
- [ ] `searchKeymap`, standard CodeMirror bindings, available in every mode unless the modal keymap consumes the key:
  - `Mod-f`: open the panel and focus the search field.
  - `F3` or `Mod-g`: next match. `Shift-F3` or `Mod-Shift-g`: previous match.
  - `Escape` (inside the panel): close.
  - `Mod-Shift-l`: select all matches.
  - `Mod-Alt-g`: go-to-line dialog.
  - `Mod-d`: select next occurrence.
- [ ] Panel contents (stock CodeMirror): search field, replace field, toggles for match case / regexp / by word, buttons next / previous / all / replace / replace all, and close (×).
- [ ] Panel styling (globals.css):
  - `.cm-panels`: background `var(--secondary)`, colour foreground, `font-family var(--font-sans)`, `12px`.
  - `.cm-panels-top`: `border-bottom 1px var(--border)`.
  - `.cm-panel.cm-search`: flex, wrap, `gap:4px`, `padding:6px 8px`.
  - Checkboxes: accent colour `var(--primary)`, margin `0 2px 0 6px`.
  - Labels: inline-flex, `11px`, muted.
  - `.cm-textfield`: border, `radius-sm`, background `var(--background)`, `12px`, `padding:3px 6px`; border becomes `var(--ring)` on focus.
  - `.cm-button`: border, `radius-sm`, background `var(--background)`, no background-image, `11px`, `padding:3px 8px`; hover uses accent.
  - Close button: muted colour, `14px`, `padding:0 4px`; hover foreground.
  - The `chromeTheme` also sets `.cm-panels` background and colour to `var(--card)` / `var(--card-foreground)`. The CSS selector `.cm-editor .cm-panels` overrides this with secondary.
- [ ] Helix `/` search is a separate bottom panel provided by the library (§4).
- [ ] `highlightSelectionMatches` marks other occurrences of the current selection.

### 6. Document load / save / autosave / dirty / external changes (`useDocument.ts`)

- [ ] Load:
  - Triggered on mount or path change: cancel any pending autosave, set status `loading`, set `dirty=false`, then `invoke("fs_read_file", {path})`.
  - Result shape (Rust `ReadResult`, `#[serde(tag="kind", lowercase)]`):
    - `{kind:"text", content, size}`
    - `{kind:"binary", size}`
    - `{kind:"toolarge", size, limit}`
  - Backend rules (`src-tauri/src/modules/fs/file.rs`):
    - `MAX_READ_BYTES = 10 MiB`. A larger file returns `toolarge` without being read.
    - A NUL byte in the first `8 KiB` → `binary`.
    - Invalid UTF-8 → `binary`.
    - Any error is returned as a string, and the pane shows the `error` status.
  - On `text`: `saved = buffer = content`, status `ready`.
- [ ] Dirty tracking:
  - `onChange(next)` sets `buffer = next` and `dirty = (next !== saved)`, then clears any pending autosave timer.
  - If dirty, a new autosave timer of **`AUTOSAVE_DEBOUNCE_MS = 800`** starts (trailing debounce: every keystroke restarts it).
  - Typing back to the saved text clears dirty and schedules no save.
  - `onDirtyChange(dirty)` fires on every dirty-state change, including the initial `false`. The tab dirty dot and the window title use it.
- [ ] `save()`:
  - Clears the timer.
  - If `buffer === saved`, it is a **no-op**: no write and no event.
  - Otherwise it calls `invoke("fs_write_file", {path, content, source:"editor"})`, then sets `saved = content` and `dirty = false`.
  - Write failures reject unhandled; there is no user-facing error.
  - Backend write: atomic (temp file in the same directory, fsync, rename), original permissions preserved. It then emits the Tauri event `fs:file-written` with payload `{path, source?}`; `source` is omitted when null.
- [ ] Save triggers:
  - Autosave 800 ms after the last change.
  - `Mod-s`.
  - Helix `:w`, `:wq`, `:x`.
  - Imperative `save()` (command palette or menu).
  - After an explicit save (not autosave), `onSaved()` is called.
  - **No** save on blur, on window close, or on unmount. The unmount cleanup *cancels* the pending autosave timer, so edits made in the 800 ms before switching tabs or closing are dropped. A port should probably flush on unmount; note this as a deliberate deviation if you add it.
- [ ] External change handling (EditorPane):
  - The pane listens to `fs:file-written` and ignores events where `payload.path !== path` or `payload.source === "editor"`.
  - Side effect: every editor writes with `source:"editor"`, so a second window editing the same file does *not* auto-reload after another editor's save. Only non-editor writers trigger a reload: vault replace, templates, CalDAV sync and so on.
  - Otherwise it calls `reload()`:
    - When not dirty: re-read the file. If the content equals `saved`, nothing happens. Otherwise `saved = buffer = new`, `dirty = false`, and the new content replaces the document. The CodeMirror value prop updates in place; this is not an edit, but history and selection may be affected.
    - When dirty: `reload()` returns `false` and the conflict banner appears.
  - Conflict banner `.editor-conflict` (`role="alert"`):
    - Text: "This file was modified elsewhere while you have unsaved changes."
    - Button **Reload from disk** → `reload(true)` (forced re-read) and hides the banner.
    - Button **Keep my version** → hides the banner (the buffer stays dirty, and the next save overwrites the disk).
    - Edge case: a forced reload whose disk content equals the old saved baseline is a no-op, so the buffer stays dirty.
    - CSS: flex, `gap:8px`, `padding:4px 10px`, `12px`, background `color-mix(var(--destructive) 12%, var(--background))`, `border-bottom 1px var(--border)`. Buttons: `padding:1px 8px`, `radius 4px`, border, background `var(--background)`.
    - Banner strings are hard-coded English, not i18n.
  - A reload that returns `binary` or `toolarge` switches the pane to that view; a reload error switches it to the error view.
- [ ] `markSaved(content)` exists (it adopts externally formatted content as the baseline) but EditorPane does not use it.

### 7. Language resolution and supported languages

- [ ] `resolveLanguage(path)` runs after the doc becomes `ready`, then reconfigures `languageCompartment`. A miss gives `[]` (plain text, no highlighting, no live preview).
  - Matching uses the lower-cased basename: first an exact filename (the `filenameMap` is empty because no language defines `filenames`), then the substring after the last `.`.
  - A name with no dot, or ending in a dot, has no extension.
  - Results are cached per key `name:<base>` or `ext:<ext>`, including negative results (`null`).
- [ ] `resolveDisplayName(filename)`: `null` → `"Plain Text"`; a known language → its name; otherwise the basename with its first letter capitalised.
- [ ] File languages (`languageDefinitions.ts`). The id is the first extension.

  | Name | Extensions | Loader |
  |---|---|---|
  | JavaScript | `js`, `cjs`, `mjs` | `javascript()` |
  | TypeScript | `ts`, `cts`, `mts` | `javascript({typescript:true})` |
  | JavaScript React | `jsx` | `{jsx:true}` |
  | TypeScript React | `tsx` | `{jsx:true, typescript:true}` |
  | Rust | `rs` | |
  | Python | `py` | |
  | JSON | `json`, `jsonc` | |
  | Markdown | `md`, `markdown` | see below |
  | HTML | `html`, `htm` | |
  | CSS | `css` | |

  Markdown loads `markdown({base: markdownLanguage /*GFM*/, codeLanguages: languages /*@codemirror/language-data*/, extensions: Table})` **plus** `markdownStyle` (§8), `codeBlockStyle` (§10) and `tableStyle` (§9). Live preview, the code-block chrome and table widgets therefore exist **only** for `.md` and `.markdown` files.
- [ ] GFM base gives: tables, task lists, strikethrough (`~~`), autolinks.
- [ ] Fenced-code highlighting inside Markdown uses `@codemirror/language-data` (143 languages, loaded lazily).
  - The info string is matched by `LanguageDescription.matchLanguageName(languages, info, fuzzy=true)`: a case-insensitive name or alias match, then a fuzzy word match. File extensions are not used for this.
  - Language names: C, C++, CQL, CSS, Go, HTML, Java, JavaScript, Jinja, JSON, JSX, LESS, Liquid, MariaDB SQL, Markdown, MS SQL, MySQL, PHP, PLSQL, PostgreSQL, Python, Rust, Sass, SCSS, SQL, SQLite, TSX, TypeScript, WebAssembly, XML, YAML, APL, PGP, ASN.1, Asterisk, Brainfuck, Cobol, C#, Clojure, ClojureScript, Closure Stylesheets (GSS), CMake, CoffeeScript, Common Lisp, Cypher, Cython, Crystal, D, Dart, diff, Dockerfile, DTD, Dylan, EBNF, ECL, edn, Eiffel, Elm, Erlang, Esper, Factor, FCL, Forth, Fortran, F#, Gas, Gherkin, Groovy, Haskell, Haxe, HXML, HTTP, IDL, JSON-LD, Julia, Kotlin, LiveScript, Lua, mIRC, Mathematica, Modelica, MUMPS, Mbox, Nginx, NSIS, NTriples, Objective-C, Objective-C++, OCaml, Octave, Oz, Pascal, Perl, Pig, PowerShell, Properties files, ProtoBuf, Pug, Puppet, Q, R, RPM Changes, RPM Spec, Ruby, SAS, Scala, Scheme, Shell, Sieve, Smalltalk, Solr, SML, SPARQL, Spreadsheet, Squirrel, Stylus, Swift, sTeX, LaTeX, SystemVerilog, Tcl, Textile, TiddlyWiki, Tiki wiki, TOML, Troff, TTCN, TTCN_CFG, Turtle, Web IDL, VB.NET, VBScript, Velocity, Verilog, VHDL, XQuery, Yacas, Z80, MscGen, Xù, MsGenny, Vue, Angular Template.
  - Aliases:
    - C++ `cpp`
    - CQL `cassandra`
    - HTML `xhtml`
    - JavaScript `ecmascript, js, node`
    - JSON `json5`
    - TypeScript `ts`
    - XML `rss, wsdl, xsd`
    - YAML `yml`
    - PGP `asciiarmor`
    - C# `csharp, cs`
    - CoffeeScript `coffee, coffee-script`
    - Common Lisp `lisp`
    - F# `fsharp`
    - JSON-LD `jsonld`
    - LiveScript `ls`
    - Objective-C `objective-c, objc`
    - Objective-C++ `objective-c++, objc++`
    - Properties files `ini, properties`
    - Pug `jade`
    - R `rscript`
    - Ruby `jruby, macruby, rake, rb, rbx`
    - Shell `bash, sh, zsh`
    - SPARQL `sparul`
    - Spreadsheet `excel, formula`
    - LaTeX `tex`
  - Port minimum: every entry in the code-block toolbar list (§10) must resolve.

### 8. Markdown live preview (`markdownStyle.ts`)

How it works:
- A ViewPlugin rebuilds its decorations on doc change, viewport change or selection change.
- It walks only the syntax tree inside the **visible viewport**.
- The "cursor line" is the line holding `selection.main.head`. Other selection ranges are ignored.
- "Off" below means a node whose **first line** differs from the cursor line; "on" means it is on the cursor line.
- Hidden markers are `Decoration.replace({})`: the text is collapsed but stays in the document.
- Nothing inside a `Table` node is decorated (§9 owns tables).
- Decorations are sorted by `from` ascending, then shorter first.

| Construct (Lezer node) | Cursor off the line | Cursor on the line |
|---|---|---|
| ATX heading `#`…`######` (`ATXHeading1-6`) | Line class `cm-md-h<N>`. `HeaderMark` hidden **plus one following space** if the next char is `" "`; trailing closing `#` marks (also HeaderMark) are hidden too | Line class still applied (size and colour stay); `#` marks visible |
| Setext headings (`===`/`---` underline) | not styled by this plugin (only the syntax highlight's bold `t.heading`) | same |
| Emphasis `*a*` / `_a_`, strong `**a**` | `EmphasisMark` hidden; text italic or bold through the highlight style | marks visible, text still italic or bold |
| Strikethrough `~~a~~` | `StrikethroughMark` hidden; text line-through | marks visible |
| Inline code `` `a` `` | `CodeMark` hidden; text `--syntax-string`, mono | marks visible |
| Fenced code fences ```` ``` ```` | the fence backticks are also `CodeMark`, so on non-cursor lines the opening and closing backticks are **hidden** (the info string such as `js` stays visible, with the toolbar after it) | backticks visible |
| Blockquote `> ` | Every line of the blockquote gets class `cm-md-blockquote` (`border-left:3px solid var(--border); padding-left:1em; color: var(--muted-foreground)`). Each `QuoteMark` is hidden **plus one following space**, per line: only the cursor line's `>` shows. Nested quotes add the class again | that line's `>` visible |
| Callout `> [!TYPE] title`, first-line regex `/^(>\s*)(\[!(NOTE\|WARNING\|TIP\|IMPORTANT\|CAUTION)\])/i` | Every line of the blockquote gets `cm-callout cm-callout-<type>` **instead of** `cm-md-blockquote`. On the first line, the `[!TYPE]` text is replaced by `CalloutHeaderWidget` → `<span class="cm-callout-header cm-callout-<type>-header">ICON Label</span>`. The title text after it stays as written. `>` marks hidden as above | First line shows raw `[!TYPE]`; line classes stay |
| Horizontal rule `---`/`***`/`___` | line class `cm-md-hr` (`border-bottom:1px solid var(--border); margin-bottom:-1px`); **the `---` characters stay visible** | same |
| Task marker `[ ]`/`[x]`/`[X]` (`TaskMarker`) | **Always** replaced, even on the cursor line, by a checkbox widget `span.cm-task-checkbox` (contentEditable=false) holding `<input type=checkbox>`. Checked if `[x]` or `[X]`. Mousedown is prevented; click re-reads the 3 characters at its position and writes `[ ]` or `[x]` (lowercase). The list bullet `- ` stays visible | same, always a widget |
| Inline link `[text](url)` (`Link`); the raw text must match `/^\[([^\]]*)\]\(([^)]*)\)$/` | Whole link replaced by `<a class="cm-md-link-widget" href=url target=_blank rel="noopener noreferrer" title=url>text</a>`. `ignoreEvent()` is true, so the click goes to the anchor (opening a URL or relative link is handled by the wikilinks section). Links with a `"title"`, a nested `]` or `)` in the URL, and reference links `[a][b]` do not match and stay raw | raw `[text](url)` (highlighted: link underline, url colour) |
| Autolink `<https://x>` (`Autolink`) | Replaced by a link widget whose text and href are the URL without `<>` | raw |
| GFM bare URLs | not widgetised (only `t.url` colour) | same |
| Images `![a](b)` | not handled here (image attachments section) | |

- [ ] Heading line classes (chromeTheme). The colours are deliberately different per level:

  | Class | font-size | font-weight | line-height | colour | opacity |
  |---|---|---|---|---|---|
  | `.cm-md-h1` | 1.6em | 700 | 1.4 | `var(--destructive)` | |
  | `.cm-md-h2` | 1.4em | 700 | 1.4 | `var(--syntax-number)` | |
  | `.cm-md-h3` | 1.2em | 700 | 1.4 | `var(--syntax-keyword)` | |
  | `.cm-md-h4` | 1.08em | 700 | | `var(--syntax-string)` | |
  | `.cm-md-h5` | 1em | 700 | | `--syntax-string` | 0.85 |
  | `.cm-md-h6` | 1em | 700 | | `--syntax-string` | 0.65 |

- [ ] Callout metadata (`CALLOUT_META`), as icon / label / class:
  - NOTE → `ℹ` / "Note" / `cm-callout-note`
  - TIP → `💡` / "Tip" / `cm-callout-tip`
  - WARNING → `⚠` / "Warning" / `cm-callout-warning`
  - IMPORTANT → `📌` / "Important" / `cm-callout-important`
  - CAUTION → `🔥` / "Caution" / `cm-callout-caution`
  - The type is upper-cased, so `[!note]` works. Any other type does not match the regex and renders as a plain blockquote. The code has a fallback (`📝` + raw type) but it is unreachable.
- [ ] Callout styles. All callouts have `.cm-callout { padding-left:1em }`.

  | Type | background | border-left |
  |---|---|---|
  | note | `color-mix(--syntax-function 8%)` | `3px solid --syntax-function` |
  | tip | `--syntax-string 8%` | `--syntax-string` |
  | warning | `--syntax-number 10%` | `--syntax-number` |
  | important | `--syntax-keyword 8%` | `--syntax-keyword` |
  | caution | `--destructive 10%` | `--destructive` |

  Header `.cm-callout-header`: `font-weight 700`, `0.85em`, `letter-spacing .04em`, uppercase. Header colours per type are the same tokens as the borders.
- [ ] Link widget: colour `var(--syntax-function)`, underline, pointer cursor; hover colour `var(--foreground)`.
- [ ] Checkbox: wrapper is `inline-flex`, items centred. Input is `14×14px`, `accent-color: var(--primary)`, `vertical-align: middle`, `margin-right: 2px`, pointer cursor. It must stay clickable inside the editable content (WKWebView needs `contentEditable=false`).
- [ ] Live-preview edge cases:
  - A marker hidden off-line reappears the moment the cursor enters that line. Only that one line reveals; a multi-line blockquote reveals only the `>` on the cursor line.
  - Because only the viewport is decorated, off-screen content is not processed.
  - Decorations from other sections are composed on top: frontmatter, wikilinks, tags, mermaid button, region styling.

### 9. Tables (`tableStyle.ts`)

Rendering:
- [ ] Every GFM `Table` node (header row + delimiter row + optional body rows) is **always** replaced by a block widget, whether or not the cursor is there. The raw pipe syntax is never shown in the editor.
- [ ] The decoration comes from a StateField (block decorations cannot come from plugins). It rebuilds on doc change, on a `setTableEdit` effect, or when the syntax tree changes; otherwise it maps through the changes. The **whole document** is scanned, not just the viewport.
- [ ] Parsing:
  - Cell text = raw text between consecutive `TableDelimiter` pipes, trimmed, with `\|` unescaped to `|`. Empty cells are kept, because a row with N cells has N+1 pipes.
  - Alignment row: trim, strip one leading and one trailing `|`, split on `|`. For each segment: `:x:` → `"center"`, `x:` → `"right"`, `:x` → `"left"`, else `null`. Padded with `null` up to the header length.
  - The column count is the header cell count. Body rows may be shorter (missing cells show empty) or longer (extras dropped on reformat).
- [ ] Widget DOM:

  ```
  div.cm-table-container (contentEditable=false; flex, align-items:flex-start, gap 4px, margin 4px 0)
    div.cm-table-wrap (overflow-x:auto; 1px border var(--border); radius var(--radius-md); flex 0 1 auto;
                       max-width = view.contentDOM.clientWidth − 32px, kept in sync by a ResizeObserver)
      table.cm-table-grid (border-collapse; table-layout:auto; width:auto; min-width:100%; font var(--font-sans); 13px)
        thead > tr > th.cm-table-cell.cm-table-cell-header × cols
        tbody > tr > td.cm-table-cell × cols
      div.cm-table-add-row "+ Add row"
    div.cm-table-add-col "+" (title "Add column")
  ```

  - Cells: `1px solid var(--border)`, `padding:4px 8px`, `min-width:80px`, `position:relative`, `white-space:pre-wrap`, `word-break:break-word`, `tabIndex=-1`.
  - Header cells: background `var(--secondary)`, weight 600.
  - Cell text goes in `span.cm-table-cell-text` (`display:block; min-height:1.4em`) as **plain text**. No inline Markdown is rendered in cells, and the delimiter alignment is **not** applied visually (no text-align).
- [ ] Wide tables scroll horizontally inside `.cm-table-wrap` only, never the whole editor. The add-column button stays outside that scroll area.

Edit state:
- [ ] `TableEditState = {tableAnchor (table start pos), activeRow (0 = header), activeCol, cellEditing}` is held in `tableEditField` and set with `setTableEdit` (`null` = not in a table).
- [ ] On doc changes the anchor is mapped with `mapPos(anchor, -1)`.
- [ ] A table is "active" when `editState.tableAnchor === node.from`.
- [ ] The active cell gets one of:
  - `.cm-table-cell-active` (navigating): `outline:2px solid var(--primary); outline-offset:-2px`.
  - `.cm-table-cell-editing` (editing): the same outline plus background `color-mix(var(--primary) 8%)` and a text cursor.
- [ ] In the editing cell, the text is replaced by `<input type=text class="cm-table-cell-input">`: block, `width:100%`, no border or outline, transparent background, inherited font and colour, `padding:0`.
- [ ] After each render, the active element is focused on the next animation frame: the input with its caret at the end, or the cell `td`. It is scrolled into view (`block/inline: "nearest"`).
- [ ] While a table is active, the editor selection is parked at `table.from + 1`, clamped to `≤ to−1`, so the Helix block cursor is hidden under the widget.

Entering:
- [ ] Clicking any cell that is not already being edited calls `activate(r, c, editing=true)`, which edits it directly.
- [ ] The insert-table command (§11) creates a table and starts editing cell (0,0).
- [ ] There is no keyboard way to enter an existing table from the text: the block widget is skipped by cursor motion.

Keys while **navigating** (`cellEditing=false`, focus on the td). All keys are `preventDefault`ed and stop propagating, including unhandled ones, which are swallowed.
- [ ] `h` / ArrowLeft and `l` / ArrowRight: move one column, clamped to `[0, cols−1]` (no wrapping).
- [ ] `j` / ArrowDown: next row. Moving past the last row **exits the table after it**: the cursor goes to `table.to + 1` (clamped to the doc length) and the edit state is cleared.
- [ ] `k` / ArrowUp: previous row. Moving above the header **exits before the table**: the cursor goes to `max(0, table.from − 1)`.
- [ ] `i` / Enter: start editing the active cell.
- [ ] Escape: exit before the table.
- [ ] Tab / Shift-Tab: same as in editing mode (below).

Keys while **editing** (inside the input):
- [ ] Typing updates the cell model and schedules a flush after `FLUSH_DEBOUNCE_MS = 200`. Blur flushes immediately.
- [ ] Enter: move down to the same column, still editing. On the last row it first appends a blank row.
- [ ] Escape: back to navigating at the same cell.
- [ ] Tab: next cell, editing. After the last column it wraps to column 0 of the next row. After the last cell of the last row it appends a row and edits its first cell.
- [ ] Shift-Tab: previous cell. Before column 0 it wraps to the last column of the previous row (row clamped at 0, so from header (0,0) it goes to the header's last column).
- [ ] Other keys behave normally in the `<input>`. Left/right move the caret; Up/Down are not intercepted.

Structural edits (mouse only):
- [ ] `×` (`div.cm-table-row-controls`, role=button, title "Remove row") sits in the first cell of every **body** row.
  - It appears only while that first cell is hovered: `visibility:hidden` becomes visible on `.cm-table-cell:hover`.
  - Style: absolute, `top:2px; right:2px`, `10px`, background, border, `radius-sm`, `padding:1px 4px`; on hover the colour and border become `--destructive`.
  - Behaviour: the header can never be removed, and at least one body row is always kept (no-op when only one body row remains).
  - After removal, the table is re-activated in navigation mode at `min(activeRow, rows−1)`, keeping the column. With no edit state this means header (0,0).
- [ ] "+ Add row" bar (`.cm-table-add-row`, `11px`, muted, `padding:3px 8px`, `border-top`; hover foreground on secondary): appends a blank row at the end without activating it.
- [ ] "+" add-column button (`.cm-table-add-col`, 22×22px, `13px`, border, `radius-md`; hover `--primary`): appends a column at the end with empty cells and `null` alignment.
- [ ] There is no UI to delete a column, reorder, or change alignment. Alignment is preserved from the source.
- [ ] Clickable controls are `div`s, not `<button>`s (WKWebView swallows button clicks inside contentEditable). Mousedown is prevented on them so the editor does not lose its selection.

Auto-format on every flush:
- [ ] Every flush rewrites the **whole table** in canonical form. It does nothing if the result equals the current text; otherwise it dispatches one change replacing `[node.from, node.to]`. If the dispatch happens during another update, it is retried with `setTimeout(0)`.
- [ ] Column width = `max(3, longest header or cell text length)`, using JS string length (UTF-16 code units, not display width).
- [ ] Data row: `|` + for each column ` ` + padded cell + ` `, joined by `|`, + `|`.
  - Padding by alignment: right → left-pad; center → `floor(pad/2)` on the left and the rest on the right; left or none → right-pad.
  - `|` in cell text is escaped as `\|`.
- [ ] Delimiter row: `-` repeated `max(3, w)` times, then:
  - left → `:` + dashes[1:]
  - right → dashes[:-1] + `:`
  - center → `:` + dashes[1:-1] + `:`
  - Each part is wrapped in spaces.
- [ ] Rows are joined with `\n`, with no trailing newline.
- [ ] `DEFAULT_TABLE_MARKDOWN` (2×2):

  ```
  | Column 1 | Column 2 |
  | -------- | -------- |
  |          |          |
  ```

### 10. Fenced code blocks (`codeBlockStyle.ts`, Markdown only)

- [ ] For every `FencedCode` node in the viewport, a line class is applied whether or not the cursor is there:
  - the opening line gets `cm-fence-header`
  - the closing line gets `cm-fence-footer`
  - lines between get `cm-fence-body`
- [ ] Box styling:
  - All three: background `var(--secondary)`.
  - Header: top-left and top-right radius `var(--radius-sm)`, border top, left and right `1px var(--border)`, `padding-top:2px`.
  - Body: border left and right.
  - Footer: bottom radii, border bottom, left and right, `padding-bottom:2px`.
- [ ] Unclosed fences: the node ends at the last line, which therefore gets the footer style. A one-line fence gets only the header style.
- [ ] Toolbar widget `span.cm-fence-toolbar` (contentEditable=false; inline-flex, `gap:6px`, `margin-left:12px`, vertical-align middle) sits at the end of the opening line (`side:1`). It renders whether or not the cursor is there and contains two controls.
- [ ] Language `<select class="cm-fence-lang" title="Change language">`:
  - Style: mono, `11px`, background `var(--background)`, muted colour, border, `radius-sm`, `padding:1px 4px`.
  - Options, in order:
    - first: `plain` (value `""`)
    - then: `typescript, javascript, tsx, jsx, python, rust, go, java, c, cpp, css, html, json, yaml, toml, xml, sql, bash, sh, markdown, dockerfile`
    - The current info string (trimmed, lower-cased) is pre-selected. If it is not in the list, it is inserted as an extra option directly after `plain`.
  - Changing the selection replaces the `CodeInfo` range with the new value. With no info string, it replaces `[node.from + 3, openingLine.to]`, which assumes the fence starts exactly at node.from with 3 characters. Choosing `plain` writes an empty info string.
  - Mousedown stops propagation so the native dropdown opens.
- [ ] `<button class="cm-fence-copy" title="Copy code to clipboard">Copy</button>`:
  - Style: sans, `11px`, `padding:1px 6px`; otherwise like the select.
  - Copies the text from `openingLine.to + 1` to `closingLine.from`, `trimEnd()`ed, with `navigator.clipboard.writeText`.
  - The label shows `Copied` for **2000 ms**, then reverts.
  - Hover on either control: foreground colour and border.
- [ ] Mermaid fences also get a "Preview" button (`.cm-mermaid-preview-btn`, from the mermaid section).

### 11. Rebindable editor shortcuts (`shortcuts.ts`)

- [ ] One `Prec.highest` keymap is built from `settings.shortcuts` (each binding has `preventDefault:true`) and is reconfigured live when bindings or fold markers change. Key syntax is CodeMirror's (`Mod-`, `Shift-`, `Alt-`).

  | Action | Default | Behaviour |
  |---|---|---|
  | `openOutline` | `Mod-o` | open the Outline overlay (§13) |
  | `toggleCheckboxAtCursor` | `Mod-Enter` | For each selection range, find the **first** match of `/\[([ xX])\]/` anywhere on the head's line and swap it: checked → `[ ]`, otherwise `[x]`. Returns false (no-op) if no line has a match. It matches any `[x]` in the text, not only task markers |
  | `insertDate` | `Mod-Shift-d` | replace each selection with local `YYYY-MM-DD`; cursor after the text |
  | `insertDateTime` | `Mod-Alt-d` | local `YYYY-MM-DD HH:mm` (24-hour, zero-padded) |
  | `insertRegion` | `Mod-Shift-r` | Empty selection: insert `<start> Region\n<end>\n` at the cursor (mid-line too, with no leading newline) and put the cursor at `from + start.length + 1`, before `Region`. Non-empty: insert `<start> Region\n` at the start of the first selected line and `\n<end>` at the end of the last selected line, then select the whole wrapped block |
  | `insertTable` | `Mod-Alt-t` | Replace the main selection with `DEFAULT_TABLE_MARKDOWN`. Add a leading `\n` if the char before is not `\n` (and not at doc start), and a trailing `\n` if the char after is not `\n` (and not at doc end). In one transaction: cursor at tableFrom+1, `setTableEdit{tableAnchor: tableFrom, row 0, col 0, editing:true}`, focus. Result: header cell "Column 1" being edited |
  | `toggleTaskLine` | `Mod-Shift-Enter` | tasks section (`taskHelpers.ts`) |
  | `pickDueDate` | `Mod-Shift-.` | tasks section |
  | `pickRecurrence` | `Mod-Shift-,` | tasks section |

- [ ] `buildShortcutCommands(foldMarkers, onOpenOutline)` is shared by the keymap and by `runAction` (command palette).
- [ ] Other non-rebindable keys inside the editor:
  - `Mod-s`: save.
  - basicSetup keymaps: default, search (§5), history (`Mod-z` undo, `Mod-y` / `Mod-Shift-z` redo, `Mod-u` undo selection), closeBrackets.
  - foldKeymap (§14).
  - lintKeymap (`Mod-Shift-m`, `F8`; inert because no linters are configured).
  - Font zoom keys belong to the settings section.

### 12. Scroll position persistence (`scrollPositions.ts`)

- [ ] A module-level `Map<path, scrollTopPx>` is loaded once at startup from localStorage key **`helix.scrollPositions`**.
  - Format: a JSON **array of `[path: string, scrollTop: number]` tuples**, ordered oldest to newest.
  - Invalid JSON or invalid entries are dropped silently.
- [ ] `setScrollPosition` deletes and re-inserts the entry, so the most recent is last. It then schedules a save. The save is throttled: the first call starts a `400 ms` timer and later calls do not reset it. The save writes the last **500** entries (`MAX_ENTRIES`). Storage errors are ignored.
- [ ] The extension restores `scrollDOM.scrollTop = saved` in a `requestAnimationFrame` after the view is created. It records the position on every `scroll` event and once more on view destroy (tab switch or close).
- [ ] The value is a raw pixel offset. It is keyed by absolute path; renames are not migrated.
- [ ] Cursor and selection are **not** persisted. A remount starts with the cursor at position 0, unless `focusLine` is given.

### 13. Outline overlay (`OutlineOverlay.tsx`, `outline.ts`, `fuzzyMatch.ts`)

- [ ] Opened by the `openOutline` shortcut (default `Mod-o`) or by `runAction("openOutline")`. It renders inside `.editor-canvas`.
- [ ] Headers are extracted **once when the overlay opens**. Every document line is tested against `HEADER_RE = /^(#{1,6})\s+(.+?)\s*$/`, giving `{level: #count, text, line (1-based), from}`.
  - Edge cases:
    - Lines inside fenced code blocks and frontmatter are also matched (for example a bash `# comment`).
    - Indented headings, `#tag` without a space, and setext headings are not matched.
    - A trailing closing `##` stays part of the text.
- [ ] Empty query: all headers in document order. Otherwise `fuzzyMatch(query, text)`:
  - Case-insensitive subsequence match; every query character must appear in order.
  - Score: +1 per matched char, +3 instead if it directly follows the previous match, +2 bonus if at index 0 or after a `\W` char.
  - Final score minus `0.01 × target length`.
  - Results are sorted by score descending; `indices` holds the matched positions for highlighting.
  - A whitespace-only query counts as empty.
- [ ] The active index resets to 0 whenever the result list changes. The input autofocuses on open.
- [ ] Keys inside the input:
  - `Escape`: close (focus is not explicitly returned to the editor).
  - `ArrowDown` / `ArrowUp`: move the active item, clamped with no wrapping.
  - `Enter`: go to the active item.
  - Mouse hover sets the active item; click goes to it.
  - Mousedown on the backdrop closes; mousedown inside the panel is stopped.
- [ ] Go-to: selects the **entire heading line** (from to `line.to`), scrolls it to the centre, focuses the editor and closes.
- [ ] UI strings (hard-coded English): placeholder `Go to header…`, empty state `No headers found`.
- [ ] Each item is a `button.outline-overlay-item`:
  - `padding-left = 10 + (level − 1) × 14 px`.
  - Contents: a lucide `Hash` icon (size 11, stroke 2, `.outline-overlay-icon` muted), the text (ellipsis), and `L<line>` (`.outline-overlay-line`, `10px`, muted).
  - Matched characters are wrapped in `<mark class="outline-overlay-highlight">`: background `color-mix(--syntax-number 35%)`, radius 2px.
- [ ] Styles:
  - Backdrop `.outline-overlay-backdrop`: `position:absolute; inset:0; z-index:30`, horizontally centred, `padding-top:10%`, background `color-mix(black 25%)`.
  - Panel `.outline-overlay`: `width:min(440px,90%)`, `max-height:60%`, card colours, border, `radius-md`, shadow `0 12px 32px color-mix(black 30%)`.
  - Input: no border except `border-bottom`, `13px`, `padding:10px 12px`.
  - List: `overflow-y:auto`, `padding:4px`.
  - Item: `12px`, `padding:6px 10px`, `radius-sm`, `gap:6px`; active item uses accent colours.
  - Empty state: `16px` padding, centred, `12px`, muted.

### 14. Folding: gutter, keymap and custom regions

- [ ] Fold sources:
  - Language-provided fold ranges: Markdown heading sections and lists (from lang-markdown), code languages' brackets and blocks.
  - The custom region `foldService` (below).
  - All of them appear as chevrons in the fold gutter (§3).
- [ ] Fold keymap (basicSetup `foldKeymap`):
  - `Ctrl-Shift-[` (mac `Cmd-Alt-[`): fold.
  - `Ctrl-Shift-]` (mac `Cmd-Alt-]`): unfold.
  - `Ctrl-Alt-[`: fold all.
  - `Ctrl-Alt-]`: unfold all.
- [ ] Generic fold placeholder: `span.cm-foldPlaceholder` with text `…`; clicking it unfolds.
- [ ] Region syntax:
  - The start line's `trimStart()` starts with `foldStartMarker` (default `:::fold`). The name is the rest of the line, trimmed, defaulting to `"Region"`. Example: `:::fold My Section`.
  - The end line's `trimStart()` starts with `foldEndMarker` (default `:::endfold`).
  - Leading indentation is allowed and anything may follow the marker.
  - Regions **nest**: depth is counted while scanning forward from the start line.
  - The start test runs before the end test, so markers where the end begins with the start would be misread.
  - If either marker is blank or whitespace, all region features are disabled (`[]` / `Decoration.none`).
- [ ] Fold range (`regionFolding.ts`): `{from: startLine.to, to: matchingEndLine.to}`. The end-marker line is hidden inside the fold, and the start line stays visible. There is no fold if no matching end exists.
- [ ] Folded placeholder (`regionFoldWidget.ts`, `codeFolding` with a custom `preparePlaceholder`/`placeholderDOM`). Used when the line before the fold starts with the start marker:
  - `span.cm-region-fold-pill` with `aria-label="Folded region: <name>"`, containing `span.cm-region-fold-icon` 📁, `span.cm-region-fold-name` (name), and `span.cm-region-fold-count` `"<N> lines"`. N = the number of lines strictly between the start and end markers (≥0, always plural "lines").
  - Click unfolds.
  - CSS: inline-flex, `gap:6px`, `padding:1px 8px`, `radius-sm`, `border 1px color-mix(--syntax-function 40%)`, background `--syntax-function 14%` (24% on hover), colour `--syntax-function`, sans `11px` weight 600, vertical-align middle. Count: muted, weight 500. Icon: `11px`, line-height 1.
- [ ] Region decorations (`regionDecorations.ts`, a ViewPlugin) scan the **whole document**. They rebuild on doc change, viewport change, selection change, or fold/unfold effects.
  - **Unfolded** start line, cursor elsewhere:
    - Line class `cm-region-start-line` (background `color-mix(--syntax-function 6%)`).
    - The whole line text is replaced by `span.cm-region-bar.cm-region-bar-start`: inline-flex, `gap:6px`, `width:100%`, border-box, `radius-sm`, `padding:2px 8px`, border `1px color-mix(--syntax-function 35%)`, background `10%`.
    - Bar contents:
      - `span.cm-region-chevron` "▾" (12px wide, `--syntax-function`, foreground on hover, pointer). Clicking it folds this region (`foldable` + `foldEffect`); mousedown is prevented.
      - `span.cm-region-icon` 📁 (`11px`).
      - `span.cm-region-label` with the name (sans `12px`, weight 600, `letter-spacing .01em`, `--syntax-function`).
  - **Folded** start line, cursor elsewhere: the raw marker text is replaced by an empty `span.cm-region-marker-hidden` (`display:none`) and only the fold pill shows. Scanning skips to the end of the fold.
  - End line, cursor elsewhere:
    - Line class `cm-region-end-line` (same 6% background).
    - Text replaced by `span.cm-region-bar.cm-region-bar-end`: `height:4px`, `width:40px`, `margin-left:2px`, `border-left` and `border-bottom` `2px color-mix(--syntax-function 30%)`, bottom-left radius. This draws a small "└" closing tick.
  - Body lines (depth > 0): line class `cm-region-body-line` (`border-left 2px color-mix(--syntax-function 18%)`, `padding-left:6px`). Nested bodies get it too.
  - When the cursor is on a start or end line, that line shows raw marker text with no line class. Depth is still tracked.
- [ ] The insert-region shortcut is described in §11.

### 15. Rulers (`rulers.ts`)

- [ ] Setting: `rulers: number[]` (default `[]`, meaning no rulers). The Settings UI edits it as text; `parseRulers(text)` and `formatRulers` live here.
  - Parsing splits on `/[\s,;]+/` and keeps integers `1..1000`, deduplicated and sorted ascending.
  - Formatting joins with `", "` (e.g. `"80, 120"`).
- [ ] Rendering:
  - `div.cm-rulers` (`aria-hidden`; absolute at top-left, `overflow:hidden`, `pointer-events:none`, `z-index:1`) is appended inside the scroller.
  - It contains one `div.cm-ruler` per column, with `data-column` set: absolute, `top:0; bottom:0`, `width:1px`, background `color-mix(in oklch, var(--foreground) 12%, transparent)`.
- [ ] Position: `left = (contentDOM.left − scroller.left + scrollLeft) + contentDOM padding-left + column × defaultCharacterWidth`, so the ruler sits just after column N in monospace characters. Lines with a larger font (headings) do not line up.
- [ ] Container size: height = `max(scrollHeight, clientHeight)`, so it spans the whole document and scrolls with it; width = `max(clientWidth, left + content width)`, so it never adds a horizontal scrollbar.
- [ ] Re-measured on `geometryChanged` or `viewportChanged`, and after the first measure.

### 16. Document statistics and status bar (`docStats.ts`, `src/modules/statusbar/StatusBar.tsx`)

- [ ] `computeDocStats(text)`:
  - `words` = 0 if the trimmed text is empty, else `trimmed.split(/\s+/).length`. Markdown syntax tokens such as `#`, `-` and `|` count as words.
  - `readingTimeMin` = 0 if there are no words, else `max(1, round(words / 200))` (`WORDS_PER_MINUTE = 200`).
- [ ] Live updates: an updateListener on doc changes, debounced **200 ms** (`REPORT_DEBOUNCE_MS`). Initial load and external reloads recompute immediately from the loaded content.
- [ ] The status bar below the editor shows `"<n> word"` or `"<n> words"` (singular only for 1), `·`, then `"<m> min read"` (`"0 min read"` when empty). Strings are hard-coded English.
- [ ] CSS `.status-bar`:
  - flex, `gap:6px`, `height:22px`, `padding:0 10px`
  - `border-top 1px var(--border)`
  - background `color-mix(var(--card) 85%)`
  - `11px`, muted colour
  - The separator has `opacity .6`.

### 17. Briefly: features composed here but documented by other sections

- [ ] Frontmatter styling: `.cm-frontmatter-line` background `foreground 4%`, fence muted, key `--syntax-keyword` at opacity .85.
- [ ] Wikilinks: `.cm-wikilink` in `--syntax-function` with a 45% bottom border; `-unresolved` is muted with a dashed border.
- [ ] Tags: `.cm-tag` pill in `--syntax-keyword` at 12% background (22% on hover), radius 999px, `padding 0 6px`.
- [ ] Mermaid preview button: `.cm-mermaid-preview-btn` (`--syntax-function` at 14% → 24%, `11px`, weight 600).
- [ ] Path drop, image paste and drop into `<root>/attachments`, and task completions and helpers.
- [ ] The completion system: one `autocompletion()` with providers. Wiring is special because codemirror-helix breaks normal input events.

All of these are passed in through the extension array in §2. These style values are defined in `extensions.ts` `chromeTheme`, so a port must carry them over with the rest of the theme.


---

## 3. Editor Extensions: Links, Tags, Completion, Tasks, Attachments, Frontmatter, Backlinks, Tags View, Search

All extensions below are installed in a single `extensions` array inside `src/modules/editor/EditorPane.tsx` (memoised once per mount; `EditorPane` remounts on every path change via `key={path}`, so `currentPath` is constant for an editor instance). Values that change at runtime (vault file list, tag list, root path, callbacks) are read through refs/getters, so updating them never reconfigures the editor. The same `EditorPane` is used in popped-out editor windows (`src/modules/editor/EditorWindowApp.tsx`), where `onNavigateFile` just swaps the window's path and there is no `getAllTags`/`onTagClick` (tag pills do nothing; tag completion shows nothing).

Order in the array matters (see "Completion aggregator"): `completionKeymap` is first, then the keybinding compartments, ..., `mermaidPreviewExtension`, `wikilinksExtension`, `tagsExtension`, `pathDropExtension`, `imageAttachmentsExtension`, `editorCompletionExtension([...])`, docStats, scroll persistence, `frontmatterExtension()`, shared extensions, language compartment.

Shared design rule for "live preview" decorations (wikilinks, tags, markdown links): the raw syntax is replaced by a widget on every line **except the line holding the main cursor** (`selection.main.head`), so the source is editable in place. Decorations are rebuilt on `docChanged || viewportChanged || selectionSet`, and only over `view.visibleRanges`. Matches whose start position resolves (via `syntaxTree(state).resolveInner(pos, 1).name`) to a node name matching `/Code/` (inline code, fenced code, CodeText, etc.) are skipped.

---

### Vault file list (`useVaultFiles`)

Source: `src/modules/editor/lib/useVaultFiles.ts`. Used by App (`src/App.tsx`), `EditorWindowApp.tsx` (and a separate copy in `src/modules/palette/lib/useVaultFiles.ts` for the quick switcher).

- [ ] With `rootPath == null`, the list is `[]`.
- [ ] On mount / root change, invokes `fs_list_markdown_files` with `{ root: rootPath }` and gets back `string[]`: canonical absolute paths of every `.md`/`.markdown` file (case-insensitive extension), recursive, sorted, skipping dot-entries (files and dirs starting with `.`) and dirs named `node_modules`, `target`, `dist`, `.git`.
- [ ] On invoke error the list becomes `[]`.
- [ ] Re-fetches on every `fs:changed` event (the file watcher event), so notes created after open resolve. Results from a stale effect (root changed meanwhile) are dropped (`cancelled` flag).
- [ ] The list feeds: wikilink resolution + decoration, `[[` completion, `[[Note#` heading completion target resolution, backlinks resolution.

---

### Fuzzy matcher (exact algorithm)

Source: `src/modules/editor/lib/fuzzyMatch.ts`. Used by wikilink, tag, heading and word completion (and the palette/quick switcher elsewhere). Returns `{ matched, score, indices }`.

- [ ] If `query.trim()` is empty -> `{ matched: true, score: 0, indices: [] }` (everything matches; order is then the stable input order since all scores tie).
- [ ] Both strings lowercased (`toLowerCase()`); greedy left-to-right subsequence scan: for each target index `ti`, if `t[ti] === q[qi]` it is consumed (first occurrence wins, no backtracking/optimal alignment).
- [ ] Per matched char: `+3` if the previous matched index was `ti - 1` (consecutive run), else `+1`. Note `prevIndex` starts at `-1`, so a match at `ti === 0` counts as consecutive (`+3`).
- [ ] Additional `+2` if `ti === 0` or the preceding target char matches `/\W/` (word boundary: anything other than `[A-Za-z0-9_]`; so `_` is NOT a boundary but space, `-`, `/`, `.` are).
- [ ] If not all query chars consumed -> `{ matched: false, score: 0, indices: [] }`.
- [ ] Final score = `score - target.length * 0.01` (shorter targets win ties). `indices` = matched target positions.
- [ ] Callers sort descending by score (JS sort is stable, so ties keep input order) and do their own truncation; they pass `filter: false` to CodeMirror so CM does not re-filter/re-rank.

---

### Completion aggregator (single autocomplete instance + helix workaround)

Source: `src/modules/editor/lib/completion.ts`; wiring in `EditorPane.tsx`; background in memory note `helix-autocomplete-gotcha`.

- [ ] Exactly ONE autocomplete engine serves all providers: `autocompletion({ override: providers.map(p => p.source), activateOnTyping: true })`. basicSetup's own autocompletion is disabled (`basicSetup.autocompletion: false`). All sources are queried together; results from every source that returns non-null are merged into one popup.
- [ ] Provider shape: `CompletionProvider = { source: CompletionSource; trigger?: (lineBeforeCursor: string) => boolean }`.
- [ ] Provider order (as registered): 1) wikilink `[[`, 2) heading `[[x#`, 3) tag `#`, 4) task `@due`/`📅`/`🔁`, 5) word.
- [ ] Explicit-open trigger: an update listener, on every transaction batch with `docChanged`, computes `before = line.text.slice(0, head - line.from)` for the main cursor and calls `startCompletion(view)` if ANY provider trigger returns true. Reason: helix insert-mode input does not emit the `input.type` userEvent, so `activateOnTyping` never fires on its own, and helix swallows `Ctrl-Space` (no manual trigger exists).
- [ ] Suppression: if any transaction in the update carries the `pickedCompletion` annotation (i.e., the edit was the acceptance of a completion), the popup is NOT reopened. Custom-`apply` completions (wikilink, heading) must add `annotations: pickedCompletion.of(completion)` explicitly; task completions do NOT add it (so after accepting a task token the trigger may reopen the popup if the text still matches -- e.g. `📅 2026-08-28` matches `📅\s*\S*`; port should replicate or deliberately fix).
- [ ] Popup keymap `completionKeymap`, at `Prec.highest` and placed FIRST in the extension array (ties within `Prec.highest` are broken by order, so it beats the helix/vim keymap):
  - `Enter` -> `acceptCompletion`
  - `ArrowDown` -> `moveCompletionSelection(true)`
  - `ArrowUp` -> `moveCompletionSelection(false)`
  - `Escape` -> `closeCompletion`
  - Each returns false when no popup is open, so the key falls through (Enter inserts newline, Escape leaves insert mode). Other CM default completion keys (PageUp/PageDown, Tab not bound) come from CM's default completion keymap and may be shadowed by helix.
- [ ] Popup visuals (EditorView theme in `src/modules/editor/lib/extensions.ts`): `.cm-tooltip` background `var(--card)`, color `var(--card-foreground)`, `1px solid var(--border)`, radius `var(--radius-sm)`; selected item `.cm-tooltip-autocomplete ul li[aria-selected]` background `var(--accent)`, color `var(--accent-foreground)`. Options show `label` plus `detail` (right-side dim text) where provided; `type: "text"` (or `keyword`/`variable` for task options) selects CM's type icon.

---

### Wikilinks `[[target#heading|alias]]`

Source: `src/modules/editor/lib/wikilinks.ts`.

#### Syntax and parsing
- [ ] Decoration regex: `WIKILINK_RE = /\[\[([^[\]\n]+)\]\]/g` -- inner text may not contain `[`, `]` or newline; must be non-empty. Matched per line.
- [ ] Inner split on the FIRST `|`: `target = inner.slice(0, pipeIdx)`, `alias = inner.slice(pipeIdx+1)` (alias may itself contain `|`). No pipe -> alias null.
- [ ] Display label = `(alias ?? target).trim() || target.trim()` -- i.e. trimmed alias if non-empty, else trimmed target. The `#heading` part is NOT stripped from the display label when there is no alias (`[[Note#Intro]]` displays `Note#Intro`).
- [ ] Embed syntax `![[x]]` is not specially handled: the `[[x]]` part renders as a normal wikilink widget and the leading `!` remains visible as raw text. No inline image/note embedding is rendered in the editor.

#### Resolution (`resolveWikilinkTarget(target, vaultFiles)`)
- [ ] `cleaned = target.split("|")[0].split("#")[0].trim()`; empty -> `null` (so `[[#heading]]` self-refs are unresolved for navigation/decoration).
- [ ] If `cleaned` ends with `.md` (case-insensitive) the 3-char extension is stripped -> `want`.
- [ ] Iterates `vaultFiles` in order; for each path computes `basenameNoExt` (basename after last `/`, minus the last `.ext` if the dot is not at index 0). First exact-case equality `base === want` returns immediately. Otherwise remembers the FIRST case-insensitive match and returns it at the end; else `null`.
- [ ] Matching is basename-only: folder components in the target are not interpreted (`[[sub/Note]]` would compare `"sub/Note"` against basenames and fail). Duplicate basenames in different folders -> the first in the (sorted) vault list wins.
- [ ] Only Markdown files are candidates (vault list comes from `fs_list_markdown_files`), so `[[image.png]]` is unresolved.

#### Rendering
- [ ] Each match off the cursor line (and not inside code) is replaced (`Decoration.replace`) by an `<a href="#">` widget whose text is the label.
- [ ] Resolved: class `cm-wikilink`, `title` = resolved absolute path, `data-resolved-path` = path.
- [ ] Unresolved: classes `cm-wikilink cm-wikilink-unresolved`, `title` = `No note named "<label>"`.
- [ ] Widget equality is by `(label, resolvedPath)`; `ignoreEvent()` returns true (CM ignores events inside it).
- [ ] Visuals: `.cm-wikilink` color `var(--syntax-function)`, no text-decoration, `border-bottom: 1px solid color-mix(in oklch, var(--syntax-function) 45%, transparent)`, pointer cursor; hover -> color `var(--foreground)`, border-bottom color `var(--foreground)`. `.cm-wikilink-unresolved` color `var(--muted-foreground)`, border-bottom style `dashed`.
- [ ] Decorations sorted by `from` then by length before `Decoration.set(..., true)`.

#### Click / follow
- [ ] Navigation fires on **mousedown** on the widget's own DOM node (not click), with `preventDefault` + `stopPropagation`, calling `onNavigate(resolvedPath)` (no focus line). This must be handled on the widget itself because the view-level event pipeline skips events from widgets with `ignoreEvent() === true`.
- [ ] Clicking an unresolved wikilink does nothing (no listener). **Creation of missing notes on click is NOT implemented** -- there is no create-on-follow path.
- [ ] To edit the raw link text, the user moves the cursor onto that line (keyboard), which reveals the source.
- [ ] `onNavigate` = App's `openFile(path, line?)` (opens/focuses a tab). In popped-out windows it replaces the window's current file.
- [ ] A secondary view-level `click` handler (`EditorView.domEventHandlers`) also exists: for `a.cm-wikilink` it navigates to `data-resolved-path`; for `a.cm-md-link-widget` (markdown `[text](href)` widgets from `markdownStyle.ts`) it resolves relative hrefs via `resolveRelativeMarkdownLink(href, currentPath)` and calls `onNavigate`, otherwise returns false (external links fall back to the anchor's `target="_blank"`). Caveat: because those widgets also return `ignoreEvent() === true`, this handler may never actually fire; intended behaviour is "clicking a rendered relative markdown link opens that vault file", external links open in the system browser.

#### Relative markdown link resolution (also used by backlinks)
- [ ] `isExternalLink(href)`: `SCHEME_RE = /^[a-z][a-z0-9+.-]*:/i` matches, or href starts with `//`.
- [ ] `resolveRelativeMarkdownLink(href, currentPath)`: trims; returns `null` if empty, starts with `#`, or external. Takes part before first `#`; empty -> null. URI-decodes it (`decodeURIComponent`, falling back to raw on error) and resolves via `resolveRelativePath`.
- [ ] `resolveRelativePath(basePath, relative)`: starts from `dirname(basePath)` (empty if no `/` or slash at index 0), splits on `/`, skips `""` and `.`, `..` pops (never above root, silently), else pushes; re-prefixes `/` if `basePath` was absolute. No extension is added (`[x](note)` resolves to `.../note`, which won't equal `note.md`).

#### `[[` completion
- [ ] Trigger regex: `WIKILINK_TRIGGER_RE = /\[\[([^[\]]*)$/` against text before cursor on the line. Query = text after `[[` (may contain spaces, `|`, `#`).
- [ ] Note: when the query contains `#`, the heading provider also matches; the wikilink provider still runs and fuzzy-matches note basenames against the whole query string (usually yielding nothing).
- [ ] Candidates: every vault file, label = `basenameNoExt(path)`, `detail` = full absolute path (so same-name notes in different folders are both listed, distinguished by detail), `type: "text"`.
- [ ] Ranking: `fuzzyMatch(query, label)`, keep matched, sort by score desc, cap `MAX_COMPLETIONS = 50`. Empty query lists the first 50 files in vault order. No matches -> no popup from this source.
- [ ] Replacement range: from the start of the query to cursor (the `[[` is kept).
- [ ] Apply: if the two chars after the cursor are exactly `]]`, insert just `label`; otherwise insert `label]]`. Cursor goes to `from + insert.length` (i.e. after `]]` when appended, but BEFORE an existing `]]`). Transaction annotated with `pickedCompletion`.
- [ ] Inserts the bare basename (no folder, no `.md`), even if another note shares the basename.

---

### Heading-reference completion `[[Note#heading]]` / `[[#heading]]`

Source: `src/modules/editor/lib/headingCompletion.ts`.

- [ ] Trigger regex: `HEADING_TRIGGER_RE = /\[\[([^[\]#]*)#([^[\]]*)$/` -- group 1 = target (no `#`), group 2 = heading query.
- [ ] Empty/whitespace target -> headings come from the current buffer (`state.doc.toString()`, including unsaved edits).
- [ ] Otherwise resolve target with `resolveWikilinkTarget(target, vaultFiles)`; unresolved -> no results. Then `invoke("fs_read_file", { path: resolved })` returning one of `{ kind: "text"; content: string; size: number }`, `{ kind: "binary"; size }`, `{ kind: "toolarge"; size; limit }`. Non-text or error -> no results. Source is async (awaits the invoke on every keystroke; reads the on-disk version, not another open tab's unsaved buffer).
- [ ] `extractHeadings(text)`: splits on `\n`; fence toggling with `/^\s*(`{3,}|~{3,})/` (fence opens with the marker char; closes only on a line starting with the same marker char -- backtick vs tilde); fence lines and lines inside fences skipped. Heading regex `/^#{1,6}\s+(.+?)\s*#*\s*$/` -> trimmed group 1; trailing closing `#`s stripped (`## Heading ##` -> `Heading`). Setext headings and headings indented by spaces are not recognized. Frontmatter is not skipped specially. Duplicate headings are kept (duplicates appear twice).
- [ ] Ranking: fuzzyMatch(query, heading), score desc, cap 50. Empty query lists all headings in document order.
- [ ] Replacement range starts after `#`. Apply identical to wikilinks: insert `heading` if `]]` follows, else `heading]]`; cursor after the insert; `pickedCompletion` annotation added.
- [ ] Inserted text is the raw heading text (no slugification).
- [ ] Note: the `#heading` part is not used by navigation (clicking `[[Note#Intro]]` opens Note at top, not at the heading).

---

### Word completion (document-local)

Source: `src/modules/editor/lib/wordCompletion.ts`.

- [ ] `MIN_PREFIX_LENGTH = 4`. Trigger regex built as `[A-Za-z][A-Za-z0-9_-]{3,}$` (ASCII only; accented letters break words). Word extraction regex `WORD_RE = /[A-Za-z][A-Za-z0-9_-]{3,}/g`.
- [ ] Never fires when the line-before-cursor matches `OPEN_WIKILINK_RE = /\[\[[^[\]]*$/` (inside an open `[[`).
- [ ] Never fires when the cursor position is inside code (`/Code/` node name check at `context.pos`).
- [ ] Candidates: distinct words of the WHOLE current document (first-seen order, case-sensitive dedupe), excluding any word equal to the typed prefix case-insensitively (the word being typed itself / exact case variants).
- [ ] Ranking: fuzzyMatch(prefix, word) (so subsequence, not just prefix, matches are offered), score desc, cap `MAX_COMPLETIONS = 20`; `type: "text"`, no detail. Accept replaces the typed word (from `match.from`) with the label via CM default apply (gets `pickedCompletion` automatically).
- [ ] Trigger also requires the trigger regex, so the popup opens automatically after typing the 4th consecutive word char.

---

### Inline tags `#tag`

Sources: `src/modules/tags/lib/parseTags.ts` (shared matcher, mirrors Rust `tags_scan` in `src-tauri/src/modules/tags/mod.rs`), `src/modules/editor/lib/tags.ts` (editor pills + completion).

#### Syntax
- [ ] Inline regex: `INLINE_TAG_RE = /(?<=^|\s)#(\p{L}[\p{L}\p{N}_/-]*)/gu` -- `#` preceded by start-of-line or whitespace, followed by a Unicode letter, then letters/digits/`_`/`/`/`-`.
- [ ] Trailing `/`, `_`, `-` stripped: `m[1].replace(/[/_-]+$/, "")` (`#foo/ end` -> `foo`). Match range `from` = index of `#`, `to = from + 1 + tag.length` (stripped chars are not part of the pill).
- [ ] Inline code spans masked first: `CODE_SPAN_RE = /(`+)([^`]*?)\1/g` replaced by equal-length spaces (offsets preserved).
- [ ] Fenced code (vault scan only, `parseInlineTags`): `FENCE_RE = /^(\s*)(`{3,}|~{3,})/`; enters fence on first fence line, exits only on a fence line with the same marker char; fence lines themselves never scanned. (In the editor, fenced code is excluded via the syntax-tree `/Code/` check instead.)
- [ ] Test-verified cases: `some text #foo more` -> `foo`; `#foo rest` at col 0; `#foo/bar` nested -> `foo/bar`; `#one and #two` -> both; `# Heading`, `## Sub heading`, `###### Deep` -> none; `## Heading #tag` -> `tag`; `example.com/#frag` -> none; `foo#bar` -> none; `issue #123` -> none; `` `#notatag` `` -> none; `great post, #foo, nice` -> `foo` (comma stops it); `(#foo).` -> none (paren is not whitespace); tilde fences honored.
- [ ] Tags are case-sensitive (`#Foo` and `#foo` are distinct tags).

#### Frontmatter tags
- [ ] `frontmatterTags(content)` reads `parseFrontmatter(content)?.data.tags`: array -> each item `String(item).trim()` (non-empty); string -> split on `/[\s,]+/` (so `tags: a b c` and `tags: a, b` both work); other scalar (number/bool) -> `String(raw).trim()`; null/missing -> `[]`.
- [ ] `normalizeTag(t) = t.replace(/^#/, "").trim()`.
- [ ] `allTags(content)` = unique union of inline tags and normalized frontmatter tags (tests: `tags: [a, b, "c d"]` -> `a`,`b`,`c d`; block list `- a`/`- b`; inline+fm deduped).

#### Editor pill rendering and click
- [ ] Every inline tag off the cursor line, not in code, replaced by `<a href="#" class="cm-tag">` with text `#<tag>` and `title` = `Filter notes tagged #<tag>`. Widget equality by tag. Decorations rebuilt on doc/viewport/selection change.
- [ ] Visuals: `.cm-tag` color `var(--syntax-keyword)`, background `color-mix(in oklch, var(--syntax-keyword) 12%, transparent)`, `border-radius: 999px`, `padding: 0 6px`, pointer; hover background 22% mix.
- [ ] Mousedown on the pill (preventDefault + stopPropagation) calls `onTagClick(tag)` (no leading `#`). In the main window this is App's `openTagFilter`: sets `selectedTag`, increments `selectedTagToken`, switches main view to `"tags"` (the Tags view then pre-selects that tag; the token makes re-clicking the same tag re-apply).

#### `#` completion
- [ ] Trigger regex: `TAG_TRIGGER_RE = /(?:^|\s)#([\p{L}][\p{L}\p{N}_/-]*)$/u` -- requires at least one letter after `#` (typing bare `#` does NOT open the popup; `# ` heading never triggers).
- [ ] Query = text after the last `#` in the match; replacement range starts after `#`.
- [ ] Candidates: `getAllTags()` = vault tag index entries' tag names in index order (App derives `allTags` from `useTagIndex` when loaded, else `[]`). fuzzyMatch, score desc, cap 50, `type: "text"`, label = tag (no `#`). Default apply replaces the query with the tag.
- [ ] Tags newly typed in the current unsaved buffer are not candidates until the file is saved and re-scanned.

---

### Tasks helpers: checkboxes, 📅 due dates, 🔁 recurrence, `@due` / `@repeat`

Source: `src/modules/editor/lib/taskHelpers.ts` (+ test). Commands bound via shortcuts in `src/modules/editor/lib/shortcuts.ts`; defaults in `src/modules/settings/lib/editorSettings.ts`. The agenda scanner (separate module, documented elsewhere) consumes the format these helpers write.

#### Canonical formats / regexes
- [ ] Due: `DUE_RE = /📅\s*(\d{4}-\d{2}-\d{2})(?:\s+(\d{2}:\d{2}))?/u` -> group1 date `YYYY-MM-DD`, group2 optional `HH:MM`.
- [ ] Recurrence: `RECUR_RE = /🔁\s*(daily|weekdays|weekends|[a-z]{3}(?:,[a-z]{3})*)(?:\s+(\d{2}:\d{2}))?/iu` -> rule is `daily`, `weekdays`, `weekends`, or comma-separated 3-letter day codes (`mon,wed,fri`), optional time.
- [ ] List item: `LIST_RE = /^(\s*)([-*+]|\d+[.)])\s+(?:\[([ xX])\]\s*)?/` -> indent, bullet (`-`,`*`,`+`, `1.`, `1)`), optional checkbox state.
- [ ] Written token form: `${emoji} ${value}` plus ` ${time}` if a time applies. `isoDate(d)` uses LOCAL date `YYYY-MM-DD`.
- [ ] No `TODO:` syntax handling exists in these helpers (if the agenda recognises `TODO:` it is documented in the agenda section).

#### Toggle task line (`toggleTaskLine`, default `Mod-Shift-Enter`)
- [ ] Applies to every line touched by every selection range (each line once).
- [ ] In multi-line ranges, blank lines are skipped; a blank single line becomes `- [ ] `.
- [ ] Non-list line -> insert `- [ ] ` after leading whitespace (`Buy milk` -> `- [ ] Buy milk`; `  nested` -> `  - [ ] nested`).
- [ ] List item without checkbox -> insert `[ ] ` after the bullet+space (`- item` -> `- [ ] item`).
- [ ] Task (checkbox present, any state ` `/`x`/`X`) -> replace `indent+bullet+spaces+[?]+spaces` with `indent + bullet + " "` (`- [ ] item` -> `- item`, `- [x] done` -> `- done`). So the command never toggles checked state; it adds/removes the checkbox.
- [ ] Dispatched with `userEvent: "input"`. Returns false (key falls through) only when there are no changes.

#### Set due date / recurrence (programmatic)
- [ ] `lineTokenSpec(state, re, emoji, value, time?)` on the main cursor line: if the token exists, replace that whole token with `emoji value[ time]`, keeping the existing time when `time` is not given (`- [ ] x 📅 2026-01-01 10:00 tail` -> `... 📅 2026-02-02 10:00 tail`). If absent, trim trailing whitespace and append ` ` + token (no leading space if the line is empty); cursor moves to end of the inserted token (`- [ ] call vet` -> `- [ ] call vet 📅 2026-08-28`; `- [ ] trash` + `mon,wed,fri` + `08:00` -> `- [ ] trash 🔁 mon,wed,fri 08:00`).
- [ ] `setDueDate(view, date, time?)`, `setRecurrence(view, rule, time?)` wrap this.

#### Pickers (`pickDueDate` default `Mod-Shift-.`, `pickRecurrence` default `Mod-Shift-,`)
- [ ] If the line already has the token: move cursor to the end of its value (date / rule) so the popup's replacement covers it. Otherwise trim trailing whitespace and append ` 📅 ` / ` 🔁 ` (no leading space on empty line), cursor after it.
- [ ] Then focus the editor and `startCompletion(view)` -> the task completion popup acts as a quick-pick.
- [ ] Shortcut labels (i18n): "Toggle task" ("Turn the current line into a `- [ ]` task (or back into plain text)."), "Set due date…" ("Add or change the 📅 due date on the current line via a quick-pick list."), "Set recurrence…" ("Add or change the 🔁 repeat rule ..."); PT-BR: "Alternar tarefa", "Definir prazo…", "Definir recorrência…".

#### Quick-pick option lists
- [ ] `dueDateOptions(now)`: `Today` (now), `Tomorrow` (+1), then +2..+6 labeled with the English weekday name (`Sunday`..`Saturday`, not localized) of that date, and +7 labeled `In a week`. Each has `date` and `detail = date`. Test: from 2026-08-27 -> first `Today 2026-08-27`, second `Tomorrow 2026-08-28`, last `In a week 2026-09-03`.
- [ ] `RECURRENCE_OPTIONS`: `Daily`/`daily`/"every day"; `Weekdays`/`weekdays`/"Mon–Fri"; `Weekends`/`weekends`/"Sat–Sun"; `Weekly (Mon)`/`mon`/"every Monday"; `Mon, Wed, Fri`/`mon,wed,fri`/""; `Tue, Thu`/`tue,thu`/"".

#### `@`-trigger completion
- [ ] Trigger regex: `TASK_TRIGGER_RE = /(?:^|\s)(@(?:d(?:ue?)?|date|today|tom(?:orrow)?|r(?:ep(?:eat)?)?|every|recur)?(?:\s\S*)?|📅\s*\S*|🔁\s*\S*)$/u` (must be at line start or after whitespace). Matches: bare `@`, `@d`, `@du`, `@due`, `@date`, `@today`, `@tom`, `@tomorrow`, `@r`, `@rep`, `@repeat`, `@every`, `@recur`, each optionally followed by a space and a partial word; or `📅`/`🔁` followed by optional spaces and a partial value.
- [ ] Kind: text starting with `📅` or matching `/^@(d|date|today|tom)/` -> due; starting with `🔁` or `/^@(r|every|recur)/` -> recur; bare `@` -> due (plus recurrence options appended). Otherwise no result.
- [ ] Options: due -> one per `dueDateOptions()`, label e.g. `Today`, detail = date, inserts `📅 <date>`, boost `10 - i`, type `keyword`. recur -> one per `RECURRENCE_OPTIONS`, detail = description, inserts `🔁 <rule>`, boost `10 - i`, type `variable`. Bare `@` additionally lists `Repeat: <label>` recurrence options with boost `-1`.
- [ ] If the line already has a time on the matching token (`DUE_RE`/`RECUR_RE` group 2), it is appended as ` HH:MM` to the inserted token.
- [ ] Replacement range = the whole trigger text (the `@word ...` or `📅 ...`, excluding the preceding whitespace) -> replaced with token (+ time); cursor at end. No `pickedCompletion` annotation added.
- [ ] Narrowing: `query = text.replace(/^(@\w*|📅|🔁)\s*/u, "").toLowerCase()`; when non-empty, keep options whose label or detail includes it (substring, case-insensitive, e.g. `@due fri` -> Friday entry; `@due 2026-09` -> by date); if none left -> no popup. Note `@today`/`@tomorrow` keywords do NOT preselect -- they just open the due list.

---

### Image paste / drop -> attachments

Source: `src/modules/editor/lib/imageAttachments.ts` (+ test). Attachments dir = `<rootPath>/attachments` (from `EditorPane`); when no vault root, the extension does nothing and the event falls through to default behaviour.

- [ ] Paste: collects `clipboardData.items` whose `getAsFile()` has a MIME type starting `image/`. Drop: collects `dataTransfer.files` with `image/*` type, inserted at `posAtCoords(clientX, clientY)` (fallback: cursor). No images -> not handled.
- [ ] Immediately inserts placeholder `![uploading image…]()` (or `![uploading N images…]()` for N>1, with the `…` ellipsis char) at the position, cursor after it; event `preventDefault`ed.
- [ ] Extension by MIME: `image/png`->`png`, `image/jpeg`->`jpg`, `image/gif`->`gif`, `image/webp`->`webp`, `image/svg+xml`->`svg`, `image/bmp`->`bmp`; otherwise the original filename's last `.` segment, else `png`.
- [ ] Base name: original filename minus extension, sanitized with `.replace(/[^\w.-]+/g, "-").replace(/^-+|-+$/g, "")`. If empty or matching `/^(image|screenshot|pasted|clipboard)/i` (generic clipboard names), name becomes `pasted-<timestamp>.<ext>`; else `<base>.<ext>`.
- [ ] `timestamp()` = local time `YYYYMMDD-HHMMSS`.
- [ ] Bytes base64-encoded (chunked 0x8000) and written via `invoke("fs_write_binary", { path: "<dir>/<name>", dataBase64 })` (no return value used). If the error string contains `already exists`, retries once as `<stem>-<timestamp><.ext>`. (Backend creates the attachments folder as needed -- see backend section.)
- [ ] Files saved sequentially; each produces `![<name without ext>](<relative path>)` where the relative path is computed from the note's directory to the saved file by `relativePath(fromDir, toPath)` (POSIX, common-prefix + `..`): `/v` -> `attachments/a.png`; `/v/sub/deep` -> `../../attachments/a.png`; `/v/attachments` -> `a.png`. Not URL-encoded (spaces kept).
- [ ] When done, finds the FIRST occurrence of the placeholder text in the doc and replaces it with the links joined by `\n`; cursor after. If the placeholder was deleted meanwhile, nothing is inserted (file still saved).
- [ ] On any error, appends `<!-- image upload failed: <error> -->` (after any links already produced) in place of the placeholder.
- [ ] Rendering: inserted `![alt](path)` is standard Markdown image syntax; the editor does NOT render images inline (no image widget; Lezer `Image` nodes are left as raw/styled text). Images display only when an image file itself is opened in a tab (`EditorPane` image viewer for `png,jpg,jpeg,gif,webp,svg,ico`) or in exports.

---

### Drag vault paths from explorer into a note

Source: `src/modules/editor/lib/pathDrop.ts` (+ test), `src/lib/dragTypes.ts`.

- [ ] Drag MIME type: `PATH_DRAG_TYPE = "application/x-fuchico-paths"`; payload = newline-separated absolute paths (empty entries dropped).
- [ ] Extension at `Prec.high` (beats image drop). `dragover`: when the drag types include the path MIME -> `preventDefault`, `dropEffect = "copy"`.
- [ ] `drop`: if paths present -> insert at `posAtCoords` (fallback cursor), links joined by `\n`; if the char before the insertion point is non-whitespace, prefix a single space; cursor after insert; editor focused.
- [ ] `linkForDroppedPath(path, currentPath)`: `.md` (case-insensitive) -> `[[<basename without .md>]]` (`/vault/sub/Inner Note.md` -> `[[Inner Note]]`); image (`IMAGE_EXT = /\.(png|jpe?g|gif|webp|svg|bmp)$/i`) -> `![<name without ext>](<relative path>)` (`/vault/attachments/pic.png` from `/vault/notes/today.md` -> `![pic](../attachments/pic.png)`); anything else -> `[<name without ext>](<relative path>)` (`/vault/notes/spec.pdf` -> `[spec](spec.pdf)`). `.markdown` files are treated as "other" (relative link).
- [ ] OS file drags (type `Files` without the path MIME) are not handled here (image files go to the image-attachment handler).

---

### Mermaid inline preview button

Source: `src/modules/editor/lib/mermaidPreviewExtension.ts`. (Diagram pane/window itself is documented in the mermaid section.)

- [ ] For each `FencedCode` node in the viewport whose `CodeInfo` text, trimmed and lowercased, equals `mermaid`, a widget is appended (side 1) at the end of the opening fence line: `<button class="cm-mermaid-preview-btn" title="Preview diagram">▶ Preview diagram</button>`. Shown on every line including the cursor line.
- [ ] Mousedown is `preventDefault`ed (keeps editor focus/cursor); click opens the block.
- [ ] Rebuilt on doc change, viewport change, or syntax tree change (the markdown language attaches asynchronously after mount).
- [ ] Open: block text = content between the opening fence line and the closing fence line (exclusive both); if the block is unterminated the "closing line" is the node's last line. A new `blockKey = crypto.randomUUID()` is generated per click (re-clicking opens a new tracked preview), tracked as `{ anchor: node.from, lastText }`, and `onOpen({ blockKey, text })` is called (App decides docked pane vs. window).
- [ ] On each doc change, tracked anchors are remapped through the changes (`mapPos`), and a debounced (`UPDATE_DEBOUNCE_MS = 400` ms) pass emits `mermaid:update` `{ blockKey, text }` for every tracked block whose text changed (`text` is `null` if the block no longer exists at the anchor).
- [ ] Listens to `mermaid:request` `{ blockKey }` -> emits `mermaid:update` with current text (forced). Listens to `mermaid:closed` `{ blockKey }` -> stops tracking. These are Tauri event-bus events (global `emit`/`listen`), shared between docked and windowed previews.
- [ ] Visuals: inline-block, `margin-left: 12px`, sans font 11px weight 600, background `color-mix(in oklch, var(--syntax-function) 14%, transparent)`, color `var(--syntax-function)`, border `1px solid` 40% mix, radius `var(--radius-sm)`, padding `1px 6px`; hover background 24% mix.

---

### Frontmatter (parse + editor styling)

Sources: `src/modules/frontmatter/lib/parseFrontmatter.ts`, `src/modules/frontmatter/lib/frontmatterExtension.ts`, `src/modules/frontmatter/index.ts`.

#### Parser `parseFrontmatter(text) -> { data, raw, bodyStart, endLine } | null`
- [ ] Returns null unless the text starts with `---` and line 1 trimmed equals `---` (so `----` or `--- x` fails; leading BOM/blank line fails).
- [ ] Closing fence: the first subsequent line whose trim is `---` or `...`; none -> null.
- [ ] `raw` = lines between fences with trailing `\r` removed, joined by `\n`. `endLine` = 1-based line of the closing fence. `bodyStart` = char offset of the line after the closing fence (or `text.length`), exact with CRLF.
- [ ] Line-oriented mini YAML: blank lines and lines starting with `#` (after trim) skipped; key line regex `/^([^:\s][^:]*):[ \t]?(.*)$/` (key cannot start with whitespace or `:`; key trimmed; indented lines are therefore ignored unless part of a block list).
- [ ] Empty value -> collects following lines (skipping blanks) whose trim starts with `- ` or equals `-` as list items (`-` alone -> `""` -> null); if any -> array, else value `null`.
- [ ] `[...]` value -> inline list, split on top-level commas (quotes `"`/`'` protect commas), each trimmed, unquoted, coerced; `[]` -> empty array.
- [ ] Otherwise scalar: strip matching surrounding `"…"` or `'…'` (no escape processing), then coerce: `""`/`null`/`~` -> null; `true`/`false` -> booleans; `/^-?\d+$/` -> int; `/^-?\d+\.\d+$/` -> float; else string. Quoted numbers are still coerced (`"42"` -> 42). Duplicate keys: last wins. Nested maps / block scalars (`|`, `>`) are not supported (value stays the raw string, e.g. `|`).
- [ ] Consumers: tag extraction (`frontmatterTags`), and others (export, templates; documented elsewhere).

#### Editor styling extension
- [ ] Parses only the first 20,000 chars of the document. If frontmatter found, every line `1..endLine` gets line class `cm-frontmatter-line`; lines 1 and `endLine` additionally `cm-frontmatter-fence`.
- [ ] On non-fence lines, `KEY_RE = /^([ \t]*)([^:\s][^:]*)(:)/` marks the key text (excluding indentation and colon) with `cm-frontmatter-key` (indented keys are also highlighted here, unlike the parser).
- [ ] Nothing is hidden or replaced; YAML stays fully editable. Rebuilt on doc or viewport change.
- [ ] Visuals: `.cm-frontmatter-line` background `color-mix(in oklch, var(--foreground) 4%, transparent)`; `.cm-frontmatter-fence` color `var(--muted-foreground)`; `.cm-frontmatter-key` color `var(--syntax-keyword)`, opacity 0.85.
- [ ] No dedicated frontmatter property-editing UI exists; editing is plain text.

---

### Backlinks pane

Sources: `src/modules/backlinks/BacklinksPane.tsx`, `src/modules/backlinks/lib/useBacklinks.ts` (+ test). Rust side: `links_scan` in `src-tauri/src/modules/links/mod.rs`.

#### Opening
- [ ] Toggled by palette command "Show Backlinks" / "Hide Backlinks" (`command.showBacklinks`/`command.hideBacklinks`; PT-BR "Mostrar backlinks"/"Ocultar backlinks"), via App's `toggleBacklinks` which sets the right-side dock to `{ kind: "backlinks" }` or closes it. Shares the right dock slot (and resizable width) with the mermaid pane -- opening one replaces the other.
- [ ] Target = active tab path, unless the active tab is a `.excalidraw` file (then null). Follows active tab changes.

#### Index
- [ ] `useLinkIndex(root)`: invokes `links_scan` `{ root }` -> `LinkRef[]` where `LinkRef = { path: string; line: number (1-based); target: string; kind: "wiki" | "markdown"; context: string }`. States: idle (no root), loading (only if not already loaded -- refreshes keep stale results visible), loaded, error (message = `String(e)`).
- [ ] Re-scans on events `fs:file-written` and `fs:changed`.
- [ ] Backend semantics (for parity): scans `.md`/`.markdown` files (same skip rules), skips fenced code (fence regex `^\s*(`{3,}|~{3,})` toggles on ANY fence line, unlike the tag scanner), removes inline code spans `` `[^`]*` `` before matching; wiki regex `\[\[([^\[\]]+?)\]\]` (target keeps `|alias`/`#heading`); markdown regex `(?:^|[^!])\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)` (images excluded), skipping `#anchors` and external schemes; `context` = trimmed raw line.

#### Resolution / grouping (`backlinksFor(refs, targetPath, vaultFiles)`)
- [ ] Ignores refs from the target note itself (self-links).
- [ ] wiki refs resolve with `resolveWikilinkTarget(target, vaultFiles)`; markdown refs with `resolveRelativeMarkdownLink(target, ref.path)`; keep those equal to `targetPath`.
- [ ] Grouped by source note; within a note, one entry per line (multiple links on the same line collapse, first context kept); groups sorted by path `localeCompare`; refs in scan (line) order.
- [ ] Test: refs `a.md:3 "b|alias"`, `a.md:3 "b#h"`, `c.md:1 "./sub/b.md"` (markdown), `c.md:2 "a"`, `sub/b.md:9 "b"` -> for target `/v/sub/b.md`: `[{ /v/a.md, [{3,"x"}] }, { /v/c.md, [{1,"y"}] }]`.

#### UI
- [ ] Header (reuses `.mermaid-pane-header` styles): `Link2` icon (14px), title "Backlinks" plus a count badge `.backlinks-count` showing total refs (only when target set and total > 0), close button (`X`, title "Close") -> closes dock.
- [ ] Body states: no target -> "Open a note to see which notes link to it."; loading -> "Scanning…"; error -> error message; no groups -> `No notes link to "<name>" yet.` where name = basename without `.md`/`.markdown`.
- [ ] Each group: file button (`FileText` icon 13px, basename without ext, tooltip full path) -> opens the note; under it one row per line: line number (`.backlinks-line`, mono 10px, opacity .7, min-width 2ch) + context (`.backlinks-context`, ellipsized single line); click -> `onOpenFile(path, line)` (opens and focuses that line).
- [ ] Right-click file or row -> context menu: "Open", separator, "Reveal in file manager", "Copy path" (`fileRowMenuItems`).
- [ ] CSS: pane flex column; `.backlinks-body` scroll, padding 6px 0, 12px font; file rows padding `4px 12px`, weight 600; line rows padding `3px 12px 3px 30px`, muted color; hover background `color-mix(in oklch, var(--accent) 60%, transparent)`, color `var(--accent-foreground)`; count badge pill (radius 999px, `var(--secondary)` bg, 10px/600).

---

### Tags view (main view "tags")

Sources: `src/modules/tags/TagsView.tsx`, `src/modules/tags/lib/useTagIndex.ts`. Rust: `tags_scan`.

- [ ] Entry points: activity-bar button with `Hash` icon (title `app.tags`), and clicking a `#tag` pill in the editor (pre-selects that tag).
- [ ] `useTagIndex(root)`: invokes `tags_scan` `{ root }` -> `TagEntry[] = { tag: string; count: number; files: string[] }[]`, backend-sorted by descending count then tag name; `count` = number of notes referencing the tag (deduped per file), `files` = canonical note paths. Same idle/loading(only-if-not-loaded)/loaded/error states. Re-scans on `fs:file-written` only (not `fs:changed`). Exposes `refresh()`. App holds one instance (also feeds `#` completion); TagsView mounts its own instance.
- [ ] No root -> centered "Open a folder to see tags".
- [ ] Layout: left column `.tags-list` (fixed 260px, wrapping chips, right border, scroll) and right column `.tags-files`.
- [ ] Left: "Scanning…" while loading; error in destructive color; when loaded & empty: "No tags yet. Use `#tag` in a note or a frontmatter `tags:` list." (with `<code>`). Each tag chip: `Hash` icon (11px), tag name, count (`.tags-chip-count`, muted 11px). Clicking toggles selection (click active chip -> deselect).
- [ ] Chip CSS: inline-flex, gap 4px, `1px solid var(--border)`, radius 999px, padding `3px 10px`, 12px; hover 5% foreground mix; active: background `color-mix(in oklch, var(--syntax-keyword) 18%, transparent)`, border 45% mix, color `var(--syntax-keyword)`, count inherits color at .75 opacity.
- [ ] Right: no selection (or selected tag no longer exists) -> "Select a tag to see its notes."; else one row per file: basename (with extension) on top and full path below (ellipsized, tooltip full path); click opens the file; right-click -> standard file menu (Open / Reveal / Copy path).
- [ ] `selectedTag`/`selectedTagToken` props: whenever either changes and `selectedTag` is truthy, the active tag is set to it.
- [ ] PT-BR strings: "Abra uma pasta para ver as tags", "Ainda não há tags. Use … em uma nota ou uma lista … no frontmatter.", "Selecione uma tag para ver suas notas.".

---

### Vault search and replace (main view "search")

Sources: `src/modules/search/SearchPanel.tsx`, `src/modules/search/lib/useSearch.ts`. Rust: `search_files`, `search_replace_files` in `src-tauri/src/modules/search/mod.rs`.

#### Entry
- [ ] Activity-bar button (`Search` icon, title `app.search`) and a palette command that sets main view `"search"`. There is no dedicated global keybinding in this module. Panel state (query, replacement, toggle) is component-local and is lost when switching to another main view.
- [ ] No root -> centered "Open a folder to search".

#### Options
- [ ] Plain, case-insensitive substring search only. **No regex, no match-case, no whole-word options.** Query is used verbatim (not trimmed for matching; only an all-whitespace query is treated as empty).

#### Search behaviour
- [ ] Input (`Search` icon 14px, placeholder "Search files…", borderless, 13px). Typing clears any notice.
- [ ] `useSearch(root, query, refreshToken)`: empty/whitespace query or no root -> idle. Otherwise sets loading immediately, then after `DEBOUNCE_MS = 200` invokes `search_files` `{ root, query }` -> `SearchMatch[] = { file: string; line: number (1-based); column: number (1-based char column); text: string (full line) }[]`. Older in-flight results are discarded. Bumping `refreshToken` re-runs the same query.
- [ ] Backend: walks ALL files (not just markdown) under root, skipping dot-entries and `node_modules`/`target`/`dist`/`.git`, sorted by path; skips non-UTF-8 files; for each line, only the FIRST occurrence per line yields a match (one result per line); cap `MAX_MATCHES = 500` total (first-found order).
- [ ] Status area: "Searching…" while loading; error message (destructive); "Type to search across files." when idle with empty query and no notice; "No matches." when loaded and zero.
- [ ] Results grouped by file in backend order. Group header button: basename (with extension) + right-aligned match count; tooltip full path; click -> open file at the group's first match line. Each match row: line text (mono, single line, ellipsized) with the FIRST case-insensitive occurrence of the query highlighted in `<mark class="search-match-highlight">` (background `color-mix(in oklch, var(--syntax-function) 35%, transparent)`, radius 2px), plus meta `line N` ("linha N"); click -> `onOpenMatch(file, line)` = App `openFile(path, line)` (jumps to line; column not used).
- [ ] Right-click group header or row -> context menu: "Open" (opens at that line), then an extra item: when replace row is closed "Replace in this file…" (opens the replace row); when open "Replace all in <file>" (disabled unless replace is possible) -> runs replace limited to that file; separator; "Reveal in file manager"; "Copy path".

#### Replace
- [ ] Toggle button (`Replace` icon, 24x24, title "Find and replace across the vault", `aria-pressed`) shows/hides a second row: input "Replace with…" and button "Replace all" with ` (<matchCount>)` suffix when matches > 0 (matchCount is the capped displayed count, max 500).
- [ ] Replace possible iff `query.trim()` non-empty, matchCount > 0, and not busy. Enter in the replacement input or clicking the button triggers it. Empty replacement is allowed (deletes occurrences).
- [ ] Confirmation via native `window.confirm`: `Replace every "<query>" with "<replacement>" <scope>? This cannot be undone.` where scope = `across N file(s)` (count of displayed result groups) or `in <basename>` for a single file. Cancel -> nothing.
- [ ] Invokes `search_replace_files` `{ root, query, replacement, files: string[] | null }` -> `{ files_changed: number, replacements: number }` (mapped to camelCase). `files` null = whole vault; otherwise canonical paths allow-list.
- [ ] Backend semantics: same file walk; replaces EVERY case-insensitive occurrence (all occurrences in all lines, not limited to the 500 displayed matches) with the replacement verbatim (no case preservation, no `$1` capture syntax); multibyte-safe char-wise fallback when lowercasing changes byte length (e.g. `İstanbul istanbul` / `istanbul` -> `İstanbul X`). Writes atomically, preserves file permissions, and emits `fs:file-written` `{ path, source: "replace" }` per changed file (open editors reload, indexes rescan).
- [ ] After success: notice `Replaced <N occurrence(s)> in <M file(s)>.` (PT-BR `{occurrences} substituída(s) em {files}.`) in a highlighted box (`.search-notice`, 18% syntax-function mix), then results re-run (refreshToken++). On failure: `Replace failed: <error>`. Busy disables the button during the operation.
- [ ] CSS specifics: input row padding `10px 16px`, bottom border; replace row padding `6px 16px 6px 38px` (aligned under input text); replace button border, 5% mix bg, 12px, disabled opacity .5; results padding `8px 12px`; group margin-bottom 6px; header 12px/600 padding `4px 8px`; rows under a group padding-left 16px; row hover 5% foreground mix.

#### In-editor find
- [ ] Separately, the editor keeps CodeMirror basicSetup's `searchKeymap: true` and `highlightSelectionMatches: true` (in-buffer find panel `Mod-f` etc., subject to the modal keymap), styled via `.cm-panels` (card bg). Documented with editor core.

---

### Tauri invokes and events used by this section (summary)

- [ ] `fs_list_markdown_files { root } -> string[]`
- [ ] `fs_read_file { path } -> { kind: "text", content, size } | { kind: "binary", size } | { kind: "toolarge", size, limit }` (heading completion)
- [ ] `fs_write_binary { path, dataBase64 } -> ()`; error string containing `already exists` on collision (image attachments)
- [ ] `links_scan { root } -> LinkRef[]`
- [ ] `tags_scan { root } -> TagEntry[]`
- [ ] `search_files { root, query } -> SearchMatch[]`
- [ ] `search_replace_files { root, query, replacement, files: string[] | null } -> { files_changed, replacements }`
- [ ] Events listened: `fs:changed` (vault files, link index), `fs:file-written` (link index, tag index), `mermaid:request {blockKey}`, `mermaid:closed {blockKey}`. Emitted: `mermaid:update {blockKey, text|null}`; backend emits `fs:file-written {path, source:"replace"}`.
- [ ] No localStorage keys are read or written by any file in this section (shortcut bindings for task commands live in editor settings, documented elsewhere).


---

## 4. Command Palette, Quick Switcher, Templates & Daily Notes, Agenda, Excalidraw, Mermaid Diagrams, Export

Sources: `src/modules/palette/**`, `src/modules/agenda/**`, `src-tauri/src/modules/tasks/mod.rs`, `src/modules/excalidraw/**`, `src/modules/mermaid/**`, `src/modules/editor/lib/mermaidPreviewExtension.ts`, `src/modules/export/**`, `src/main.tsx`, `src/App.tsx`, `src/styles/globals.css`, `src/lib/i18n/{en,ptBR}.ts`, `demo-notes/`.

Some of the current behavior is buggy or odd. Those items are marked **QUIRK**. Where the fix is obvious, the preferred port behavior is given next to it. The port must decide for each quirk whether to keep it bit-for-bit or fix it.

---

### 1. Global shortcuts that open the overlays (`src/modules/palette/lib/usePaletteShortcuts.ts`)

- [ ] A window-level `keydown` listener runs in the bubble phase on `window`. It is active in the main window only (`App.tsx`), not in secondary editor, mermaid or print windows.
- [ ] Trigger: `(e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "p"`. It calls `preventDefault()`. Alt is not checked, and on macOS both Cmd and Ctrl count.
  - With `Shift` it opens the **Command Palette** with an empty query (`Mod-Shift-p`, listed as a fixed shortcut "Command palette" / "Run any app command by name.").
  - Without `Shift` it opens the **Quick Switcher** (`Mod-p`, "Quick open file" / "Fuzzy-find a note in the vault.").
- [ ] CodeMirror does not bind these combos, so the event reaches `window` even while the editor has focus.
- [ ] Pressing `:` in the editor opens the palette pre-filled with `":"` (`src/modules/editor/EditorPane.tsx`). This happens only when all of these hold:
  - the key is `:` with no Meta, Ctrl or Alt;
  - `editorSettingsStore.get().keybindingMode === "helix"`;
  - the Helix mode is not `"insert"` (normal or select);
  - the event target is inside the CodeMirror `contentDOM`, so the `/` search input keeps its `:`.

  The listener is registered in the window **capture** phase with `preventDefault` + `stopPropagation`, so it wins over codemirror-helix's own `:` command line.
- [ ] The quick-switcher and command-palette states are independent booleans (`quickSwitcherOpen`, `commandPaletteOpen`), so both overlays can be open at the same time. The palette renders after the switcher, so it sits on top. Each one closes on its own.

### 2. Overlay chrome, shared by the palette and the switcher (CSS in `globals.css` ~L1873-1987)

- [ ] Backdrop `.palette-overlay-backdrop`: `position:fixed; inset:0; z-index:50`, horizontally centered, `padding-top:10%`, background `color-mix(black 25%, transparent)`. **mousedown** on the backdrop closes the overlay. mousedown inside the panel is stopped so it does not close.
- [ ] Panel `.palette-overlay`: width `min(560px, 90%)`, `max-height:60%`, column flex, `var(--card)` background, 1px border, `--radius-md`, shadow `0 12px 32px` black 30%.
- [ ] Input `.palette-overlay-input`: 13px font, padding `10px 12px`, bottom border, no outline. Auto-focused on mount. For the palette the caret goes to the end of any pre-filled text, so typing after `:` appends.
- [ ] List `.palette-overlay-list` scrolls with 4px padding. Rows `.palette-overlay-item` are 12px font, padding `6px 10px`, with a 13px icon (`strokeWidth 1.75`, muted colour) then the text (ellipsis). The active row `.palette-overlay-item-active` uses the accent background and accent foreground.
- [ ] Matched characters are wrapped in `<mark class="palette-overlay-highlight">`, background `color-mix(var(--syntax-number) 35%, transparent)`, radius 2px (`lib/HighlightedText.tsx`). Indices are code-point indices over `[...text]`.
- [ ] Empty or status message `.palette-overlay-empty`: centered, 12px, muted, padding 16px.
- [ ] Keyboard, handled on the input:
  - `Escape` closes.
  - `ArrowDown` / `ArrowUp` move the active index, clamped to `[0, len-1]` with no wrap.
  - `Enter` runs the active row.
  - Hovering a row with the mouse makes it active, and clicking runs it.

  The active index resets to 0 whenever the result list changes. No Tab, PageUp/PageDown, or Ctrl-N/P handling.
- [ ] The active row is not scrolled into view automatically. With many results, arrowing past the visible area does not scroll the list.

### 3. Fuzzy matcher (`src/modules/editor/lib/fuzzyMatch.ts`), used by both overlays

- [ ] An empty or whitespace query gives `{matched:true, score:0, indices:[]}`.
- [ ] Case-insensitive subsequence match: every query character must appear in order. The scan is greedy left to right, taking the first occurrence of each character.
- [ ] Scoring, per matched character:
  - +3 if it directly follows the previous match (consecutive), otherwise +1;
  - an extra +2 if it is at index 0 or the previous target character matches `/\W/` (word boundary).

  Final score = `score - target.length * 0.01`, so shorter targets win ties.
- [ ] Not matched gives `{matched:false, score:0, indices:[]}`.

### 4. Command Palette (`src/modules/palette/CommandPalette.tsx`)

- [ ] Placeholder: `palette.runCommand` (en "Run a command…", pt-BR "Executar um comando…"). Empty state: `palette.noMatchingCommands` (en "No matching commands", pt-BR "Nenhum comando corresponde").
- [ ] Row layout:
  1. a `Command` icon;
  2. the title, with highlight;
  3. an optional alias chip `.palette-overlay-alias` (monospace 10.5px, muted, `margin-left:auto`);
  4. an optional binding chip `<kbd class="palette-overlay-keys">` (10.5px, 1px border, light muted background; inherits colour when the row is active). When both chips are present, the key chip loses its auto margin.
- [ ] The binding is rendered by `formatBinding()` (`src/modules/settings/lib/fixedShortcuts.ts`):
  - It parses the prefixes `Mod|Meta|Ctrl|Alt|Shift` from strings like `"Mod-Shift-p"`.
  - A single-character key is upper-cased.
  - On macOS, symbols are joined with no separator (`Mod`→`⌘`, `Ctrl`→`⌃`, `Alt`→`⌥`, `Shift`→`⇧`, e.g. `⌘⇧P`). On other platforms it is `Ctrl+Shift+P` (`Mod`→`Ctrl`, `Meta`→`Win`).
  - `" / "` separates alternatives. A string with no modifier passes through unchanged.
- [ ] Running a command calls `onClose()` first, then `command.run()`.
- [ ] Results are recomputed when `commands`, `query`, `onGoToLine` or `locale` change. The registry is rebuilt on locale change, so titles are re-translated.

#### 4.1 Ranking, normal query (no leading `:`)

- [ ] The query is trimmed. If empty, every command is listed in registry order with no highlight.
- [ ] Otherwise, for each command:
  - If `fuzzyMatch(q, title)` matches, the command is included with score + 1 and title highlight indices.
  - Otherwise it is included with score 0 and no highlight if any **keyword** fuzzy-matches. The keywords are the command's `keywords`, plus (when a `binding` exists) `binding.replace(/-/g," ")` (e.g. `"Mod Shift p"`) and `formatBinding(binding)` (e.g. `"⌘S"`).
- [ ] Results are sorted by score, descending. JS sort is stable, so ties keep registry order.

#### 4.2 Ranking, `:` query (Helix/vim command line)

- [ ] If the trimmed query matches `^:(\d+)$` and `onGoToLine` is provided (always, in the main app), a synthetic row comes first:
  - id `go-to-line`, title `command.goToLine` with `{n}` (en "Go to Line {n}", pt-BR "Ir para a linha {n}"), score 1000;
  - it runs `editorRef.current?.goToLine(line)`. This is a no-op when no text editor is active.
- [ ] For each command, the first alias where `alias.startsWith(q)` is used. The comparison is case-sensitive and includes the colon. Score 200 if `alias === q`, otherwise 100. The chip shows that alias, with no highlight.
- [ ] Otherwise, if `body = q.slice(1)` is non-empty and `fuzzyMatch(body, title)` matches, the command is included with the fuzzy score and highlight. The chip shows `aliases[0]` if the command has any.
- [ ] A bare `":"` lists every command that has an alias, at score 100, with the first alias as the chip.
- [ ] Sort: by score, descending.

#### 4.3 Command registry (`src/modules/palette/lib/commands.ts`, `buildAppCommands(ctx)`), in exact order

The translation function is `t()` (`src/lib/i18n`). `{name}` placeholders are interpolated. The `CommandContext` comes from `App.tsx`.

| # | id | en title | pt-BR title | Condition | Keywords | Binding hint | Aliases | Action |
|---|----|----------|-------------|-----------|----------|--------------|---------|--------|
| 1 | `go-to-editor` | Go to Editor | Ir para o editor | always | files, explorer, view | – | – | `setMainView("editor")` |
| 2 | `go-to-agenda` | Go to Tasks & Calendar | Ir para tarefas e calendário | always | agenda, tasks, calendar, view | – | – | `setMainView("agenda")` |
| 3 | `go-to-search` | Go to Search | Ir para a busca | always | find, view | – | – | `setMainView("search")` |
| 4 | `go-to-settings` | Go to Settings | Ir para as configurações | always | preferences, config, view | – | – | `setMainView("settings")` |
| 5 | `editor:<action>` ×9 | `Editor: {label}` | `Editor: {label}` | `hasActiveTab` | task, insert, editor, + translated description | the **current user binding** from `editorSettingsStore.get().shortcuts[action]` | – | `editorRef.current?.runAction(action)` |
| 6 | `settings:<id>` ×6 | `Settings: {section}` | `Configurações: {section}` | always | preferences, config + section keywords | – | – | `settingsNav.set(id)` then `setMainView("settings")` |
| 7 | `open-folder` | Open Folder… | Abrir pasta… | always | vault, workspace, directory | – | – | native folder picker (`handleOpenFolder`) |
| 8 | `quick-open-file` | Quick Open: Go to File… | Abertura rápida: ir para o arquivo… | `hasRootPath` | switcher, find file, open file | `Mod-p` | `:o` `:open` `:e` `:edit` | open the Quick Switcher |
| 9 | `save-file` | Save File | Salvar arquivo | `hasActiveTab` | write | `Mod-s` | `:w` `:write` | `editorRef.current?.save()` |
| 10 | `save-and-close` | Save and Close Tab | Salvar e fechar aba | `hasActiveTab` | write quit | – | `:wq` `:x` | save, then close the active tab (the close is not awaited after the save) |
| 11 | `close-active-tab` | Close Active Tab | Fechar a aba ativa | `hasActiveTab` | close file | – | `:q` `:quit` `:bc` | `closeTab(activePath)` |
| 12 | `close-all-tabs` | Close All Tabs | Fechar todas as abas | `tabs.length > 0` | close everything | – | `:qa` | `closeAll()` |
| 13 | `open-daily-note` | Open Today's Daily Note | Abrir a nota diária de hoje | `rootPath` set | journal, today, diary, new note | – | – | see §6 |
| 14 | `new-from-template:<absPath>` ×N | `New Note from Template: {stem}` | `Nova nota a partir do modelo: {stem}` | `rootPath` set, one per template | template, create, new note | – | – | see §6 |
| 15 | `keyboard-shortcuts` | Keyboard Shortcuts | Atalhos de teclado | always | keys, hotkeys, bindings, help | – | – | `openSettings("shortcuts")` |
| 16 | `open-in-new-window` | Open in New Window | Abrir em nova janela | `hasActiveTab` | detach, window, popout, second monitor | – | – | `openInWindow(activePath)` (detached editor window) |
| 17 | `toggle-backlinks` | Show Backlinks / Hide Backlinks (depends on whether the backlinks dock is open) | Mostrar backlinks / Ocultar backlinks | `hasRootPath` | links, references, linked mentions, graph | – | `:backlinks` `:bl` | toggle the right dock between backlinks and closed. This replaces an open mermaid dock. |
| 18 | `export-html` | Export as HTML… | Exportar como HTML… | `activePath` matches `/\.(md\|markdown)$/i` | export, html, web page, share | – | `:export` | `exportAsHtml(path, getActiveContent())`, skipped if the content is null (§11) |
| 19 | `export-pdf` | Export as PDF (Print)… | Exportar como PDF (imprimir)… | same as 18 | export, pdf, print, share | – | `:pdf` `:print` | `exportAsPdf(path, content)` (§12) |
| 20 | `toggle-theme` | Toggle Theme (Light / Dark) | Alternar tema (claro / escuro) | always | dark mode, light mode, appearance, color scheme | – | `:theme` | `mode === "dark" ? "light" : "dark"`. **QUIRK:** `"system"` (the default) goes to `"dark"`. |
| 21 | `select-theme:ayu` / `:dracula` / `:catppuccin` | `Theme: Ayu` / `Theme: Dracula` / `Theme: Catppuccin` | `Tema: …` | always | palette, appearance, color scheme | – | – | `editorSettingsStore.set({palette})` |
| 22 | `select-theme:<customId>` ×N | `Theme: {custom name}` | `Tema: {name}` | one per `customThemes` entry | palette, appearance, custom theme | – | – | `set({palette:"custom", customThemeId})` |

Row 5 expands in `SHORTCUT_ACTIONS` order (`src/modules/settings/lib/shortcutActions.ts`). The binding hints are the defaults from `editorSettings.ts`:

| action | en label | pt-BR label | default binding |
|---|---|---|---|
| `openOutline` | Go to header | Ir para o cabeçalho | `Mod-o` |
| `toggleCheckboxAtCursor` | Toggle checkbox | Alternar checkbox | `Mod-Enter` |
| `insertDate` | Insert date | Inserir data | `Mod-Shift-d` |
| `insertDateTime` | Insert date & time | Inserir data e hora | `Mod-Alt-d` |
| `insertRegion` | Insert fold region | Inserir região dobrável | `Mod-Shift-r` |
| `insertTable` | Insert table | Inserir tabela | `Mod-Alt-t` |
| `toggleTaskLine` | Toggle task | Alternar tarefa | `Mod-Shift-Enter` |
| `pickDueDate` | Set due date… | Definir prazo… | `Mod-Shift-.` |
| `pickRecurrence` | Set recurrence… | Definir recorrência… | `Mod-Shift-,` |

The English descriptions, also matched as keywords, are:
- "Open the document outline (fuzzy-searchable header list)."
- "Mark/unmark the checkbox on the cursor's line."
- "Insert today's date at the cursor."
- "Insert the current date and time at the cursor."
- "Wrap the selected lines in a foldable region (or insert an empty one at the cursor)."
- "Insert a 2x2 Markdown table at the cursor and start editing the header."
- "Turn the current line into a `- [ ]` task (or back into plain text)."
- "Add or change the 📅 due date on the current line via a quick-pick list."
- "Add or change the 🔁 repeat rule on the current line via a quick-pick list."

Row 6 expands in `SETTINGS_SECTIONS` order (`src/modules/settings/lib/settingsNav.ts`). Extra keywords are in brackets:

| id | en | pt-BR | keywords |
|---|---|---|---|
| `appearance` | Appearance | Aparência | theme, font, zoom, language, idioma |
| `editor` | Editor | Editor | keybindings, helix, vim, folding, behavior |
| `shortcuts` | Keyboard Shortcuts | Atalhos de teclado | keys, hotkeys |
| `vault` | Vault | Cofre | folders, daily, templates, external tool |
| `integrations` | Integrations | Integrações | caldav, sync |
| `about` | About | Sobre | version |

- [ ] The selected settings section is persisted in **localStorage `helix.settingsSection`**. The default is `"appearance"`, and an invalid stored value falls back to it.
- [ ] `hasActiveTab` is true for **any** open tab, including `.excalidraw` tabs. For a drawing tab, the "Editor: …", "Save File", `:w` and go-to-line commands are listed but are no-ops, because `editorRef` is null. **QUIRK.**
- [ ] The theme mode and palette live in **localStorage `helix.editorSettings`** (JSON). The relevant fields are `mode` (`"system"|"light"|"dark"`, default `"system"`), `palette`, `customThemeId`, `customThemes`, `shortcuts`, `dailyNotesFolder` and `templatesFolder`.

### 5. Quick Switcher (`src/modules/palette/QuickSwitcher.tsx`, `lib/useVaultFiles.ts`)

- [ ] Placeholder `palette.goToFile`: en "Go to file…", pt-BR "Ir para o arquivo…".
- [ ] Status messages, shown in the list area:
  - no root: `palette.openFolderFirst` ("Open a folder first" / "Abra uma pasta primeiro");
  - loading: `palette.scanningVault` ("Scanning vault…" / "Escaneando o cofre…");
  - error: `palette.cantReadVault` ("Couldn't read vault: {message}" / "Não foi possível ler o cofre: {message}");
  - no results: `palette.noFilesFound` ("No files found" / "Nenhum arquivo encontrado").
- [ ] **File source.** It recursively walks the vault with Tauri `invoke("fs_read_dir", { path, showHidden: false })`, which returns `DirEntry[] = {name, kind: "file"|"dir"|"symlink", size, mtime, …}`.
  - Entries with `kind === "dir"` are recursed into, with all subdirectories in parallel via `Promise.all`.
  - `kind === "file"` entries are collected.
  - Symlinks are skipped to avoid cycles. Hidden entries are excluded by the backend.
  - A directory that fails to read is silently skipped.
  - **ALL file types are listed** (md, excalidraw, images, pdf…), not just Markdown.
- [ ] Order with an empty query: the walk's collection order. Files of a directory come in `fs_read_dir` order, and subdirectory contents follow in completion order. Capped at **`MAX_RESULTS = 200`**.
- [ ] Display text is the path relative to the root (`relativeTo`): it strips `root + "/"`. If the path equals the root, the basename is shown. A non-matching prefix keeps the full path. Icon: `FileText`.
- [ ] With a query, it fuzzy-matches against the **relative path**, including folder names, and sorts by score descending. Capped at 200.
- [ ] Enter or click calls `onOpenFile(absPath)`, then closes. This opens or focuses a tab in the main window.
- [ ] **No "create note when no match"**: pressing Enter with zero results does nothing. There are no modifiers (no Cmd+Enter or Shift+Enter variants, no open-in-split).
- [ ] Caching: the hook caches per root in a `useRef`. Because `QuickSwitcher` unmounts when closed, the cache is effectively discarded, so **the vault is re-walked on every open**. **QUIRK.** A port may cache, but must still pick up new files.
- [ ] The palette's template list uses a different source: `vaultFiles` from `src/modules/editor/lib/useVaultFiles.ts`.
  - It calls `invoke("fs_list_markdown_files", { root })`, which returns `string[]` of absolute Markdown paths.
  - It refetches on every Tauri event `"fs:changed"`.
  - On error it sets `[]`.

### 6. Templates and daily notes (`src/modules/palette/lib/templates.ts`, test `templates.test.ts`; settings UI `src/modules/settings/VaultSection.tsx`)

- [ ] Settings in `helix.editorSettings`:
  - `dailyNotesFolder`, default **`"daily"**, label "Daily notes folder" / pt "Pasta de notas diárias" (key `settings.vault.dailyNotesFolder`);
  - `templatesFolder`, default **`"templates"`**, label "Templates folder".

  Both are free-text inputs that save on every keystroke. Hint: "A `daily.md` here seeds new daily notes." (`settings.vault.templatesHintPrefix` / `Suffix`). Leading and trailing slashes are stripped with `/^\/+|\/+$/g` wherever the settings are used.
- [ ] **`formatDate(d, fmt = "YYYY-MM-DD")`** uses **local** time. The tokens are replaced in this order, globally: `YYYY` (full year), `MM` (month 01-12), `DD` (day), `HH` (24h hour), `mm` (minutes), `ss` (seconds). Each is zero-padded to 2 digits, except the year. Other text is literal. There is no escaping, so a literal "MM" in the format is always replaced.
- [ ] **`expandTemplate(text, {title, now?})`** replaces, in this order:
  1. `/\{\{\s*date:([^}]+?)\s*\}\}/g` with `formatDate(now, fmt.trim())`, e.g. `{{date:DD/MM/YYYY HH:mm}}` gives `27/08/2026 09:05`;
  2. `/\{\{\s*date\s*\}\}/g` with `YYYY-MM-DD`;
  3. `/\{\{\s*time\s*\}\}/g` with `HH:mm`;
  4. `/\{\{\s*title\s*\}\}/g` with the title.

  Inner whitespace is allowed (`{{ time }}`). Unknown tokens such as `{{unknown}}` are left verbatim.
  - Test vector: for `new Date(2026,7,27,9,5,7)`, the template `"# {{title}}\n{{date}} {{ time }} {{date:YYYY}} {{unknown}}"` with title `T` expands to `"# T\n2026-08-27 09:05 2026 {{unknown}}"`.
  - **QUIRK:** the title goes through `String.replace` as a replacement string, so `$&`, `$1` and similar in a title would be interpreted.
- [ ] **`listTemplates(root, templatesFolder, vaultFiles)`**:
  - The prefix is `${root}/${folderStripped}/`.
  - It keeps files that start with the prefix, end with `.md` (case-sensitive; `.markdown` and `.MD` are excluded), and whose basename is not `"daily.md"` (constant `DAILY_TEMPLATE`).
  - Nested subfolders are included.
  - The list is sorted by string order of the absolute path.
  - Test: from `["/v/templates/daily.md","/v/templates/meeting.md","/v/templates/notes.txt","/v/notes.md","/v/templatesX/a.md"]` with folder `"/templates/"`, the result is `["/v/templates/meeting.md"]`.
- [ ] `stem(path)` is the basename with its last extension removed (`/\.[^.]+$/`). It is the display name in "New Note from Template: {stem}".
- [ ] **Open Today's Daily Note** (`openDailyNote`):
  1. `title = formatDate(new Date())`, i.e. `YYYY-MM-DD` in local time.
  2. `path = ${root}/${dailyFolder}/${title}.md`. **Edge:** an empty folder setting produces `root//YYYY-MM-DD.md` (double slash).
  3. If the path exists (`invoke("fs_stat", {path})` succeeds), it just opens the note.
  4. Otherwise it reads the template `${root}/${templatesFolder}/daily.md` via `invoke("fs_read_file", {path})`. That returns `{kind:"text",content,size} | {kind:"binary",size} | {kind:"toolarge",size,limit}`, and only `text` is used.
  5. Content = `expandTemplate(template ?? "# {{title}}\n\n", {title})`. With no template, the note is `# 2026-10-18\n\n`.
  6. It creates the note (see the next item), then opens it in a tab.
- [ ] Note creation (`createNote`):
  1. `invoke("fs_create_dir", {path: parentDir})`. An error containing `"already exists"` is ignored.
  2. `invoke("fs_write_file", {path, content, source: "template"})`.
  3. `notifyFsChanged(parentDir)`. This dispatches the DOM `CustomEvent("helix:fs-changed", {detail: dir})` and emits the Tauri event `"fs:changed"` with `{dir, from: currentWindowLabel()}`, so the explorer refreshes in every window.

  Errors are not caught (an unhandled promise means nothing is shown to the user).
- [ ] **New Note from Template** (`createFromTemplate(root, templatePath)`):
  - It reads the template text. On failure it uses `""`.
  - `title = "${stem(template)} ${formatDate(now, "YYYY-MM-DD HHmm")}"`, e.g. `meeting 2026-10-18 1432`.
  - The note goes in the **vault root**: `${root}/${title}.md`. On collision it tries `"${title} 2.md"`, `"${title} 3.md"`, … (existence checked with `fs_stat`).
  - Content = `expandTemplate(template, {title})`, so `{{title}}` becomes the generated title. The new note opens.
  - The user is expected to rename it afterwards (F2).
- [ ] There is no "insert template into current note" command. Templates only create new files.
- [ ] Demo: `demo-notes/templates/first_template.md` contains plain Markdown with no variables: `# This is a template` / `## First section` / `  - Hello` / `  - World` / `## Second section`. It yields the command "New Note from Template: first_template". The demo has no `daily.md`.

### 7. Agenda: task syntax (backend `src-tauri/src/modules/tasks/mod.rs`)

- [ ] Tauri `invoke("tasks_scan", { root })` returns `AgendaItem[]`, with JSON fields:
  - `kind: "task"|"todo"|"event"`
  - `checked: bool`
  - `text: string`
  - `date: "YYYY-MM-DD"|null`
  - `time: "HH:MM"|null`
  - `file`: canonical absolute path via `to_canon`
  - `line`: 1-based
  - `recurrence: string|null`
  - `recurTime: "HH:MM"|null`
- [ ] File walk:
  - recursive `read_dir`;
  - skips any entry whose name starts with `.`;
  - skips directories named `node_modules`, `target`, `dist`, `.git`;
  - follows only real dirs and files (symlinks are skipped, because `file_type()` is a symlink);
  - extensions `md` / `markdown`, case-insensitive;
  - no sorting: filesystem `read_dir` order. Unreadable or non-UTF-8 files are skipped.
- [ ] Each line is processed with `trim()`, so indentation is ignored. Empty lines are skipped. There is **no awareness of code fences or frontmatter**: tasks inside ``` blocks are scanned too.
- [ ] Rule 1, **Event**: the trimmed line starts with `📅` and the date regex `📅\s*(\d{4}-\d{2}-\d{2})(?:\s+(\d{2}:\d{2}))?` matches anywhere in the line.
  - `text` = the rest of the line after the match end, trimmed.
  - `checked:false`, no recurrence (never parsed for events).
  - If the line starts with 📅 but has no valid date, it falls through to rules 2 and 3.
- [ ] Rule 2, **Task**: `^[-*+]\s\[([ xX])\]\s*(.*)$` on the trimmed line.
  - The bullet must be `-`, `*` or `+` followed by exactly one whitespace character.
  - The box must be `[ ]`, `[x]` or `[X]`. Ordered lists (`1. [ ]`) and `> - [ ]` are not tasks.
  - `checked` = x or X.
  - `text` = everything after `]` plus optional spaces. It is **raw**, so it still contains the `📅 …` / `🔁 …` markers.
  - The date and time come from the first date-regex match anywhere in the text.
  - The recurrence comes from `(?i)🔁\s*(daily|weekdays|weekends|[a-z]{3}(?:,[a-z]{3})*)(?:\s+(\d{2}:\d{2}))?`, **lower-cased**.
- [ ] Rule 3, **Todo**: `(?i)^TODO:?\s+(.+)$`. The keyword is case-insensitive and the colon is optional, so `todo buy milk` also matches. `text` = the captured rest. Date, time and recurrence are extracted the same way. `checked:false`.
- [ ] Edge cases:
  - Dates are not validated (`2026-13-45` is accepted).
  - The time needs whitespace between the date and `HH:MM`.
  - The 3-letter recurrence branch accepts any letters: `🔁 weekly` gives `"wee"`, and `🔁 foo` gives `"foo"`.
  - Only the first 📅 and first 🔁 per line are used.
- [ ] Test vectors (Rust tests):
  - `"- [ ] one 📅 2026-07-20\n- [x] two\nTODO: three\n📅 2026-07-21 10:00 four\nplain text\n"` gives 4 items: task(date 2026-07-20, unchecked), task(checked), todo(text "three"), event(date 2026-07-21, time 10:00, text "four").
  - `"- [ ] give dog food 🔁 daily 08:00"` gives recurrence `daily` with recurTime `08:00`.
  - `"🔁 mon,wed,fri"` gives `mon,wed,fri` with no time.
  - `"TODO: standup 🔁 weekdays"` gives `weekdays`.
- [ ] `invoke("tasks_toggle", { path, line })` returns `()`.
  - It re-reads the file and uses `content.lines()`.
  - Errors: `"line out of range"` if `line == 0 || line > count`, and `"line is not a checkbox"` unless the raw line matches `^(\s*[-*+]\s\[)([ xX])(\]\s*.*)$`. Indentation is preserved.
  - It flips `x`/`X` to `" "` and `" "` to `"x"`.
  - It rejoins the lines with `\n` and keeps the trailing newline if one was present. **QUIRK:** CRLF files become LF.
  - The write is atomic: a `NamedTempFile` in the same dir, `sync_all`, then `persist`.
  - It does **not** emit `fs:file-written`, so CalDAV push is not triggered by agenda toggles and open editor buffers of that file are not told to reload.
- [ ] Demo syntax is in `demo-notes/project-launch.md` and `demo-notes/personal.md`. Examples:
  - `- [ ] Post standup notes 🔁 weekdays 09:00`
  - `- [ ] Water the office plants 🔁 mon,thu`
  - `TODO: rotate the staging API keys 📅 2026-07-18`
  - `📅 2026-07-16 10:00 Launch readiness standup`
  - `📅 2026-09-06` (an event with empty text, shown as "(empty)")

### 8. Agenda: data hook (`src/modules/agenda/lib/useAgenda.ts`)

- [ ] State: `idle | loading | loaded{items} | error{message}`. With no root the state is `idle`.
- [ ] It scans on mount and on root change. A rescan keeps the previous `loaded` list visible: "loading" appears only on the first scan.
- [ ] It re-scans the **whole vault on every Tauri `"fs:file-written"` event**, from any source (editor save, template, export, search replace, caldav). There is no debounce.
- [ ] `toggle(item)` acts only when `kind === "task"`. It calls `tasks_toggle(file, line)` and then rescans. On failure it only logs `console.error("tasks_toggle failed:", e)` and the UI does not revert. Because the checkbox is controlled by `item.checked`, it stays in its old state until the rescan.
- [ ] A `refresh()` function is exposed but not used by the UI.

### 9. Agenda: view (`src/modules/agenda/AgendaView.tsx`, CSS `globals.css` ~L1097-1372)

- [ ] It is a main view (`mainView === "agenda"`), reached with the palette command "Go to Tasks & Calendar" (and the activity bar, documented elsewhere). With no root it shows a centered `agenda.openFolder` ("Open a folder to see tasks and events" / "Abra uma pasta para ver tarefas e eventos").
- [ ] Layout `.agenda-view`: a flex row.
  - Left: `.agenda-calendar`, a 280px fixed column with 16px padding, a right border and its own vertical scroll.
  - Right: `.agenda-list`, flex 1, with padding `16px 20px` and its own scroll.
- [ ] **"Today"** = `new Date().toISOString().slice(0,10)`, which is the **UTC** date. Calendar cells, however, use local dates. **QUIRK:** near midnight in non-UTC timezones, "today", overdue and upcoming are off by one day. The preferred port behavior is a local date. Today is evaluated on each render; there is no midnight timer.
- [ ] **Month calendar**:
  - The header has a ChevronLeft button, the title, and a ChevronRight button. The 24×24 buttons step the month, wrapping the year. There is no "today" button and no keyboard navigation.
  - The title is `Intl.DateTimeFormat(locale, {month:"long", year:"numeric"})`, e.g. "October 2026" or "outubro de 2026". It starts at the current local month.
  - The grid has 7 columns with a 2px gap. Weekday headers come from `agenda.weekdayLetters` split on `,`: en `S,M,T,W,T,F,S`, pt-BR `D,S,T,Q,Q,S,S`. **Sunday first.** Headers are 10px, bold, muted.
  - Leading and trailing empty cells pad the grid to whole weeks. Cells are 30px high.
  - A day cell is a button showing the day number. `.agenda-calendar-cell-today` has a `--ring` border and weight 700. `.agenda-calendar-cell-selected` has the accent background.
  - If **any** item, including checked tasks and events, has that date, a 4px dot (`--syntax-function`) appears at the bottom. The dot uses the accent foreground when the cell is selected.
  - Clicking a day toggles `selectedDate` (clicking it again clears it). While a date is selected, a full-width button shows `agenda.clearFilter` ("Clear filter ({date})" / "Limpar filtro ({date})", with the ISO date). Clicking it clears the selection.
  - The selection and the month cursor are not persisted.
- [ ] **Routines panel** (below the calendar, top border):
  - Title `agenda.routines` ("Routines" / "Rotinas").
  - It lists **all** items with a `recurrence`, ignoring the date filter, in scan order.
  - Each row is a button with two lines: the text (12px, ellipsis, no "(empty)" fallback) and `🔁 {recurrence}[ {recurTime}]` (10px, muted).
  - Click opens the item. Right-click opens the context menu.
  - Empty state: "No routines yet. Add `🔁 daily|weekdays|weekends|mon,…` to a task." (pt: "Ainda não há rotinas. Adicione … a uma tarefa.").
- [ ] **List sections**, in order, each hidden when empty. A section title is 11px, bold, uppercase, letter-spacing .04em, muted. Let `F` = the items on the selected date if one is selected, otherwise all items.
  1. **Overdue** (`agenda.overdue`, "Overdue" / "Atrasadas"): `F` where `!checked && date && date < today`.
  2. **Today** (`agenda.today`, "Today" / "Hoje"): `F` where `date === (selectedDate ?? today)`. This includes checked items, todos and events. When a date is selected this section shows that date's items, still titled "Today".
  3. **Upcoming** (`agenda.upcoming`, "Upcoming" / "Próximas"): `F` where `date && date > today`. Checked items are included.
  4. **No date** (`agenda.noDate`, "No date" / "Sem data"): `F` where `!date && !recurrence`. Undated recurring items appear only under Routines.
- [ ] Rules that follow from the filters:
  - With a past date selected, unchecked items appear in both Overdue and Today. With a future date selected, items appear in both Today and Upcoming. This duplication is current behavior.
  - Items within a section are in raw **scan order** and **not sorted by date or time**.
  - Dated recurring items appear in the dated sections and in Routines.
  - Checked items whose date is before today disappear (they are not in Overdue and not in Today).
- [ ] When items exist but all four sections are empty: `agenda.nothingHere` ("Nothing here." / "Nada por aqui.").
- [ ] When the vault has no items at all: "No tasks yet. Use `- [ ] …`, `TODO: …`, or `📅 YYYY-MM-DD …` in your notes." (pt: "Ainda não há tarefas. Use … ou … nas suas notas.").
- [ ] While scanning: `common.scanning` ("Scanning…" / "Escaneando…"). On error the raw message is shown in the destructive colour.
- [ ] **Row** `.agenda-row`: 13px, padding `5px 6px`, hover tint, `role=button`, `tabIndex=0`.
  - Leading control:
    - a task gets a native checkbox (14px, `accent-color: var(--primary)`). Clicking it toggles via `tasks_toggle` and does not open the file (`stopPropagation`);
    - an event gets a `CalendarClock` icon;
    - a todo gets a `ListTodo` icon.
  - The text is the raw `item.text`, or `agenda.emptyItem` ("(empty)" / "(vazio)") if empty. Ellipsis. Checked tasks are struck through and muted (`.agenda-row-done`).
  - An optional time chip (mono, 11px) shows `item.time`.
  - An optional recurrence pill `🔁 {recurrence}[ {recurTime}]` (10px, secondary background) has the tooltip `agenda.recurringTask` ("Recurring task" / "Tarefa recorrente").
  - The file basename (11px, muted, max-width 140px, ellipsis) has the full path as its tooltip.
  - Click, `Enter` or `Space` calls `openFile(item.file, item.line)`: it opens or focuses the tab and moves the cursor to the 1-based line.
- [ ] Right-click context menu (`src/lib/fileRowMenu.ts`) on rows and routine rows:
  - "Open" / "Abrir";
  - a separator;
  - "Reveal in file manager" / "Mostrar no gerenciador de arquivos";
  - "Copy path" / "Copiar caminho".
- [ ] Recurrence is **display-only**: no occurrences are expanded onto calendar days, there is no next-due calculation, and checking a recurring task just marks it `[x]` with no auto-reset.
- [ ] CalDAV interplay visible here: none in the agenda UI itself. CalDAV sync (configured in Settings › Integrations) writes notes and emits `fs:file-written` with `{path, source:"caldav"}` per touched file. That rescans the agenda, so synced tasks show up automatically. Separately, editor saves trigger CalDAV push through the backend listener on `fs:file-written`.

### 10. Excalidraw drawings (`src/modules/excalidraw/**`)

- [ ] Any tab whose path ends with `.excalidraw` (case-insensitive) renders `ExcalidrawPane` instead of the text editor. The pane is lazy-loaded and keyed by path. The Suspense fallback is `common.loading` ("Loading…" / "Carregando…").
  - The pane's own loading text is the hard-coded English "Loading…" in `.editor-status`.
  - An error is shown in `.editor-status.editor-status-error`.
  - Container `.excalidraw-pane`: `height:100%; min-height:0`.
- [ ] **Creating a drawing**:
  - The explorer header has a "New drawing" button (`PenTool` icon; en "New drawing", pt "Novo desenho").
  - Folder context menus have a "New drawing" item.
  - Both start an inline name input with the placeholder "New drawing" and `defaultExt: "excalidraw"`. If the entered name has no `.`, `.excalidraw` is appended.
  - The file is created **empty** via `invoke("fs_create_file", {path})`.
  - In the tree, `.excalidraw` files show a `PenTool` icon.
- [ ] **Load** (`useExcalidrawDocument`):
  1. `invoke("fs_read_file", {path})`. A result that is not `text` gives the error **"Not a valid drawing file"**.
  2. Trimmed content: empty becomes `{}`, otherwise `JSON.parse`. A parse or restore exception gives **"Malformed drawing file"**. An invoke rejection shows `String(e)`.
  3. `appState.collaborators` is deleted before `restore()`, because a persisted `{}` crashes the renderer, which expects a Map.
  4. `restore({elements: parsed.elements ?? [], appState, files: parsed.files ?? {}}, null, null)` fills in defaults and migrates old element versions.
  5. Baseline `savedRef = serializeAsJSON(elements, appState, files, "local")`.
- [ ] **File format written** (Excalidraw `serializeAsJSON(..., "local")`): pretty-printed JSON (2-space indent):
  ```
  {"type":"excalidraw","version":2,"source":<window.location.origin>,"elements":[…],"appState":{<persistable subset, e.g. gridSize, viewBackgroundColor>},"files":{<fileId>:{mimeType,id,dataURL,created,…}}}
  ```
  Embedded images are stored inline as data URLs in `files`. Demo files show `source: "http://localhost:1423"` (dev origin) and an older hand-rolled `source: "helix-editor"` with a full appState that includes `collaborators: {}`. Both must load. The port must read and write the standard Excalidraw v2 JSON schema so files stay compatible with excalidraw.com and Obsidian Excalidraw.
- [ ] **Autosave**:
  - Every Excalidraw `onChange` (fired for strokes and also for selection, pan or zoom) immediately marks the tab dirty with `setDirty(path, true)`, which shows the unsaved dot on the tab.
  - It (re)starts a **1000 ms** debounce (`AUTOSAVE_DELAY_MS`).
  - The flush serializes the current scene. If it equals `savedRef`, nothing is written. **QUIRK:** dirty is then never cleared, so a pan-only change leaves the dot set.
  - Otherwise it calls `invoke("fs_write_file", {path, content, source:"editor"})`, updates the baseline and clears dirty. Write errors are unhandled, so dirty stays set.
  - On unmount (tab switch or close) a pending debounce is flushed.
  - There is no manual save; Cmd+S and `:w` do nothing for drawings.
- [ ] **Theme**: `theme={usePrefersDark() ? "dark" : "light"}`, following the **OS** `prefers-color-scheme` live. It ignores the app's light/dark/system setting and palette. **QUIRK.** The preferred port behavior follows the resolved app mode. The canvas UI is Excalidraw's stock UI (toolbar, library sidebar, menus) with default props. No `UIOptions` are customised.
- [ ] **Library**: no persistence is wired up (no `libraryItems`, no `onLibraryChange`). Library items added in a session are not saved.
- [ ] **Mermaid → Excalidraw** is deliberately disabled. `@excalidraw/mermaid-to-excalidraw` is aliased in `vite.config.ts` to `src/modules/excalidraw/lib/mermaidToExcalidrawStub.ts` and removed from installs by the `pnpm-workspace.yaml` override `"@excalidraw/mermaid-to-excalidraw": "-"`. The stub's `parseMermaidToExcalidraw()` throws `"Mermaid to Excalidraw conversion is not available in Fuchico."`.
  - Excalidraw's "Text to diagram → Mermaid" dialog shows that error.
  - Pasting Mermaid text into the canvas inserts plain text.
- [ ] **Embedding in notes: none.** No `![[x.excalidraw]]` preview exists anywhere in the editor. In HTML/PDF export, `![[drawing.excalidraw]]` becomes `<img src="drawing.excalidraw">`, which renders as a broken image.
- [ ] Backlinks dock: while an `.excalidraw` tab is active, the dock's target is `null`, so it shows no backlinks.

### 11. Mermaid diagrams: source side, the preview button in notes (`src/modules/editor/lib/mermaidPreviewExtension.ts`)

- [ ] For every fenced code block whose info string, trimmed and lower-cased, equals `mermaid`, a widget button is placed at the end of the opening fence line.
  - Class `cm-mermaid-preview-btn`, text `"▶ Preview diagram"`, tooltip "Preview diagram".
  - Style: 11px, weight 600, `--syntax-function` text with a 14% tinted background, a 40% border, `margin-left:12px`, padding `1px 6px`.
  - mousedown is prevented, so editor focus and selection are kept.
  - Only blocks in the viewport are decorated. Decorations are rebuilt on doc, viewport or syntax-tree change.
- [ ] Clicking it:
  1. extracts the block body (the line after the opening fence up to the start of the closing fence line);
  2. generates `blockKey = crypto.randomUUID()`;
  3. tracks `{anchor: fenceStart, lastText}`;
  4. calls `onOpen({blockKey, text})`, which **docks** a `MermaidPane` in the right dock. The dock label is `app.diagram` ("Diagram" / "Diagrama").

  Opening a diagram replaces the backlinks dock, and vice versa.
- [ ] Live updates:
  - On each doc change, tracked anchors are remapped through the changes.
  - After a **400 ms** debounce (`UPDATE_DEBOUNCE_MS`), each tracked block's text is re-extracted at its anchor. If it changed, the extension emits the Tauri event `"mermaid:update"` with `{blockKey, text}`.
  - `text: null` means the block no longer exists.
- [ ] It listens for `"mermaid:request"` `{blockKey}` and force-emits the current text. It listens for `"mermaid:closed"` `{blockKey}` and stops tracking that key.
- [ ] The dock (`App.tsx`):
  - It has a 5px `col-resize` resizer that tints with the accent on hover.
  - Width: default **380px**, clamped to **240-800px** (`MIN_DOCK_WIDTH` / `MAX_DOCK_WIDTH`).
  - The panel `.mermaid-dock-panel` has a left border and an 85% card background.
  - App also listens for `"mermaid:dock-request"` `{blockKey, label}` (sent from a popped-out window) and docks that block with no initial text, so the pane requests it.

### 12. Mermaid diagrams: pane, window and view (`src/modules/mermaid/**`)

- [ ] **`MermaidPane`** (docked):
  - The header `.mermaid-pane-header` (card background, bottom border, padding `6px 10px`) shows the label at 12px, weight 600, left-aligned.
  - Two buttons sit on the right:
    - `ExternalLink`, "Open in new window": it calls `openMermaidWindow(blockKey, label)` and closes the dock without sending `closed`.
    - `X`, "Close preview": it closes the dock.
  - State: `ready{text}` if `initialText` was given, otherwise `loading` plus `emit("mermaid:request", {blockKey})`.
  - On `mermaid:update` for its key: `text === null` gives `removed`, otherwise `ready{text}`.
  - On unmount it emits `"mermaid:closed"` `{blockKey}` deferred by `setTimeout(0)`, so a StrictMode remount can cancel it. This is skipped when popping out.
- [ ] **`openMermaidWindow`** (`lib/mermaidWindow.ts`):
  - `new WebviewWindow("mermaid-" + blockKey, {url: "index.html?mermaidBlockKey=<enc>&title=<enc>", title, width: 720, height: 560})`.
  - The capability `src-tauri/capabilities/mermaid-window.json` covers the windows `mermaid-*`.
- [ ] **`MermaidWindowApp`** (`src/main.tsx` routes `?mermaidBlockKey=`; the title defaults to `"Diagram"`):
  - It sets `document.title = title` and emits `mermaid:request` on mount.
  - Its header has a single button: `PanelRightOpen` icon, label **"Dock"**, tooltip "Dock back into main window". It emits `"mermaid:dock-request"` `{blockKey, label: title}` and then closes its own window with `getCurrentWindow().close()`.
  - Closing the window any other way emits the deferred `mermaid:closed`.
  - The window keeps working after the source tab is closed, but updates stop. The editor plugin is destroyed, so the window just keeps the last diagram.
- [ ] **`MermaidDiagramView`** (working tree has **uncommitted** changes; this describes the current tree):
  - Renderer: **`pele`** (`render(source, {fontFamily, fontFamilyMono})` from npm `pele@^1.5.2`, which replaced `mermaid@^11` in `package.json`).
  - Rendering is **synchronous** and runs in an effect on each new text. `fontFamily` and `fontFamilyMono` are the computed `--font-sans` / `--font-mono` of `<html>`, or undefined if empty.
  - On error (a PeleError whose message includes the line and a caret snippet) the **last good SVG stays on screen** and the message is shown in a red banner. Success clears the banner.
  - Rendering does not depend on the theme: colours come from CSS variables on `.mermaid-canvas-content`, so a theme switch restyles the existing SVG live with no re-render.
    - `--pele-bg:var(--background)`, `--pele-fg:var(--foreground)`, `--pele-muted:var(--muted-foreground)`, `--pele-line:var(--muted-foreground)`
    - `--pele-surface:var(--card)`, `--pele-surface-alt:var(--secondary)`, `--pele-border:var(--ring)`, `--pele-accent:var(--syntax-function)`
    - `--pele-error` and `--pele-critical: var(--destructive)`
    - `--pele-font:var(--font-sans)`, `--pele-font-mono:var(--font-mono)`, `--pele-radius:var(--radius-sm)`
    - series 1-8: `--syntax-function`, `--syntax-string`, `--syntax-number`, `--syntax-keyword`, `--syntax-type`, `--destructive`, `--syntax-comment`, `--primary`
  - The previous committed version used `mermaid.render` with a `theme` of dark or default, `securityLevel:"strict"`, async rendering and an off-screen container. That version is gone.
  - The SVG is injected as HTML. Pele escapes text and filters links and styles.
  - Banners (`.mermaid-banner`, absolute top-left 12px, blurred card background):
    - "Loading…" while `loading` and no SVG yet;
    - "Source block no longer available" when `removed`;
    - the error text (`.mermaid-banner-error`: destructive colour, `max-width:60%`, `pre-wrap`).

    All of these strings are hard-coded English.
  - Toolbar `.mermaid-toolbar` (absolute top-right 10px, 24×24 buttons, blurred card background), in this order:
    1. ZoomOut, "Zoom out";
    2. ZoomIn, "Zoom in";
    3. RotateCcw, "Reset view".
  - Canvas `.mermaid-canvas`: `overflow:hidden`, `cursor:grab` (`grabbing` while active), `touch-action:none`, and a dotted grid background (radial-gradient 1px dots at 8% foreground, `16px 16px`).
  - The first successful SVG triggers `reset()`. Later SVGs re-apply the current transform, so the view does not jump while the user edits.
- [ ] **Pan/zoom** (`lib/usePanZoom.ts`): direct DOM transforms with no React state.
  - Transform: `translate(${x}px, ${y}px) scale(${scale})` with `transform-origin: 0 0` on the inline-block content.
  - **Limits: `MIN_SCALE = 0.1`, `MAX_SCALE = 8`.**
  - `reset()` sets `{x:0, y:0, scale:1}`, which puts the diagram at the top-left. It is not truly centered, despite the code comment.
  - **Wheel**: a native listener with `{passive:false}` calls `preventDefault`. Every wheel event zooms; there is no scroll-to-pan and no modifier is needed. Trackpad pinch arrives as a ctrl+wheel and also zooms.
    - factor = `Math.exp(-deltaY * 0.001)`
    - anchored at the cursor: `x' = cx - (cx - x) * ratio`, likewise for y, where `ratio = newScale/oldScale` after clamping
  - Buttons: zoom in multiplies by **1.25** and zoom out by **0.8**, both anchored at the container center (`clientWidth/2`, `clientHeight/2`).
  - **Drag to pan**: pointerdown on the canvas calls `setPointerCapture` and records the start position. pointermove for the same pointerId sets `x = origX + dx`, `y = origY + dy`. pointerup ends the drag. Any button and any pointer type (mouse, pen, touch) works. There is no inertia.
  - No keyboard shortcuts for zoom or pan. No double-click behaviour.

### 13. Export: Markdown → HTML (`src/modules/export/lib/renderHtml.ts`, test `renderHtml.test.ts`)

- [ ] `renderHtml(markdown, {title, resolveHref?, bodyExtra?, themeVars?, mode?})` returns a full standalone HTML string.
- [ ] **Frontmatter is stripped.** `parseFrontmatter` requires the first line to be `---` and a closing `---` or `...` line. The body starts at the line after the close. If frontmatter is unterminated, the whole text is rendered.
- [ ] **Wikilinks are rewritten in the source before parsing** (`rewriteWikilinks`):
  - Embeds come first, `EMBED_RE = /!\[\[([^[\]|]+)(?:\|([^[\]]+))?\]\]/g`: `![[target|alt]]` becomes `![alt or target](encodeURI(target))`. For example `![[pic.png]]` becomes `![pic.png](pic.png)`.
  - Then links, `WIKILINK_RE = /\[\[([^[\]|#]+)(#[^[\]|]*)?(?:\|([^[\]]+))?\]\]/g`: `[[T#H|alias]]` becomes `[label](encodeURI(T).html#anchor)`.
    - label = alias, or `T#H` when a heading is given, otherwise T;
    - anchor = the heading without `#`, trimmed, lower-cased, with `/[^\w]+/g` replaced by `-`.
  - Test vectors: `"see [[Note B|alias]] and [[C#Head ing]]"` becomes `"see [alias](Note%20B.html) and [C#Head ing](C.html#head-ing)"`.
  - Links resolve to sibling `.html` files by name (no vault resolution or folder paths). Marked does not add heading ids, so the anchors do not resolve.
- [ ] Markdown engine: `marked` with `{gfm:true, breaks:false}` (GFM tables, strikethrough, autolinks, task lists). Raw HTML passes through unchanged.
- [ ] Custom renderers:
  - image: `<img src="…" alt="…"[ title="…"]>` with escaping (`& < > "`). `src` goes through `resolveHref(href,"image")`.
  - link: `<a href="…"[ title]>inline</a>`, with `resolveHref(href,"link")`.
  - task list item: `<li class="task"><input type="checkbox" disabled[ checked]> …</li>`. **QUIRK (verified):** marked v18 also emits its own `<input disabled="" type="checkbox"> ` inside `inner`, so each task shows **two checkboxes**. The intended output is one disabled checkbox. The test only checks the prefix.
- [ ] Not specially rendered:
  - `#tags` are plain text (CSS has an unused `.tag { color: var(--syntax-type) }`);
  - mermaid fences render as `<pre><code class="language-mermaid">` source with no diagram;
  - no syntax highlighting;
  - no fold-region markers or callouts.
- [ ] Document skeleton:
  - `<!doctype html><html lang="en" data-mode="{light|dark}">`, `<meta charset="utf-8">`, the viewport meta, and `<title>{escaped title}</title>`;
  - `<style>:root{ <each theme var>: <value>; } + READING_CSS</style>`;
  - `<body><main>{html}</main>{bodyExtra}</body>`.
- [ ] Theme vars: the default `currentThemeVars()` takes computed values from `<html>` for every `THEME_VARIABLES` name, so custom themes export too:
  - `--background`, `--foreground`, `--card`, `--card-foreground`, `--primary`, `--primary-foreground`
  - `--secondary`, `--secondary-foreground`, `--muted`, `--muted-foreground`, `--accent`, `--accent-foreground`
  - `--destructive`, `--border`, `--input`, `--ring`
  - `--syntax-keyword`, `--syntax-string`, `--syntax-number`, `--syntax-function`, `--syntax-type`, `--syntax-comment`
  - `--radius`, `--font-sans`, `--font-mono`

  Empty values are omitted. `mode` defaults to `document.documentElement.dataset.mode` (`"dark"` gives dark, anything else light). The exported page always uses the current app theme.
- [ ] `READING_CSS` (exact behavioral values):
  - `color-scheme: light dark`; html background and colour from vars;
  - body: margin 0, padding `48px 24px 96px`, font `var(--font-sans, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif)` at 16px, line-height 1.65;
  - `main` max-width 760px, centered;
  - headings use `--syntax-keyword` colour, line-height 1.25, margin `1.6em 0 0.6em`. h1 is 2em with top margin 0, h2 1.5em, h3 1.25em;
  - links use `--syntax-function`, underlined only on hover;
  - code is mono at 0.9em with a `--muted` background, padding `.1em .35em`, radius 4px. `pre` uses `--muted`, padding `14px 16px`, radius 8px and `overflow-x:auto`;
  - blockquote has a 3px left `--border` and muted text;
  - tables collapse to 100% width with 1px cell borders, padding `6px 10px`, and a muted `th`;
  - `img` max-width 100%. `hr` is a 1px top border with 2em margin;
  - `li.task { list-style:none; margin-left:-1.4em }`; checkboxes have `margin-right:.5em`;
  - `@media print`: body padding 0 and 12pt, `pre` uses `pre-wrap`, links inherit colour, `.no-print { display:none !important }`.

### 14. Export as HTML (`src/modules/export/lib/exportActions.ts`, `exportAsHtml`)

- [ ] It is triggered only from the palette (`export-html`, `:export`), and only for an active `.md`/`.markdown` tab. It uses the **live editor buffer** (`editorRef.getContent()`), unsaved edits included.
- [ ] It opens a native save dialog (`@tauri-apps/plugin-dialog` `save`) with `defaultPath: "<note dir>/<stem>.html"` and `filters: [{name:"HTML", extensions:["html"]}]`. The stem is the basename with `/\.(md|markdown)$/i` removed. Cancelling aborts silently.
- [ ] It renders with `title: stem` and **no** href rewriting, so relative image and link paths stay relative and work when the file is saved beside the note.
- [ ] It writes with `invoke("fs_write_file", {path: target, content: html, source: "export"})`.
  - On success, a native message dialog `message(t("export.saved",{path}), {kind:"info"})`: "Exported to {path}" / "Exportado para {path}".
  - On failure, `{kind:"error"}` with "Export failed: {message}" / "Falha ao exportar: {message}".
  - `window.alert` is not used because it is a no-op in the macOS webview.

### 15. Export as PDF via the print window (`exportAsPdf`, `src/modules/export/PrintWindow.ts`)

- [ ] It is triggered from the palette (`export-pdf`, `:pdf`, `:print`) under the same conditions as §14, using the live buffer.
- [ ] Window key: `"export-" + Date.now().toString(36)` (`EXPORT_WINDOW_PREFIX = "export-"`; capability `src-tauri/capabilities/export-window.json` covers `export-*`).
- [ ] HTML is rendered with `title: stem` and a `bodyExtra` toolbar. The toolbar is
  ```
  <div class="no-print" style="position:fixed;top:0;left:0;right:0;display:flex;align-items:center;gap:12px;padding:8px 16px;background:var(--card);color:var(--card-foreground);border-bottom:1px solid var(--border);font-size:13px;z-index:10">
  ```
  It contains a pill button `id="print"` (`padding:6px 14px; border-radius:999px; background:var(--primary); color:var(--primary-foreground)`) with the label `export.print` ("Print / Save as PDF" / "Imprimir / Salvar como PDF"), followed by a span at opacity .75 with `export.printHint` ("Use the print dialog to save this note as a PDF." / "Use a caixa de diálogo de impressão para salvar esta nota como PDF.").
- [ ] `resolveHref` for **images only**: an href that is not external (`/^[a-z][a-z0-9+.-]*:/i` or starting with `//`) and not `data:` becomes `convertFileSrc(resolveRelativePath(notePath, decodeURI(href)))`, an `asset://` URL resolved against the note's folder with `.` and `..` handled. Links are left unchanged.
- [ ] Handshake over Tauri events (the same request/reply pattern the mermaid pop-out uses):
  1. The opener stores `pending[key] = html` and installs a single global listener (once per JS context) for `"export:request"` `{key}`. It replies with `emit("export:html", {key, html})` if the key is known.
  2. It opens `new WebviewWindow(key, {url: "index.html?window=print&key=<enc>", title: "<stem> — PDF", width: 860, height: 900})`. On `"tauri://destroyed"` it deletes the pending entry.
  3. The print window (`main.tsx` routes `?window=print&key=`; no React) runs `mountPrintWindow(key)`. It listens for `export:html` matching its key, then emits `export:request`. On reply it runs `document.open(); document.write(html); document.close()`, which replaces the app document entirely.
  4. It wires `#print` click to `invoke("export_print")`, falling back to `window.print()` on error.
  5. It sets `body.style.paddingTop = "64px"` so the toolbar does not cover the content.
- [ ] Rust command `export_print(window: WebviewWindow) -> Result<(), String>` calls `window.print()` (`src-tauri/src/lib.rs`), which opens the OS print dialog for the calling window. "Save as PDF" is offered there. The toolbar is hidden in print output by `.no-print`.
- [ ] No PDF file is written by the app. The PDF exists only through the OS print dialog. There is no auto-print on open.

### 16. Events, invokes and storage summary for this section

- [ ] Tauri invokes:

  | Command | Arguments | Returns |
  |---|---|---|
  | `fs_read_dir` | `{path, showHidden:false}` | `DirEntry[]` |
  | `fs_list_markdown_files` | `{root}` | `string[]` |
  | `fs_stat` | `{path}` | throws if missing |
  | `fs_read_file` | `{path}` | `{kind:"text",content,size}\|{kind:"binary",size}\|{kind:"toolarge",size,limit}` |
  | `fs_create_dir` | `{path}` | |
  | `fs_create_file` | `{path}` | |
  | `fs_write_file` | `{path, content, source: "template"\|"editor"\|"export"}` | emits `fs:file-written` |
  | `tasks_scan` | `{root}` | `AgendaItem[]` |
  | `tasks_toggle` | `{path, line}` | |
  | `export_print` | none | |

- [ ] Tauri events:

  | Event | Payload |
  |---|---|
  | `fs:file-written` | `{path, source}` |
  | `fs:changed` | `{dir, from}` |
  | `mermaid:update` | `{blockKey, text\|null}` |
  | `mermaid:request` | `{blockKey}` |
  | `mermaid:closed` | `{blockKey}` |
  | `mermaid:dock-request` | `{blockKey, label}` |
  | `export:request` | `{key}` |
  | `export:html` | `{key, html}` |
  | `tauri://destroyed` | (built-in) |

  DOM event: `helix:fs-changed` (`detail: dir`).
- [ ] Window URL routes (`src/main.tsx`):
  - `?mermaidBlockKey=<k>&title=<t>` gives a diagram window labelled `mermaid-<k>`;
  - `?window=print&key=<k>` gives a print window labelled `export-…`;
  - `?window=editor&path=…&root=…` gives a detached editor (documented elsewhere).
- [ ] localStorage:
  - `helix.editorSettings`: theme mode and palette, custom themes, shortcuts, `dailyNotesFolder` (default `"daily"`), `templatesFolder` (default `"templates"`);
  - `helix.settingsSection`: the last Settings section;
  - `helix.lastRootPath`: the vault root, set when a folder is opened.

  Nothing in the palette, agenda, Excalidraw, Mermaid or export modules persists its own UI state. Excalidraw's library and the agenda's month and selection are not persisted.


---

## 5. Settings, Themes & i18n

Sources read in full: `src/modules/settings/**` (all `*.tsx`, `lib/*.ts`, `lib/fixedShortcuts.test.ts`), `src-tauri/src/modules/fonts/mod.rs`, `src/styles/globals.css` (2761 lines), `src/App.css`, `src/lib/i18n.ts`, `src/lib/i18n.test.ts`, `src/lib/i18n/en.ts`, `src/lib/i18n/ptBR.ts`, `src/lib/usePrefersDark.ts`, `README.md`. Cross-checked consumers: `src/modules/editor/lib/shortcuts.ts`, `src/modules/editor/lib/rulers.ts`, `src/modules/editor/lib/extensions.ts`, `src/modules/palette/lib/commands.ts`, `src-tauri/src/lib.rs`, `src-tauri/src/modules/caldav/commands.rs`.

### 1. Settings store (`lib/editorSettings.ts`)

#### 1.1 Persistence

- [ ] Single global settings object, persisted as **one JSON blob** under the key **`helix.editorSettings`** (webview `localStorage`). The port needs an equivalent per-user file (e.g. `settings.json` in the app config dir) with the **same JSON shape** (camelCase keys below) so it can be migrated/imported.
- [ ] Every write (`set(partial)` / `setShortcut`) shallow-merges into current state, re-serializes the **entire** object, writes it, then notifies all subscribers synchronously. No debounce.
- [ ] Load: if missing → `DEFAULT_SETTINGS`. If present: `{...DEFAULT_SETTINGS, ...parsed, shortcuts: {...DEFAULT_SETTINGS.shortcuts, ...parsed.shortcuts}}` (unknown keys are kept as-is; missing keys get defaults; shortcuts merged per action). Any parse error → defaults (silent).
- [ ] **No type validation on load** (a wrong-typed stored value is used verbatim). Clamping only happens at the input points (zoom shortcuts, number inputs).
- [ ] **Migration (pre-issue-#3):** if the stored object has a string `customThemeCss` that is non-blank **and** `customThemes` is empty → create one theme `{id: newCustomThemeId(), name: "Custom", css: customThemeCss}`, set `customThemes=[it]`, `customThemeId=it.id`. The `customThemeCss` key is dropped from state (not re-saved).
- [ ] **Dangling id repair:** if `customThemeId !== null` and no theme has that id → `customThemeId = customThemes[0]?.id ?? null`. (Note: `palette` is *not* changed here; if palette is `"custom"` with no themes, `activeCustomTheme` is null and no custom CSS is injected.)
- [ ] **Multi-window sync (issue #29):** each OS window has its own JS instance; on the `storage` event for key `helix.editorSettings` the other windows re-`load()` and re-emit. Port: all windows must observe one shared settings model and re-render on any change.
- [ ] Module-level pub/sub (`editorSettingsStore.get/set/setShortcut/subscribe`), used from React via `useSyncExternalStore`, and also called imperatively (palette commands, zoom shortcut handler, `t()`).

#### 1.2 Every setting key

| Key | Type | Default | Validation / notes | Effect |
|---|---|---|---|---|
| `language` | `"system" \| "en" \| "pt-BR"` | `"system"` | — | UI locale (§4). |
| `palette` | `"ayu" \| "dracula" \| "catppuccin" \| "custom"` | `"ayu"` | — | `data-palette` on `<html>` (§3). |
| `mode` | `"system" \| "light" \| "dark"` | `"system"` | — | Resolved to light/dark → `data-mode`. |
| `customThemes` | `{id: string, name: string, css: string}[]` | `[]` | — | User themes. |
| `customThemeId` | `string \| null` | `null` | repaired on load (above) | Theme used when `palette === "custom"`. |
| `keybindingMode` | `"helix" \| "vim" \| "normal"` | `"helix"` | — | Editor keymap; applies immediately to open editors. |
| `shortcuts` | `Record<ShortcutAction, string>` | see §2.2 | per-action merge on load; no validation | Rebindable editor action keys. |
| `foldStartMarker` | string | `":::fold"` | free text, saved per keystroke | Fold region start marker. |
| `foldEndMarker` | string | `":::endfold"` | free text | Fold region end marker. |
| `uiFont` | string (family name) | `""` (= default stack) | — | `--font-sans` override. |
| `editorFont` | string (family name) | `""` | — | `--font-mono` override. |
| `relativeLineNumbers` | boolean | `false` | — | Helix/Vim-style relative gutter (current line absolute). |
| `tabSize` | number | `2` | UI offers only 2/4/8 | Spaces per indent / tab stop. |
| `rulers` | number[] | `[]` | `parseRulers`: split on `/[\s,;]+/`, keep integers `1..1000`, dedupe, sort ascending | Vertical guide lines at columns. |
| `editorFontSize` | number (px) | `13` | `clampEditorFontSize`: non-finite → 13; clamp `[8, 32]` (no rounding) | Editor content font size. |
| `uiScale` | number (factor) | `1` | `clampUiScale`: non-finite → 1; clamp `[0.7, 1.6]`, round to 2 decimals | Whole-app zoom. |
| `dailyNotesFolder` | string (vault-relative) | `"daily"` | free text | Daily notes `YYYY-MM-DD.md` location. |
| `templatesFolder` | string (vault-relative) | `"templates"` | free text | Template source; `daily.md` there seeds daily notes. |
| `externalTool` | string (app name) | `""` (= OS default handler) | free text | Used by explorer "Open with {tool}". |

Constants: `EDITOR_FONT_SIZE_DEFAULT=13`, `_MIN=8`, `_MAX=32`; `UI_SCALE_DEFAULT=1`, `_MIN=0.7`, `_MAX=1.6`.

Stored JSON example (defaults):

```json
{"language":"system","palette":"ayu","mode":"system","customThemes":[],"customThemeId":null,
 "keybindingMode":"helix","shortcuts":{"openOutline":"Mod-o","toggleCheckboxAtCursor":"Mod-Enter",
 "insertDate":"Mod-Shift-d","insertDateTime":"Mod-Alt-d","insertRegion":"Mod-Shift-r","insertTable":"Mod-Alt-t",
 "toggleTaskLine":"Mod-Shift-Enter","pickDueDate":"Mod-Shift-.","pickRecurrence":"Mod-Shift-,"},
 "foldStartMarker":":::fold","foldEndMarker":":::endfold","uiFont":"","editorFont":"",
 "relativeLineNumbers":false,"tabSize":2,"rulers":[],"editorFontSize":13,"uiScale":1,
 "dailyNotesFolder":"daily","templatesFolder":"templates","externalTool":""}
```

Helpers:
- [ ] `newCustomThemeId()` = `` `theme-${Date.now().toString(36)}-${random base36 5 chars}` ``.
- [ ] `activeCustomTheme(s)` = theme whose `id === s.customThemeId`, else null.

#### 1.3 Other persisted UI state

- [ ] **`helix.settingsSection`** (localStorage string): last selected settings section id; validated against the section list, default `"appearance"`. Read/write errors swallowed. `settingsNav.set(id)` is a no-op if unchanged. Module-level so the command palette can pre-select a section before switching the main view to Settings.

### 2. Keyboard shortcuts

#### 2.1 Binding string format

- [ ] CodeMirror key syntax: modifiers joined with `-` then the key: `Mod-Shift-p`. Modifier tokens: `Mod` (Cmd on macOS, Ctrl elsewhere), `Meta`, `Ctrl`, `Alt`, `Shift`. Key: single char lowercase (`d`, `.`, `,`, `-`) or `KeyboardEvent.key` name (`Enter`, `ArrowUp`, `F2`).
- [ ] **Capture (rebinding)** — `bindingFromEvent`: ignore pure modifier keydowns (`Control`, `Meta`, `Alt`, `Shift`); `Mod` if `ctrlKey || metaKey` (Ctrl and Cmd collapse to `Mod`); then `Alt` if altKey; then `Shift` if shiftKey; key = `e.key.toLowerCase()` if length 1 else `e.key`. Join with `-`. Order is always `Mod-Alt-Shift-key`. Note: uses `e.key`, so Shift+`.` on US layout would record `Mod-Shift->` (layout-dependent) — replicate or normalize consistently with matching.
- [ ] A binding with no modifier is allowed (e.g. just `a`).
- [ ] **Display** — `formatBinding(binding, isMac)`: split alternatives on `" / "`; per chunk peel leading `(Mod|Meta|Ctrl|Alt|Shift)-` repeatedly with regex `^(Mod|Meta|Ctrl|Alt|Shift)-(.+)$` (so `Mod--` = Mod + `-`); chunk with no modifier passes through untouched (`F2`, `Click`, `@`, `Delete / Backspace`); single-char key uppercased.
  - macOS (`navigator.platform` matches `/Mac|iP(hone|ad|od)/`): symbols concatenated, no separator: Mod `⌘`, Meta `⌘`, Ctrl `⌃`, Alt `⌥`, Shift `⇧` → `⌘⇧P`, `⌘⇧Enter`, `⌥ArrowUp`, `⌘-`.
  - Others: names joined with `+`: Mod `Ctrl`, Meta `Win`, Ctrl `Ctrl`, Alt `Alt`, Shift `Shift` → `Ctrl+Shift+P`, `Ctrl+.`, `Ctrl+Shift+-`.
- [ ] Unit tests to port (`fixedShortcuts.test.ts`): `Mod-Shift-p`→`⌘⇧P`/`Ctrl+Shift+P`; `Mod-s`→`⌘S`; `Mod-Shift-Enter`→`⌘⇧Enter`; `Alt-ArrowUp`→`⌥ArrowUp`; `Mod-.`→`Ctrl+.`; `Mod--`→`⌘-`; `Mod-Shift--`→`Ctrl+Shift+-`; `F2`, `Right-click`, `Delete / Backspace` unchanged; `Mod-= / Mod-- / Mod-0`→`Ctrl+= / Ctrl+- / Ctrl+0`.

#### 2.2 Rebindable editor actions (`SHORTCUT_ACTIONS`, order as listed)

| Action id | Default | Label (en) | Description (en) | Behavior (from `editor/lib/shortcuts.ts`) |
|---|---|---|---|---|
| `openOutline` | `Mod-o` | Go to header | Open the document outline (fuzzy-searchable header list). | Opens outline overlay. |
| `toggleCheckboxAtCursor` | `Mod-Enter` | Toggle checkbox | Mark/unmark the checkbox on the cursor's line. | For each selection's line, first `[ ]`/`[x]`/`[X]` toggles; returns false if none (key falls through). |
| `insertDate` | `Mod-Shift-d` | Insert date | Insert today's date at the cursor. | Inserts `YYYY-MM-DD` (local) replacing each selection. |
| `insertDateTime` | `Mod-Alt-d` | Insert date & time | Insert the current date and time at the cursor. | `YYYY-MM-DD HH:mm`. |
| `insertRegion` | `Mod-Shift-r` | Insert fold region | Wrap the selected lines in a foldable region (or insert an empty one at the cursor). | Empty sel: inserts `<start> Region\n<end>\n`, cursor after `<start> `; else wraps full lines with `<start> Region\n` … `\n<end>`. Uses current fold markers. |
| `insertTable` | `Mod-Alt-t` | Insert table | Insert a 2x2 Markdown table at the cursor and start editing the header. | |
| `toggleTaskLine` | `Mod-Shift-Enter` | Toggle task | Turn the current line into a `- [ ]` task (or back into plain text). | |
| `pickDueDate` | `Mod-Shift-.` | Set due date… | Add or change the 📅 due date on the current line via a quick-pick list. | |
| `pickRecurrence` | `Mod-Shift-,` | Set recurrence… | Add or change the 🔁 repeat rule on the current line via a quick-pick list. | |

- [ ] Scope: active editor only. Installed as a **highest-precedence** keymap (`Prec.highest`) over Helix/Vim/normal keymaps, `preventDefault: true`. Rebuilt when `shortcuts` or fold markers change.
- [ ] Also exposed as palette commands `"Editor: {label}"` (only when a tab is active) with binding hint = current binding.
- [ ] **Conflict handling: none.** No duplicate detection between actions, nor against fixed shortcuts or Helix keys; no "reset to default" button. Two actions with the same key → CodeMirror runs the first in array order that returns true. Port should keep behavior (optionally add warnings).

#### 2.3 Fixed (non-rebindable) shortcuts (`FIXED_SHORTCUTS`, display order)

| Group | Label (en) | Keys (raw) | Description (en) |
|---|---|---|---|
| Global | Command palette | `Mod-Shift-p` | Run any app command by name. |
| Global | Quick open file | `Mod-p` | Fuzzy-find a note in the vault. |
| Global | Editor font size | `Mod-= / Mod-- / Mod-0` | Bigger / smaller / reset editor text. |
| Global | UI zoom | `Mod-Shift-= / Mod-Shift-- / Mod-Shift-0` | Zoom the whole app in / out / reset. |
| Editor | Command palette from Helix | `:` | In Helix normal mode, : opens the command palette; :q, :w, :wq and :<line> work as aliases. |
| Editor | Save | `Mod-s` | Write the current file to disk. |
| Editor | Find / replace | `Mod-f` | Search within the current file. |
| Editor | Task date / repeat autocomplete | `@` | Type @due, @today, @repeat (or 📅 / 🔁) on a task line to pick a date or rule. |
| Editor | Paste image | `Mod-v` | Paste or drop an image to save it under attachments/ and link it. |
| Editor | Follow link | `Click` (translated: "Click"/"Clique") | Open the [[wikilink]] or Markdown link under the cursor. |
| File explorer | Navigate | `↑ ↓ ← →` | Move selection, expand/collapse folders. |
| File explorer | Open / toggle folder | `Enter` | Open the selected file or expand/collapse the folder. |
| File explorer | Rename | `F2` | Rename the selected entry. |
| File explorer | Delete | `Delete / Backspace` | Delete the selected entry (asks for confirmation). |
| File explorer | Type-ahead | `a–z` | Type a name prefix to jump to the matching entry. |
| File explorer | Context menu | `Right-click` (translated) | New file/folder, rename, delete. |
| Tabs | Tab menu | `Right-click` (translated) | Close, close others, close all. |

- [ ] Rows with `keysKey` render `t(keysKey)` instead of `formatBinding(keys)`. Fixed list is also reused by the command palette for binding hints.

#### 2.4 Zoom shortcuts (`useZoomShortcuts`, issue #5)

- [ ] Global window keydown listener in **capture phase** (wins over editor keymaps and the WebView's own zoom); installed in main window (`App`) and the secondary editor window (`EditorWindowApp`).
- [ ] Requires `metaKey || ctrlKey`, and **no** Alt. Matches on physical `e.code` (layout-independent; Shift turns `=` into `+`): `Equal`/`NumpadAdd` → +1; `Minus`/`NumpadSubtract` → −1; `Digit0`/`Numpad0` → reset. Else ignored. On match: `preventDefault` + `stopPropagation`.
- [ ] Without Shift: `editorFontSize` ±1 px clamped [8,32]; reset → 13.
- [ ] With Shift: `uiScale` ±0.1 clamped [0.7,1.6] (rounded 2 dp); reset → 1.0. Steps: 0.7, 0.8 … 1.6 (10 levels).
- [ ] Persisted immediately via store.

### 3. Settings view (`SettingsView.tsx`, `lib/settingsNav.ts`)

#### 3.1 Layout

- [ ] Two columns: left nav (190px, border-right, 16px/10px padding, scrollable) + content (flex, padding 24/28/40, each child max-width 560px). At window width ≤720px: stacked, nav becomes a wrapping row with bottom border, search full-width.
- [ ] Nav top: search box (Search icon 13px, `type=search` input, placeholder "Search settings…"), then one button per section (label `t(labelKey)`). Active item (`section === id && !searching`) highlighted (bg foreground 9%, weight 500); hover bg 5%.
- [ ] Clicking a nav item clears the search and selects the section (persisted).
- [ ] `aria-label` of nav: "Settings sections".

Sections (`SETTINGS_SECTIONS`, order) and contents:

| id | Label (en) | Keywords (for palette search) | Renders |
|---|---|---|---|
| `appearance` | Appearance | theme, font, zoom, language, idioma | Language, Theme, Font |
| `editor` | Editor | keybindings, helix, vim, folding, behavior | Keybindings, Editor Behavior, Fold Regions |
| `shortcuts` | Keyboard Shortcuts | keys, hotkeys | Shortcuts |
| `vault` | Vault | folders, daily, templates, external tool | Vault Folders |
| `integrations` | Integrations | caldav, sync | CalDAV (accounts, links, sync) |
| `about` | About | version | About |

- [ ] Palette exposes `"Settings: {name}"` per section and "Keyboard Shortcuts" → opens `shortcuts` section.

#### 3.2 Settings search (issue #46)

- [ ] When query (trimmed) non-empty: render **all** sections, each wrapped in a group with an uppercase group title (section label); otherwise only the selected section.
- [ ] Filtering by visible text: terms = lowercase query split on whitespace; a node matches when **every** term is a substring of its text. Per `.settings-section`: if section title+desc matches all terms → whole section shown; else each "field" (field, shortcut row, account row, link row, actions row) shown iff it matches; section shown iff head matches or ≥1 field shown or (no fields and section text matches). Groups with no visible section are hidden.
- [ ] If zero sections visible: show `No settings match "{query}".`
- [ ] Re-filters on query/section change and when language changes (because it matches translated text).

#### 3.3 Language section

- Title "Language"; desc `Language of the app interface. "System" follows your operating system language.`; field label "Language".
- [ ] Toggle-button group (single select): `System` (translated), `English`, `Português (Brasil)` (endonyms, not translated). Sets `language`; whole UI re-renders immediately.

#### 3.4 Theme section

Title "Theme"; desc "A theme is a color choice, independent of light/dark mode. Each palette may support a light and/or dark variant."

- [ ] **Palette** button group: `Ayu`, `Dracula`, `Catppuccin` (proper nouns) → `palette=value`; then one button per custom theme (its name) → `palette="custom", customThemeId=id` (active only if both match); then `+ New theme` and `⤒ Import…` icon-buttons.
- [ ] **Mode** button group: System / Light / Dark → `mode`. All three **disabled when palette is Dracula**, with hint "Dracula only ships a dark palette, so mode is fixed to dark." The stored `mode` value is still shown as active.
- [ ] When `palette === "custom"` and an active custom theme exists ("editing"):
  - [ ] **Theme name** text input → updates theme name on every keystroke (persisted).
  - [ ] **Custom CSS variables** — CSS code editor (§3.5) bound to a local *draft*; draft resets to saved CSS whenever the edited theme id changes. Hint: "Declarations are injected as-is into a `[data-palette="custom"]` rule. Preview updates as you type; click Apply to use it across the whole app. Type -- to autocomplete the app's theme variables."
  - [ ] Buttons: **Apply** (disabled when draft == saved; saves draft to theme), **Duplicate** (copy icon; new theme with `uniqueName(name)` and current draft CSS), **Export…** (download icon), **Delete** (trash, danger color). Status "Applied" (ok color) shown while draft == saved; plus a transient status text (export/import result or error string).
  - [ ] Live preview: draft CSS injected (150 ms debounce) as `.theme-preview[data-palette="custom"] { <draft> }` into a separate style element (`fuchico-custom-theme-preview`) — affects only the preview card, not the app. Removed when not editing / on unmount.
- [ ] **New theme**: name = `uniqueName("My theme")` (translated), CSS = snapshot of the current computed values of every **color** theme variable (`--name: value;` one per line, `inherit` if empty) so it starts identical to the active palette; adds it, selects it, sets `palette="custom"`.
- [ ] `uniqueName(base)`: base if unused, else `base 2`, `base 3`, … (first free).
- [ ] **Delete**: native confirm `Delete the theme "{name}"?`; remove; `customThemeId = remaining[0]?.id ?? null`; `palette = remaining.length ? "custom" : "ayu"`.
- [ ] **Export**: save dialog, default file name `themeFileName(name)`, filter `CSS` / `css`; writes via `invoke("fs_write_file", {path, content: serializeTheme({...theme, css: draft}), source: "theme-export"})` (exports the **draft**, not saved CSS). Status "Theme exported." or error.
- [ ] **Import**: open dialog (single file, filter CSS); `invoke("fs_read_file", {path})` → `{kind, content?}`; if `kind !== "text"` → "Couldn't read that file as a theme."; else `parseThemeFile(content, stem(path))` → add with `uniqueName(parsed.name)`; status "Theme imported.". `stem` strips dir and `.fuchico-theme.css` or `.css` (case-insensitive).
- [ ] **Preview card** (label "Preview"): `data-palette={palette}`, `data-mode={resolved}` (Dracula → dark; system → OS). Contents: toolbar with 3 dots; buttons "Primary" (`.btn`) and "Secondary" (`.btn-secondary`); pangram text "The quick brown fox jumps over the lazy dog."; code block with syntax-colored sample:
  ```
  function greet(name) {
    return `Hello, ${name}`; // greets someone
  }
  ```
  colors: `function`/`return` keyword, `greet` function, `name` (param) type, template string string, comment comment.

#### 3.5 Theme CSS editor (`ThemeCssEditor.tsx`)

- [ ] CodeMirror with CSS language, line numbers, active-line highlight, no fold gutter, line wrapping, max height 360px, min content height 160px, 12px font, mono font, line-height 1.5; bg `--background`, border `--input`, focused border `--ring`, radius `--radius-md`; active line bg foreground 5%.
- [ ] Autocomplete (activate on typing): custom source first, then standard CSS property/value completion.
- [ ] Custom source: matches `/-{1,2}[\w-]*/` before cursor (requires non-empty unless explicit); only offered when at declaration start (line text before word is whitespace) or inside `var(` . Options = every `THEME_VARIABLES` name, type "variable", detail = translated description, info popup = current computed value (with a 12px color swatch for color kinds); applied text = `name` inside `var(`, else `name: `.

#### 3.6 Custom theme files (`themeFiles.ts`)

- [ ] Custom themes live **only in settings JSON** (`customThemes`), not on disk; files are import/export only.
- [ ] File format: plain CSS declarations (no selector), with header comment line: `/* fuchico-theme: <name> */\n<css.trim()>\n`. `*/` sequences stripped from the name.
- [ ] Parse: regex `^\s*\/\*\s*fuchico-theme:\s*(.+?)\s*\*\/\s*\n?`; name = captured (trimmed) or fallback (file stem); css = remainder trimmed (or whole text trimmed if no header).
- [ ] File name: name lowercased, runs of non `[a-z0-9]` → `-`, trim leading/trailing `-`, or `theme` if empty, + `.fuchico-theme.css`.

#### 3.7 Font section

Title "Font"; desc "Choose fonts installed on this machine for the app's interface and for the editor."

- [ ] On mount, `invoke("fonts_list_system")` → `string[]`. Rust: `font_kit::SystemSource::new().all_families()`, sorted, deduped; error → string. While loading each select shows "Loading system fonts…"; on error "Couldn't list system fonts: {message}".
- [ ] **UI font** select: first option "Default" (value `""`), then every family → `uiFont`.
- [ ] **Editor font** select: same → `editorFont`.
- [ ] **Editor font size** number input (width 90px, min 8, max 32) → `clampEditorFontSize(valueAsNumber)` (empty/NaN → 13). Hint "Also Cmd/Ctrl +, − and 0 to reset."
- [ ] **UI zoom** number input in percent (min 70, max 160, step 10, value `round(uiScale*100)`) → `clampUiScale(v/100)`. Hint "Percent. Also Cmd/Ctrl Shift +, − and 0 to reset."
- [ ] Preview card (same as theme preview but using the app's live palette): button row + pangram use `font-family: "<uiFont>"` if set; code block uses `"<editorFont>"` if set.

#### 3.8 Keybindings section (Editor)

Title "Keybindings"; desc "Choose how the editor interprets keystrokes. Takes effect immediately in any open file."
- [ ] Button group: `Helix` (tooltip "Modal editing, Helix-style selection-first commands."), `Vim` (tooltip "Modal editing, Vim keybindings."), `Normal` (translated; tooltip "Standard text-editor keybindings, no modes.") → `keybindingMode`.

#### 3.9 Editor Behavior section

Title "Editor Behavior"; desc "Gutter and indentation preferences for the code editor."
- [ ] **Relative line numbers**: two buttons `Absolute` (tooltip "Every line shows its absolute number.") / `Relative` (tooltip "The current line shows its absolute number; every other line shows its distance from the cursor.") → `relativeLineNumbers` false/true. Hint "Helix/Vim-style: handy for jumping N lines with motion commands."
- [ ] **Tab size**: buttons `2`, `4`, `8` → `tabSize`. Hint "Spaces per indent level and per tab stop."
- [ ] **Vertical rulers**: text input, placeholder `80, 120`, no spellcheck; local draft initialized from `rulers.join(", ")`; **commit on blur or Enter**: parse (§1.2), reformat draft to canonical `a, b`, save only if changed. Hint "Comma-separated columns where a guide line is drawn (e.g. 80, 120). Leave empty for none." Rulers render as 1px lines (foreground 12%) at `contentLeft + padding + col * charWidth`.

#### 3.10 Fold Regions section

Title "Fold Regions"; desc "Wrap any lines between a start and end marker to make them foldable — this is an app-specific convention, not part of Markdown. The rest of the start-marker line becomes the region's name."
- [ ] **Start marker** text input (no spellcheck) → `foldStartMarker` per keystroke; hint `e.g. <code>{startMarker} Region name</code>` (live).
- [ ] **End marker** text input → `foldEndMarker`.
- [ ] **Preview**: row "Expanded" showing region bar (`▾` chevron, `📁` icon, "Region name" label in `--syntax-function` color, tinted bg/border); row "Collapsed" showing fold pill (`📁`, "Region name", muted "3 lines").
- [ ] No validation (empty marker allowed).

#### 3.11 Shortcuts section

Title "Keyboard Shortcuts"; desc "Everything the keyboard can do, in one place. Editor actions are rebindable: click a binding, then press the new key combination."
- [ ] Own search input (placeholder "Search shortcuts…", max-width 320px). Filter: every whitespace term must be a substring (case-insensitive) of the joined haystack: rebindable rows → `"editor"`, label, desc, raw binding, formatted binding; fixed rows → group name, label, desc, keys text (translated if keysKey), formatted keys. Re-filters on language change.
- [ ] Group header "Editor (rebindable)" (only if any match) then rebindable rows; then fixed rows grouped by group (Global, Editor, File explorer, Tabs) in first-appearance order, uppercase group headers.
- [ ] Row: label (bold 12px) + hint description, right: key chip (mono 11px, min-width 110px, centered).
- [ ] **Rebinding flow**: click chip → recording state (chip text "Press keys…", primary-colored border/text, tinted bg). Keydown while recording: `preventDefault`; `Escape` cancels; pure modifier ignored (keep waiting); otherwise binding saved via `setShortcut` and recording ends. Blur cancels. Only one row records at a time.
- [ ] Fixed chips: not interactive, opacity 0.85.
- [ ] No match at all → `No shortcuts match "{query}".`

#### 3.12 Vault Folders section

Title "Vault Folders"; desc composed: `Relative to the open vault. "Open Today's Daily Note" and "New Note from Template" live in the command palette (Cmd/Ctrl-Shift-P); templates may use {{date}}, {{time}}, {{title}} and {{date:YYYY-MM-DD}}. Pasted images go to <code>attachments/</code>.`
- [ ] **Daily notes folder** text → `dailyNotesFolder`.
- [ ] **Templates folder** text → `templatesFolder`; hint "A `daily.md` here seeds new daily notes."
- [ ] **External tool** text, placeholder `e.g. "Visual Studio Code" (empty = OS default)` → `externalTool`; hint `Used by "Open with…" in the file explorer's right-click menu.`
- [ ] All saved on every keystroke, no validation/trimming.

#### 3.13 About section

- [ ] Title "About"; desc `Fuchico {version} — a keyboard-first Markdown notes app with Helix editing, tasks and agenda.` (`version` from package.json, currently 0.1.0).
- [ ] Secondary button "Source & issues on GitHub" → opens `https://github.com/FulecoRafa/fuchico` in the system browser.

#### 3.14 Integrations section (CalDAV) — UI strings are hardcoded English (NOT translated)

Four sub-sections:

1. **"CalDAV Sync"** — desc "Link folders to a CalDAV calendar (e.g. iCloud Reminders) so checkbox tasks in your notes sync to your phone and back."
2. **"Accounts"** — list of saved accounts (row: bold username, muted server URL ellipsized, trash icon button title "Remove account" → `caldav_remove_account`, no confirm), then **AccountForm**:
   - [ ] **Server URL** (default `https://caldav.icloud.com`), **Username** (placeholder `you@icloud.com`), **Password** (masked).
   - [ ] If server URL contains `icloud.com`: hint "iCloud requires an app-specific password, not your Apple ID password." + link-button "Generate one at appleid.apple.com" → opens `https://appleid.apple.com/account/manage`.
   - [ ] Buttons: **Test Connection** (secondary; label "Testing…" while running) and **Save Account** ("Saving…"). Both disabled unless all three fields non-blank (trimmed); Test also disabled while testing, Save while saving.
   - [ ] Test → `caldav_test_connection {serverUrl(trimmed), username(trimmed), password(raw)}` → `{calendarHomeUrl}`; success: "Connected. Calendar home: {url}" (ok color); error: error string (destructive).
   - [ ] Save → `caldav_save_account {input: {serverUrl, username, password}}` → id; then refresh list; clears username+password (server URL kept) and test status; error shown in same status line.
   - [ ] Passwords are never held in list state; only forwarded to Rust (stored in OS keyring backend side).
3. **"Linked Folders"** — **FolderLinkTable**:
   - [ ] If no accounts: hint "Save an account above to link a folder."
   - [ ] **Account** select (options `username (serverUrl)`), auto-selects first account; changing it clears discovered calendars.
   - [ ] **Discover Calendars** button (refresh icon; "Discovering…") → `caldav_discover_calendars_for_account {accountId}` → `CalendarInfo[] {href, displayName}`; selects first.
   - [ ] **Calendar** select (only after discovery) — options by displayName, value href.
   - [ ] **Folder**: path text (ellipsized, title = full path; "No folder selected" if none) + "Choose…" button (folder-open icon) → native directory picker. **Defaults to the currently open vault root.**
   - [ ] **Link Folder** button, enabled when account+calendar+folder set → `caldav_link_folder {accountId, calendarHref, calendarDisplayName (falls back to href), folderPath}`; then refresh links. Rust creates the folder's `.fuchico-sync.json` sidecar if missing.
   - [ ] Errors shown as destructive status.
   - [ ] Links table (`caldav_list_links` → `{accountId, calendarHref, calendarDisplayName, folderPath}[]`): empty → "No folders linked yet."; row: folder basename (bold, title = full path) over calendar name (160px column); meta: `{taskCount} tasks[, {pendingDeletes} pending delete][ · synced {localeString(lastSyncedAt)}]` or "Never synced"; actions: refresh icon "Sync this folder now" → `caldav_sync_now {folderPath}`; unlink icon "Unlink folder" → `caldav_unlink_folder {folderPath}` (no confirm; drops config entry only, sidecar kept).
4. **"Sync"** — **Sync Now** button (refresh icon; "Syncing…" and disabled while running) → `caldav_sync_now {folderPath: null}` (all links) → `SyncReport[]`.
   - [ ] Error → destructive status. After a sync, log list: per report row `<folder basename>` (title full path) + `+{pushedNew} new, {pushedUpdated} pushed, {pulledUpdated} pulled, {pulledNew} from server[, {errors.length} error(s)]`, then each error string on its own line.
   - [ ] `SyncReport = {folder, pushedNew, pushedUpdated, pulledUpdated, pulledNew, pendingDeletes, confirmedDeletesRemote, deletedOnServer, touchedFiles: string[], errors: string[]}`.
   - [ ] Status table data: `caldav_get_sync_status` → `FolderSyncStatus[] {folderPath, calendarDisplayName, lastSyncedAt: string|null (ISO), taskCount, pendingDeletes}`; fetched on mount and **re-fetched on every `caldav:sync-complete` event**.
- [ ] Sync triggers (backend, for context): launch-time `sync_all`, then every **15 min** (`SYNC_POLL_INTERVAL`); on-save sync per linked folder debounced **2 s** (writes with `source: "caldav"` ignored to avoid loops); manual Sync Now. Events: `caldav:sync-progress`, `caldav:sync-complete`.
- [ ] Hook semantics: account/link list state `idle → loading → loaded|error`; refresh keeps showing "loaded" data while re-fetching.

#### 3.15 Shared settings styling (for visual parity)

| Element | Style |
|---|---|
| section | margin-bottom 28px; title 13px bold mb 8px; desc 12px muted, mb 12px |
| form | column, gap 10px; field column gap 4px; label 11px semibold muted |
| input | 1px `--border`, radius `--radius-sm`, bg `--background`, 13px, padding 6×8; focus border `--ring` |
| hint | 12px muted; status 12px mt 6px; ok = `--syntax-function`; error = `--destructive` |
| `.btn` | radius `--radius-xl`, bg `--primary`/fg `--primary-foreground`, 13px/500, padding 7×16, hover primary 80%, active translateY(1px); disabled opacity .5 |
| `.btn-secondary` | bg `--secondary`, fg `--secondary-foreground`; `.btn-danger` text `--destructive` |
| toggle button (`theme-palette-btn`) | border `--border`, radius-sm, bg background, 12px, padding 5×10; hover border ring; active bg/border `--primary`, fg primary-foreground; group wraps with gap 6px |
| icon button | 24×24, transparent, muted; hover fg + bg foreground 6% |
| rows (account/link/shortcut/sync log) | 1px border, radius-sm, padding 6–8px |
| preview card | border, radius-md; toolbar bg `--card` with three 8px `--muted` dots; body padding 14 gap 10; code 11px mono line-height 1.6 bg card |

### 4. Themes (`styles/globals.css`, `lib/useTheme.ts`, `lib/themeVariables.ts`)

#### 4.1 Mechanism

- [ ] Two independent axes on `<html>`: `data-palette` ∈ {ayu, dracula, catppuccin, custom} and `data-mode` ∈ {light, dark}. `useTheme()` (mounted once in main window and in secondary editor windows) sets them from settings; `mode:"system"` resolves via `matchMedia("(prefers-color-scheme: dark)")` and **live-updates on OS change**.
- [ ] Palette blocks match both `:root[...]` and any `[data-palette][data-mode]` element (so the settings preview card can render a different palette locally).
- [ ] **Dracula** has no `[data-mode]` qualifier → always dark regardless of mode.
- [ ] **Custom**: no built-in block for `data-palette="custom"`; variables come from (a) the base `:root` defaults (Ayu light) — or the `prefers-color-scheme` pre-JS fallback only while `data-mode` is absent — plus (b) the active custom theme's CSS injected into `<style id="fuchico-custom-theme">` as `:root[data-palette="custom"] {\n<css>\n}`. Tag removed when CSS is blank. A custom theme is **not** mode-aware (same values in light and dark) unless the user writes their own selectors.
- [ ] Pre-JS: bare `:root` = Ayu light; `@media (prefers-color-scheme: dark) { :root:not([data-mode]) {Ayu dark} }`. `color-scheme: light dark` on `:root`.
- [ ] Fonts: if `uiFont` non-blank set inline `--font-sans: "<uiFont>", var(--font-sans-fallback)` on `<html>`, else remove; same for `editorFont` → `--font-mono` with `--font-mono-fallback`. (Inline style wins over custom theme `--font-*` declarations.)
- [ ] Zoom: if `uiScale !== 1` set CSS `zoom: <uiScale>` on `<html>` else remove. Always set `--editor-font-size: <editorFontSize / uiScale>px` so editor text size is independent of UI zoom. Editor `.cm-scroller` uses `font-size: var(--editor-font-size, 13px)`, `line-height: 1.55`, `font-family: var(--font-mono)`.
- [ ] Palette commands: `Theme: Ayu|Dracula|Catppuccin|<custom name>` (select palette / custom theme) and `Toggle Theme (Light / Dark)` (alias `:theme`): `mode = (mode === "dark") ? "light" : "dark"` (from "system" → dark).

#### 4.2 Built-in themes

| Palette id | Display | Variants | Notes |
|---|---|---|---|
| `ayu` | Ayu | light, dark | default; values are oklch neutrals (shadcn "neutral/slate"-like) |
| `dracula` | Dracula | dark only | mode buttons disabled |
| `catppuccin` | Catppuccin | light = Latte, dark = Mocha | official hex palettes |
| `custom` | user names | n/a | from settings |

#### 4.3 Color variables per theme (exact values)

| Variable | Ayu light (also `:root` default) | Ayu dark (also pre-JS dark fallback) | Dracula | Catppuccin Latte (light) | Catppuccin Mocha (dark) |
|---|---|---|---|---|---|
| `--background` | `oklch(1 0 0)` | `oklch(0.148 0.004 228.8)` | `#282a36` | `#eff1f5` | `#1e1e2e` |
| `--foreground` | `oklch(0.148 0.004 228.8)` | `oklch(0.987 0.002 197.1)` | `#f8f8f2` | `#4c4f69` | `#cdd6f4` |
| `--card` | `oklch(1 0 0)` | `oklch(0.218 0.008 223.9)` | `#21222c` | `#e6e9ef` | `#181825` |
| `--card-foreground` | `oklch(0.148 0.004 228.8)` | `oklch(0.987 0.002 197.1)` | `#f8f8f2` | `#4c4f69` | `#cdd6f4` |
| `--primary` | `oklch(0.218 0.008 223.9)` | `oklch(0.925 0.005 214.3)` | `#bd93f9` | `#8839ef` | `#cba6f7` |
| `--primary-foreground` | `oklch(0.987 0.002 197.1)` | `oklch(0.218 0.008 223.9)` | `#282a36` | `#eff1f5` | `#1e1e2e` |
| `--secondary` | `oklch(0.963 0.002 197.1)` | `oklch(0.275 0.011 216.9)` | `#343746` | `#ccd0da` | `#313244` |
| `--secondary-foreground` | `oklch(0.218 0.008 223.9)` | `oklch(0.987 0.002 197.1)` | `#f8f8f2` | `#4c4f69` | `#cdd6f4` |
| `--muted` | `oklch(0.963 0.002 197.1)` | `oklch(0.275 0.011 216.9)` | `#343746` | `#ccd0da` | `#313244` |
| `--muted-foreground` | `oklch(0.56 0.021 213.5)` | `oklch(0.723 0.014 214.4)` | `#6272a4` | `#6c6f85` | `#a6adc8` |
| `--accent` | `oklch(0.963 0.002 197.1)` | `oklch(0.275 0.011 216.9)` | `#44475a` | `#bcc0cc` | `#45475a` |
| `--accent-foreground` | `oklch(0.218 0.008 223.9)` | `oklch(0.987 0.002 197.1)` | `#f8f8f2` | `#4c4f69` | `#cdd6f4` |
| `--destructive` | `oklch(0.577 0.245 27.325)` | `oklch(0.704 0.191 22.216)` | `#ff5555` | `#d20f39` | `#f38ba8` |
| `--border` | `oklch(0.925 0.005 214.3)` | `oklch(1 0 0 / 10%)` | `rgb(255 255 255 / 10%)` | `rgb(76 79 105 / 15%)` | `rgb(255 255 255 / 10%)` |
| `--input` | `oklch(0.925 0.005 214.3)` | `oklch(1 0 0 / 15%)` | `rgb(255 255 255 / 15%)` | `rgb(76 79 105 / 20%)` | `rgb(255 255 255 / 15%)` |
| `--ring` | `oklch(0.723 0.014 214.4)` | `oklch(0.56 0.021 213.5)` | `#bd93f9` | `#8839ef` | `#cba6f7` |
| `--syntax-keyword` | `oklch(0.5 0.19 305)` | `oklch(0.78 0.14 305)` | `#ff79c6` | `#8839ef` | `#cba6f7` |
| `--syntax-string` | `oklch(0.55 0.14 148)` | `oklch(0.78 0.13 148)` | `#f1fa8c` | `#40a02b` | `#a6e3a1` |
| `--syntax-number` | `oklch(0.62 0.16 55)` | `oklch(0.8 0.14 55)` | `#bd93f9` | `#fe640b` | `#fab387` |
| `--syntax-function` | `oklch(0.53 0.17 255)` | `oklch(0.78 0.13 255)` | `#50fa7b` | `#1e66f5` | `#89b4fa` |
| `--syntax-type` | `oklch(0.55 0.13 195)` | `oklch(0.78 0.11 195)` | `#8be9fd` | `#209fb5` | `#74c7ec` |
| `--syntax-comment` | `oklch(0.6 0.015 240)` | `oklch(0.62 0.015 240)` | `#6272a4` | `#9ca0b0` | `#6c7086` |

- [ ] GPUI has no oklch/color-mix natively: port must convert oklch → sRGB (and implement `color-mix(in oklch, A p%, transparent)` = A with alpha p, and `color-mix(in oklch, A p%, B)` = oklch interpolation) for both built-ins and user CSS.

#### 4.4 Non-color global variables (`:root`, all themes)

| Variable | Value |
|---|---|
| `--radius` | `0.625rem` (10px) |
| `--radius-sm` | `calc(var(--radius) * 0.6)` (6px) |
| `--radius-md` | `calc(var(--radius) * 0.8)` (8px) |
| `--radius-lg` | `var(--radius)` (10px) |
| `--radius-xl` | `calc(var(--radius) * 1.4)` (14px) |
| `--dur-fast` | `160ms` |
| `--dur-base` | `240ms` |
| `--ease-premium` | `cubic-bezier(0.16, 1, 0.3, 1)` |
| `--font-sans-fallback` | `"Inter Variable", -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif` |
| `--font-sans` | `var(--font-sans-fallback)` (overridden by `uiFont`) |
| `--font-mono-fallback` | `"JetBrains Mono", ui-monospace, SFMono-Regular, monospace` |
| `--font-mono` | `var(--font-mono-fallback)` (overridden by `editorFont`) |
| `--editor-font-size` | set at runtime: `editorFontSize / uiScale` px (fallback 13px) |

Derived (scoped to `.mermaid-canvas-content`, so diagrams follow the theme live): `--pele-bg`=background, `--pele-fg`=foreground, `--pele-muted`=muted-foreground, `--pele-line`=muted-foreground, `--pele-surface`=card, `--pele-surface-alt`=secondary, `--pele-border`=ring, `--pele-accent`=syntax-function, `--pele-error`=destructive, `--pele-critical`=destructive, `--pele-font`=font-sans, `--pele-font-mono`=font-mono, `--pele-radius`=radius-sm, `--pele-series-1..8` = syntax-function, syntax-string, syntax-number, syntax-keyword, syntax-type, destructive, syntax-comment, primary.

#### 4.5 Customizable variables (`THEME_VARIABLES`, editor autocomplete list)

25 entries (name → kind → en description): `--background` color "App background"; `--foreground` color "Default text color"; `--card` color "Panels, sidebars, status bar"; `--card-foreground` color "Text on panels"; `--primary` color "Primary buttons and selection"; `--primary-foreground` color "Text on primary"; `--secondary` color "Secondary buttons and chips"; `--secondary-foreground` color "Text on secondary"; `--muted` color "Subtle backgrounds"; `--muted-foreground` color "Secondary/dimmed text"; `--accent` color "Hover and active highlights"; `--accent-foreground` color "Text on accent"; `--destructive` color "Errors and destructive actions"; `--border` color "Borders and separators"; `--input` color "Input borders"; `--ring` color "Focus ring"; `--syntax-keyword` color "Syntax: keywords, headings"; `--syntax-string` color "Syntax: strings"; `--syntax-number` color "Syntax: numbers"; `--syntax-function` color "Syntax: functions, links"; `--syntax-type` color "Syntax: types, tags"; `--syntax-comment` color "Syntax: comments"; `--radius` length "Corner radius base (e.g. 0.625rem)"; `--font-sans` font "UI font family"; `--font-mono` font "Editor font family".

- [ ] "New theme" snapshot includes only the 22 color variables.
- [ ] Since custom CSS is injected verbatim, users can technically write *any* CSS there (including nested rules); port should at minimum support `--var: value;` declarations for the 25 variables (+ `--radius-*` derivation from `--radius`), and ideally ignore/flag unknown content.

#### 4.6 Global typography and base styles

- [ ] Fonts bundled: **Inter Variable** (`@fontsource-variable/inter`), **JetBrains Mono** 400 and 700 (`@fontsource/jetbrains-mono`). Port must bundle these.
- [ ] `body`: bg `--background`, color `--foreground`, `font-family: var(--font-sans)`; html/body/#root height 100%, margin 0; `box-sizing: border-box` everywhere; buttons/inputs inherit font. No global base font-size (browser default 16px), but nearly all UI text is explicitly 10–13px: section titles 13px, body/labels 11–13px, chips 10–12px, editor 13px default.
- [ ] Mono font used for: editor, search result text, shortcut chips, theme CSS editor, code previews, agenda times, backlinks line numbers.
- [ ] Common translucency idioms: hover bg = foreground 5–6%; panels = card 85%; overlay backdrops black 25%; shadows `0 12px 32px black 30%` (palette/outline), `0 8px 24px oklch(0 0 0/0.18)` (context menu).
- [ ] Transitions use `--dur-fast`/`--dur-base` with `--ease-premium`.
- [ ] `src/App.css` is the stale Tauri template stylesheet and is **not imported anywhere** — nothing to port.

### 5. i18n (`src/lib/i18n.ts`)

#### 5.1 Mechanism

- [ ] Supported locales: `en` (source of truth) and `pt-BR`. Setting values: `system | en | pt-BR`.
- [ ] Locale resolution: explicit value passes through; `"system"` → `navigator.language` lowercased starts with `pt` → `pt-BR`, everything else → `en`. Port: use OS locale (e.g. `sys-locale`), same `pt*` rule.
- [ ] Storage: the `language` field of the settings blob (no separate key). Changing it re-renders all subscribers (shares the settings store subscription), and propagates to other windows.
- [ ] Lookup `t(key, params?)`: `catalog[locale][key] ?? en[key]` (fallback to English). Interpolation: for each param, replace **all** occurrences of literal `{name}` with `String(value)` (no escaping, no nested formats, unknown placeholders left as-is).
- [ ] Plural helper `tn(n, oneKey, manyKey)`: `n === 1` → one key else many, with `{n}` filled.
- [ ] `t` is callable outside UI (menus, command builders, confirm dialogs) and reads the current locale at call time; components subscribe for re-render.
- [ ] Keys are a typed union (`MessageKey = keyof typeof en`); pt-BR typed `Record<MessageKey,string>` → compile-time completeness.
- [ ] Tests to port: pt-BR has exactly en's key set; no empty (whitespace-only) message in either; pt-BR preserves the multiset of `{param}` placeholders per key; `t("search.line",{n:7})` = `line 7`; `t("explorer.confirmDelete",{name:'"a.md"'})` = `Delete "a.md"?`; `tn(1,"count.fileOne","count.fileMany")` = `1 file`, `tn(3,…)` = `3 files`; `resolveLocale` passes en/pt-BR and maps system to a supported locale.
- [ ] **Untranslated areas** (hardcoded English): entire Integrations/CalDAV UI; palette names (Ayu/Dracula/Catppuccin), Helix/Vim labels, language endonyms; fixed-shortcut raw key strings.

#### 5.2 pt-BR structural differences (where word order/splitting matters)

- `agenda.weekdayLetters`: en `S,M,T,W,T,F,S` vs pt-BR `D,S,T,Q,Q,S,S` (comma-separated, Sunday first; split on `,`).
- `tags.noTags*` fragments: en "No tags yet. Use" `#tag` "in a note or a frontmatter" `tags:` "list."; pt-BR "Ainda não há tags. Use" … "em uma nota ou uma lista" … "no frontmatter." — fragments are concatenated around code spans, so the port must keep the 3-part split.
- `settings.vault.templatesHintPrefix`: en "A" vs pt-BR "Um arquivo" (around `daily.md`).
- `settings.theme.customCssHintSuffix` pt-BR begins "do tema." after the `[data-palette="custom"]` code span.
- `settings.folding.eg`: "e.g." vs "ex.:"; `settings.preview.pangram` pt-BR: "Um pequeno jabuti xereta viu dez cegonhas felizes."
- Other split-sentence keys rendered around inline code: `settings.vault.descPrefix/and/descImages`, `agenda.noRoutinesPrefix/Suffix`, `agenda.noTasksPrefix/or/noTasksSuffix`, `settings.theme.customCssHintPrefix/Suffix`.

#### 5.3 Full English catalog (300 keys)

Format: `key` — en string. (pt-BR has the identical key set.)


**`common.*`** (13)

- `common.loading`: "Loading…"
- `common.scanning`: "Scanning…"
- `common.close`: "Close"
- `common.open`: "Open"
- `common.default`: "Default"
- `common.preview`: "Preview"
- `common.apply`: "Apply"
- `common.applied`: "Applied"
- `common.rename`: "Rename"
- `common.delete`: "Delete"
- `common.copyPath`: "Copy path"
- `common.revealInFileManager`: "Reveal in file manager"
- `common.openFolder`: "Open Folder"

**`count.*`** (4)

- `count.fileOne`: "{n} file"
- `count.fileMany`: "{n} files"
- `count.occurrenceOne`: "{n} occurrence"
- `count.occurrenceMany`: "{n} occurrences"

**`app.*`** (7)

- `app.files`: "Files"
- `app.tasksCalendar`: "Tasks & Calendar"
- `app.search`: "Search"
- `app.tags`: "Tags"
- `app.settings`: "Settings"
- `app.noFileOpen`: "No file open"
- `app.diagram`: "Diagram"

**`tabs.*`** (4)

- `tabs.close`: "Close"
- `tabs.closeOthers`: "Close others"
- `tabs.closeAll`: "Close all"
- `tabs.openInNewWindow`: "Open in new window"

**`explorer.*`** (18)

- `explorer.noFolderOpen`: "No folder open"
- `explorer.newFile`: "New file"
- `explorer.newFolder`: "New folder"
- `explorer.newDrawing`: "New drawing"
- `explorer.openFolderAction`: "Open folder…"
- `explorer.refresh`: "Refresh"
- `explorer.cut`: "Cut"
- `explorer.cutItems`: "Cut {count} items"
- `explorer.pasteHere`: "Paste {name} here"
- `explorer.openWithDefault`: "Open with default app"
- `explorer.openWith`: "Open with {tool}"
- `explorer.copyPaths`: "Copy {count} paths"
- `explorer.deleteItems`: "Delete {count} items"
- `explorer.confirmDelete`: "Delete {name}?"
- `explorer.itemsCount`: "{count} items"
- `explorer.importFailed`: "Import failed: {errors}"
- `explorer.importedOne`: "Imported 1 file into {folder}"
- `explorer.importedMany`: "Imported {count} files into {folder}"

**`palette.*`** (7)

- `palette.runCommand`: "Run a command…"
- `palette.noMatchingCommands`: "No matching commands"
- `palette.goToFile`: "Go to file…"
- `palette.openFolderFirst`: "Open a folder first"
- `palette.scanningVault`: "Scanning vault…"
- `palette.cantReadVault`: "Couldn't read vault: {message}"
- `palette.noFilesFound`: "No files found"

**`command.*`** (23)

- `command.goToEditor`: "Go to Editor"
- `command.goToAgenda`: "Go to Tasks & Calendar"
- `command.goToSearch`: "Go to Search"
- `command.goToSettings`: "Go to Settings"
- `command.editorAction`: "Editor: {name}"
- `command.settingsSection`: "Settings: {name}"
- `command.openFolder`: "Open Folder…"
- `command.quickOpen`: "Quick Open: Go to File…"
- `command.closeActiveTab`: "Close Active Tab"
- `command.closeAllTabs`: "Close All Tabs"
- `command.openDailyNote`: "Open Today's Daily Note"
- `command.newFromTemplate`: "New Note from Template: {name}"
- `command.keyboardShortcuts`: "Keyboard Shortcuts"
- `command.openInNewWindow`: "Open in New Window"
- `command.toggleTheme`: "Toggle Theme (Light / Dark)"
- `command.saveFile`: "Save File"
- `command.saveAndCloseTab`: "Save and Close Tab"
- `command.showBacklinks`: "Show Backlinks"
- `command.hideBacklinks`: "Hide Backlinks"
- `command.exportHtml`: "Export as HTML…"
- `command.exportPdf`: "Export as PDF (Print)…"
- `command.goToLine`: "Go to Line {n}"
- `command.selectTheme`: "Theme: {name}"

**`backlinks.*`** (3)

- `backlinks.title`: "Backlinks"
- `backlinks.noFile`: "Open a note to see which notes link to it."
- `backlinks.empty`: "No notes link to \"{name}\" yet."

**`export.*`** (4)

- `export.saved`: "Exported to {path}"
- `export.failed`: "Export failed: {message}"
- `export.print`: "Print / Save as PDF"
- `export.printHint`: "Use the print dialog to save this note as a PDF."

**`search.*`** (16)

- `search.openFolderToSearch`: "Open a folder to search"
- `search.placeholder`: "Search files…"
- `search.findReplaceTitle`: "Find and replace across the vault"
- `search.replaceWith`: "Replace with…"
- `search.replaceAll`: "Replace all"
- `search.searching`: "Searching…"
- `search.typeToSearch`: "Type to search across files."
- `search.noMatches`: "No matches."
- `search.line`: "line {n}"
- `search.scopeInFile`: "in {file}"
- `search.scopeAcrossFiles`: "across {files}"
- `search.confirmReplace`: "Replace every \"{query}\" with \"{replacement}\" {scope}? This cannot be undone."
- `search.replaced`: "Replaced {occurrences} in {files}."
- `search.replaceFailed`: "Replace failed: {error}"
- `search.replaceAllInFile`: "Replace all in {file}"
- `search.replaceInThisFile`: "Replace in this file…"

**`agenda.*`** (16)

- `agenda.openFolder`: "Open a folder to see tasks and events"
- `agenda.emptyItem`: "(empty)"
- `agenda.recurringTask`: "Recurring task"
- `agenda.routines`: "Routines"
- `agenda.noRoutinesPrefix`: "No routines yet. Add"
- `agenda.noRoutinesSuffix`: "to a task."
- `agenda.noTasksPrefix`: "No tasks yet. Use"
- `agenda.or`: "or"
- `agenda.noTasksSuffix`: "in your notes."
- `agenda.overdue`: "Overdue"
- `agenda.today`: "Today"
- `agenda.upcoming`: "Upcoming"
- `agenda.noDate`: "No date"
- `agenda.nothingHere`: "Nothing here."
- `agenda.clearFilter`: "Clear filter ({date})"
- `agenda.weekdayLetters`: "S,M,T,W,T,F,S"

**`tags.*`** (5)

- `tags.openFolder`: "Open a folder to see tags"
- `tags.noTagsPrefix`: "No tags yet. Use"
- `tags.noTagsMid`: "in a note or a frontmatter"
- `tags.noTagsSuffix`: "list."
- `tags.selectTag`: "Select a tag to see its notes."

**`settingsNav.*`** (6)

- `settingsNav.appearance`: "Appearance"
- `settingsNav.editor`: "Editor"
- `settingsNav.shortcuts`: "Keyboard Shortcuts"
- `settingsNav.vault`: "Vault"
- `settingsNav.integrations`: "Integrations"
- `settingsNav.about`: "About"

**`settings.*`** (3)

- `settings.searchPlaceholder`: "Search settings…"
- `settings.noResults`: "No settings match \"{query}\"."
- `settings.sectionsAria`: "Settings sections"

**`settings.language.*`** (4)

- `settings.language.title`: "Language"
- `settings.language.desc`: "Language of the app interface. \"System\" follows your operating system language."
- `settings.language.label`: "Language"
- `settings.language.system`: "System"

**`settings.theme.*`** (23)

- `settings.theme.title`: "Theme"
- `settings.theme.desc`: "A theme is a color choice, independent of light/dark mode. Each palette may support a light and/or dark variant."
- `settings.theme.palette`: "Palette"
- `settings.theme.paletteCustom`: "Custom"
- `settings.theme.mode`: "Mode"
- `settings.theme.modeSystem`: "System"
- `settings.theme.modeLight`: "Light"
- `settings.theme.modeDark`: "Dark"
- `settings.theme.draculaHint`: "Dracula only ships a dark palette, so mode is fixed to dark."
- `settings.theme.customCss`: "Custom CSS variables"
- `settings.theme.customCssHintPrefix`: "Declarations are injected as-is into a"
- `settings.theme.customCssHintSuffix`: "rule. Preview updates as you type; click Apply to use it across the whole app."
- `settings.theme.autocompleteHint`: "Type -- to autocomplete the app's theme variables."
- `settings.theme.newTheme`: "New theme"
- `settings.theme.newThemeName`: "My theme"
- `settings.theme.importTheme`: "Import…"
- `settings.theme.exportTheme`: "Export…"
- `settings.theme.duplicate`: "Duplicate"
- `settings.theme.themeName`: "Theme name"
- `settings.theme.deleteConfirm`: "Delete the theme \"{name}\"?"
- `settings.theme.exported`: "Theme exported."
- `settings.theme.imported`: "Theme imported."
- `settings.theme.importFailed`: "Couldn't read that file as a theme."

**`themeVar.*`** (25)

- `themeVar.background`: "App background"
- `themeVar.foreground`: "Default text color"
- `themeVar.card`: "Panels, sidebars, status bar"
- `themeVar.cardForeground`: "Text on panels"
- `themeVar.primary`: "Primary buttons and selection"
- `themeVar.primaryForeground`: "Text on primary"
- `themeVar.secondary`: "Secondary buttons and chips"
- `themeVar.secondaryForeground`: "Text on secondary"
- `themeVar.muted`: "Subtle backgrounds"
- `themeVar.mutedForeground`: "Secondary/dimmed text"
- `themeVar.accent`: "Hover and active highlights"
- `themeVar.accentForeground`: "Text on accent"
- `themeVar.destructive`: "Errors and destructive actions"
- `themeVar.border`: "Borders and separators"
- `themeVar.input`: "Input borders"
- `themeVar.ring`: "Focus ring"
- `themeVar.syntaxKeyword`: "Syntax: keywords, headings"
- `themeVar.syntaxString`: "Syntax: strings"
- `themeVar.syntaxNumber`: "Syntax: numbers"
- `themeVar.syntaxFunction`: "Syntax: functions, links"
- `themeVar.syntaxType`: "Syntax: types, tags"
- `themeVar.syntaxComment`: "Syntax: comments"
- `themeVar.radius`: "Corner radius base (e.g. 0.625rem)"
- `themeVar.fontSans`: "UI font family"
- `themeVar.fontMono`: "Editor font family"

**`settings.preview.*`** (3)

- `settings.preview.primary`: "Primary"
- `settings.preview.secondary`: "Secondary"
- `settings.preview.pangram`: "The quick brown fox jumps over the lazy dog."

**`settings.font.*`** (10)

- `settings.font.title`: "Font"
- `settings.font.desc`: "Choose fonts installed on this machine for the app's interface and for the editor."
- `settings.font.uiFont`: "UI font"
- `settings.font.editorFont`: "Editor font"
- `settings.font.loading`: "Loading system fonts…"
- `settings.font.error`: "Couldn't list system fonts: {message}"
- `settings.font.editorFontSize`: "Editor font size"
- `settings.font.editorFontSizeHint`: "Also Cmd/Ctrl +, − and 0 to reset."
- `settings.font.uiZoom`: "UI zoom"
- `settings.font.uiZoomHint`: "Percent. Also Cmd/Ctrl Shift +, − and 0 to reset."

**`settings.keybindings.*`** (6)

- `settings.keybindings.title`: "Keybindings"
- `settings.keybindings.desc`: "Choose how the editor interprets keystrokes. Takes effect immediately in any open file."
- `settings.keybindings.helixDesc`: "Modal editing, Helix-style selection-first commands."
- `settings.keybindings.vimDesc`: "Modal editing, Vim keybindings."
- `settings.keybindings.normalLabel`: "Normal"
- `settings.keybindings.normalDesc`: "Standard text-editor keybindings, no modes."

**`settings.behavior.*`** (12)

- `settings.behavior.title`: "Editor Behavior"
- `settings.behavior.desc`: "Gutter and indentation preferences for the code editor."
- `settings.behavior.relativeLineNumbers`: "Relative line numbers"
- `settings.behavior.absolute`: "Absolute"
- `settings.behavior.absoluteTitle`: "Every line shows its absolute number."
- `settings.behavior.relative`: "Relative"
- `settings.behavior.relativeTitle`: "The current line shows its absolute number; every other line shows its distance from the cursor."
- `settings.behavior.relativeHint`: "Helix/Vim-style: handy for jumping N lines with motion commands."
- `settings.behavior.tabSize`: "Tab size"
- `settings.behavior.tabSizeHint`: "Spaces per indent level and per tab stop."
- `settings.behavior.rulers`: "Vertical rulers"
- `settings.behavior.rulersHint`: "Comma-separated columns where a guide line is drawn (e.g. 80, 120). Leave empty for none."

**`settings.folding.*`** (9)

- `settings.folding.title`: "Fold Regions"
- `settings.folding.desc`: "Wrap any lines between a start and end marker to make them foldable — this is an app-specific convention, not part of Markdown. The rest of the start-marker line becomes the region's name."
- `settings.folding.startMarker`: "Start marker"
- `settings.folding.endMarker`: "End marker"
- `settings.folding.eg`: "e.g."
- `settings.folding.regionName`: "Region name"
- `settings.folding.expanded`: "Expanded"
- `settings.folding.collapsed`: "Collapsed"
- `settings.folding.linesExample`: "3 lines"

**`settings.vault.*`** (11)

- `settings.vault.title`: "Vault Folders"
- `settings.vault.descPrefix`: "Relative to the open vault. \"Open Today's Daily Note\" and \"New Note from Template\" live in the command palette (Cmd/Ctrl-Shift-P); templates may use"
- `settings.vault.and`: "and"
- `settings.vault.descImages`: ". Pasted images go to"
- `settings.vault.dailyNotesFolder`: "Daily notes folder"
- `settings.vault.templatesFolder`: "Templates folder"
- `settings.vault.templatesHintPrefix`: "A"
- `settings.vault.templatesHintSuffix`: "here seeds new daily notes."
- `settings.vault.externalTool`: "External tool"
- `settings.vault.externalToolPlaceholder`: "e.g. \"Visual Studio Code\" (empty = OS default)"
- `settings.vault.externalToolHint`: "Used by \"Open with…\" in the file explorer's right-click menu."

**`settings.about.*`** (3)

- `settings.about.title`: "About"
- `settings.about.desc`: "Fuchico {version} — a keyboard-first Markdown notes app with Helix editing, tasks and agenda."
- `settings.about.github`: "Source & issues on GitHub"

**`shortcuts.*`** (6)

- `shortcuts.title`: "Keyboard Shortcuts"
- `shortcuts.desc`: "Everything the keyboard can do, in one place. Editor actions are rebindable: click a binding, then press the new key combination."
- `shortcuts.searchPlaceholder`: "Search shortcuts…"
- `shortcuts.editorRebindable`: "Editor (rebindable)"
- `shortcuts.pressKeys`: "Press keys…"
- `shortcuts.noMatch`: "No shortcuts match \"{query}\"."

**`shortcuts.group.*`** (4)

- `shortcuts.group.global`: "Global"
- `shortcuts.group.editor`: "Editor"
- `shortcuts.group.explorer`: "File explorer"
- `shortcuts.group.tabs`: "Tabs"

**`shortcuts.fixed.*`** (37)

- `shortcuts.fixed.commandPalette.label`: "Command palette"
- `shortcuts.fixed.commandPalette.desc`: "Run any app command by name."
- `shortcuts.fixed.quickOpen.label`: "Quick open file"
- `shortcuts.fixed.quickOpen.desc`: "Fuzzy-find a note in the vault."
- `shortcuts.fixed.editorFontSize.label`: "Editor font size"
- `shortcuts.fixed.editorFontSize.desc`: "Bigger / smaller / reset editor text."
- `shortcuts.fixed.uiZoom.label`: "UI zoom"
- `shortcuts.fixed.uiZoom.desc`: "Zoom the whole app in / out / reset."
- `shortcuts.fixed.helixPalette.label`: "Command palette from Helix"
- `shortcuts.fixed.helixPalette.desc`: "In Helix normal mode, : opens the command palette; :q, :w, :wq and :<line> work as aliases."
- `shortcuts.fixed.save.label`: "Save"
- `shortcuts.fixed.save.desc`: "Write the current file to disk."
- `shortcuts.fixed.findReplace.label`: "Find / replace"
- `shortcuts.fixed.findReplace.desc`: "Search within the current file."
- `shortcuts.fixed.taskAutocomplete.label`: "Task date / repeat autocomplete"
- `shortcuts.fixed.taskAutocomplete.desc`: "Type @due, @today, @repeat (or 📅 / 🔁) on a task line to pick a date or rule."
- `shortcuts.fixed.pasteImage.label`: "Paste image"
- `shortcuts.fixed.pasteImage.desc`: "Paste or drop an image to save it under attachments/ and link it."
- `shortcuts.fixed.followLink.label`: "Follow link"
- `shortcuts.fixed.followLink.desc`: "Open the [[wikilink]] or Markdown link under the cursor."
- `shortcuts.fixed.followLink.keys`: "Click"
- `shortcuts.fixed.explorerNavigate.label`: "Navigate"
- `shortcuts.fixed.explorerNavigate.desc`: "Move selection, expand/collapse folders."
- `shortcuts.fixed.explorerOpen.label`: "Open / toggle folder"
- `shortcuts.fixed.explorerOpen.desc`: "Open the selected file or expand/collapse the folder."
- `shortcuts.fixed.explorerRename.label`: "Rename"
- `shortcuts.fixed.explorerRename.desc`: "Rename the selected entry."
- `shortcuts.fixed.explorerDelete.label`: "Delete"
- `shortcuts.fixed.explorerDelete.desc`: "Delete the selected entry (asks for confirmation)."
- `shortcuts.fixed.explorerTypeAhead.label`: "Type-ahead"
- `shortcuts.fixed.explorerTypeAhead.desc`: "Type a name prefix to jump to the matching entry."
- `shortcuts.fixed.explorerMenu.label`: "Context menu"
- `shortcuts.fixed.explorerMenu.desc`: "New file/folder, rename, delete."
- `shortcuts.fixed.explorerMenu.keys`: "Right-click"
- `shortcuts.fixed.tabMenu.label`: "Tab menu"
- `shortcuts.fixed.tabMenu.desc`: "Close, close others, close all."
- `shortcuts.fixed.tabMenu.keys`: "Right-click"

**`shortcutAction.*`** (18)

- `shortcutAction.openOutline.label`: "Go to header"
- `shortcutAction.openOutline.desc`: "Open the document outline (fuzzy-searchable header list)."
- `shortcutAction.toggleCheckboxAtCursor.label`: "Toggle checkbox"
- `shortcutAction.toggleCheckboxAtCursor.desc`: "Mark/unmark the checkbox on the cursor's line."
- `shortcutAction.insertDate.label`: "Insert date"
- `shortcutAction.insertDate.desc`: "Insert today's date at the cursor."
- `shortcutAction.insertDateTime.label`: "Insert date & time"
- `shortcutAction.insertDateTime.desc`: "Insert the current date and time at the cursor."
- `shortcutAction.insertRegion.label`: "Insert fold region"
- `shortcutAction.insertRegion.desc`: "Wrap the selected lines in a foldable region (or insert an empty one at the cursor)."
- `shortcutAction.insertTable.label`: "Insert table"
- `shortcutAction.insertTable.desc`: "Insert a 2x2 Markdown table at the cursor and start editing the header."
- `shortcutAction.toggleTaskLine.label`: "Toggle task"
- `shortcutAction.toggleTaskLine.desc`: "Turn the current line into a `- [ ]` task (or back into plain text)."
- `shortcutAction.pickDueDate.label`: "Set due date…"
- `shortcutAction.pickDueDate.desc`: "Add or change the 📅 due date on the current line via a quick-pick list."
- `shortcutAction.pickRecurrence.label`: "Set recurrence…"
- `shortcutAction.pickRecurrence.desc`: "Add or change the 🔁 repeat rule on the current line via a quick-pick list."


### 6. README-documented settings and discrepancies

- [ ] README "Customization" claims: theme selection; full keybinding mode switch (Helix/Vim/Normal); custom keyboard shortcuts (rebind editor actions); outline overlay + fold-region marker configuration; system font picker (font-kit). All present as described above.
- [ ] README CalDAV claims: accounts (iCloud, Nextcloud, Fastmail — any CalDAV server), calendar discovery, two-way folder ↔ calendar links, per-folder sync status, "Sync now"; credentials in the OS keyring. Matches §3.14.
- [ ] **Discrepancy:** README describes region folding with HTML-comment-style markers (`<!-- fold -->`-like); actual defaults are `:::fold` / `:::endfold` (configurable). Port should follow the code.
- [ ] **Discrepancy:** CalDAV UI is not localized although the rest of Settings is; decide whether parity means "English-only" (as today) or add keys.
- [ ] No settings import/export, no "reset to defaults", no per-vault settings: all settings are global per user (port: same).


---

## 6. Backend (Rust core: `src-tauri/src/**`, Cargo.toml, tauri.conf.json, capabilities)

This section covers everything the Tauri Rust side does. In the GPUI port, the "commands" stop being IPC endpoints and become plain Rust functions the UI calls. Their **semantics** (sorting, limits, error strings, event side effects) are what parity means here. Each IPC event below becomes an internal app event/bus message with the same trigger and payload meaning.

Source files covered (all read in full): `lib.rs` (152 lines), `main.rs`, `modules/mod.rs`, `open_files.rs`, `fonts/mod.rs`, `fs/{mod,file,mutate,tree}.rs`, `search/mod.rs`, `tags/mod.rs`, `links/mod.rs`, `tasks/mod.rs`, `caldav/{mod,client,commands,config,ical,matcher,sidecar,sync}.rs`.

### Global conventions

- [ ] **No path validation / vault-escape protection exists anywhere.** Every command takes an absolute path string from the UI and acts on it directly. Nothing checks that a path is inside the open vault (root). The port may add protection, but the current behavior is "trust the caller".
- [ ] All fallible commands return `Result<T, String>`. Errors reach JS as the plain string, usually `std::io::Error::to_string()` (e.g. `"No such file or directory (os error 2)"`), or one of the custom strings quoted below.
- [ ] **Path canonical form (`fs::to_canon`)**: `to_string_lossy()`. Backslashes are left alone on Unix. On Windows, a `\\?\UNC\server\share` prefix becomes `//server/share`, a `\\?\` prefix is stripped, and every `\` becomes `/` (`\\?\C:\x` -> `C:/x`, `C:\Users\foo` -> `C:/Users/foo`). It does **not** touch the filesystem (no symlink resolution); only `fs_canonicalize` does that.
- [ ] **Shared vault walker** (copied separately in tree.rs, search, tags, links, tasks, caldav/sync): recursive `read_dir`, with these rules:
  - skip **any** entry (file or dir) whose name starts with `.`;
  - skip dirs named exactly one of `SKIP_DIRS = ["node_modules", "target", "dist", ".git"]`;
  - use `entry.file_type()`, which does **not** follow symlinks, so symlinked files and dirs are skipped entirely (neither `is_dir` nor `is_file`);
  - silently ignore unreadable dirs and entries;
  - "Markdown" means the extension equals `md` or `markdown`, ASCII case-insensitive (search walks **all** files instead).
- [ ] File reads in scanners use `read_to_string`. Files that are not valid UTF-8 are skipped silently.
- [ ] Line splitting everywhere uses Rust `str::lines()`, which splits on `\n` and strips one trailing `\r` (so CRLF works). Line numbers are **1-based**.
- [ ] **Atomic write helper `write_atomic(target, bytes)`**:
  1. create a `tempfile::NamedTempFile` in the target's **parent dir** (O_EXCL, random name, which blocks pre-staged symlink attacks);
  2. `write_all`, then `sync_all` (fsync);
  3. `persist` (rename over the target).
  - It fails with `InvalidInput "path has no parent"` if the target has no parent.
  - It does **not** create parent dirs.
  - The temp file is created with mode **0600** on Unix. Only callers that restore permissions afterwards keep the old mode (`fs_write_file`, `search_replace_files`). Brand-new files, and files written by `tasks_toggle`, `fs_write_binary`, CalDAV `apply_file_edits`/`Synced Tasks.md`, the sidecar and config, all end up **0600**.
  - If the target is a symlink, the rename replaces the link itself with a regular file (the link is broken, and its target is not written).
- [ ] Logging: `log::debug!` on most IO failures and `log::warn!` on write/delete failures, via `tauri-plugin-log` (see App setup).

### 1. Command table (every registered command)

Registered in `lib.rs` `generate_handler!` (31 commands). JS arg names are camelCase (Tauri converts snake_case parameters by default). "Callers" lists the frontend files that call each command.

| # | Command | JS args (type) | Returns (JSON) | Behavior / errors / edge cases | Frontend callers |
|---|---|---|---|---|---|
| 1 | `fs_read_file` | `path: string` | `ReadResult` (see below) | `metadata(path)` follows symlinks; stat error -> Err. If `size > 10*1024*1024` (`MAX_READ_BYTES` = 10 MiB), returns `toolarge` **without reading**; there is no "open anyway" override argument. Otherwise reads all bytes. If any NUL byte appears in the first `BINARY_SNIFF_BYTES` = 8192 bytes, returns `binary`. If `String::from_utf8` fails, returns `binary`. Otherwise returns `text`. No BOM stripping and no encoding detection (UTF-16 is reported as binary via NULs). | editor/lib/useDocument.ts (x2), editor/lib/headingCompletion.ts, excalidraw/lib/useExcalidrawDocument.ts, palette/lib/templates.ts, settings/ThemeSection.tsx |
| 2 | `fs_write_file` | `path: string, content: string, source?: string \| null` | `null` | Records the original permissions if the file exists, then calls `write_atomic(path, utf8 bytes)` and restores those permissions (ignoring errors). Emits **`fs:file-written`** `{path, source?}`: `path` is the string exactly as passed (not canonicalized), and `source` is omitted when None. The emit happens only on success. It does not create parent dirs (missing parent -> Err). New files get 0600. Known `source` values from the UI: `"editor"`, `"template"`, `"theme-export"`, `"export"`. | editor/lib/useDocument.ts (`source:"editor"`), excalidraw/lib/useExcalidrawDocument.ts (`"editor"`), palette/lib/templates.ts (`"template"`), settings/ThemeSection.tsx (`"theme-export"`), export/lib/exportActions.ts (`"export"`) |
| 3 | `fs_write_binary` | `path: string, dataBase64: string` | `null` | If the path exists, returns Err `"already exists: {path}"` (never clobbers). Runs `create_dir_all(parent)`, then decodes base64 with the STANDARD alphabet (padding required); a bad payload gives Err `"invalid base64: {e}"`. Then `write_atomic`. **No event is emitted.** Mode 0600. Used for pasted/dropped images and imported files. | editor/lib/imageAttachments.ts (x2), explorer/lib/importFiles.ts (x2) |
| 4 | `fs_stat` | `path: string` | `FileStat {size:u64, mtime:u64, kind:"file"\|"dir"\|"symlink"}` | Uses `metadata` (follows symlinks), so `symlink` is effectively never returned. `mtime` is ms since the epoch, or 0 if unavailable. Missing path -> Err. The UI uses it as an existence check. | App.tsx (validate cached last vault root on startup), palette/lib/templates.ts |
| 5 | `fs_canonicalize` | `path: string` | `string` | `std::fs::canonicalize` (resolves symlinks; the path must exist), then `to_canon`. | **not called by the frontend** (registered only) |
| 6 | `fs_read_dir` | `path: string, showHidden: bool` | `DirEntry[]` | Immediate children only. Details in §3. Errors from `read_dir(root)` -> Err. | explorer/lib/useFileTree.ts (`showHidden:false`), palette/lib/useVaultFiles.ts (`showHidden:false`, recursive walk in JS that skips symlinks) |
| 7 | `fs_list_markdown_files` | `root: string` | `string[]` | Recursive Markdown walk (shared walker). Sorted by `PathBuf` order (component-wise byte order), then `to_canon`. Returns absolute paths. A missing root gives `[]`, not Err. | editor/lib/useVaultFiles.ts (wikilink resolver/autocomplete; re-run on the `fs:changed` event) |
| 8 | `fs_create_file` | `path: string` | `null` | If `exists()`, Err `"already exists: {path}"`. Otherwise `std::fs::write(path, "")`: not atomic, default umask perms, parent must exist. No event. | explorer/lib/useFileTree.ts (UI appends `.{defaultExt}` when the name has no `.`) |
| 9 | `fs_create_dir` | `path: string` | `null` | If `exists()`, Err `"already exists: {path}"`. Otherwise `create_dir_all`, so typing `a/b/c` creates the whole chain. No event. | explorer/lib/useFileTree.ts, palette/lib/templates.ts |
| 10 | `fs_rename` | `from: string, to: string` | `null` | If `from` is missing, Err `"not found: {from}"`. If `to.exists()`, Err `"already exists: {to}"`, so overwriting is never allowed. Otherwise `std::fs::rename` (fails across devices, with no copy fallback). Edge case: a case-only rename on a case-insensitive FS (macOS APFS default) is **rejected** as "already exists". This command handles both rename and move (drag-drop). No event. | explorer/lib/useFileTree.ts (rename and drag-move) |
| 11 | `fs_delete` | `path: string` | `null` | `symlink_metadata` (missing -> Err). Dir: `remove_dir_all` (recursive). Anything else, symlinks included: `remove_file`. **Permanent delete, no OS trash.** A symlink to a dir removes only the link and never recurses into the target. The UI must confirm first. No event. | explorer/lib/useFileTree.ts |
| 12 | `tasks_scan` | `root: string` | `AgendaItem[]` | §5.3 | agenda/lib/useAgenda.ts |
| 13 | `tasks_toggle` | `path: string, line: number` (1-based) | `null` | §5.4. **Emits no event.** | agenda/lib/useAgenda.ts (re-scans itself after the call) |
| 14 | `tags_scan` | `root: string` | `TagEntry[]` | §5.1 | tags/lib/useTagIndex.ts (re-run on `fs:file-written`) |
| 15 | `links_scan` | `root: string` | `LinkRef[]` | §5.2 | backlinks/lib/useBacklinks.ts (re-run on `fs:file-written` and `fs:changed`) |
| 16 | `export_print` | none (receives the calling `WebviewWindow`) | `null` | `window.print()` opens the OS print dialog for **the calling window** ("Save as PDF" lives there). Err -> string. | export/PrintWindow.ts (falls back to JS `window.print()` on error) |
| 17 | `search_files` | `root: string, query: string` | `SearchMatch[]` | §4 | search/lib/useSearch.ts |
| 18 | `search_replace_files` | `root: string, query: string, replacement: string, files: string[] \| null` | `ReplaceResult {files_changed:number, replacements:number}` (**snake_case**, no rename) | §4. Emits `fs:file-written` `{path, source:"replace"}` for each changed file. | search/lib/useSearch.ts (maps to `filesChanged`/`replacements`) |
| 19 | `caldav_test_connection` | `serverUrl, username, password: string` | `AccountInfo {calendarHomeUrl: string}` | Two-hop discovery (§6.4). Saves nothing. | settings/lib/useCalDavAccounts.ts |
| 20 | `caldav_discover_calendars` | `serverUrl, username, password: string` | `CalendarInfo[] {href, displayName}` | Discovery, then a VTODO-capable calendar list. `href` is resolved to an absolute URL. `displayName` falls back to the raw (unresolved) href. | settings/lib/useCalDavAccounts.ts |
| 21 | `caldav_discover_calendars_for_account` | `accountId: string` | `CalendarInfo[]` | Like #20, but loads the account from config and the password from the keyring. Errors: `"no saved account for id {id}"`, or the keyring error strings. | settings/lib/useCalDavAccounts.ts |
| 22 | `caldav_save_account` | `input: {serverUrl, username, password}` (nested object!) | `string` (new account id, UUID v4) | Generates a UUID, saves the password to the keyring **first**, then appends `{id, serverUrl, username}` to the config and saves. It does **not** verify credentials. If the config save fails, an orphan keyring entry stays behind. Duplicate accounts are allowed. | settings/lib/useCalDavAccounts.ts |
| 23 | `caldav_list_accounts` | none | `Account[] {id, serverUrl, username}` | Never includes a password. | settings/lib/useCalDavAccounts.ts |
| 24 | `caldav_remove_account` | `accountId: string` | `null` | Removes the account **and every link with that accountId**, saves the config, then deletes the keyring entry (a missing entry is OK). Sidecar files in linked folders are **kept** on disk. | settings/lib/useCalDavAccounts.ts |
| 25 | `caldav_link_folder` | `accountId, calendarHref, calendarDisplayName, folderPath: string` | `null` | `folderPath` goes through `to_canon` (string-only). Removes any existing link with the same folderPath (re-linking replaces it), pushes the new link, and saves. If the folder has no sidecar yet, writes a fresh `Sidecar{accountId, calendarHref, tasks:[]}`. An existing sidecar is left untouched, **even if it names a different account or calendar** (sync overwrites those two fields later). The accountId's existence is not validated. | settings/lib/useCalDavLinks.ts |
| 26 | `caldav_unlink_folder` | `folderPath: string` | `null` | Removes the link from the config only. **The sidecar is kept**, so re-linking resumes where it left off. | settings/lib/useCalDavLinks.ts |
| 27 | `caldav_list_links` | none | `FolderLink[] {accountId, calendarHref, calendarDisplayName, folderPath}` | | settings/lib/useCalDavLinks.ts |
| 28 | `caldav_sync_now` | `folderPath: string \| null` | `SyncReport[]` | `to_canon(folderPath)`, then `run_sync(target)` (§6.7), then emits `caldav:sync-complete`. **Always Ok**: per-folder failures go into `report.errors`. If `folderPath` matches no link, returns `[]`. | settings/lib/useCalDavSync.ts |
| 29 | `caldav_get_sync_status` | none | `FolderSyncStatus[]` | One entry per link, read from its sidecar: `{folderPath, calendarDisplayName, lastSyncedAt: RFC3339\|null, taskCount: sidecar.tasks.len(), pendingDeletes: count with pendingDeleteSince set}`. With no sidecar: `null, 0, 0`. A corrupt sidecar fails the **whole** command with Err. | settings/lib/useCalDavSync.ts (refreshes on `caldav:sync-complete`) |
| 30 | `fonts_list_system` | none | `string[]` | `font_kit::SystemSource::new().all_families()`, then `sort()` (byte order, case-sensitive), then `dedup()`. Err -> string. | settings/FontSection.tsx |
| 31 | `open_requests_take` | none | `string[]` | Returns **and clears** the queued OS "open file" paths (§7.2). | lib/useOpenRequests.ts |


#### Exact JSON shapes

```jsonc
// ReadResult: serde tag "kind", rename_all lowercase
{ "kind": "text",     "content": "...", "size": 123 }
{ "kind": "binary",   "size": 123 }
{ "kind": "toolarge", "size": 99999999, "limit": 10485760 }

// DirEntry
{ "name": "note.md", "kind": "file" | "dir" | "symlink", "size": 123, "mtime": 1721400000000 }
// FileStat
{ "size": 123, "mtime": 1721400000000, "kind": "file" | "dir" | "symlink" }

// SearchMatch
{ "file": "/abs/a.md", "line": 3, "column": 5, "text": "full line text" }
// ReplaceResult
{ "files_changed": 1, "replacements": 3 }

// TagEntry
{ "tag": "gamma/nested", "count": 2, "files": ["/abs/a.md", "/abs/b.md"] }
// LinkRef
{ "path": "/abs/a.md", "line": 1, "target": "Note B|alias", "kind": "wiki" | "markdown", "context": "trimmed line" }
// AgendaItem
{ "kind": "task" | "todo" | "event", "checked": false, "text": "Buy milk 📅 2026-07-20",
  "date": "2026-07-20" | null, "time": "10:00" | null, "file": "/abs/a.md", "line": 1,
  "recurrence": "daily" | "weekdays" | "weekends" | "mon,wed,fri" | null, "recurTime": "08:00" | null }

// CalDAV
// AccountInfo
{ "calendarHomeUrl": "https://.../calendars/" }
// CalendarInfo
{ "href": "https://host/123/calendars/home/", "displayName": "Reminders" }
// Account
{ "id": "uuid", "serverUrl": "https://caldav.icloud.com", "username": "u@x" }
// FolderLink
{ "accountId": "uuid", "calendarHref": "https://...", "calendarDisplayName": "Reminders", "folderPath": "/abs/folder" }
// SyncReport
{ "folder": "/abs/folder", "pushedNew": 0, "pushedUpdated": 0, "pulledUpdated": 0, "pulledNew": 0,
  "pendingDeletes": 0, "confirmedDeletesRemote": 0, "deletedOnServer": 0,
  "touchedFiles": ["/abs/folder/Synced Tasks.md"], "errors": ["..."] }
// FolderSyncStatus
{ "folderPath": "...", "calendarDisplayName": "...", "lastSyncedAt": "2026-07-15T12:00:00.123Z" | null,
  "taskCount": 3, "pendingDeletes": 1 }
```

- [ ] `chrono::DateTime<Utc>` serializes as an RFC 3339 string with a `Z` suffix and fractional seconds.

### 2. Events

There is **no filesystem watcher** (no `notify` crate, no polling of the vault). Every "file changed" signal is an explicit emit after an app-initiated write. External edits are only noticed when the UI re-reads.

| Event | Direction | Payload | When |
|---|---|---|---|
| `fs:file-written` | Rust -> all windows | `{path: string, source?: string}` | After a successful `fs_write_file` (source as passed, omitted if null); after each file changed by `search_replace_files` (`source:"replace"`, path = canonical); and after each `touchedFiles` entry of a CalDAV sync (`source:"caldav"`, path = canonical absolute). **Not** emitted by `tasks_toggle`, `fs_write_binary`, `fs_create_*`, `fs_rename`, `fs_delete`. |
| `fs:file-written` | listened **in Rust** (`app.listen` in setup) | parsed as `{path: String, source: Option<String>}`; unparseable payloads are ignored | Feeds the CalDAV on-save trigger (§6.8). Because `app.listen` sees every emit of that name, any frontend `emit("fs:file-written")` would also trigger it. |
| `caldav:sync-progress` | Rust -> all | `{folderPath: string}` | After each linked folder finishes syncing, successfully or not (inside `run_sync`). No frontend listener currently. |
| `caldav:sync-complete` | Rust -> all | `{reports: number}` (count of SyncReports) | Once per `caldav_sync_now`, per debounced on-save sync, per launch sync, and per poll tick. |
| `app:open-files` (`OPEN_FILES_EVENT`) | Rust -> all | `string[]` (absolute paths) | Whenever `open_files::push` receives a non-empty list (§7.2). |

Frontend-only events that travel over the Tauri bus (no Rust logic; listed so the port provides an equivalent cross-window bus):

- `fs:changed` `{dir, from: windowLabel}`: explorer tree refresh across windows; also triggers `fs_list_markdown_files` and backlinks re-scans.
- `mermaid:request` / `mermaid:update` / `mermaid:closed` / `mermaid:dock-request`.
- `export:request` / `export:html`.

Built-in events: `tauri://destroyed` (export window cleanup).

**Windows created by Rust:** none. The main window comes from `tauri.conf.json`. All other windows (`editor-*`, `mermaid-*`, `export-*`/print) are created by the JS `new WebviewWindow(...)`. The only window-related Rust code:

- `export_print` prints the calling window.
- `open_files::push` focuses `main` (`get_webview_window("main").set_focus()`).

### 3. Filesystem semantics

#### 3.1 Tree listing (`fs_read_dir(path, showHidden)`)

- [ ] Non-recursive (the UI lazily expands folders).
- [ ] Entries whose name isn't valid UTF-8 are **dropped** (`into_string().ok()?`).
- [ ] Kind detection:
  1. Call `std::fs::metadata(entry)`, which follows symlinks: a symlink to a dir is `dir`, a symlink to a file is `file`.
  2. If that fails (broken symlink), fall back to `entry.metadata()` (lstat) and report `symlink`.
  3. If both fail, drop the entry.
- [ ] Hidden: if `name.starts_with('.') && !showHidden`, skip it. **No other ignore rules**: `node_modules`, `target` and `dist` **are** listed in the tree (unlike the scanners).
- [ ] `size` = `meta.len()` (target size for followed links). `mtime` = ms since the epoch, or 0.
- [ ] **All extensions are listed.** There is no filtering by type.
- [ ] Sort: rank `dir=0`, `symlink=1`, `file=2`; then `name.to_lowercase()` compared lexicographically (Unicode lowercase, then byte compare; not natural/numeric sort).

#### 3.2 File read/write

- [ ] Read: limit is 10 MiB, plus the NUL sniff in the first 8 KiB and the UTF-8 validity check (see #1). Text is returned verbatim (CRLF preserved).
- [ ] Write: atomic temp file plus rename, fsync, permission restore. Content is written as UTF-8 bytes exactly as given (no newline normalization).
- [ ] Binary write: create-only, parents auto-created.

#### 3.3 Create / rename / move / delete

- [ ] Create file: fails if the path exists; empty content; no parent creation; no auto-renaming (`Untitled 1`, etc. is a UI concern if anywhere).
- [ ] Create dir: recursive, and fails if the path exists.
- [ ] Rename/move: same API for both; never overwrites (returns "already exists"); no auto-rename on conflict; no cross-device fallback.
- [ ] Delete: permanent and recursive; does not follow symlinks; **no trash**.
- [ ] No fs mutation emits events. The UI calls `notifyFsChanged(dir)` itself.

### 4. Search & replace

- [ ] **Algorithm (`search_files`)**:
  - If `query.trim()` is empty, return `[]`. The query itself is NOT trimmed, so leading and trailing spaces are significant.
  - Walk **all files** (any extension) with the shared walker rules (dotfiles/dirs and SKIP_DIRS skipped, symlinks skipped).
  - Sort the paths (`PathBuf` order).
  - For each file: skip it if it isn't valid UTF-8, which is how "binary" files are detected here (no NUL sniff and no size limit, so a huge text file is read entirely).
  - For each `line` of `content.lines()`: `line.to_lowercase().find(needle_lower)` finds the **first occurrence only per line**. A line with 3 hits yields 1 match.
  - `column = line[..byte_col].chars().count() + 1`, a 1-based **char** column. Caveat: `byte_col` comes from the lowercased line. For scripts where lowercasing changes byte lengths (e.g. `İ`), the column can be off, and the slice may even panic on a non-char boundary. Parity target: char column of the match in the original line.
  - `text` = the full original line (not trimmed).
  - Stop entirely at `MAX_MATCHES = 500` total (first-found order: sorted file order, then line order).
- [ ] **No regex, no case-sensitive mode, no whole-word mode.** Case-insensitive only (Unicode `to_lowercase`). Matches never span lines.
- [ ] **Replace (`search_replace_files`)**:
  - Empty-trimmed query: returns `{0,0}`.
  - Uses the same walk and sort.
  - If `files` is given, only files whose `to_canon(path)` **exactly equals** an entry are touched. Other files are skipped (not read).
  - Each file's **whole content** is processed (so a query containing `\n` could match across lines in replace, though search can't find it).
  - Fast path: if `content.to_lowercase().len() == content.len()`, find non-overlapping occurrences of `needle_lower` in the lowercased text, left to right, and splice in `replacement` at the same byte offsets in the original.
  - Otherwise (`replace_by_chars`): char-by-char, where `chars[i..i+n]` matches when each pair compares equal via `a.to_lowercase() == b.to_lowercase()`. On a hit, push the replacement and advance by n chars.
  - The replacement is literal (no `$1` or case preservation).
  - Files with 0 hits are not written.
  - Write: record perms, `write_atomic`, restore perms; then `files_changed += 1`, `replacements += n`, and emit `fs:file-written {path: canon, source:"replace"}`.
  - **The first write error aborts** with Err `"{path}: {io error}"`. Files already rewritten stay rewritten (no rollback), and their events have already been emitted.
  - Test vector: `"Hello hello\nHELLO\n"`, replacing `hello` with `bye`, gives `"bye bye\nbye\n"` and `{1, 3}`. `"İstanbul istanbul"` with `istanbul` -> `X` gives `"İstanbul X"` (count 1).

### 5. Tags, links, tasks

#### 5.1 Tags (`tags_scan`)

- [ ] Walk Markdown files (shared walker; walk order is unsorted `read_dir` order).
- [ ] Regexes (Rust `regex` crate):
  - tag: `#(\p{L}[\p{L}\p{N}_/\-]*)`
  - code span: `` `[^`\n]*` `` (single-backtick only; no backreferences)
  - fence: `^(\s*)(`{3,}|~{3,})`
- [ ] Per line, in this order:
  1. **Fence handling**: if the line matches the fence regex, the marker char is the first char of group 2. If not in a fence, enter one and remember the marker. If in a fence and the marker char equals the opener's (length not compared), exit. In both cases the fence line itself is skipped. Lines inside a fence are skipped.
  2. **Mask inline code**: replace each code-span match with spaces of equal byte length.
  3. For each tag-regex match on the masked line: accept it only if the char before `#` is start-of-line or `char::is_whitespace`. This excludes `a#b` and `example.com/#frag`. `# Heading` doesn't match, because the regex needs a letter right after `#`, while `## Sub #real` yields `real`.
  4. **Trim trailing** `/`, `_` and `-` (`trim_end_matches(['/','_','-'])`). Drop the tag if that leaves it empty.
- [ ] Tags are **case-sensitive** (no lowercasing) and stored without `#`. Nesting via `/` is kept in the string as-is (`gamma/nested`). The backend does not expand parents.
- [ ] Frontmatter lines are also scanned by the inline pass (there is no frontmatter skip). Fence state is tracked from line 1.
- [ ] **Frontmatter tags** (`frontmatter_tags`):
  - Only applies if the first line, trimmed, is exactly `---`. The block runs to the next line whose trimmed form is `---`. With no closing line there is no frontmatter.
  - Find the first line inside it that **starts exactly with** `tags:` (column 0, no leading spaces, no space before the colon). Only the first `tags:` key is used. `rest` = the text after `tags:`, trimmed.
  - **Inline array**: `rest` starts with `[`. Keep appending the following lines (trimmed, joined with a space) until the text contains `]` or the block ends. Take the text up to the first `]`, split on `,`, `trim` and `strip_quotes` each item, and drop empties.
  - **Block list**: `rest` is empty. The following lines are items while `trim_start` begins with `-` **and** the line is indented (`len != trimmed len`). Each item is `strip_quotes(item.trim())`, with empties dropped. The list stops at the first non-item line (including blank lines).
  - **Scalar**: otherwise, `strip_quotes(rest)` is split on whitespace or `,`, with empties dropped.
  - `strip_quotes`: if the string has length ≥ 2 and is wrapped in matching `"…"` or `'…'`, the outer quotes are removed.
  - Each frontmatter tag is normalized with `trim_start_matches('#').trim()` and dropped if empty. Frontmatter tags are NOT trailing-separator trimmed or validated (can contain spaces, e.g. `["a b"]` gives `a b`).
- [ ] Per file: collect frontmatter and inline tags, then `sort` and `dedup`. Each unique tag gets the file appended once.
- [ ] Result: one `TagEntry` per tag, where `count` = number of **files** (not occurrences) and `files` = the files in walk order. Order: `count` descending, then `tag` ascending (byte order).
- [ ] Comment notes this mirrors the frontend `src/modules/tags/lib/parseTags.ts` (`matchInlineTagsInLine`, `frontmatterTags`). Keep the two in sync.

#### 5.2 Links / backlinks (`links_scan`)

- [ ] The backend only **extracts** links. Resolution of a target to a file (`resolveWikilinkTarget`, `resolveRelativeMarkdownLink`) happens in the frontend.
- [ ] Regexes:
  - wiki: `\[\[([^\[\]]+?)\]\]`
  - markdown: `(?:^|[^!])\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)`
  - fence: `^\s*(`{3,}|~{3,})`
  - code span: `` `[^`]*` ``
- [ ] Per line:
  1. If it matches fence, **toggle** `in_fence` (any fence char, no marker matching, unlike tags) and skip the line. Skip lines inside a fence.
  2. `line` = the raw line with code spans **removed** (replaced by ""). `context` = `raw_line.trim()` (code spans still present).
  3. Wiki: each capture yields `target` = the inner text verbatim, with alias (`Note|alias`) and heading (`Note#H`) kept. `kind:"wiki"`.
  4. Markdown: the `(?:^|[^!])` prefix excludes images `![]()`. The href is the text up to the first `)`, whitespace or `>`. Optional `<…>` and a `"title"` are dropped. Hrefs that start with `#` or are external are skipped.
     - External = starts with `//`, or the text before the first `:` is non-empty and made only of ASCII alphanumerics and `+.-`. So `https:`, `mailto:`, and even `C:` count as external, while `./a:b.md` does not.
     - `kind:"markdown"`, `target` = the href (not URL-decoded).
  5. Quirk: the prefix consumes one char, so a Markdown link immediately following another (`[a](x)[b](y)`) loses the second link. Wiki links inside Markdown link text are also reported.
- [ ] The output is a flat list in walk order (unsorted), then line order; within a line, all wiki links come first, then Markdown links.
- [ ] Test vectors:
  - `"see [[Note B|alias]] and [x](./c.md)\n[ext](https://x.y)\n![img](i.png)"` yields 2 refs: `Note B|alias` (wiki, line 1) and `./c.md` (markdown).
  - `` "`[[no]]`\n```\n[[no]]\n```\n[[yes]]" `` yields only `yes` at line 5.

#### 5.3 Tasks / agenda scan (`tasks_scan`)

- [ ] Regexes:
  - checkbox: `^[-*+]\s\[([ xX])\]\s*(.*)$` (applied to the **trimmed** line, so any indentation is accepted)
  - todo: `(?i)^TODO:?\s+(.+)$` (case-insensitive, colon optional: `todo buy x` matches)
  - date: `📅\s*(\d{4}-\d{2}-\d{2})(?:\s+(\d{2}:\d{2}))?`
  - recur: `(?i)🔁\s*(daily|weekdays|weekends|[a-z]{3}(?:,[a-z]{3})*)(?:\s+(\d{2}:\d{2}))?`
- [ ] Per line (`trimmed = line.trim()`, empty lines skipped), with first match winning:
  1. **Event**: `trimmed` starts with `📅` **and** the date regex finds a match. Then `kind:"event"`, `checked:false`, date/time from the captures, `text` = `trimmed[match_end..].trim()` (the text after the date and time), `recurrence:null`, `recurTime:null`. If the line starts with 📅 but has no valid date, it falls through.
  2. **Task**: the checkbox matches. `checked` = `x` or `X`. `text` = group 2 **including** the markers (raw). `date`/`time` = the first date-regex capture anywhere in the text. `recurrence` = the first recur capture, lowercased. `recurTime` = its time.
  3. **Todo**: the todo regex matches. `checked:false`, `text` = group 1, with date and recurrence extracted the same way.
- [ ] There is no validation of the date (e.g. `2026-13-45` is accepted) or of the time. The recurrence `[a-z]{3}` list accepts any 3-letter tokens (`🔁 foo` gives `foo`). There are no word boundaries: `🔁 monday` gives `mon`, and `🔁 dailyx` gives `daily`.
- [ ] There is no `@due`, `@repeat`, priority, `⏫`, `✅ done-date`, or `⏳`/`🛫` syntax. Only `📅` and `🔁` are recognized. Recurrence **expansion and logic live in the frontend**. The backend only reports the raw rule string (`daily` | `weekdays` | `weekends` | comma-separated 3-letter abbreviations) and an optional `HH:MM`.
- [ ] Output order: walk order (unsorted), then line order.
- [ ] Test vectors:
  - `"- [ ] one 📅 2026-07-20\n- [x] two\nTODO: three\n📅 2026-07-21 10:00 four\nplain text\n"` yields 4 items: task (date 2026-07-20), task (checked), todo `three`, event (2026-07-21 10:00, text `four`).
  - `"- [ ] give dog food 🔁 daily 08:00"` gives recurrence `daily` and recurTime `08:00`.
  - `"- [ ] trash 🔁 mon,wed,fri"` gives `mon,wed,fri` with no time.
  - `"TODO: standup 🔁 weekdays"` gives `weekdays`.

#### 5.4 Task completion (`tasks_toggle(path, line)`)

- [ ] Read the file. If `line == 0 || line > lines.len()`, Err `"line out of range"`.
- [ ] Regex: `^(\s*[-*+]\s\[)([ xX])(\]\s*.*)$` on the untrimmed line. No match gives Err `"line is not a checkbox"` (e.g. the file changed since the scan).
- [ ] `x`/`X` becomes ` `, and ` ` becomes `x` (always lowercase). Nothing else changes: **no completion date is appended, no recurrence roll-forward, no next-instance creation.**
- [ ] Rebuild with `lines.join("\n")` and append `\n` only if the original ended with `\n`. **Side effect: CRLF files are converted to LF.**
- [ ] Atomic write via a temp file plus persist (perms become 0600, not restored). **No `fs:file-written` emit.** As a consequence, CalDAV on-save sync is not triggered by agenda toggles (only by the next poll or manual sync), and open editors are not notified.

### 6. CalDAV (full detail)

#### 6.1 Config storage

- [ ] File: `{app_data_dir}/caldav/config.json`, where `app_data_dir` is Tauri's per-identifier data dir for `dev.fuleco.fuchico`:
  - macOS: `~/Library/Application Support/dev.fuleco.fuchico/`
  - Linux: `~/.local/share/dev.fuleco.fuchico/`
  - Windows: `%APPDATA%\dev.fuleco.fuchico\`
- [ ] JSON is written pretty (`to_vec_pretty`) via `write_atomic`, with `create_dir_all` on the parent first:
  ```json
  { "accounts": [ { "id": "uuid", "serverUrl": "https://caldav.icloud.com", "username": "user@example.com" } ],
    "links": [ { "accountId": "uuid", "calendarHref": "https://.../calendars/home/", "calendarDisplayName": "Reminders", "folderPath": "/Users/x/notes" } ] }
  ```
  Both arrays use `#[serde(default)]`, so `{}` parses. A missing file means an empty config. Invalid JSON gives Err `"corrupt caldav config at {path}: {e}"`. Other read errors give `"failed to read {path}: {e}"`. Write errors give `"failed to create {dir}: {e}"` / `"failed to write {path}: {e}"`.
- [ ] Invariant: **the config never contains a password** (tested).
- [ ] The config is re-read from disk for every operation. There is no in-memory cache.

#### 6.2 Credentials

- [ ] OS keyring with service `KEYRING_SERVICE = "dev.fuleco.fuchico"` and user/account = the account `id` (UUID). The secret is the password, typically an app-specific password.
- [ ] macOS, Windows and others: `keyring` v4 `Entry::new(service, id)` (macOS Keychain, Windows Credential Manager).
- [ ] Linux: `keyring-core` with `linux-keyutils-keyring-store`, i.e. the **kernel keyring** (works without Secret Service/D-Bus; note that kernel-keyring persistence is per user session or boot).
- [ ] Error strings:
  - `"keyring unavailable: {e}"` (Linux: `"kernel keyring unavailable: {e}"`)
  - `"failed to save password to keyring: {e}"`
  - `"failed to read password from keyring: {e}"`
  - `"failed to delete password from keyring: {e}"`
  - Delete treats `NoEntry` as success.

#### 6.3 Sidecar (per linked folder sync state)

- [ ] File `{folder}/.fuchico-sync.json` (`SIDECAR_FILENAME`). The leading dot keeps it out of every scanner and out of the explorer by default. It is written pretty via `write_atomic`. Missing gives `None`. Corrupt gives Err `"corrupt sidecar at {path}: {e}"`.
- [ ] Shape:
  ```json
  { "accountId": "uuid", "calendarHref": "https://...",
    "syncToken": "...",          // omitted when None; reserved, always None in v1
    "lastSyncedAt": "RFC3339",    // omitted when None
    "tasks": [ {
      "uid": "uuid-or-remote-uid",
      "file": "sub/Tasks.md",      // relative to the folder root, canonical slashes
      "lineHint": 3,               // 1-based; 0 for remote-pulled-new entries
      "contentHash": "sha256 hex of normalize_text(text)",
      "lastText": "raw checkbox text incl. markers",
      "checked": false,
      "etag": "\"abc\"" | null,
      "lastModifiedLocal": "RFC3339" | null,
      "lastModifiedRemote": "RFC3339" | null,
      "pendingDeleteSince": "RFC3339",   // omitted when None
      "pendingDeleteStreak": 0           // default 0
    } ] }
  ```
  `tasks` defaults to `[]`.
- [ ] A subfolder containing its own sidecar counts as a separately linked folder. The parent's scan does not descend into it (most specific link wins).

#### 6.4 HTTP client & discovery

- [ ] `reqwest::Client::new()` with features `rustls` and `http2` (no default features). There is **no explicit timeout and no redirect customization** (reqwest defaults: up to 10 redirects, no timeout). Every request uses HTTP **Basic auth** (username, password) and sends `Content-Type: application/xml; charset=utf-8`.
  - PUT additionally **appends** a second `Content-Type: text/calendar; charset=utf-8`, so both headers are sent. The port should send just `text/calendar`.
  - `Depth` is sent when specified.
- [ ] Status rule: PROPFIND/REPORT accept `2xx` or `207`. PUT requires 2xx. DELETE accepts 2xx or 404.
- [ ] Errors:
  - `Transport(e)` displays as `"connection failed: {e}"`.
  - `Status{status, body}` displays as `"server returned HTTP {status}: {first 300 chars of body}"`.
  - A missing discovery element is `Status{0, "no current-user-principal in response"}` or `Status{0, "no calendar-home-set in response"}`.
- [ ] `resolve(href)`: an href starting with `http://` or `https://` is used as-is. Otherwise `Url::parse(server_url).join(href)`, falling back to the raw href on error.
- [ ] **Discovery** (`discover_calendar_home`), two hops:
  1. `PROPFIND {serverUrl}` with `Depth: 0` and this body:
     ```xml
     <?xml version="1.0" encoding="utf-8"?>
     <D:propfind xmlns:D="DAV:">
       <D:prop><D:current-user-principal/></D:prop>
     </D:propfind>
     ```
     Take the first non-empty `<href>` nested inside an element whose local name is `current-user-principal`, and resolve it.
  2. `PROPFIND {principalUrl}` with `Depth: 0`:
     ```xml
     <?xml version="1.0" encoding="utf-8"?>
     <D:propfind xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
       <D:prop><C:calendar-home-set/></D:prop>
     </D:propfind>
     ```
     Take the first `<href>` inside `calendar-home-set`, and resolve it. That is the `calendarHomeUrl`.
  - There is no `.well-known/caldav` lookup and no DNS SRV. The user must enter a server URL that answers PROPFIND (e.g. `https://caldav.icloud.com`).
- [ ] **List task calendars**: `PROPFIND {homeUrl}` with `Depth: 1`:
  ```xml
  <?xml version="1.0" encoding="utf-8"?>
  <D:propfind xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
    <D:prop>
      <D:resourcetype/>
      <D:displayname/>
      <C:supported-calendar-component-set/>
    </D:prop>
  </D:propfind>
  ```
  Keep only responses where `resourcetype` contains a `calendar` element **and** some `<comp name="…">` equals `VTODO` (ASCII case-insensitive). Event-only calendars and the home collection itself are excluded.
- [ ] **List VTODOs**: `REPORT {calendarHref}` with `Depth: 1`:
  ```xml
  <?xml version="1.0" encoding="utf-8"?>
  <C:calendar-query xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
    <D:prop>
      <D:getetag/>
      <C:calendar-data/>
    </D:prop>
    <C:filter>
      <C:comp-filter name="VCALENDAR">
        <C:comp-filter name="VTODO"/>
      </C:comp-filter>
    </C:filter>
  </C:calendar-query>
  ```
  This is a full listing every sync. There is no `sync-collection`/sync-token and no multiget.
- [ ] **PUT** `{url}` with the iCal body. Header `If-Match: {etag}` when updating, or `If-None-Match: *` when creating. Returns the response `ETag` header if present.
- [ ] **DELETE** `{url}`, with no If-Match. A 404 counts as success.
- [ ] **Multistatus parser** (`parse_multistatus`, quick-xml, trim_text):
  - Matches on **local names only**, so namespace prefixes are ignored.
  - Each `<response>` produces a `DavResponse{href, displayname, is_calendar_collection, supported_components[], etag, calendar_data}`.
  - `href` is taken only from an `<href>` that is a **direct child** of `<response>`, and only the first one.
  - `displayname`, `getetag` and `calendar-data` are set from the element's trimmed text when it is non-empty (the last occurrence wins).
  - Text and CDATA are both accumulated.
  - `<calendar>` (start or empty element) sets `is_calendar_collection`.
  - Each `<comp>` contributes its `name` attribute (local-name match, unescaped).
  - Parse errors stop the parse silently, returning whatever was parsed so far.

#### 6.5 iCal VTODO mapping (`icalendar` crate 0.17)

- [ ] **Local to remote** (`to_vtodo_ics`): a `VCALENDAR` (crate defaults: `VERSION:2.0`, crate PRODID) containing one `VTODO` with:
  - `UID` = the sidecar uid (a fresh UUID v4 for new tasks)
  - `SUMMARY` = `strip_markers(text)`, i.e. the 📅/🔁 markers removed, whitespace collapsed, **case preserved**
  - `STATUS` = `COMPLETED` if checked, else `NEEDS-ACTION`
  - `DUE` = date-only (`DUE;VALUE=DATE:YYYYMMDD`) from the first `📅 YYYY-MM-DD` (parsed with `%Y-%m-%d`; an invalid date is omitted). The `HH:MM` time is **not** sent.
  - `LAST-MODIFIED` = now (UTC)
  - plus whatever the crate adds by default (DTSTAMP).
  - Not sent: RRULE (recurrence is not mapped), COMPLETED timestamp, PRIORITY, DESCRIPTION.
- [ ] **Remote to local** (`parse_vtodos`): parse the `calendar-data` text. Invalid input gives Err `"invalid iCalendar: {e}"`, which sync records as `"bad VTODO at {href}: invalid iCalendar: …"` and the resource is skipped.
  - Each `VTODO` with a UID yields `SyncTask{uid, summary (default ""), completed: STATUS==COMPLETED, due: DUE.date_naive(), last_modified: LAST-MODIFIED}`.
  - Components without a UID and non-VTODO components are ignored.
- [ ] **Pulled fields**: only SUMMARY and STATUS reach the Markdown. DUE is **never pulled back**; local 📅/🔁 markers are preserved instead (see pull below).

#### 6.6 Matcher (identity of a Markdown task across edits)

- [ ] Constants:
  - `SIMILARITY_THRESHOLD = 0.6`
  - `PROXIMITY_WINDOW = 5` (lines)
  - `PENDING_DELETE_STREAK_THRESHOLD = 3`
- [ ] Regexes:
  - date marker: `📅\s*\d{4}-\d{2}-\d{2}(?:\s+\d{2}:\d{2})?`
  - recur marker: `(?i)🔁\s*(?:daily|weekdays|weekends|[a-z]{3}(?:,[a-z]{3})*)(?:\s+\d{2}:\d{2})?`
  - date capture: `📅\s*(\d{4}-\d{2}-\d{2})(?:\s+\d{2}:\d{2})?`
- [ ] `strip_markers(t)`: remove **all** date markers, then all recur markers, then collapse whitespace (`split_whitespace().join(" ")`).
- [ ] `normalize_text(t)` = `strip_markers(t).to_lowercase()`.
- [ ] `content_hash(t)` = lowercase hex SHA-256 of `normalize_text(t)`.
- [ ] `extract_trailing_markers(t)`: take the first date-marker match and the first recur-marker match (original case), join them with a space, or return `""`.
- [ ] `similarity(a, b)` = Jaccard similarity over the **sets** of whitespace tokens of `normalize_text`. If both sets are empty, the result is 1.0.
- [ ] Input: scanned tasks `{file (relative), line, text (raw after brackets), checked}` and the sidecar entries.
- [ ] `match_tasks` runs three cascading stages. Each stage only sees what earlier stages left unmatched, and scanned tasks are processed in scan order:
  1. **Exact**: the first unmatched sidecar entry with the same `file` and `contentHash == content_hash(scanned.text)`. This survives checkbox toggles, date/recurrence edits, case changes, whitespace changes and pure reorders.
  2. **Fuzzy, same file**: candidates are unmatched sidecar entries in the same file with `similarity(scanned.text, entry.lastText) >= 0.6`, sorted by similarity descending. Take the best one if `|best.lineHint - scanned.line| <= 5` **or** it is the unique best (`len == 1 || second < best`).
  3. **Fuzzy, whole folder**: the same rule without the same-file restriction. This covers file renames and moves within the folder.
  - Output: `matched` pairs, `new_scanned` (no match: create remotely) and `unmatched_sidecar` (soft-delete candidates).
- [ ] Documented limitation: a heavy edit plus a reorder in the same window is indistinguishable from delete plus create.

#### 6.7 Sync algorithm (`sync_folder(client, folder, accountId, calendarHref, calendarDisplayName)`)

1. [ ] **Scan local tasks.** Walk the folder (shared walker rules, plus: skip any subdir that contains `.fuchico-sync.json`). For each Markdown file, take every line matching `^(\s*[-*+]\s\[)([ xX])(\]\s*)(.*)$` to get a RawTask `{file: relative canonical path, line, prefix = group 1, bracket_spacer = group 3, text = group 4, checked}`. Only checkbox tasks are synced (no TODO:/events).
2. [ ] **Load the sidecar** (create a new one if missing), then overwrite its `accountId` and `calendarHref` with the link's values.
3. [ ] **Fetch remote.** `list_vtodos(calendarHref)`. A failure aborts the whole folder with Err (nothing written). Build `remote_by_uid: uid -> (resolved href, etag, SyncTask)`; if UIDs are duplicated, the last one wins.
4. [ ] **Match** with `match_tasks(scanned, sidecar.tasks)`.
5. [ ] **Matched pairs.** If the old entry's uid is **not on the server**, it was deleted server-side: `deletedOnServer += 1`, the sidecar entry is dropped, and the Markdown line is left untouched. (Consequence: on the next sync the line is unmatched, so it is **re-created** as a new remote task.) Otherwise:
   - `local_changed` = `normalize_text(raw.text) != normalize_text(entry.lastText)` **or** `raw.checked != entry.checked`. Edits to the date or recurrence alone do not count as local changes.
   - `remote_changed` = `remote etag != entry.etag` (Option comparison).
   - If both changed, it's a **conflict**: compare remote `LAST-MODIFIED` (or `MIN_UTC` if absent) with the local **file mtime** (or now if unavailable). If the remote is strictly newer, **pull**; otherwise **push** (with `If-Match: old etag`, which will usually fail with 412, so an error is logged and the entry is kept, to be retried next pass).
   - If only local changed: **push**. If only remote changed: **pull**.
   - If neither changed: update the entry's `file` and `lineHint` only.
   - In all cases: clear `pendingDeleteSince` and set `pendingDeleteStreak = 0`, keep the entry, and mark the uid as consumed.
   - **Push** (`push_update`): PUT the ICS (summary = stripped text, completed, due) to the remote href with If-Match old etag. On success, update `file`, `lineHint`, `lastText = raw.text`, `checked`, `contentHash`, `etag` = the new etag (or the old one if the server sent none), `lastModifiedLocal` = `lastModifiedRemote` = now, and `pushedUpdated += 1`. On failure, record `"failed to push update for {file}:{line}: {e}"` and keep the old entry values.
   - **Pull** (`pull_into`): `new_text` = `remote.summary`, plus `" " + trailing_markers` if any. `new_line` = `prefix + ("x" if completed else " ") + bracket_spacer + new_text`. Queue it as a line edit `(file, line)`. Update the entry: `file`, `lineHint`, `lastText = new_text`, `checked`, `contentHash`, `etag` = the remote etag, `lastModifiedRemote` = remote LAST-MODIFIED, `lastModifiedLocal` = now, and `pulledUpdated += 1`.
6. [ ] **New local tasks** (`new_scanned`):
   - Mint `uid = UUID v4` and `PUT {ensure_trailing_slash(calendarHref)}{uid}.ics` with `If-None-Match: *` and summary = `strip_markers(text)`, completed, due.
   - On success, `pushedNew += 1` and a new sidecar entry `{uid, file, lineHint: line, contentHash, lastText: raw text, checked, etag, lastModifiedLocal = lastModifiedRemote = now}`.
   - On failure, record `"failed to create remote task for {file}:{line}: {e}"` and add no entry, so the task is retried next pass.
   - The UID is **not** written into the Markdown. Identity lives only in the sidecar.
7. [ ] **Unmatched sidecar entries** (local deletion candidates): mark the uid as consumed, `streak += 1`, and set `pendingDeleteSince` to now if it is unset.
   - If `streak >= 3`, DELETE the remote href if the uid exists remotely (if it is already gone, that counts as success). On success, `confirmedDeletesRemote += 1` and the entry is dropped. On failure, record `"failed to delete remote task {uid}: {e}"` and keep the entry.
   - If `streak < 3`, `pendingDeletes += 1` and keep the entry.
   - So a local deletion reaches the server on the **3rd consecutive** sync pass in which the task is missing. Any re-match before then resets the counter.
8. [ ] **New remote tasks**: any remote uid that is not consumed and not present in the old sidecar.
   - Collect it and add a sidecar entry `{uid, file:"Synced Tasks.md", lineHint:0, contentHash(summary), lastText: summary, checked: completed, etag, lastModifiedLocal: now, lastModifiedRemote: remote LM}`, with `pulledNew += 1`.
   - Then `append_new_remote_tasks`: read `{folder}/Synced Tasks.md` (`SYNCED_TASKS_FILENAME`, empty if missing). If the content doesn't contain `## {calendarDisplayName}`, add a newline if the content is non-empty without a trailing `\n`, then a blank line if the content is non-empty, then the heading and `\n`.
   - For each task, append `- [x] {summary}\n` or `- [ ] {summary}\n`. Tasks are **always appended at EOF**, even if the heading already exists earlier in the file.
   - Write with `write_atomic` and add the canonical absolute path to `touchedFiles`. A write failure aborts the folder with Err.
   - Iteration order is HashMap order (unspecified). Remote due dates are not written.
9. [ ] **Apply line edits.** For each file with queued edits, re-read it and replace the 1-based lines (an out-of-range line is skipped silently). Join with `\n`, append a trailing `\n` if the original had one (CRLF becomes LF), and `write_atomic` (perms 0600). Add the path to `touchedFiles` (deduplicated). A read or write failure aborts with Err.
10. [ ] **Save the sidecar**: `tasks = new list`, `lastSyncedAt = now`. Note: if step 8 or 9 errors, the sidecar is **not** saved, although remote PUTs and DELETEs already happened. The next pass recovers through matching.

- [ ] Sync direction is **bidirectional**. Fields synced: title (SUMMARY) and completion (both directions); due date (local to remote only). Not synced: recurrence, time, TODO lines, events.
- [ ] `run_sync(app, target: Option<folderPath>)`:
  - Load the config. On failure, return a single report `{folder: target or "", errors:[e]}`.
  - For each link (filtered to `folderPath == target` when a target is given), in config order:
    - Find the account. If missing, report `"no saved account for id {id}"`.
    - Load the password (error goes into the report).
    - Build a client and call `sync_folder`. An Err becomes a report with `errors:[e]`.
    - Emit `fs:file-written {path, source:"caldav"}` for each touched file, then emit `caldav:sync-progress {folderPath}`.
  - Folders are processed sequentially. **There is no mutex preventing concurrent syncs** of the same folder (poll, on-save and manual triggers can overlap).

#### 6.8 Sync triggers

- [ ] **Launch**: in `setup`, an async task runs `sync_all` once at startup.
- [ ] **Poll**: `SYNC_POLL_INTERVAL = 15 min` (`tokio::time::interval`). The first tick is consumed immediately, so the next sync comes 15 minutes after launch, and so on forever. Each run emits `caldav:sync-complete`.
- [ ] **On-save** (`on_file_written`, triggered by every `fs:file-written`):
  - Ignore events with `source == "caldav"` (loop guard). `"replace"`, `"editor"`, `"template"` and others do trigger it.
  - Spawn a task: load the config and find the **deepest linked folder** containing the path. With `p = to_canon(path)`, a folder `f` matches when `p == f || p.starts_with(f + "/")`, and the longest `f` wins. If none matches, stop.
  - Debounce: `DebounceState = Arc<Mutex<HashMap<folder, u64 generation>>>`. Increment the folder's generation, sleep `ON_SAVE_DEBOUNCE = 2 s`, and run only if the generation is unchanged (a trailing-edge debounce per folder). Then `run_sync(Some(folder))` and emit `caldav:sync-complete`. The map is never pruned.
- [ ] **Manual**: `caldav_sync_now(folderPath?)`.
- [ ] CalDAV offers no push or notification mechanism here, which is why the poll exists.

### 7. Fonts, open files, app setup

#### 7.1 Fonts

- [ ] `fonts_list_system`: all installed family names via font-kit (CoreText, DirectWrite or fontconfig), sorted by bytes and deduplicated. The GPUI port can use its own text system's family enumeration, but it must produce the same sorted, unique list.

#### 7.2 Opening files from the OS (issue #38)

- [ ] State `OpenRequests(Mutex<Vec<String>>)`, plus event `app:open-files`.
- [ ] `push(app, paths)`: if `paths` is empty, return. Otherwise **append** to the queue, focus the `main` window, and emit `app:open-files` with **only the new paths**. The frontend drains the queue on startup via `open_requests_take` (which returns everything and clears the queue) and also listens to the event, so nothing is lost if the event fires before the UI is ready.
- [ ] `paths_from_args(args, cwd)`:
  - Drop args that start with `-` and empty args.
  - Resolve relative paths against `cwd` (dropped if there is no cwd).
  - Keep only paths where `is_file()` is true (missing files and dirs are dropped).
  - Paths are not canonicalized.
- [ ] macOS: `RunEvent::Opened { urls }` (Finder double-click, `open -a Fuchico note.md`, dock drop). Convert each URL with `to_file_path()` (non-file URLs are dropped), then `push`. This may arrive before the webview exists.
- [ ] Linux/Windows first launch: in `setup`, `paths_from_args(std::env::args().skip(1), current_dir)`, then `push`. This branch is not compiled on macOS, so CLI args are ignored there.
- [ ] **Single instance** (`tauri-plugin-single-instance`, registered **first**): a second launch hands `(args, cwd)` to the running instance, which runs `paths_from_args(args.skip(1), Some(cwd))` and `push`. The second process exits.
- [ ] Routing of opened paths (vault vs single-file editor window) is done in the frontend (`routeOpenRequest`).

#### 7.3 App setup / plugins

- [ ] `main.rs`: `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` (no console window on Windows release builds), then `fuchico_lib::run()`.
- [ ] Plugin order: `single-instance` -> `.manage(OpenRequests)` -> `opener` -> `dialog` -> `log` (`Builder::new()` defaults: stdout plus a log file in the app log dir) -> `window-state` (`Builder::new()` defaults: persists size, position, maximized, fullscreen, etc. **per window label**, i.e. `main`, `editor-*`, `mermaid-*`, `export-*`, restored on create and saved on close/exit).
- [ ] `setup`: registers the `fs:file-written` listener (CalDAV on-save), spawns the launch-sync and 15-minute poll task, and queues the non-macOS argv files.
- [ ] **No custom menu, tray, global shortcuts, or deep links** are defined in Rust. Whatever menu exists is Tauri's default (on macOS, the standard app/Edit/Window menu, which provides Cmd+C/V/X/Z/Q). The port must provide at least the standard macOS app and Edit menus.
- [ ] Plugins used from JS: `opener` (open URLs and files externally), `dialog` (`open`, `save`, `message` native dialogs), and core `webview:allow-create-webview-window`.

#### 7.4 tauri.conf.json

- [ ] `productName: "Fuchico"`, `version: "0.1.0"`, `identifier: "dev.fuleco.fuchico"` (also the keyring service name and the app-data dir name).
- [ ] Main window: label `main` (default), `title: "Fuchico"`, `width: 1000`, `height: 700`, `dragDropEnabled: false` (native file drop is disabled so that HTML5 DnD works in the webview; in GPUI, handle OS file drops natively instead).
- [ ] CSP: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost` (irrelevant to GPUI except as a reminder that the frontend makes no network calls; all CalDAV HTTP goes through Rust).
- [ ] Build: dev server `http://localhost:1423` (`pnpm dev`), dist at `../dist`.
- [ ] Bundle:
  - `targets: "all"`; icons 32x32, 128x128, 128x128@2x, icns, ico; `category: "Productivity"`.
  - `shortDescription`: "Notes editor with Helix modal editing".
  - `longDescription`: "Fuchico is a standalone desktop notes editor: plain markdown files on disk, edited with real Helix keybindings, with live preview, tasks, tags, backlinks and CalDAV sync."
  - **File associations**: ext `md`, `markdown`; name "Markdown"; description "Markdown document"; mimeType `text/markdown`; role `Editor`.
  - Linux deb section `editors`; macOS `minimumSystemVersion: "11.0"`; publisher "FulecoRafa"; copyright "© 2026 FulecoRafa"; license MIT.

#### 7.5 Capabilities (window permission scopes)

| File | identifier | windows | permissions |
|---|---|---|---|
| default.json | `default` | `main` | `core:default`, `core:webview:allow-create-webview-window`, `opener:default`, `dialog:allow-open`, `dialog:allow-save`, `dialog:allow-message` |
| editor-window.json | `editor-window` | `editor-*` | same as main (issue #29 single-file editor windows) |
| export-window.json | `export-window` | `export-*` | `core:default` only (print/PDF preview, issue #28) |
| mermaid-window.json | `mermaid-window` | `mermaid-*` | `core:default` only |

- [ ] Window label conventions the port must reproduce: `main`, `editor-<key>` (single file, 900x650, focuses an existing window with the same label instead of duplicating it), `mermaid-<blockKey>` (720x560), and print windows `<key>` (860x900, URL `index.html?window=print&key=…`). All are created from the UI layer.

#### 7.6 Cargo dependencies (what each is for)

| Crate | Use |
|---|---|
| `tauri 2`, `tauri-build 2` | app shell/IPC (replaced by GPUI) |
| `tauri-plugin-opener/dialog/log/window-state/single-instance 2` | external open, native dialogs, logging, window geometry persistence, single instance plus argv forwarding |
| `serde`, `serde_json` | IPC and JSON files |
| `tempfile 3` | atomic writes (`NamedTempFile::new_in(parent)` plus `persist`) |
| `log 0.4` | logging |
| `regex 1` | all parsers (no lookaround or backrefs) |
| `reqwest 0.13.4` (`rustls`, `http2`, no default features) | CalDAV HTTP |
| `quick-xml 0.41` | multistatus parsing |
| `icalendar 0.17.12` | VTODO build/parse |
| `uuid 1.24` (v4) | account ids, task UIDs |
| `sha2 0.11` | content hash |
| `chrono 0.4.45` (serde) | timestamps |
| `tokio 1.53` (`time`, `rt`, `macros`) | poll interval, debounce sleep |
| `font-kit 0.14` | system font families |
| `base64 0.22` | `fs_write_binary` payload |
| `keyring 4.1.5` (non-Linux), `keyring-core 1.0` + `linux-keyutils-keyring-store 1.0` (Linux) | password storage |
| dev: `wiremock 0.6.5` | mock CalDAV server in tests |

The crate is named `fuchico`, with lib `fuchico_lib` (`staticlib`, `cdylib`, `rlib`), edition 2021, MIT.

### 8. Parity checklist: behaviors that are easy to miss

- [ ] Search is case-insensitive only, first hit per line, 500 cap, all file types, char column.
- [ ] Replace is case-insensitive, literal, whole-file, optionally restricted to an exact-path allowlist, emits `fs:file-written` with source `replace` per file, and stops at the first write error.
- [ ] `fs_read_file` 10 MiB cap returns `toolarge` with `limit: 10485760`; binary detection is NUL-in-first-8 KiB or invalid UTF-8.
- [ ] Tree sort is dirs, then broken symlinks, then files, case-insensitively; dotfiles are hidden, while node_modules/target/dist are visible in the tree but skipped by every scanner.
- [ ] Rename and create never overwrite; delete is permanent.
- [ ] `fs_write_file` preserves existing file permissions, and its emit carries the caller's `source`.
- [ ] `tasks_toggle` writes lowercase `x`, normalizes CRLF to LF, and emits no event.
- [ ] Tags: Unicode letter first; preceded by whitespace or start of line; trailing `/_-` trimmed; fenced code (same marker char) and single-backtick spans ignored; frontmatter `tags:` in 3 forms; count = files; sort count desc, then name.
- [ ] Links: wiki inner text raw (alias/heading kept); relative Markdown links only (no images, anchors or schemes); fence toggling on any marker.
- [ ] CalDAV:
  - [ ] Keyring passwords and the JSON config hold no secrets.
  - [ ] The sidecar `.fuchico-sync.json` holds identity (no UIDs in Markdown).
  - [ ] Three-stage matcher thresholds: 0.6 / ±5 lines / 3-pass delete streak.
  - [ ] Etag-based remote-change detection; mtime-vs-LAST-MODIFIED conflict resolution.
  - [ ] New remote tasks are appended to `Synced Tasks.md` under `## {calendar name}`.
  - [ ] A server-side delete only unlinks (and the task is re-created next pass).
  - [ ] Triggers: launch, every 15 min, 2 s per-folder debounced on-save (ignoring `caldav`-sourced writes), and manual.
  - [ ] Nested linked folders are excluded from the parent's scan.
- [ ] Open-file requests: queue plus event, focus main, single instance, macOS `Opened` event, argv on Linux/Windows.
- [ ] Window geometry persisted per label.


---

## 7. Helix modal editing (codemirror-helix 0.5.7)

Sources read in full: `node_modules/codemirror-helix/dist/lib.js` (3692 lines; `lib.development.js` differs only by enabling five `if (false)` debug assertions), `dist/lib.d.ts`, `README.md`. App glue: `src/modules/editor/lib/helix.ts`, `src/modules/editor/lib/extensions.ts`, `src/modules/editor/EditorPane.tsx`, `src/modules/editor/lib/completion.ts`, `src/modules/palette/{CommandPalette.tsx,lib/commands.ts}`, `src/styles/globals.css`.

**About `~/Documents/Dev/codemirror-helix`:** this is NOT a fork of the package. It is a throwaway prototyping folder (no git repo). `step-1` uses `helix({ drawSelection: false })` from npm. `step-2` is a hand-written ~20-key vim-like keymap (`helixKeymap.ts`/`helixCommands.ts`) with modes `normal|insert|visual`, a module-global clipboard, and `gg` built on a 1-second timer. `step-3` is a Monaco port of the same toy keymap. `test.ts` is an unrelated `Result<T>` class. Fuchico does not use any of it for modal editing. Only the markdown and code-block styling ideas from step-2 were carried over (see comments in `markdownStyle.ts` and `codeBlockStyle.ts`). **The parity target is the npm package. Do not use the experiment as the target.**

Notation: "sel" means a selection range. "Main" means the primary range. "Count" means the numeric prefix. A ✗ on a Helix feature means it is not implemented.

---

### 7.1 Selection model (core invariant the port must replicate)

- In Normal and Select modes there is always at least a 1-grapheme selection. A CodeMirror range `[from,to)` with `to-from ≥ 1` is the Helix "block". The **internal** representation used by the motions (`cmSelToInternal`) moves `to` back by one grapheme cluster. Internal `head` is therefore the char the block cursor sits on. `internalSelToCM` converts back by extending `to` by one cluster.
- `nextClusterBreak` treats a line end as a single step: from `line.to` it goes forward to `pos+1` (over the newline), and from `line.from` it goes backward to `pos-1`. A block on an empty line or on a line end therefore covers the newline character.
- Range direction: forward if `head > from`. "Atomic" ranges (length ≤1, or a 2-code-unit surrogate pair) count as direction-less.
- On mount, if the doc is non-empty, `setTimeout(() => select(anchor=1, head=0))` puts the block on the first char.
- **Mouse:** outside Insert mode, a `select.pointer` transaction that produces an empty selection is turned into a 1-char block (`selectByClickFilter`). Drag selections pass through unchanged.
- Leaving Insert with Escape turns every empty range into a 1-char block after the caret. Non-empty ranges are kept.
- `EditorState.allowMultipleSelections` is true. Multi-cursor typing in Insert mode is native CodeMirror (all ranges receive input).

### 7.2 Exported API and how Fuchico uses it

| Export | Purpose | Used by Fuchico? |
|---|---|---|
| `helix(options?)` | Main extension. Adds the keymap, mode/registers/history/syntax-history state, two bottom panels (status and command line), cursor decorations, filters and built-in typable commands. | **Yes.** `keybindingExtensionFor("helix")` returns `Prec.highest(helix())` with **no options** inside `keybindingCompartment`. It can be switched at runtime to `vim()` or to no modal mode. |
| `Options.config.theme` / `Options.themes` | Themes available to `:theme`. Throws if `config.theme` is set without `themes`. | No. So `:theme` is **not registered**, and the light panel theme is used (`.cm-hx-selected-option` #ccc, overridden by the app CSS). |
| `Options.config["editor.cursor-shape.insert"]` | `"block"` (default) or `"bar"`. | No, so the default **block** applies. Insert mode in the app also shows a block cursor (see 7.10). |
| `Options.config["editor.default-yank-register"]` | Default register (default `"`). | No, so it is `"`. |
| `Options.drawSelection` | Defaults to true, which includes `drawSelection({cursorBlinkRate:0, drawRangeCursor: shape==="bar"})`. | Default (true). |
| `Options.init` / `globalInit` + `snapshot(state, global?)` | Restore registers, register history, theme and undo history from a previous state or a JSON snapshot. | **No.** EditorPane remounts with `key={path}`, so **registers, `:`/`/` history and helix undo history are per tab and lost on every tab switch**. A yank in file A followed by `p` in file B pastes nothing unless the `+` register is used. The port can match this or improve on it (recommended: share registers globally). |
| `globalStateSync(state)` | TransactionSpecs that copy registers, history and theme into another editor. | No. |
| `commands` (Facet<TypableCommand[]>) | Register `:` commands. Combined lists are flattened and sorted by `name`. Collisions are not checked, and the first match by name/alias wins in sorted order. | **Yes.** `helixHandlersExtension` registers `write`(w), `quit`(q) and `wq`(x). In practice these are dead code (see 7.7). |
| `externalCommands` (Facet) | Host callbacks: `file_picker`, `buffer_picker`, `":buffer-next"`, `":buffer-previous"`, `":buffer-close" {handler(buffers?), autocomplete?}`, `global_search(input)`. Each may return `{message, error?}`. Later definitions win. | **No.** So `Space f`, `Space b`, `g n` and `g p` are silent no-ops that only reset the mode. `Space /` does nothing. `:bn`/`:bp`/`:bc` are not registered. |
| `readRegister(state, reg)` | Synchronous register read (`+` is read as the stored value, not the OS clipboard). | No. |
| `pathRegister` (Facet<string>) | Contents of the `%` register. | No, so `%` is empty. |
| `resetMode` (StateEffect) | Forces Normal mode. | No. |
| `themeListener`, `changeTheme(view,name)` | Theme hooks. | No. |
| `TypableCommand {name, aliases?, help, autocomplete?(args)=>string[], handler(view,args)=>{message,error?}|void}` | | |

**Mode reporting in the app.** The package does not export its mode field. `helixModeReporterExtension` scrapes `.cm-hx-status-panel > span:first-child` on every update and maps `NOR→normal`, `INS→insert`, `SEL→select`. It then hides the status panel (`display:none`) and strips the `.cm-panels-bottom` border and background. EditorPane renders the mode as a `.helix-mode-chip` overlay (absolutely positioned bottom-left 12px, 10px, weight 600, uppercase, translucent card background, blur, `pointer-events:none`). The chip shows `NORMAL`, `INSERT` or `SELECT`, and only when `keybindingMode === "helix"`. Because the status panel is hidden, **line:col and `reg=x` are not visible in the app**. The command panel's right-hand "minor" indicator (count, `g`, `m`, `<space>`, `[`, `]`, pending `f`/`r`/`"`/`mi`…) is still visible inside the bottom command panel.

**Precedence in the app.** `completionKeymap` (Prec.highest: Enter=acceptCompletion, ArrowUp/Down=moveCompletionSelection, Escape=closeCompletion) is placed before the helix compartment so it wins while a completion popup is open. Each of its commands returns false otherwise. Helix is wrapped in `Prec.highest` to beat `basicSetup`. **Keys helix leaves unhandled fall through to basicSetup's `defaultKeymap`/`historyKeymap`/`searchKeymap` and the app keymaps, even in Normal mode** (see 7.4.12).

### 7.3 Mode state machine

`mode = { type: Normal(0)|Insert(1)|Select(4), minor: Normal(2)|Goto(3)|Match(5)|Space(6)|LeftBracket(7)|RightBracket(8), count?, register?, expecting?: {minor: string, callback, metadata} }`

- **Dispatch** (`toCodemirrorKeymap`): every key that appears in any table becomes one CodeMirror keymap entry.
  - Insert type: only the insert table is consulted. If the key is unbound there, it returns false (native behaviour).
  - If `expecting` is set and the key is a char (length 1 or `Space`), the keymap returns false. The typed text then reaches `EditorView.inputHandler`, which calls `expecting.callback(view, text, metadata)` and swallows the input.
  - `Escape` always runs the Normal-table Escape, whatever the minor mode.
  - Otherwise the table for the current minor mode is used. A key unbound in that minor table returns false.
- **Unbound printable chars outside Insert mode:** `unhandledCommandsFilter` drops every `input.type*` transaction. If a minor mode (g/m/space/[/]) is active, the minor mode resets to `Normal`/`Select`, which also clears count and register. Drag-and-drop (`input.drop`, `move.drop`) is blocked outside Insert mode. Paste (`input.paste`) is NOT blocked, so Cmd-V in Normal mode pastes natively at the cursors.
- **Unbound non-char keys in a minor mode** (for example `g` then `ArrowDown`) return false and fall through to CodeMirror defaults while the minor mode stays pending.
- `MODE_EFF.NORMAL` / `MODE_EFF.SELECT` reset minor, count, register and expecting. Most commands end with one of them, which is how counts and registers are "consumed".
- Status strings: `NOR`, `INS`, `SEL`. Minor indicator = `count` + (`""|g|m|<space>|[|]`) + `expecting.minor` (`f F t T r " s a i <C-r>`). For example `3g`, `mi`, `ms`, `"`. The count is not shown in Insert mode.

### 7.4 Keys, Normal mode (Select mode uses the same table, see 7.4.13)

#### 7.4.1 Counts and registers
| Key | Behaviour |
|---|---|
| `1`–`9`, then `0`–`9` | Accumulate the count (`count*10+d`). A leading `0` is ignored (next count is 0, so it is dropped). The count is shown in the minor indicator. |
| `"<c>` | Selects register `c` for the next command and shows `reg=c` in the status panel (hidden in the app). It is honoured by `y`, `p`, `P` and `R` only. **`d`/`c`/`Alt-d`/`Alt-c` ignore it and always write to the default register** (deviation). |

Count consumers: `h j k l` and the arrow keys, `x`, `u`, `U`, `n`, `N`, `f F t T`, `p P R`, `Space p`/`Space P`, `g j`/`g k`, `[ Space`/`] Space`. **`w`/`b`/`e` neither use nor clear the count**, so it stays pending for the next command (bug). Every other command ignores the count but clears it whenever it resets the mode.

#### 7.4.2 Motions (Normal mode collapses to a 1-char block; Select mode extends from the anchor)
| Key | Behaviour / deviation |
|---|---|
| `h`/`ArrowLeft`, `l`/`ArrowRight` | Move by one grapheme (×count) using `view.moveByChar`. **Wraps across line boundaries** (Helix does not). |
| `j`/`ArrowDown`, `k`/`ArrowUp` | Vertical move with goal column (×count) using `view.moveVertically`. Uses **visual (wrapped) lines**. Normal mode stops at the first/last doc line. |
| `w`, `e` | **Identical**: CodeMirror `moveByGroup` forward. Skips leading whitespace, then selects to the end of the next same-category run (word, punctuation or space), so it behaves like Helix `e`. If the result equals the current block end, it repeats once. Helix `w` (select word plus trailing space up to the next word start) is not reproduced. No count. Normal mode makes a range from the old head to the new head. Select mode extends from the anchor. |
| `b` | Same as `w`/`e`, backward. |
| `Home` | Line start (`cursorLineStart` / `selectLineStart`). |
| `End` | Last char of the line (block on the last grapheme). An empty line keeps the block on the newline. |
| `Ctrl-d`/`PageDown`, `Ctrl-u`/`PageUp` | Move every head by `min(clientHeight/2, distance to doc end)` px vertically. The viewport is not scrolled separately (`scrollIntoView` only). No count. |
| `%` | Select the whole document (`selectAll`). |

#### 7.4.3 Goto mode (`g`)
| Key | Behaviour |
|---|---|
| `gg` | Doc start. **`<n>gg` (go to line n) is not supported.** The count is ignored. |
| `ge` | Start of the last line. If the last line is empty (trailing newline), the start of the line before it. Select mode makes a range from `ranges[0].from` to that point. |
| `gh` | Line start. |
| `gl` | Line end (last char). |
| `gs` | First non-whitespace char of the line. |
| `gj`, `gk` | Same as `j`/`k` (count honoured). |
| `gn`, `gp` | Call external `:buffer-next` / `:buffer-previous`. These do nothing in the app. |
✗ `gt gc gb gd gy gr gi ga gm gw gf g. g|`, and `G`.

#### 7.4.4 Selection manipulation
| Key | Behaviour |
|---|---|
| `x` | Each range becomes its full line(s) including the trailing newline. If the range is already exactly that, or a count is given, it extends down by `count` lines. ✗ `X`, ✗ `Alt-x`. |
| `;` | Collapse each range to a 1-char block at its head. |
| `Alt-;` | Flip anchor and head (not for atomic ranges). |
| `Alt-:` | Make every range forward. |
| `,` | Keep only the main range. ✗ `Alt-,` (remove primary). |
| `(` / `)` | Rotate the main index backward / forward (wraps). ✗ `Alt-(`/`Alt-)` (rotate contents). |
| `C` | Copy **the main range only** to the next line where both the anchor and head columns exist (tab-aware). Shorter lines are skipped. Added as the new main range. No count. ✗ `Alt-C`. |
| `_` | Trim leading and trailing whitespace from each range. |
| `s` | "Select regex" prompt (label `search:`, not Helix's `select:`). Matches inside each initial range are previewed live and replace the selection (main index becomes 0). With no match or an invalid regex, the initial selection is kept. Escape restores the selection. Enter stores the pattern in `/` (and its history). |
| `Alt-s` | For each range, select every line it touches **as a whole line without the newline** (Helix only splits the range at newlines). Lines are de-duplicated, main index becomes 0. |
| `Alt-o`/`Alt-ArrowUp` | Expand to the parent syntax node (`selectParentSyntax`, applied to all ranges). Prior selections go on a stack. |
| `Alt-i`/`Alt-ArrowDown` | Shrink: pop the stack. Any other transaction clears the stack. With nothing to pop, it is a no-op (no child-node fallback). |
| `Alt-n`/`Alt-ArrowRight`, `Alt-p`/`Alt-ArrowLeft` | Select the next / previous sibling syntax node of the **main** range. The result is a single range. **With no sibling, it selects the whole document.** |
✗ `S` (split on regex), ✗ `K`/`Alt-K` (keep/remove matching), ✗ `&` (align), ✗ `Alt-a`/`Alt-e`/`Alt-b`/`Alt-I`, ✗ `Alt-J`/`Alt-minus`/`Alt-_`/`Alt-U`.

#### 7.4.5 Find char
`f<c>`, `F<c>`, `t<c>`, `T<c>`: these wait for one typed char (shown as `f`…) and search the **whole document string** (not just the line) for the `count`-th occurrence after or before the head. `f`/`F` land on the char. `t` lands one position before it, `T` one after. Normal mode makes a range from the old head to the target. Select mode extends from the anchor. If not found, the range is unchanged. Escape cancels. Only printable chars work (no Enter or Tab target). ✗ `Alt-.` (repeat last motion).

#### 7.4.6 Search
| Key | Behaviour |
|---|---|
| `/` | Forward search prompt (label `search:`). The pattern is a **regex**, smart-case (case-sensitive if it contains `[A-Z]`). Live preview: the first match at or after `initialSelection.main.to`, wrapping to the doc start, becomes the single selection and is centred. An invalid or partial regex leaves the preview untouched. No match restores the initial selection and scroll. Escape restores the selection and scroll. Enter stores the input in `/` and pushes it to history. **Enter on an empty input stores `""`, which effectively disables `n`/`N`** (Helix would reuse the last pattern). The placeholder shows the current `/` value. |
| `?` | ✗ Not bound (no backward search prompt). |
| `n` | The `count`-th next match after `main.to`, wrapping with the message "Wrapped around document". With no matches it shows the error "No more matches". An invalid stored regex shows `Invalid regex /x/: …`. **Normal mode replaces all selections with the match. Select mode adds the match as a new range** (Helix-like). |
| `N` | The `count`-th previous match (ring-buffer logic, wraps). Always replaces the selection with the match (no add in Select mode), centred. Same messages as `n`. |
| `*` | Sets `/` to the selections' texts, regex-escaped (`|\{}()[]^$+*?.` escaped, `-`→`\x2d`), de-duplicated and joined with `|`. Message: `register '/' set to '<pat>'`. No movement. ✗ `Alt-*`. |
| `Space /` | Global search prompt (`global-search:`). Opens only if `global_search` is defined, so it never opens in the app. |
Search highlighting comes from basicSetup's `highlightSelectionMatches`. Helix itself paints no match highlighting.

#### 7.4.7 Insert-mode entry (all are "temp" history checkpoints, see 7.6)
| Key | Behaviour / deviation |
|---|---|
| `i` | Collapse every range to a caret at `from`, then Insert. (Helix keeps the selection and inserts before it. Here the selection is dropped.) |
| `a` | Caret at `to` of every range (after the block), then Insert. Same deviation. |
| `I` | Caret at the first non-blank of each range's `from` line. |
| `A` | Caret at the true line end (`line.to`) of each head, then Insert. |
| `o` / `O` | Insert a bare line break below / above **the main range's line only**. Leaves a single caret (other cursors are lost). **No auto-indent.** No count. |
| `c` | Yank the ranges to the default register, delete them, then Insert. |
| `Alt-c` | Same without yanking. |

#### 7.4.8 Editing (checkpointed)
| Key | Behaviour / deviation |
|---|---|
| `d` | Yank to the **default** register, delete the ranges, and re-expand them to 1-char blocks. |
| `Alt-d` | Delete without yanking. |
| `r<c>` | Replace **the main range only** (others untouched) with `c` repeated once per grapheme, newlines included. Then Normal. Only printable chars. |
| `R` | Replace each range with the register value (`"x` or default), repeated `count` times. Values are distributed per range (see 7.5). The new range covers the inserted text. |
| `p` / `P` | Paste after `to` / before `from` of each range, value ×count. The pasted text becomes the selection. Then Normal. **Not linewise-aware**: a yanked line pasted with a 1-char block inserts after that char, mid-line, where Helix would paste below the line. It only behaves linewise when the selection itself ends at a line boundary (for example after `x`). |
| `Space p` / `Space P` / `Space R` | Same, from the `+` register (the OS clipboard). `Space R` uses `replaceSelection(first value)` on all ranges. |
| `y` | Yank ranges to `"x`/default. Mode becomes Normal. Message: `yanked 1 selection to register "` (or `N selections`). The selection is not collapsed. |
| `Space y` | Yank to `+`, which also writes the OS clipboard (values joined with `\n`). ✗ `Space Y`. |
| `J` | Join lines of the **main range** (or its line with the next one): every joined line's leading whitespace is replaced by a single space. The selection is adjusted. ✗ `Alt-J`. |
| `>` / `<` | `indentMore` / `indentLess` on the lines of every range (`indentUnit` = `tabSize` spaces in the app). No count. |
| `` ` `` / `` Alt-` `` / `~` | Lowercase / uppercase / swap case of each range. |
| `Ctrl-a` / `Ctrl-x` | If the range text contains `-?\d+`, the **whole range text** is passed to `Number()` ±1. Non-numeric text gives `NaN` (bug). There is no automatic "number under cursor". It acts only on the selected text: with a 1-char block on the `9` of `19`, the `9` becomes `10` and the line reads `110`. No count, no dates. |
| `Ctrl-c`, `Space c` | `toggleComment` (CodeMirror language-aware; in Markdown that is `<!-- -->`). |
| `[ Space` / `] Space` | Insert `count` empty lines above / below the main range's line. The cursor does not move. Mode is kept. Digits are accepted inside `[`/`]` mode. |
| `u` / `U` | Undo / redo ×count on the plugin's own checkpoint history (7.6). |
| `Tab`, `Shift-Tab` | No-op in Normal and Select modes (swallowed). |
✗ `.` (repeat), ✗ `q`/`Q` macros, ✗ `Alt-u`/`Alt-U` (earlier/later), ✗ `=` (format), ✗ `|`, `!`, `Alt-|`, `Alt-!`, `$` (shell), ✗ `Ctrl-s` (save jump), ✗ `Ctrl-o`/`Ctrl-i` (jumplist), ✗ `Ctrl-e`/`Ctrl-y` (scroll), ✗ `Ctrl-f`/`Ctrl-b`, ✗ `Ctrl-w` window mode, ✗ `z`/`Z` view mode, ✗ `Space k/s/S/a/w/?/'/d/D/g/h/r/j`, ✗ `md` (delete surround), ✗ `mr` (replace surround), ✗ `[`/`]` unimpaired other than `Space` (`]d ]f ]t ]p ]g` etc.).

#### 7.4.9 Match mode (`m`)
| Key | Behaviour |
|---|---|
| `mm` | For each range whose head char is a bracket, jump to the matching bracket (CodeMirror `matchBrackets`, which is syntax-tree first, then plain `()[]{}`). Normal mode selects that bracket. Select mode extends from the main range's anchor (bug: it uses `main` for every range). Quotes are listed but never match. |
| `ms<c>` | Surround each range with a pair: `(`/`)` gives `()`, `[`/`]` gives `[]`, `{`/`}` gives `{}`, `<`/`>` gives `<>`. Any other char wraps with itself on both sides. The range grows to include the pair. Checkpointed. Then Normal. |
| `mi<c>` / `ma<c>` | Text objects, inner / around. Applied per range. The mode (Normal or Select) is kept. Object chars: |
| — `m` | Nearest enclosing pair among `( [ {` and `" '` (custom depth scan from `from`, picking the closer side). `ma` extends ±1 char. |
| — `p` | Paragraph: from the line after the previous empty line to the end of the next empty line (bounded by doc start/end). **`mi` and `ma` are identical.** |
| — `( ) [ ] { }` | Bracket pair around the head, via regex search plus `matchBrackets`. Inner excludes one grapheme on each side. |
| — `< >` | Only works if the language's syntax tree marks them as brackets (not plain Markdown). Otherwise it is a no-op. |
| — any other char (`"` `'` `` ` `` `*` `_` `w` …) | Treated as a symmetric delimiter, but `matchBrackets` cannot pair non-bracket chars, so in practice it is **a no-op**. **`miw`/`maw` (word), `mi"` (use `mim`), `W f t a c T g e x` objects are all unsupported.** Uses `RegExp.escape`, which needs a modern JS engine. |

#### 7.4.10 Space mode summary
`Space y p P R c` (above), `Space f` (file_picker), `Space b` (buffer_picker), `Space /` (global search). The last three are inert in the app because no external commands are registered.

#### 7.4.11 Escape
- In Normal mode with nothing pending, it is a no-op that returns true and does not collapse the selection.
- With any pending state (minor mode, count, register, expecting) or in Select mode, it goes to Normal and keeps the selection.
- When a completion popup is open, the app's `completionKeymap` closes the popup first.

#### 7.4.12 Fall-through keys (must be decided explicitly in the port)
Keys not bound by helix reach CodeMirror's basicSetup and the app keymaps **in every mode**:
- `Enter` in Normal mode runs `insertNewlineAndIndent`. Its userEvent is `input`, not `input.type`, so the filter lets it through and **a newline is inserted**.
- `Backspace` and `Delete` delete text.
- `Mod-z`/`Mod-Shift-z`/`Mod-y` use CodeMirror's own history, which is separate from `u`/`U`. The app's menu undo/redo also calls CodeMirror `undo`/`redo`.
- `Mod-f` and the search panel come from `searchKeymap`.
- `Mod-a` selects all.
- `Mod-s` (app) saves.
- App shortcut-extension keys and `Mod-v` paste.
For parity, the port should either replicate these (likely unintentional) behaviours or treat them as out of scope and swallow them.

#### 7.4.13 Select / Extend mode (`v`)
`v` toggles Normal↔Select. In Select mode the same table applies, but motions extend from the anchor: `h j k l`, arrows, `w b e`, `Home End`, `Ctrl-d/u`, `gg ge gh gl gs gj gk`, `f t F T`, `mm`. `n` adds a range. `x`, `mi`/`ma`, `%`, `s` and `Alt-s` behave as in Normal mode and stay in Select. Commands that end with `MODE_EFF.NORMAL` exit Select: `y d c p P R r ms Ctrl-c Space-*`, Escape, `v`. `> < ~ ` `` ` `` `J Ctrl-a/x u U ; , ( ) C _ Alt-;` keep Select mode.

### 7.5 Registers

| Register | Read | Write |
|---|---|---|
| `"` (default, configurable) | stored values | `y`, `d`, `c` |
| any other char | stored values | `"x y` |
| `+` | `navigator.clipboard.readText()`. If it equals the stored `+` values joined by `\n`, the stored multi-value list is used (which preserves per-selection values). Otherwise the clipboard text becomes a single value. Async. | stores the values and writes the clipboard (joined with `\n`) |
| `_` | always empty | discarded |
| `#` | `["1","2",…]`, one per range | read-only |
| `.` | current selection texts | read-only |
| `%` | `pathRegister` facet (empty in the app) | read-only |
| `/` | last search (`*`, `/` and `s` write it) | also appended to `/` history |
| `:` | last command line (single value) | also appended to `:` history |

Writing an empty list deletes the register. **Distribution of values onto ranges** (`yanksForSelection`): with equal counts, one value per range. With more values than ranges, the list is truncated. With fewer values, the last value is repeated. ✗ `*` (primary selection) is not special.

### 7.6 Undo model (plugin-owned, not CodeMirror history)

- Commands flagged `checkpoint: true` push a snapshot (full `EditorState`) **before** they run: `d Alt-d p P R r C ms J > < ` `` ` `` `Alt-` ~ Ctrl-a Ctrl-x Ctrl-c Space-p/P/R/c [Space ]Space`.
- `checkpoint: "temp"` (`i a I A o O c Alt-c`) stores a pending snapshot. Escape from Insert commits it only if the doc changed. A whole insert session is therefore one undo step.
- `u` / `U` replace the **entire document** with the snapshot's doc and restore its selection (×count). A redo `head` remembers the newest state. A new checkpoint after undoing truncates the redo branch.
- Edits made outside checkpointed commands (native paste, Normal-mode Enter/Backspace fall-through, app widgets such as tables and checkboxes, external reloads, CodeMirror undo) are not tracked. A later `u` restores an older full snapshot and silently discards them. The port should use a proper transaction history but keep the same granularity: one step per command and one per insert session.

### 7.7 Typable commands (`:`)

**Reachability in Fuchico:** EditorPane installs a **window capture-phase** keydown listener. A plain `:` (no Cmd/Ctrl/Alt) pressed while the content DOM is focused, in helix mode and not in Insert mode, calls `preventDefault` and `stopPropagation` and opens the **app command palette** with the query `":"`. The built-in helix `:` prompt is therefore never shown in the app, and the package's `write/quit/wq` registrations are effectively unused. The `/` prompt is excluded, so a `:` typed there stays literal. The palette's `:` handling (`CommandPalette.tsx`):
- `:<digits>` gives a "Go to line N" entry (score 1000). It calls `EditorPaneHandle.goToLine`, which clamps to the doc and puts the caret at the line start, centred.
- Aliases match by prefix, with an exact match ranked above a prefix match. The text after `:` also fuzzy-matches command titles.
- App aliases: `:w :write` (save), `:wq :x` (save and close tab), `:q :quit :bc` (close active tab), `:qa` (close all tabs), `:o :open :e :edit` (quick switcher), `:theme` (toggle theme), `:backlinks :bl`, `:export` (HTML), `:pdf :print`.
These belong to the palette spec. The Helix port must route `:` to it.

**Package built-ins** (in case the native prompt is kept):

| Name | Aliases | Behaviour |
|---|---|---|
| `goto` | `g` | `:goto N`: caret at the start of line `min(N, lines)`. Errors: "Line number required", "Invalid line number". |
| `<number>` (prompt feature) | — | Typing digits jumps the caret live to that line (centred). Escape restores the selection and scroll. Enter keeps the jump. |
| `clipboard-yank` | — | Writes the main range to the OS clipboard (does not set `+`). Message: "Yanked main selection to + register". |
| `clear-register` | — | No arg: clear all registers. One char: clear it. Errors for >1 arg or an invalid name. |
| `echo` | — | Prints the args. |
| `theme` | — | Only when `themes` is given (not in the app). |
| `buffer-next`, `buffer-previous`, `buffer-close` | `bn bnext`, `bp bprev`, `bc bclose` | Only when the external command is defined (not in the app). |
| app: `write`, `quit`, `wq` | `w`, `q`, `x` | Save, close, or save then close. |
Unknown command: red error `no such command: 'x'`. Arguments are split on runs of spaces. Lookup is an exact match on name or alias. ✗ All other Helix typables (`:set`, `:sort`, `:reflow`, `:reload`, `:new`, `:open`, `:sh`, `:pipe`, `:w!`, `:wa`, `:qa!`, `:vsplit`, `:lsp-*`, `:config-*`, …).

### 7.8 Command-line / prompt UX (package panel; restyled by the app)

- A bottom panel `.cm-hx-command-panel` holds a flex row: [label + `<input>`] on the left and the minor indicator (min-width 8em, centred) on the right. The input row is `visibility:hidden` when idle.
- App CSS: `--secondary` background, 1px top border, 3×8px padding, 12px monospace, transparent input.
- Labels: `:`, `search:` (both `/` and `s`), `global-search:`. The placeholder shows the last value (`:` register or `/` register).
- **Completion popup:** positioned absolutely **above** the input. It contains a help box (`help` + `\nAliases: a, b`, preserved whitespace, 1px border) and a CSS grid of options (`repeat(auto-fill, minmax(10em,1fr))`) re-ordered column-major.
  - For `:`, the options are command **names** whose name or any alias starts with the first word. After a space, the options come from `command.autocomplete(args)`.
  - For search, the options are history entries starting with the input, sorted.
  - `Tab` / `Shift-Tab` cycle through the options, highlight the selected one (`.cm-hx-selected-option`, app: `--accent`), and replace the last word of the input.
- `ArrowUp` / `ArrowDown` walk the history (newest first) and put the caret at the end.
- `Ctrl-r <reg>` (`:` prompt only) appends the register's first value. Escape cancels the pending `Ctrl-r`.
- `Enter` accepts and `Escape` cancels. **Blur cancels.** IME composition is respected.
- Enter on an empty `:` prompt re-runs the last command (`:` register).
- A handler result `{message, error}` is shown in place of the label (red if `error`). Messages are cleared on the next selection change, or on a doc change while a message is displayed.
- Focus returns to the editor on the next animation frame.
- Status panel (hidden in the app): `NOR`/`INS`/`SEL` | `reg=x` | `line:col` (1-based, column in UTF-16 units from the main head).

### 7.9 Insert mode keys
| Key | Behaviour |
|---|---|
| `Escape` | Back to Normal. Commits the pending checkpoint. Empty carets become 1-char blocks. If `Ctrl-r` is pending, Escape only cancels that. |
| `Backspace`, `Delete`, `Enter` | `deleteCharBackward`, `deleteCharForward`, `insertNewlineAndIndent`. |
| `ArrowLeft`/`ArrowRight` | `cursorCharLeft/Right`. Other arrows, Home/End and all text input are native. |
| `Tab` / `Shift-Tab` | **`indentMore` / `indentLess` on the whole line** (Helix inserts indentation at the cursor). |
| `Ctrl-r <reg>` | Insert the register's contents at every caret (carets end up after the text, no selection). Stays in Insert. |
✗ Insert-mode Helix bindings `Ctrl-w`, `Alt-Backspace`, `Ctrl-u`, `Ctrl-k`, `Ctrl-h`, `Ctrl-d`, `Alt-d`, `Ctrl-j`, `Ctrl-x` (completion), `Ctrl-s`. Completion is the app's own (completion.ts). Helix insert input does not trigger `activateOnTyping`, and `Ctrl-Space` is swallowed, so the app opens the popup from an updateListener using per-provider triggers.

### 7.10 Cursor and selection rendering

- The block cursor is a mark decoration `.cm-hx-cursor` (background `#ccc`) on each range's head grapheme. At a line end or at EOF it is a widget `<span class="cm-hx-cursor cm-hx-cursor-endline"> </span>` (side 1).
- `.cm-hx-block-cursor .cm-cursor {display:none}` hides CodeMirror's caret. The class sits on `scrollDOM`. With `cursor-shape.insert="bar"` it is toggled off in Insert mode and the decorations are suppressed, so CodeMirror's bar caret shows. **In the app (default block shape) Insert mode also shows a block on the char after the caret, and CodeMirror's caret stays hidden.** All cursors render the same way, primary and secondary. The block shows even when the editor is unfocused.
- Cursor blink rate is 0.
- Selections are CodeMirror `drawSelection` backgrounds. The app colours them with `.cm-selectionBackground` = `color-mix(--primary 30%)`. The 1-char block range is therefore also painted as a selection under the cursor mark.
- App-specific workarounds that depend on this: tables (`tableStyle.ts`) and `shortcuts.ts` park the selection at a widget boundary because the helix block is drawn at `selection.head` regardless of DOM focus. Table widgets have their own hjkl/i/Enter/Esc cell navigation (see the tables section).

### 7.11 Not supported (out of scope for parity unless desired)

- Modes and submodes: view mode `z`/`Z`, window mode `Ctrl-w`/`Space w`, sticky modes, the picker UIs, `Space ?` command palette, `Space '` last picker, LSP (`Space k s S a r d D h`, `gd gy gr gi`), DAP (`Space g`), jump labels `gw`.
- Editing: `.` repeat, macros `q`/`Q`/`@`, `Alt-.`, `S` split, `K`/`Alt-K` keep/remove, `&`/`Alt-&` align, `Alt-C`, `Alt-,`, `Alt-(`/`Alt-)`, `X`/`Alt-x`, `W`/`B`/`E` (WORD motions), a real Helix `w`/`e` distinction, `Alt-J`, `md`/`mr` surround, `miw` and every text object except `m p ( ) [ ] { }` (and `< >` with grammar support), number-under-cursor increment and date increment, `=` format, shell pipes, `Alt-u`/`Alt-U`, `Alt-*`, `?` backward search, `Ctrl-s`/`Ctrl-o`/`Ctrl-i` jumplist, `Ctrl-e`/`Ctrl-y`/`Ctrl-f`/`Ctrl-b` scrolling, `[`/`]` unimpaired other than `[Space` / `]Space`, `<n>gg`, `G`, `gt gc gb gm ga g. g|`, `Space Y`, `Space D/d`.
- Counts on `w b e o O J > < Ctrl-a Ctrl-x C Ctrl-d/u gg`.
- Linewise paste semantics. Registers honoured by `d`/`c`. Shared registers across tabs (the app does not sync).
- Config: only `cursor-shape.insert` and `default-yank-register`. No `:set`, no keymap remapping, no soft-wrap-aware motions beyond what CodeMirror's visual-line `moveVertically` does incidentally.


---

