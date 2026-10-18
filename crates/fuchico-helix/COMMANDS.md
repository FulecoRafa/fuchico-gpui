# fuchico-helix commands

`fuchico-helix` ports the Helix modal editing that Fuchico gets from
[codemirror-helix](https://gitlab.com/_rvidal/codemirror-helix) 0.5.7. Where
cm-helix and real Helix disagree, it follows cm-helix.

**Status column:**

- **parity**: behaves like cm-helix.
- **quirk**: behaves like cm-helix where cm-helix differs from Helix. The
  description says how.
- **extension**: not in cm-helix. Only active when `Config::extensions` is
  set (`Config::default()` sets it; `Config::parity()` does not).

**Undo column:**

- **yes**: the command records an undo checkpoint.
- **session**: the command starts an insert session, which becomes a single
  undo step when you press `Escape`, and only if the document changed.

Keys use CodeMirror names. Tests feed them with `parse_keys`, which accepts
`<A-s>`, `<C-r>`, `<Esc>`, `<S-Tab>` and similar.

A typed character with no binding:

- in normal or select mode, is swallowed;
- in a pending minor mode (`g`, `m`, `Space`, …), cancels it.

A non-character key with no binding returns `Effect::Unhandled`, so the host
can handle it.

The tables below are generated from `keymap::bindings()` by
`keymap::commands_markdown()`, and a test (`commands_md_is_up_to_date`) keeps
them in sync. To regenerate:

```
UPDATE_COMMANDS_MD=1 cargo test -p fuchico-helix commands_md
```

## Bindings

<!-- BEGIN GENERATED BINDINGS -->
### insert

| Key | Command | Status | Undo | Description |
|---|---|---|---|---|
| `Backspace` | `delete_char_backward` | parity |  | Delete previous char (by indent unit in leading whitespace) |
| `Delete` | `delete_char_forward` | parity |  | Delete next char |
| `Enter` | `insert_newline_and_indent` | parity |  | Newline keeping indentation (continues lists/quotes with continue_markup) |
| `ArrowLeft` | `cursor_char_left` | parity |  | Move left |
| `ArrowRight` | `cursor_char_right` | parity |  | Move right |
| `Escape` | `normal_mode` | parity |  | Back to normal mode (commits the insert session to undo history) |
| `Ctrl-r` | `insert_register` | parity |  | Insert the contents of a register |
| `Tab` | `indent_more` | parity |  | Indent the selected lines |
| `Shift-Tab` | `indent_less` | parity |  | Dedent the selected lines |
| `ArrowUp` | `cursor_line_up` | parity |  | Move up (CodeMirror default keymap) |
| `ArrowDown` | `cursor_line_down` | parity |  | Move down (CodeMirror default keymap) |
| `Home` | `cursor_line_boundary_backward` | parity |  | Smart home (CodeMirror default keymap) |
| `End` | `cursor_line_boundary_forward` | parity |  | Line end (CodeMirror default keymap) |
| `Alt-Backspace` | `delete_group_backward` | parity |  | Delete word backward (CodeMirror default keymap, macOS) |
| `Ctrl-Backspace` | `delete_group_backward` | parity |  | Delete word backward (CodeMirror default keymap) |
| `Alt-Delete` | `delete_group_forward` | parity |  | Delete word forward (CodeMirror default keymap, macOS) |
| `Ctrl-Delete` | `delete_group_forward` | parity |  | Delete word forward (CodeMirror default keymap) |
| `Meta-Backspace` | `delete_line_boundary_backward` | parity |  | Delete to line start (CodeMirror default keymap, macOS) |
| `Meta-Delete` | `delete_line_boundary_forward` | parity |  | Delete to line end (CodeMirror default keymap, macOS) |
| `Ctrl-w` | `delete_word_backward` | extension |  | Delete word backward (Helix) |
| `Ctrl-u` | `kill_to_line_start` | extension |  | Delete to line start (Helix) |
| `Ctrl-k` | `kill_to_line_end` | extension |  | Delete to line end (Helix) |

### normal/select

| Key | Command | Status | Undo | Description |
|---|---|---|---|---|
| `Escape` | `normal_mode` | quirk |  | Reset to normal mode from any pending state (from select mode too; never steps back to select) |
| `/` | `search` | parity |  | Search forward (prompt) |
| `s` | `select_regex` | parity |  | Select regex matches inside the selections (prompt) |
| `Alt-s` | `split_selection_on_newline` | parity |  | Split selections into lines |
| `,` | `keep_primary_selection` | parity |  | Keep only the primary selection |
| `(` | `rotate_selections_backward` | parity |  | Make the previous selection primary |
| `)` | `rotate_selections_forward` | parity |  | Make the next selection primary |
| `0`–`9` | `count` | parity |  | Count prefix (a lone 0 is ignored) |
| `:` | `command_mode` | parity |  | Open the command line |
| `y` | `yank` | quirk |  | Yank selections (always returns to normal mode, even from select) |
| `a` | `append_mode` | parity | session | Insert after the selection |
| `A` | `insert_at_line_end` | parity | session | Insert at line end |
| `I` | `insert_at_line_start` | parity | session | Insert at the first non-blank |
| `C` | `copy_selection_on_next_line` | quirk | yes | Copy the primary selection to the next line where it fits (direction of the copy is flipped) |
| `Alt-C` | `copy_selection_on_prev_line` | extension | yes | Copy the primary selection to the previous line where it fits |
| `c` | `change_selection` | parity | session | Delete selections (yank to the default register) and insert |
| `d` | `delete_selection` | quirk | yes | Delete selections (yanks to the default register, ignores "x; count/register are not reset) |
| `Alt-c` | `change_selection_noyank` | parity | session | Change without yanking |
| `Alt-d` | `delete_selection_noyank` | parity | yes | Delete without yanking |
| `P` | `paste_before` | quirk | yes | Paste before (with a count, the selection covers one copy only) |
| `p` | `paste_after` | quirk | yes | Paste after (with a count, the selection covers one copy only) |
| `R` | `replace_with_yanked` | parity | yes | Replace selections with the register contents |
| `r` | `replace` | quirk | yes | Replace every grapheme of the primary selection with a char (newlines too) |
| `w` | `move_next_word_start` | quirk |  | Next word (count ignored) |
| `e` | `move_next_word_end` | quirk |  | Same as w (cm-helix quirk) |
| `b` | `move_prev_word_start` | quirk |  | Previous word (count ignored) |
| `W` | `move_next_long_word_start` | extension |  | Next WORD |
| `E` | `move_next_long_word_end` | extension |  | Next WORD (same as W, like e) |
| `B` | `move_prev_long_word_start` | extension |  | Previous WORD |
| `v` | `select_mode` | parity |  | Toggle select mode |
| `g` | `goto_mode` | parity |  | Goto prefix |
| `Space` | `space_mode` | parity |  | Space prefix |
| `m` | `match_mode` | parity |  | Match prefix |
| `z` | `view_mode` | extension |  | View prefix |
| `i` | `insert_mode` | parity | session | Insert before the selection |
| `h` | `move_char_left` | parity |  | Left |
| `j` | `move_visual_line_down` | quirk |  | Down (single cursor forgets its goal column) |
| `k` | `move_visual_line_up` | quirk |  | Up (single cursor forgets its goal column) |
| `l` | `move_char_right` | parity |  | Right |
| `ArrowDown` | `move_visual_line_down` | parity |  | alias of j |
| `ArrowUp` | `move_visual_line_up` | parity |  | alias of k |
| `ArrowRight` | `move_char_right` | parity |  | alias of l |
| `ArrowLeft` | `move_char_left` | parity |  | alias of h |
| `%` | `select_all` | parity |  | Select the whole document |
| `o` | `open_below` | parity | session | Open a line below (keeps indentation / list markers with continue_markup) |
| `O` | `open_above` | parity | session | Open a line above (keeps indentation / list markers with continue_markup) |
| `f` | `find_next_char` | quirk |  | Find char forward (drops a pending register) |
| `F` | `find_prev_char` | quirk |  | Find char backward (drops a pending register) |
| `t` | `find_till_char` | quirk |  | Till char forward (drops a pending register) |
| `T` | `till_prev_char` | quirk |  | Till char backward (drops a pending register) |
| `u` | `undo` | parity |  | Undo |
| `U` | `redo` | parity |  | Redo |
| `x` | `extend_line_below` | quirk |  | Select line / extend to the next line (a count always extends) |
| `n` | `search_next` | quirk |  | Next match (count not reset) |
| `N` | `search_prev` | quirk |  | Previous match (cm-helix ring-buffer bug with counts) |
| `Ctrl-a` | `increment` | quirk | yes | Increment the selected number (JS Number() of the whole selection) |
| `Ctrl-x` | `decrement` | quirk | yes | Decrement the selected number (JS Number() of the whole selection) |
| `Ctrl-d` | `page_cursor_half_down` | parity |  | Half page down |
| `Ctrl-u` | `page_cursor_half_up` | parity |  | Half page up |
| `PageDown` | `page_cursor_half_down` | parity |  | alias of Ctrl-d |
| `PageUp` | `page_cursor_half_up` | parity |  | alias of Ctrl-u |
| `;` | `collapse_selection` | parity |  | Collapse to the cursor |
| `Alt-;` | `flip_selections` | parity |  | Flip anchor and head |
| `Alt-:` | `ensure_selections_forward` | parity |  | Make selections forward |
| `Alt-ArrowUp` | `expand_selection` | quirk |  | Expand to the parent syntax node (host syntax tree; history freezes when there is no parent) |
| `Alt-ArrowDown` | `shrink_selection` | parity |  | Undo the last expansion |
| `Alt-ArrowRight` | `select_next_sibling` | quirk |  | Next syntax sibling (no tree: selects the whole doc) |
| `Alt-ArrowLeft` | `select_prev_sibling` | quirk |  | Previous syntax sibling (no tree: selects the whole doc) |
| `Alt-o` | `expand_selection` | parity |  | alias of Alt-ArrowUp |
| `Alt-i` | `shrink_selection` | parity |  | alias of Alt-ArrowDown |
| `Alt-n` | `select_next_sibling` | parity |  | alias of Alt-ArrowRight |
| `Alt-p` | `select_prev_sibling` | parity |  | alias of Alt-ArrowLeft |
| `Ctrl-c` | `toggle_comments` | parity | yes | Toggle comments (block comments by line for Markdown) |
| `>` | `indent` | parity | yes | Indent |
| `<` | `unindent` | parity | yes | Dedent |
| ``` | `switch_to_lowercase` | parity | yes | Lower case |
| `Alt-`` | `switch_to_uppercase` | parity | yes | Upper case |
| `~` | `switch_case` | parity | yes | Toggle case |
| `*` | `search_selection` | parity |  | Use the selections as the search pattern |
| `_` | `trim_selections` | parity |  | Trim whitespace from the selections |
| `Home` | `goto_line_start` | parity |  | Line start |
| `End` | `goto_line_end` | parity |  | Line end |
| `J` | `join_selections` | parity | yes | Join lines (primary selection) |
| `[` | `left_bracket_mode` | parity |  | [ prefix |
| `]` | `right_bracket_mode` | parity |  | ] prefix |
| `"` | `select_register` | parity |  | Select a register for the next command |
| `Tab` | `no_op` | parity |  | Swallowed |
| `Shift-Tab` | `no_op` | parity |  | Swallowed |
| `S` | `split_selection` | extension |  | Split selections on regex matches (prompt) |
| `K` | `keep_selections` | extension |  | Keep selections matching a regex (prompt) |
| `Alt-K` | `remove_selections` | extension |  | Remove selections matching a regex (prompt) |

### goto (g)

| Key | Command | Status | Undo | Description |
|---|---|---|---|---|
| `g` | `goto_file_start` | parity |  | Document start |
| `e` | `goto_last_line` | quirk |  | Last line (the line before a trailing empty line) |
| `h` | `goto_line_start` | parity |  | Line start |
| `j` | `move_line_down` | parity |  | Down |
| `k` | `move_line_up` | parity |  | Up |
| `l` | `goto_line_end` | parity |  | Line end |
| `s` | `goto_first_nonwhitespace` | parity |  | First non-blank |
| `n` | `goto_next_buffer` | parity |  | Next buffer (external) |
| `p` | `goto_previous_buffer` | parity |  | Previous buffer (external) |
| `t` | `goto_window_top` | extension |  | Top of the window |
| `c` | `goto_window_center` | extension |  | Center of the window |
| `b` | `goto_window_bottom` | extension |  | Bottom of the window |

### match (m)

| Key | Command | Status | Undo | Description |
|---|---|---|---|---|
| `s` | `surround_add` | parity | yes | Surround the selections with a pair |
| `m` | `match_brackets` | quirk |  | Jump to the matching bracket (quotes are not matched) |
| `a` | `select_textobject_around` | quirk |  | Select around a pair / m (closest pair) / p (paragraph); w and quotes with extensions |
| `i` | `select_textobject_inner` | quirk |  | Select inside a pair / m / p; w and quotes with extensions |
| `d` | `surround_delete` | extension | yes | Delete the surrounding pair |
| `r` | `surround_replace` | extension | yes | Replace the surrounding pair |

### space

| Key | Command | Status | Undo | Description |
|---|---|---|---|---|
| `y` | `yank_to_clipboard` | parity |  | Yank to the clipboard (+) |
| `p` | `paste_clipboard_after` | parity | yes | Paste the clipboard after |
| `P` | `paste_clipboard_before` | parity | yes | Paste the clipboard before |
| `R` | `replace_selections_with_clipboard` | quirk | yes | Replace selections with the clipboard (cursors end up after the text) |
| `f` | `file_picker` | parity |  | File picker (external) |
| `b` | `buffer_picker` | parity |  | Buffer picker (external) |
| `/` | `global_search` | parity |  | Global search (external) |
| `c` | `toggle_comments` | parity | yes | Toggle comments |

### left bracket ([)

| Key | Command | Status | Undo | Description |
|---|---|---|---|---|
| `0`–`9` | `count` | quirk |  | Count digits are accepted after [ |
| `Space` | `add_newline_above` | parity | yes | Add blank lines above |

### right bracket (])

| Key | Command | Status | Undo | Description |
|---|---|---|---|---|
| `0`–`9` | `count` | quirk |  | Count digits are accepted after ] |
| `Space` | `add_newline_below` | parity | yes | Add blank lines below |

### view (z)

| Key | Command | Status | Undo | Description |
|---|---|---|---|---|
| `z` | `align_view_center` | extension |  | Center the cursor line |
| `c` | `align_view_center` | extension |  | Center the cursor line |
| `t` | `align_view_top` | extension |  | Cursor line to the top |
| `b` | `align_view_bottom` | extension |  | Cursor line to the bottom |
| `j` | `scroll_down` | extension |  | Scroll down one line |
| `k` | `scroll_up` | extension |  | Scroll up one line |
| `ArrowDown` | `scroll_down` | extension |  | Scroll down one line |
| `ArrowUp` | `scroll_up` | extension |  | Scroll up one line |

<!-- END GENERATED BINDINGS -->

### Pending keys (not table entries)

| Keys | Effect | Status |
|---|---|---|
| `f`/`t`/`F`/`T` `<char>` | Find or till a char. Takes a count. Multi-line. | quirk (drops a pending register) |
| `r<char>` | Replace every grapheme of the primary selection. | quirk |
| `"<reg>` | Select a register for the next command. | parity |
| `ms<char>` | Surround with the pair for `<char>`. | parity |
| `ma<char>` / `mi<char>` | Select around or inside a pair. `m` gives the closest bracket pair and `p` gives the paragraph. | quirk |
| `maw`/`miw`/`maW`/`miW` | Select around or inside a word or WORD. | extension |
| `ma"`/`mi"` (also `'` and `` ` ``) | Select around or inside a quoted span on the cursor's line. | extension |
| `md<char>` | Delete the surrounding pair. | extension |
| `mr<from><to>` | Replace the surrounding pair. | extension |
| insert `Ctrl-r <reg>` | Insert a register's contents. | parity |

## Prompts

The host draws the prompt and drives it through:

- `Editor::prompt_input(value)`, called on every change;
- `Editor::prompt_close(accept)`;
- `Editor::prompt_completions`;
- `Editor::prompt_accept_completion`;
- `Editor::prompt_history`.

**Search prompts:**

| Key | Prompt | Behaviour | Status |
|---|---|---|---|
| `/` | `search:` | Previews the next match after the selection, wrapping to the start. | parity |
| `s` | `search:` | Selects every match inside the selections. | parity |
| `Space /` | `global-search:` | Sends `External::GlobalSearch` (with the input, or the `/` register when the input is empty). | parity |
| `S` | `split:` | Splits the selections on matches. | extension |
| `K` | `keep:` | Keeps the selections that match. | extension |
| `Alt-K` | `remove:` | Removes the selections that match. | extension |

Accepting a search prompt always yanks the input, even an empty one, to the
`/` register. Cancelling restores the selection and emits
`Scroll::RestorePromptStart`.

### Typable commands (`:`)

| Command | Aliases | Notes | Status |
|---|---|---|---|
| `:<n>` | | Previews line `n` (centred). Cancel restores the selection. | parity |
| `goto <n>` | `g` | Leaves an empty cursor at the line start. Errors: `Line number required`, `Invalid line number`. | quirk (bar cursor) |
| `clipboard-yank` | | Copies the main selection to the host clipboard. | parity |
| `clear-register [r]` | | Clears one register, or all of them. | parity |
| `echo …` | | Shows its arguments in the status line. | parity |
| `theme [name]` | | Only when `Config::themes` is non-empty. Autocompletes theme names. | parity |
| `buffer-next` | `bn`, `bnext` | Only when `ExternalCommands::buffer_next` is set. | parity |
| `buffer-previous` | `bp`, `bprev` | Only when `ExternalCommands::buffer_previous` is set. | parity |
| `buffer-close [names…]` | `bc`, `bclose` | Only when `ExternalCommands::buffer_close` is set. | parity |
| `write` | `w` | Emits `Effect::Save`. | extension (`Config::app_commands`) |
| `quit` | `q` | Emits `Effect::Quit`. | extension (`Config::app_commands`) |
| `write-quit` | `wq`, `x` | Emits `Effect::Save` and then `Effect::Quit`. | extension (`Config::app_commands`) |
| host commands | | Added with `Editor::register_command`. Running one emits `Effect::HostCommand`. | extension |

The command line otherwise behaves like cm-helix:

- An unknown command shows the error `no such command: 'x'`.
- Accepting an empty line reruns the last entry in the `:` register.
- Words are split with JS `split(/ +/)`, after trimming trailing spaces.

## Reproduced cm-helix quirks

- `e` is the same as `w`.
- `w` and `b` ignore their count and do not reset it.
- `j`/`k` with a single cursor forget the goal column. CodeMirror's
  `EditorSelection.single` drops it.
- `C` flips the direction of the copied selection.
- `d` ignores `"x` and leaves the count and register pending.
- `p`/`P` with a count insert *n* copies but select only one of them.
- Pasting an empty register leaves a cursor.
- `r` acts on the primary selection only, and replaces newlines too.
- `J` joins only the primary selection and drops the others.
- `Ctrl-a`/`Ctrl-x` apply JS `Number()` to the whole selection, so `0x1`,
  `1.5` and `NaN` behave as they do in JS.
- In insert mode, `Enter` with a selection in the leading whitespace keeps
  that whitespace (CodeMirror's `insertNewlineAndIndent` slice bug).
- `goto` and `:<n>` leave an empty (bar) cursor.
- `y` always returns to normal mode, even from select mode.
- `N` with a count goes through cm-helix's `Ring`, whose `start` is off by one
  once the ring is full. For example, `2N` lands on the closest previous
  match.
- `n` keeps the count after a successful search.
- `mi(` with the cursor *on* an opening bracket selects the enclosing pair,
  because the backward scan stops before the head.
- `mip` and `map` are the same. The range runs to the next empty line,
  including that line's newline.
- In parity mode, `mi"` and `miw` are no-ops. cm-helix treats them as
  self-paired delimiters that `matchBrackets` never matches.
- `Escape` from select mode goes to normal mode.
- `Alt-o` freezes the syntax history when there is no parent node.

## cm-helix behaviour that is not reproduced

**Not reproduced because it is a bug with no visible effect, or because a
UI-less engine cannot express it:**

- **Two checkpoints per dispatch, and `matchInBrackets` dispatching inside its
  loop.** The engine records one checkpoint per command, and `mam`/`mim`
  apply all their ranges at once. The text and selections that result are the
  same.
- **Async clipboard.** `Host::clipboard_read` is synchronous. A host that
  needs an async read can prefetch the clipboard. A read error is shown as an
  error message, as cm-helix does with a rejected promise.
- **Async themes, popup layout, automatic clearing of status messages.** These
  are host UI concerns. The engine only emits `Effect::ChangeTheme` and
  `Effect::Message`.
- **Result messages for `gn`/`gp`.** The host owns buffers and gets
  `External::BufferNext` / `BufferPrevious`.
- **Pixel goal columns.** Goals are char columns. `VisualLines::move_vertically` lets a host with soft wrapping supply visual-line motion.
- **UTF-16 offsets.** Positions are Unicode scalar (char) offsets.
  `TextEdit` also carries byte offsets. Lengths that cm-helix measures in
  UTF-16 (such as the paste selection) are measured in chars.

**Not reproduced because the engine does not depend on a CodeMirror
facility:**

- **Typing a bare number on the command line.** cm-helix errors on accept. The
  engine moves to the line silently, as the preview already did.
- **JS regular expressions.** Search uses the Rust `regex` crate, so
  lookaround and backreferences are unsupported. `*` escapes with
  `regex::escape` instead of `RegExp.escape`. Invalid patterns report
  `Invalid regex /…/: <regex error>`.
- **Syntax-tree bracket matching.** `mm`, `ma(` and `mi(` use a plain-text
  scan, so brackets inside strings and comments are not skipped. Syntax
  expand, shrink and sibling selection are delegated to
  `Host::syntax_parent` / `Host::syntax_sibling`. With no tree, siblings
  select the whole document.
- **Fall-through to CodeMirror's default keymap.** Only the defaults that
  Fuchico relies on in insert mode are ported: arrows, Home/End, the
  word-delete and line-delete keys, Tab/Shift-Tab. Other keys return
  `Effect::Unhandled`. Neither `Mod-z` nor `closeBrackets` is ported.
- **Markdown `insertNewlineContinueMarkup`.** This is replaced by the
  `Config::continue_markup` option, which continues lists, task lists (`- [x]`
  becomes `- [ ] `), ordered lists and quotes on `Enter`, `o` and `O`.
- **`Alt-s` on a selection with no lines.** cm-helix throws; the engine leaves
  the selection unchanged.
