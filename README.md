<div align="center">
  <img src="apps/fuchico/resources/icons/icon.png" alt="Fuchico" width="120" />

  # Fuchico (GPUI)

  **A native rewrite of [Fuchico](https://github.com/FulecoRafa/fuchico) in Rust and [GPUI](https://www.gpui.rs/): a notes editor with real Helix modal editing.**

  Rust · GPUI · no webview

  [Português (BR)](#português-br) · [English](#english)
</div>

---

## English

### What is this?

[Fuchico](https://github.com/FulecoRafa/fuchico) is a desktop notes editor built
with Tauri 2, React and CodeMirror 6. It pairs
[Helix](https://helix-editor.com/)-style modal editing with Obsidian-like
markdown live preview, a file explorer, a task and calendar view, and CalDAV
sync.

This repository is a from-scratch rewrite of that app as a native GPUI
application, with no webview. The goal is **feature parity** with the
original. Behaviour is checked against the original item by item; see
[docs/PARITY.md](docs/PARITY.md). Places where this version intentionally
differs are listed in [docs/DECISIONS.md](docs/DECISIONS.md).

> **Status:** work in progress. Most of the original works, but some parts are
> still missing; [docs/PARITY.md](docs/PARITY.md) tracks what is left. Only
> macOS is tested so far.

### Features

- **Helix modal editing.** The engine is our own Rust implementation of
  `codemirror-helix`, the Helix mode the original app uses, so the keys behave
  the same way.
- **Markdown live preview** in the editor: headings, tables, tasks, wikilinks
  (`[[Note#heading]]`) and code blocks.
- **Mermaid diagrams** are drawn by [pele](https://pele.run), the same renderer
  the original uses, running in an embedded QuickJS engine.
- **Excalidraw drawings** (`.excalidraw`) open in a native canvas.
- **Vault tools:** file explorer with a file watcher, command palette,
  full-text search, backlinks, tags and outline.
- **Agenda:** a task and calendar view with native CalDAV sync.
- **PDF export** of notes, using Typst.
- **Themes:** light and dark, plus a custom CSS theme editor.

### Building

You need a recent stable Rust toolchain (edition 2024).

```sh
cargo run -p fuchico -- [vault-or-file ...]
cargo test --workspace
apps/fuchico/scripts/bundle-macos.sh      # builds dist/Fuchico.app
```

More options are in [docs/PACKAGING.md](docs/PACKAGING.md).

### Layout

| Path | What it holds |
| --- | --- |
| `apps/fuchico` | The application: workspace, explorer, panels, settings, agenda, export |
| `crates/fuchico-core` | Vault, search, CalDAV and file logic, ported from the original's `src-tauri` |
| `crates/fuchico-helix` | The Helix modal editing engine (ropey) |
| `crates/fuchico-editor` | The GPUI editor view: rendering, find/replace, completion |
| `crates/fuchico-markdown` | Model for the markdown live preview and syntax highlighting |
| `crates/fuchico-mermaid` | Mermaid diagrams: pele in QuickJS, rasterised to PNG with resvg |
| `crates/fuchico-excalidraw` | The native Excalidraw canvas and `.excalidraw` file format |

### Main dependencies

- [gpui-kit](https://crates.io/crates/gpui-kit) — GPUI, the platform layer and
  [gpui-component](https://github.com/longbridge/gpui-component)
- [ropey](https://crates.io/crates/ropey) — text buffer
- [pulldown-cmark](https://crates.io/crates/pulldown-cmark),
  [syntect](https://crates.io/crates/syntect) and
  [two-face](https://crates.io/crates/two-face) — markdown and syntax highlighting
- [pele](https://github.com/obsidianmd/pele) (vendored, MIT) running in
  [rquickjs](https://crates.io/crates/rquickjs) — Mermaid
- [resvg](https://crates.io/crates/resvg) and
  [rustybuzz](https://crates.io/crates/rustybuzz) — drawing SVG and measuring text
- [roughr](https://crates.io/crates/roughr) — the hand-drawn look in Excalidraw
- [typst](https://crates.io/crates/typst) and
  [typst-pdf](https://crates.io/crates/typst-pdf) — PDF export
- [reqwest](https://crates.io/crates/reqwest),
  [quick-xml](https://crates.io/crates/quick-xml) and
  [icalendar](https://crates.io/crates/icalendar) — CalDAV
- [notify](https://crates.io/crates/notify) — vault file watcher
- [fancy-regex](https://crates.io/crates/fancy-regex) — Helix search
- [icu_collator](https://crates.io/crates/icu_collator) — locale-aware sorting

### Credits

- [Fuchico](https://github.com/FulecoRafa/fuchico), the original Tauri app
  this rewrite follows.
- [codemirror-helix](https://www.npmjs.com/package/codemirror-helix), which
  defines how the Helix keys behave, and [Helix](https://helix-editor.com/).
- [pele](https://github.com/obsidianmd/pele) by kepano.

### License

[MIT](LICENSE)

---

## Português (BR)

### O que é isto?

O [Fuchico](https://github.com/FulecoRafa/fuchico) é um editor de notas para
desktop feito com Tauri 2, React e CodeMirror 6. Ele junta a edição modal no
estilo [Helix](https://helix-editor.com/) com a pré-visualização de markdown
ao estilo Obsidian. Tem também um explorador de arquivos, uma visão de
tarefas e calendário e sincronização CalDAV.

Este repositório reescreve esse app do zero, como aplicação nativa em GPUI,
sem webview. A meta é **paridade de funcionalidades** com o original. O
comportamento é comparado com o original item a item; veja
[docs/PARITY.md](docs/PARITY.md). Os pontos em que esta versão difere de
propósito estão em [docs/DECISIONS.md](docs/DECISIONS.md).

> **Situação:** em andamento. A maior parte do original já funciona, mas
> algumas partes ainda faltam; [docs/PARITY.md](docs/PARITY.md) mostra o que
> falta. Por enquanto só o macOS foi testado.

### Funcionalidades

- **Edição modal Helix.** O motor é uma implementação própria, em Rust, do
  `codemirror-helix` (o modo Helix que o app original usa), então as teclas
  se comportam do mesmo jeito.
- **Pré-visualização de markdown** dentro do editor: títulos, tabelas,
  tarefas, wikilinks (`[[Nota#título]]`) e blocos de código.
- **Diagramas Mermaid**, desenhados pelo [pele](https://pele.run). É o mesmo
  renderizador do original, rodando num motor QuickJS embutido.
- **Desenhos Excalidraw** (`.excalidraw`) abrem num quadro nativo.
- **Ferramentas do cofre:** explorador de arquivos que acompanha mudanças no
  disco, paleta de comandos, busca no texto, backlinks, tags e sumário.
- **Agenda:** visão de tarefas e calendário com sincronização CalDAV nativa.
- **Exportação para PDF** das notas, via Typst.
- **Temas:** claro e escuro, além de um editor de tema em CSS.

### Compilação

Você precisa de uma versão estável e recente do Rust (edição 2024).

```sh
cargo run -p fuchico -- [cofre-ou-arquivo ...]
cargo test --workspace
apps/fuchico/scripts/bundle-macos.sh      # gera dist/Fuchico.app
```

Mais opções em [docs/PACKAGING.md](docs/PACKAGING.md).

### Estrutura

| Caminho | Conteúdo |
| --- | --- |
| `apps/fuchico` | O aplicativo: workspace, explorador, painéis, configurações, agenda, exportação |
| `crates/fuchico-core` | Lógica de cofre, busca, CalDAV e arquivos, portada do `src-tauri` do original |
| `crates/fuchico-helix` | O motor de edição modal Helix (ropey) |
| `crates/fuchico-editor` | A tela do editor em GPUI: desenho do texto, localizar/substituir, autocompletar |
| `crates/fuchico-markdown` | Modelo da pré-visualização de markdown e destaque de sintaxe |
| `crates/fuchico-mermaid` | Diagramas Mermaid: pele no QuickJS, convertido em PNG pelo resvg |
| `crates/fuchico-excalidraw` | O quadro nativo do Excalidraw e o formato de arquivo `.excalidraw` |

### Principais dependências

- [gpui-kit](https://crates.io/crates/gpui-kit): GPUI, a camada de plataforma
  e o [gpui-component](https://github.com/longbridge/gpui-component)
- [ropey](https://crates.io/crates/ropey): armazena o texto do editor
- [pulldown-cmark](https://crates.io/crates/pulldown-cmark),
  [syntect](https://crates.io/crates/syntect) e
  [two-face](https://crates.io/crates/two-face): markdown e destaque de sintaxe
- [pele](https://github.com/obsidianmd/pele) (código incluído no repositório,
  MIT), rodando no [rquickjs](https://crates.io/crates/rquickjs): Mermaid
- [resvg](https://crates.io/crates/resvg) e
  [rustybuzz](https://crates.io/crates/rustybuzz): desenho de SVG e medição de
  texto
- [roughr](https://crates.io/crates/roughr): o traço de desenho à mão do
  Excalidraw
- [typst](https://crates.io/crates/typst) e
  [typst-pdf](https://crates.io/crates/typst-pdf): exportação para PDF
- [reqwest](https://crates.io/crates/reqwest),
  [quick-xml](https://crates.io/crates/quick-xml) e
  [icalendar](https://crates.io/crates/icalendar): CalDAV
- [notify](https://crates.io/crates/notify): acompanha mudanças nos arquivos do
  cofre
- [fancy-regex](https://crates.io/crates/fancy-regex): busca do Helix
- [icu_collator](https://crates.io/crates/icu_collator): ordenação conforme o
  idioma

### Créditos

- [Fuchico](https://github.com/FulecoRafa/fuchico), o app original em Tauri
  que esta versão segue.
- [codemirror-helix](https://www.npmjs.com/package/codemirror-helix), que
  define como as teclas Helix se comportam, e o [Helix](https://helix-editor.com/).
- [pele](https://github.com/obsidianmd/pele), de kepano.

### Licença

[MIT](LICENSE)
