## Backend (Rust core: `src-tauri/src/**`, Cargo.toml, tauri.conf.json, capabilities)

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
