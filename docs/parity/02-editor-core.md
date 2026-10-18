## Editor Core and Display Extensions

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
