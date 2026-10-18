# fuchico-core

Tauri-free backend and pure-logic port of Fuchico (obsidian-helix) for the GPUI app.

## Command mapping

| Tauri command | fuchico-core |
|---|---|
| `fs_read_file`, `fs_write_file`, `fs_write_binary`, `fs_stat`, `fs_canonicalize` | `fs::file::*` (`fs_write_file` takes `&dyn EventSink`) |
| `fs_read_dir`, `fs_list_markdown_files` | `fs::tree::*` |
| `fs_create_file`, `fs_create_dir`, `fs_rename`, `fs_delete` | `fs::mutate::*` |
| `search_files`, `search_replace_files` | `search::*` |
| `tags_scan` / `tasks_scan`, `tasks_toggle` / `links_scan` | `tags`, `tasks`, `links` |
| `fonts_list_system` | `fonts::fonts_list_system` |
| `caldav_*` | `caldav::commands::*` (take `&CalDavCtx { data_dir, sink }`; async on `runtime`) |
| open-files event / state | `open_files::OpenRequests`, `route_open_request` |
| Tauri events | `CoreEvent` + `EventSink` (also `crossbeam_channel::Sender`, closures) |
| AppHandle path resolver | `paths::*` (identifier `dev.fuleco.fuchico`) |
| (new) | `watcher::VaultWatcher` emits `CoreEvent::FsChanged` |

Frontend logic ports: `frontmatter`, `fuzzy`, `parse_tags`, `wikilinks`, `backlinks`, `templates`
(daily notes), `task_helpers`, `render_html`, `attachments`, `editor_window`, `doc_stats`,
`outline`, `agenda`, `shortcuts`.
