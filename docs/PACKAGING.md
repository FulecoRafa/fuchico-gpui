# Building and packaging

## Run from source

```sh
cargo run -p fuchico -- [vault-or-file ...]
FUCHICO_BACKGROUND=1 cargo run -p fuchico   # never activates or focuses its window
FUCHICO_CONFIG_DIR=/tmp/fuchico-cfg cargo run -p fuchico   # scratch settings
```

## macOS app bundle

```sh
apps/fuchico/scripts/bundle-macos.sh        # cargo build --release, then "dist/Fuchico GPUI.app"
TARGET=x86_64-apple-darwin apps/fuchico/scripts/bundle-macos.sh   # Intel build
PROFILE=debug NO_BUILD=1 OUT_DIR=/tmp/out apps/fuchico/scripts/bundle-macos.sh
```

The script needs no extra tools. It copies the binary, `resources/icons/icon.icns`
and `resources/macos/Info.plist` (the version comes from the workspace `Cargo.toml`)
and checks the plist with `plutil -lint`. The bundle is unsigned, like the original's
releases. After copying it to Applications, run
`xattr -dr com.apple.quarantine "/Applications/Fuchico GPUI.app"` once.

`cargo bundle --release -p fuchico` works too: `[package.metadata.bundle]` in
`apps/fuchico/Cargo.toml` mirrors the same metadata. The document types come from
`resources/macos/document-types.plist`, through `osx_info_plist_exts`.

The bundle mirrors `src-tauri/tauri.conf.json`, except for the name and identifier: the GPUI build installs next to the Tauri app with its own data.

| Key | Value |
|---|---|
| Identifier | `dev.fuleco.fuchico-gpui` |
| Name | Fuchico GPUI |
| Category | Productivity (`public.app-category.productivity`) |
| Minimum macOS | 11.0 |
| Icons | the original's `src-tauri/icons`, copied to `apps/fuchico/resources/icons` |
| Document types | `.md` and `.markdown`, "Markdown", role Editor, rank Default |

### Opening files

Finder's "Open With" and double-clicking a note send the files to the app through
`application:openURLs:`, as `file://` URLs. gpui registers that handler, and AppKit
uses it instead of `application:openFiles:`. `main` hands the URLs to
`windows::paths_from_urls` and then `push_open_requests`.

Command-line paths take the same queue. When an instance is already running, a second
launch forwards its argv over the single-instance socket and exits. The main workspace
then routes each path:

- the vault root when none is open;
- a tab when the file is inside the vault;
- an editor window otherwise.

This is the original's second-launch path.

## Logs

The logger mirrors `tauri_plugin_log::Builder::new()`. It writes to stdout and to
`Fuchico.log` in the app log dir:

- macOS: `~/Library/Logs/dev.fuleco.fuchico-gpui/`
- elsewhere: `<local data dir>/dev.fuleco.fuchico-gpui/logs/`

The level is Trace, and `RUST_LOG=<level>` overrides it. The format is
`[date][time][target][LEVEL]` in UTC. Rotation is "keep one": at 40 kB the file is
deleted and a new one starts.

## Known limitations

- **File-type filters in dialogs:** gpui 0.3.9's `prompt_for_new_path(dir, name)` and
  `prompt_for_paths(PathPromptOptions)` take no extension filters, on any platform. The
  save and open dialogs for HTML export and for theme import and export therefore can't
  restrict the file type, as the original's `{ name: "HTML", extensions: ["html"] }` and
  `{ name: "CSS", extensions: ["css"] }` did. To compensate:
  - the save dialogs pre-fill a name with the right extension;
  - a missing `.html` or `.css` extension is added after the dialog returns;
  - theme import rejects files that are not `.css`, with an error.
- **Zoom keys and the numpad:** the original matched physical keys (`e.code`: `Equal` /
  `NumpadAdd`, `Minus` / `NumpadSubtract`, `Digit0` / `Numpad0`) with `ctrlKey || metaKey`.
  gpui reports characters, not key codes. Numpad- and Numpad0 arrive as `-` and `0`, so
  they work. But on macOS, Cmd+Numpad+ arrives as `cmd-+`, the same keystroke as
  Cmd-Shift-=. It therefore zooms the UI, not the editor font. Ctrl works as Mod on
  macOS as in the original.
