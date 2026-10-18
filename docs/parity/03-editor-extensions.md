## Editor Extensions: Links, Tags, Completion, Tasks, Attachments, Frontmatter, Backlinks, Tags View, Search

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
