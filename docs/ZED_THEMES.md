# Zed themes and icon themes

An extra of the GPUI port (the Tauri app has no equivalent; see
DECISIONS.md). Fuchico reads colour themes and file icon themes from Zed
editor extensions. Code: `apps/fuchico/src/zed_themes/`.

## Sources

Two kinds of extension root are scanned at startup and again after each import:

1. **Imported copies**: `<config dir>/zed-themes/<extension id>/`. The config dir is `FUCHICO_CONFIG_DIR` if set, else the app config dir. These win when a theme name also appears in an installed extension.
2. **Zed's installed extensions**, read in place and never written to:
   - macOS: `~/Library/Application Support/Zed/extensions/installed/`
   - Linux: `$XDG_DATA_HOME/zed/extensions/installed/` (`~/.local/share/...`)
   - Windows: `%LOCALAPPDATA%\Zed\extensions\installed\`
   - `FUCHICO_ZED_EXTENSIONS_DIR` (a path list) replaces these, for development.

Each extension holds an `extension.toml` (`id`, `name`, `version`, and
optionally `themes` / `icon_themes` lists), `themes/*.json` (colour
theme families) and `icon_themes/*.json` (icon theme families). A JSON file
can define several themes, each with a `name` and an `appearance`.

The following are logged and skipped, never fatal:
- unreadable or invalid JSON (comments and trailing commas are accepted)
- a broken manifest
- a theme entry with the wrong shape
- an icon path that is missing, absolute or escapes the extension
- an SVG that fails to render (the Lucide icon is drawn instead)

## Import

Go to Settings, then Appearance, then Zed themes, then **Import from Zed...**.
It accepts an extension folder or a single theme `.json`. The import:

1. Validates the files, then copies them to `<config>/zed-themes/<id>/`, keeping Zed's layout:
   - `extension.toml` (a minimal one is written when a single `.json` is imported)
   - `themes/`
   - `icon_themes/`
   - the icon SVGs that the icon themes reference
2. Keeps icon paths relative to the extension root, as in Zed. For a single icon theme `.json`, the root is the parent of its `icon_themes/` folder, else the folder the JSON is in.
3. Writes into a hidden scratch dir and swaps it in, so importing the same id again replaces the old copy.

## Settings

Both are stored in `helix.editorSettings` and omitted when unset, so the
JSON stays the same as the original's:

- `"palette": "zed"` together with `"zedTheme": "<theme name>"` selects a Zed colour theme.
  - The light/dark mode picks the theme's counterpart in the same family: the name with Dark/Light swapped, else the first theme of the other appearance.
  - A theme with no counterpart fixes the appearance, the way Dracula does. The mode buttons and the toggle-theme command are then disabled.
  - Clicking a theme in Settings paints exactly that theme, and the mode follows it.
  - If the theme has gone missing, the Ayu palette is used instead.
- `"iconTheme": "<icon theme name>"` sets the file icons. When it is absent, or names a theme that no longer exists, the built-in Lucide icons are used.

The icons are used in:
- explorer rows, including folders: named folder icons, and expanded or collapsed state
- editor tabs, which only show icons while an icon theme is active, to match the original
- quick open

Icon lookup follows Zed's order. For files:
1. `file_stems` (full file name)
2. `file_suffixes` (the longest compound suffix first, e.g. `test.ts` before `ts`)
3. the `default` icon

For folders, `named_directory_icons` is checked before `directory_icons`.

The former `FUCHICO_ICON_THEME` env override has been removed in favour of
this setting. `FUCHICO_BADGE` is unchanged.

## Colour mapping

For each token, the first Zed key that is present wins. Missing or
unparsable keys keep the Ayu token for the theme's appearance.

| Fuchico token            | Zed style keys                                                   |
|--------------------------|------------------------------------------------------------------|
| `--background`           | `editor.background`, `background`                                |
| `--foreground`           | `editor.foreground`, `text`                                      |
| `--card`                 | `panel.background`, `tab_bar.background`, `surface.background`, `background` |
| `--card-foreground`      | `text`, `editor.foreground`                                      |
| `--primary`              | `text.accent`, `icon.accent`, `accents[0]`, `players[0].cursor`   |
| `--primary-foreground`   | `editor.background`, `background`                                |
| `--secondary`            | `element.background`, `surface.background`                       |
| `--secondary-foreground` | `text`                                                           |
| `--muted`                | `surface.background`, `element.background`                       |
| `--muted-foreground`     | `text.muted`, `editor.line_number`                               |
| `--accent`               | `element.selected`, `ghost_element.selected`, `element.hover`    |
| `--accent-foreground`    | `text`                                                           |
| `--destructive`          | `error`, `version_control.deleted`, `deleted`                    |
| `--border`               | `border.variant`, `border`                                       |
| `--input`                | `border`, `border.variant`                                       |
| `--ring`                 | `border.focused`, `text.accent`                                  |
| `--syntax-keyword`       | `syntax.keyword`                                                 |
| `--syntax-string`        | `syntax.string`                                                  |
| `--syntax-number`        | `syntax.number`, `syntax.constant`, `syntax.boolean`             |
| `--syntax-function`      | `syntax.function`, `syntax.link_text`, `syntax.title`            |
| `--syntax-type`          | `syntax.type`, `syntax.constructor`                              |
| `--syntax-comment`       | `syntax.comment`, `syntax.comment.doc`                           |

Fuchico also uses these Zed colours, which are outside its tokens:
- caret and text selection come from `players[0].cursor` and `players[0].selection`
- `success`, `warning` and `info` come from the keys of the same names

These are applied to the library widgets (gpui-component). The markdown
editor element still takes its selection colour from `--ring`.
