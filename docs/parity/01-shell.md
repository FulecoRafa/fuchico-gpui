## App Shell, Layout, Windows, Tabs, Explorer

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
