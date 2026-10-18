## Settings, Themes & i18n

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
