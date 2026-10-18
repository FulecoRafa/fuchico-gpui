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
