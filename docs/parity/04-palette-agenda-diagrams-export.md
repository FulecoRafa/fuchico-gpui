## Command Palette, Quick Switcher, Templates & Daily Notes, Agenda, Excalidraw, Mermaid Diagrams, Export

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
