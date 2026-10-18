//! The command registry (`buildAppCommands`, spec 4 §4.3) and the palette
//! ranking (spec 4 §4.1/§4.2). Pure: no GPUI, so it is unit-tested here; the
//! palette view executes the [`CommandAction`]s (see `run.rs`).

use std::sync::Arc;

use fuchico_core::fuzzy::fuzzy_match;
use fuchico_core::templates::{list_templates, stem};

use crate::i18n::{Msg, tr};
use crate::keymap::{SHORTCUT_ACTIONS, display_binding};
use crate::settings::ui_state::SettingsSection;
use crate::settings::{PaletteId, Settings};
use crate::workspace::MainViewKind;

/// What running a command does; executed against the workspace.
#[derive(Clone, Debug, PartialEq)]
pub enum CommandAction {
    SetView(MainViewKind),
    /// A `SHORTCUT_ACTIONS` id, dispatched to the editor pane.
    EditorAction(&'static str),
    OpenSettings(SettingsSection),
    OpenFolder,
    QuickOpen,
    Save,
    SaveAndClose,
    CloseActiveTab,
    CloseAllTabs,
    OpenDailyNote,
    /// Absolute template path.
    NewFromTemplate(String),
    OpenInNewWindow,
    ToggleBacklinks,
    /// Absolute path of the active note.
    ExportHtml(String),
    ExportPdf(String),
    PrintPdf(String),
    ToggleTheme,
    SelectPalette(PaletteId),
    SelectCustomTheme(String),
    /// 1-based line.
    GoToLine(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppCommand {
    pub id: String,
    pub title: String,
    /// Extra terms matched (not highlighted).
    pub keywords: Vec<String>,
    /// Raw binding hint (`Mod-Shift-p`), formatted at render time.
    pub binding: Option<String>,
    /// `:` aliases, matched by prefix for `:` queries.
    pub aliases: &'static [&'static str],
    pub action: CommandAction,
}

/// The parts of the app state the registry depends on (`CommandContext`).
#[derive(Clone, Debug, Default)]
pub struct CommandContext {
    pub root: Option<String>,
    pub active_path: Option<String>,
    pub has_open_tabs: bool,
    pub backlinks_open: bool,
    /// Absolute Markdown paths of the vault (`fs_list_markdown_files`).
    pub md_files: Arc<Vec<String>>,
}

/// `SETTINGS_SECTIONS` labels and extra keywords.
fn section_info(s: SettingsSection) -> (Msg, &'static [&'static str]) {
    match s {
        SettingsSection::Appearance => (
            Msg::SettingsNavAppearance,
            &["theme", "font", "zoom", "language", "idioma"],
        ),
        SettingsSection::Editor => (
            Msg::SettingsNavEditor,
            &["keybindings", "helix", "vim", "folding", "behavior"],
        ),
        SettingsSection::Shortcuts => (Msg::SettingsNavShortcuts, &["keys", "hotkeys"]),
        SettingsSection::Vault => (
            Msg::SettingsNavVault,
            &["folders", "daily", "templates", "external tool"],
        ),
        SettingsSection::Integrations => (Msg::SettingsNavIntegrations, &["caldav", "sync"]),
        SettingsSection::About => (Msg::SettingsNavAbout, &["version"]),
    }
}

fn kw(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| s.to_string()).collect()
}

fn cmd(
    id: impl Into<String>,
    title: String,
    keywords: Vec<String>,
    action: CommandAction,
) -> AppCommand {
    AppCommand {
        id: id.into(),
        title,
        keywords,
        binding: None,
        aliases: &[],
        action,
    }
}

fn t(msg: Msg) -> String {
    tr(msg, &[])
}

fn t_name(msg: Msg, name: &str) -> String {
    tr(msg, &[("name", &name)])
}

/// `/\.(md|markdown)$/i`
fn is_markdown(path: &str) -> bool {
    let l = path.to_ascii_lowercase();
    l.ends_with(".md") || l.ends_with(".markdown")
}

/// `buildAppCommands(ctx)`: the registry, in the original's exact order.
pub fn build_commands(ctx: &CommandContext, settings: &Settings) -> Vec<AppCommand> {
    use CommandAction as A;
    let has_root = ctx.root.is_some();
    let has_active = ctx.active_path.is_some();
    let mut out = vec![
        cmd(
            "go-to-editor",
            t(Msg::CommandGoToEditor),
            kw(&["files", "explorer", "view"]),
            A::SetView(MainViewKind::Editor),
        ),
        cmd(
            "go-to-agenda",
            t(Msg::CommandGoToAgenda),
            kw(&["agenda", "tasks", "calendar", "view"]),
            A::SetView(MainViewKind::Agenda),
        ),
        cmd(
            "go-to-search",
            t(Msg::CommandGoToSearch),
            kw(&["find", "view"]),
            A::SetView(MainViewKind::Search),
        ),
        cmd(
            "go-to-settings",
            t(Msg::CommandGoToSettings),
            kw(&["preferences", "config", "view"]),
            A::SetView(MainViewKind::Settings),
        ),
    ];
    if has_active {
        for a in SHORTCUT_ACTIONS {
            let mut keywords = kw(&["task", "insert", "editor"]);
            keywords.push(t(a.desc));
            out.push(AppCommand {
                binding: Some(settings.shortcut(a.id).to_string()),
                ..cmd(
                    format!("editor:{}", a.id),
                    t_name(Msg::CommandEditorAction, &t(a.label)),
                    keywords,
                    A::EditorAction(a.id),
                )
            });
        }
    }
    for s in SettingsSection::ALL {
        let (label, extra) = section_info(s);
        let mut keywords = kw(&["preferences", "config"]);
        keywords.extend(kw(extra));
        out.push(cmd(
            format!("settings:{}", s.id()),
            t_name(Msg::CommandSettingsSection, &t(label)),
            keywords,
            A::OpenSettings(s),
        ));
    }
    out.push(cmd(
        "open-folder",
        t(Msg::CommandOpenFolder),
        kw(&["vault", "workspace", "directory"]),
        A::OpenFolder,
    ));
    if has_root {
        out.push(AppCommand {
            binding: Some("Mod-p".into()),
            aliases: &[":o", ":open", ":e", ":edit"],
            ..cmd(
                "quick-open-file",
                t(Msg::CommandQuickOpen),
                kw(&["switcher", "find file", "open file"]),
                A::QuickOpen,
            )
        });
    }
    if has_active {
        out.push(AppCommand {
            binding: Some("Mod-s".into()),
            aliases: &[":w", ":write"],
            ..cmd(
                "save-file",
                t(Msg::CommandSaveFile),
                kw(&["write"]),
                A::Save,
            )
        });
        out.push(AppCommand {
            aliases: &[":wq", ":x"],
            ..cmd(
                "save-and-close",
                t(Msg::CommandSaveAndCloseTab),
                kw(&["write quit"]),
                A::SaveAndClose,
            )
        });
        out.push(AppCommand {
            aliases: &[":q", ":quit", ":bc"],
            ..cmd(
                "close-active-tab",
                t(Msg::CommandCloseActiveTab),
                kw(&["close file"]),
                A::CloseActiveTab,
            )
        });
    }
    if ctx.has_open_tabs {
        out.push(AppCommand {
            aliases: &[":qa"],
            ..cmd(
                "close-all-tabs",
                t(Msg::CommandCloseAllTabs),
                kw(&["close everything"]),
                A::CloseAllTabs,
            )
        });
    }
    if let Some(root) = &ctx.root {
        out.push(cmd(
            "open-daily-note",
            t(Msg::CommandOpenDailyNote),
            kw(&["journal", "today", "diary", "new note"]),
            A::OpenDailyNote,
        ));
        for tpl in list_templates(root, &settings.templates_folder, ctx.md_files.as_slice()) {
            out.push(cmd(
                format!("new-from-template:{tpl}"),
                t_name(Msg::CommandNewFromTemplate, &stem(&tpl)),
                kw(&["template", "create", "new note"]),
                A::NewFromTemplate(tpl),
            ));
        }
    }
    out.push(cmd(
        "keyboard-shortcuts",
        t(Msg::CommandKeyboardShortcuts),
        kw(&["keys", "hotkeys", "bindings", "help"]),
        A::OpenSettings(SettingsSection::Shortcuts),
    ));
    if has_active {
        out.push(cmd(
            "open-in-new-window",
            t(Msg::CommandOpenInNewWindow),
            kw(&["detach", "window", "popout", "second monitor"]),
            A::OpenInNewWindow,
        ));
    }
    if has_root {
        let title = t(if ctx.backlinks_open {
            Msg::CommandHideBacklinks
        } else {
            Msg::CommandShowBacklinks
        });
        out.push(AppCommand {
            aliases: &[":backlinks", ":bl"],
            ..cmd(
                "toggle-backlinks",
                title,
                kw(&["links", "references", "linked mentions", "graph"]),
                A::ToggleBacklinks,
            )
        });
    }
    if let Some(path) = ctx.active_path.as_ref().filter(|p| is_markdown(p)) {
        out.push(AppCommand {
            aliases: &[":export"],
            ..cmd(
                "export-html",
                t(Msg::CommandExportHtml),
                kw(&["export", "html", "web page", "share"]),
                A::ExportHtml(path.clone()),
            )
        });
        out.push(AppCommand {
            aliases: &[":pdf"],
            ..cmd(
                "export-pdf",
                t(Msg::CommandExportPdf),
                kw(&["export", "pdf", "print", "share"]),
                A::ExportPdf(path.clone()),
            )
        });
        // Native split of the original's `:pdf`/`:print` row (which ran the
        // webview's print dialog): `:print` hands the rendered PDF to the
        // system viewer's print dialog; `export-pdf` keeps the "print"
        // keyword so the search still finds both.
        out.push(AppCommand {
            aliases: &[":print"],
            ..cmd(
                "print-pdf",
                t(Msg::ExportPrint),
                kw(&["print", "pdf", "printer"]),
                A::PrintPdf(path.clone()),
            )
        });
    }
    out.push(AppCommand {
        aliases: &[":theme"],
        ..cmd(
            "toggle-theme",
            t(Msg::CommandToggleTheme),
            kw(&["dark mode", "light mode", "appearance", "color scheme"]),
            A::ToggleTheme,
        )
    });
    for p in PaletteId::BUILT_IN {
        out.push(cmd(
            format!("select-theme:{}", palette_value(p)),
            t_name(Msg::CommandSelectTheme, p.display_name()),
            kw(&["palette", "appearance", "color scheme"]),
            A::SelectPalette(p),
        ));
    }
    for th in &settings.custom_themes {
        out.push(cmd(
            format!("select-theme:{}", th.id),
            t_name(Msg::CommandSelectTheme, &th.name),
            kw(&["palette", "appearance", "custom theme"]),
            A::SelectCustomTheme(th.id.clone()),
        ));
    }
    out
}

fn palette_value(p: PaletteId) -> &'static str {
    match p {
        PaletteId::Ayu => "ayu",
        PaletteId::Dracula => "dracula",
        PaletteId::Catppuccin => "catppuccin",
        PaletteId::Custom => "custom",
    }
}

/// One palette row.
#[derive(Clone, Debug, PartialEq)]
pub struct Ranked {
    pub command: AppCommand,
    /// Highlighted char indices of the title.
    pub indices: Vec<usize>,
    pub score: f64,
    /// The alias chip.
    pub alias: Option<&'static str>,
}

fn row(
    command: &AppCommand,
    indices: Vec<usize>,
    score: f64,
    alias: Option<&'static str>,
) -> Ranked {
    Ranked {
        command: command.clone(),
        indices,
        score,
        alias,
    }
}

/// `^:(\d+)$`
fn line_query(q: &str) -> Option<u32> {
    let digits = q.strip_prefix(':')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(
        digits
            .parse::<u64>()
            .map_or(u32::MAX, |n| n.min(u32::MAX as u64) as u32),
    )
}

/// The palette's result list for `query` (CommandPalette.tsx `results`).
pub fn rank(commands: &[AppCommand], query: &str) -> Vec<Ranked> {
    let q = query.trim();
    if q.is_empty() {
        return commands.iter().map(|c| row(c, vec![], 0., None)).collect();
    }
    let mut ranked = Vec::new();
    if let Some(body) = q.strip_prefix(':') {
        if let Some(line) = line_query(q) {
            let go = AppCommand {
                id: "go-to-line".into(),
                title: tr(Msg::CommandGoToLine, &[("n", &line)]),
                keywords: vec![],
                binding: None,
                aliases: &[],
                action: CommandAction::GoToLine(line),
            };
            ranked.push(row(&go, vec![], 1000., None));
        }
        for c in commands {
            if let Some(alias) = c.aliases.iter().find(|a| a.starts_with(q)) {
                ranked.push(row(
                    c,
                    vec![],
                    if *alias == q { 200. } else { 100. },
                    Some(alias),
                ));
                continue;
            }
            if body.is_empty() {
                continue;
            }
            let m = fuzzy_match(body, &c.title);
            if m.matched {
                ranked.push(row(c, m.indices, m.score, c.aliases.first().copied()));
            }
        }
    } else {
        for c in commands {
            let m = fuzzy_match(q, &c.title);
            if m.matched {
                // Title hits beat keyword-only hits.
                ranked.push(row(c, m.indices, m.score + 1., None));
                continue;
            }
            let extras = c
                .binding
                .iter()
                .flat_map(|b| [b.replace('-', " "), display_binding(b)]);
            if c.keywords
                .iter()
                .cloned()
                .chain(extras)
                .any(|k| fuzzy_match(q, &k).matched)
            {
                ranked.push(row(c, vec![], 0., None));
            }
        }
    }
    // Stable, like JS `sort`: ties keep registry order.
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::CustomTheme;

    fn ctx_full() -> CommandContext {
        CommandContext {
            root: Some("/v".into()),
            active_path: Some("/v/a.md".into()),
            has_open_tabs: true,
            backlinks_open: false,
            md_files: Arc::new(vec![
                "/v/a.md".into(),
                "/v/templates/daily.md".into(),
                "/v/templates/meeting.md".into(),
                "/v/templates/first_template.md".into(),
            ]),
        }
    }

    fn ids(cmds: &[AppCommand]) -> Vec<String> {
        cmds.iter().map(|c| c.id.clone()).collect()
    }

    #[test]
    fn registry_order_with_everything() {
        let mut s = Settings::default();
        s.custom_themes.push(CustomTheme {
            id: "c1".into(),
            name: "Mine".into(),
            css: String::new(),
        });
        let ids = ids(&build_commands(&ctx_full(), &s));
        let mut want: Vec<String> = [
            "go-to-editor",
            "go-to-agenda",
            "go-to-search",
            "go-to-settings",
        ]
        .map(String::from)
        .into();
        want.extend(SHORTCUT_ACTIONS.iter().map(|a| format!("editor:{}", a.id)));
        want.extend(
            SettingsSection::ALL
                .iter()
                .map(|s| format!("settings:{}", s.id())),
        );
        want.extend(
            [
                "open-folder",
                "quick-open-file",
                "save-file",
                "save-and-close",
                "close-active-tab",
                "close-all-tabs",
                "open-daily-note",
                "new-from-template:/v/templates/first_template.md",
                "new-from-template:/v/templates/meeting.md",
                "keyboard-shortcuts",
                "open-in-new-window",
                "toggle-backlinks",
                "export-html",
                "export-pdf",
                "print-pdf",
                "toggle-theme",
                "select-theme:ayu",
                "select-theme:dracula",
                "select-theme:catppuccin",
                "select-theme:c1",
            ]
            .map(String::from),
        );
        assert_eq!(ids, want);
    }

    #[test]
    fn registry_without_vault_or_tabs() {
        let ids = ids(&build_commands(
            &CommandContext::default(),
            &Settings::default(),
        ));
        assert_eq!(ids.len(), 4 + 6 + 1 + 1 + 1 + 3);
        assert!(!ids.iter().any(|i| i.starts_with("editor:")
            || i == "quick-open-file"
            || i == "toggle-backlinks"));
        assert!(ids.contains(&"keyboard-shortcuts".to_string()));
    }

    #[test]
    fn conditions() {
        // A drawing tab: editor commands listed, export not.
        let ctx = CommandContext {
            active_path: Some("/v/d.excalidraw".into()),
            ..ctx_full()
        };
        let ids = ids(&build_commands(&ctx, &Settings::default()));
        assert!(ids.contains(&"editor:insertDate".to_string()));
        assert!(!ids.contains(&"export-html".to_string()));
        let ctx = CommandContext {
            active_path: Some("/v/A.MARKDOWN".into()),
            ..ctx_full()
        };
        let cmds = build_commands(&ctx, &Settings::default());
        assert!(cmds.iter().any(|c| c.id == "export-pdf"
            && c.aliases == [":pdf"]
            && c.keywords.iter().any(|k| k == "print")));
        assert!(
            cmds.iter()
                .any(|c| c.id == "print-pdf" && c.aliases == [":print"])
        );
        assert!(
            cmds.iter()
                .any(|c| c.id == "export-html" && c.aliases == [":export"])
        );
        let ctx = CommandContext {
            backlinks_open: true,
            ..ctx_full()
        };
        let c = build_commands(&ctx, &Settings::default())
            .into_iter()
            .find(|c| c.id == "toggle-backlinks")
            .unwrap();
        assert_eq!(c.title, "Hide Backlinks");
    }

    #[test]
    fn titles_bindings_and_template_folder() {
        let mut s = Settings::default();
        s.shortcuts.insert("insertDate".into(), "Mod-Alt-x".into());
        let cmds = build_commands(&ctx_full(), &s);
        let c = cmds.iter().find(|c| c.id == "editor:insertDate").unwrap();
        assert_eq!(c.title, "Editor: Insert date");
        assert_eq!(c.binding.as_deref(), Some("Mod-Alt-x"));
        assert!(
            c.keywords
                .contains(&"Insert today's date at the cursor.".to_string())
        );
        let c = cmds.iter().find(|c| c.id == "settings:vault").unwrap();
        assert_eq!(c.title, "Settings: Vault");
        let c = cmds
            .iter()
            .find(|c| c.id.ends_with("first_template.md"))
            .unwrap();
        assert_eq!(c.title, "New Note from Template: first_template");
        assert_eq!(
            cmds.iter()
                .find(|c| c.id == "select-theme:dracula")
                .unwrap()
                .title,
            "Theme: Dracula"
        );
        s.templates_folder = "/nope/".into();
        assert!(
            !build_commands(&ctx_full(), &s)
                .iter()
                .any(|c| c.id.starts_with("new-from-template"))
        );
    }

    fn titles(r: &[Ranked]) -> Vec<&str> {
        r.iter().map(|r| r.command.id.as_str()).collect()
    }

    #[test]
    fn empty_query_lists_everything_in_order() {
        let cmds = build_commands(&ctx_full(), &Settings::default());
        let r = rank(&cmds, "  ");
        assert_eq!(r.len(), cmds.len());
        assert!(r.iter().all(|r| r.indices.is_empty() && r.alias.is_none()));
        assert_eq!(r[0].command.id, "go-to-editor");
    }

    #[test]
    fn title_hits_beat_keyword_hits() {
        let cmds = build_commands(&ctx_full(), &Settings::default());
        let r = rank(&cmds, "dark mode");
        // Only the keyword matches.
        assert_eq!(titles(&r), vec!["toggle-theme"]);
        assert_eq!(r[0].score, 0.);
        let r = rank(&cmds, "save");
        assert_eq!(r[0].command.id, "save-file");
        assert!(r[0].score > 1.);
        assert_eq!(r[0].indices, vec![0, 1, 2, 3]);
    }

    #[test]
    fn binding_text_is_searchable() {
        let cmds = build_commands(&ctx_full(), &Settings::default());
        // `Mod-s` → "Mod s" keyword.
        let r = rank(&cmds, "mod s");
        assert!(
            r.iter()
                .any(|r| r.command.id == "save-file" && r.score == 0.)
        );
        let formatted = display_binding("Mod-s");
        assert!(
            rank(&cmds, &formatted)
                .iter()
                .any(|r| r.command.id == "save-file")
        );
    }

    #[test]
    fn colon_aliases() {
        let cmds = build_commands(&ctx_full(), &Settings::default());
        let r = rank(&cmds, ":w");
        assert_eq!(r[0].command.id, "save-file");
        assert_eq!(r[0].score, 200.);
        assert_eq!(r[0].alias, Some(":w"));
        // `:wq` prefix of `:w`? No: `:wq` starts with `:w`, so it is listed at 100.
        let wq = r.iter().find(|r| r.command.id == "save-and-close").unwrap();
        assert_eq!((wq.score, wq.alias), (100., Some(":wq")));
        // `:q` exact beats `:qa` and `:quit` prefix.
        let r = rank(&cmds, ":q");
        assert_eq!(r[0].command.id, "close-active-tab");
        assert_eq!(r[0].alias, Some(":q"));
        assert_eq!(r[1].command.id, "close-all-tabs");
        // Case-sensitive with the colon.
        assert!(rank(&cmds, ":W").iter().all(|r| r.score < 100.));
    }

    #[test]
    fn bare_colon_lists_aliased_commands() {
        let cmds = build_commands(&ctx_full(), &Settings::default());
        let r = rank(&cmds, ":");
        assert!(
            r.iter()
                .all(|r| !r.command.aliases.is_empty() && r.score == 100.)
        );
        assert_eq!(
            r.len(),
            cmds.iter().filter(|c| !c.aliases.is_empty()).count()
        );
        assert_eq!(r[0].command.id, "quick-open-file");
        assert_eq!(r[0].alias, Some(":o"));
    }

    #[test]
    fn colon_body_fuzzy_matches_titles_with_first_alias_chip() {
        let cmds = build_commands(&ctx_full(), &Settings::default());
        let r = rank(&cmds, ":close");
        let c = r
            .iter()
            .find(|r| r.command.id == "close-active-tab")
            .unwrap();
        assert_eq!(c.alias, Some(":q"));
        assert!(!c.indices.is_empty());
        let c = r.iter().find(|r| r.command.id == "keyboard-shortcuts");
        assert!(c.is_none());
    }

    #[test]
    fn go_to_line() {
        let cmds = build_commands(&ctx_full(), &Settings::default());
        let r = rank(&cmds, " :42 ");
        assert_eq!(r[0].command.action, CommandAction::GoToLine(42));
        assert_eq!(r[0].command.title, "Go to Line 42");
        assert_eq!(r[0].score, 1000.);
        assert_eq!(line_query(":4a"), None);
        assert_eq!(line_query(":"), None);
        assert_eq!(line_query(":99999999999"), Some(u32::MAX));
        // Listed even with no file open (a no-op then).
        let r = rank(
            &build_commands(&CommandContext::default(), &Settings::default()),
            ":7",
        );
        assert_eq!(r[0].command.id, "go-to-line");
    }
}
