//! Typable (`:`) commands (cm-helix `commands` facet).

use crate::editor::{Editor, Tr};
use crate::effect::{Effect, External};
use crate::jsnum::js_number;
use crate::selection::Selection;

/// A command the `:` palette can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandInfo {
    pub name: String,
    pub aliases: Vec<String>,
    pub help: String,
}

impl CommandInfo {
    pub fn new(name: &str, aliases: &[&str], help: &str) -> Self {
        CommandInfo {
            name: name.to_string(),
            aliases: aliases.iter().map(|a| a.to_string()).collect(),
            help: help.to_string(),
        }
    }

    pub fn matches(&self, cmd: &str) -> bool {
        self.name == cmd || self.aliases.iter().any(|a| a == cmd)
    }

    pub fn matches_prefix(&self, cmd: &str) -> bool {
        self.name.starts_with(cmd) || self.aliases.iter().any(|a| a.starts_with(cmd))
    }

    /// cm-helix `commandToHelp`.
    pub fn full_help(&self) -> String {
        if self.aliases.is_empty() {
            self.help.clone()
        } else {
            format!("{}\nAliases: {}", self.help, self.aliases.join(", "))
        }
    }
}

/// Result of a handler: `Some((message, error))` is shown in the status line.
pub(crate) type CmdResult = Option<(String, bool)>;

impl Editor {
    /// Every command available in the `:` palette, in cm-helix order.
    pub fn commands(&self) -> Vec<CommandInfo> {
        let mut out = vec![
            CommandInfo::new("goto", &["g"], "Goto line number"),
            CommandInfo::new(
                "clipboard-yank",
                &[],
                "Yank main selection into system clipboard",
            ),
            CommandInfo::new(
                "clear-register",
                &[],
                "Clear given register. If no argument is provided, clear all registers",
            ),
            CommandInfo::new("echo", &[], "Prints the given argumens to the statusline."),
        ];
        if !self.config.themes.is_empty() {
            out.push(CommandInfo::new(
                "theme",
                &[],
                "Change the editor theme (or show the current them if none specified)",
            ));
        }
        let ext = &self.config.externals;
        if ext.buffer_next {
            out.push(CommandInfo::new(
                "buffer-next",
                &["bn", "bnext"],
                "Goto next buffer",
            ));
        }
        if ext.buffer_previous {
            out.push(CommandInfo::new(
                "buffer-previous",
                &["bp", "bprev"],
                "Goto previous buffer",
            ));
        }
        if ext.buffer_close {
            out.push(CommandInfo::new(
                "buffer-close",
                &["bc", "bclose"],
                "Close the current buffer",
            ));
        }
        if self.config.app_commands {
            out.push(CommandInfo::new("write", &["w"], "Write changes to disk"));
            out.push(CommandInfo::new("quit", &["q"], "Close the current view"));
            out.push(CommandInfo::new(
                "write-quit",
                &["wq", "x"],
                "Write changes to disk and close the view",
            ));
        }
        out.extend(self.host_commands.iter().cloned());
        out
    }

    /// Add a host command; running it yields [`Effect::HostCommand`].
    pub fn register_command(&mut self, info: CommandInfo) {
        self.host_commands.retain(|c| c.name != info.name);
        self.host_commands.push(info);
    }

    /// Autocomplete for a command's arguments (only `:theme` has one).
    pub(crate) fn command_autocomplete(&self, name: &str, args: &[&str]) -> Option<Vec<String>> {
        if name != "theme" || self.config.themes.is_empty() {
            return None;
        }
        if args.get(1).is_some_and(|a| !a.is_empty()) {
            return Some(Vec::new());
        }
        let prefix = args.first().copied().unwrap_or("");
        Some(
            self.config
                .themes
                .iter()
                .filter(|t| t.starts_with(prefix))
                .cloned()
                .collect(),
        )
    }

    /// Run a typable command by (canonical) name.
    pub(crate) fn run_typable(&mut self, info: &CommandInfo, args: &[String]) -> CmdResult {
        match info.name.as_str() {
            "goto" => {
                let Some(arg) = args.first() else {
                    return Some(("Line number required".into(), true));
                };
                let n = js_number(arg);
                if !n.is_finite() || n <= 0.0 {
                    return Some(("Invalid line number".into(), true));
                }
                let lines = self.doc.lines();
                let line = (n.floor().max(1.0) as usize).min(lines);
                let from = self.doc.line(line).from;
                // cm-helix leaves an empty (bar) cursor here.
                self.dispatch(Tr::sel(Selection::single(from, from)).scrolled());
                None
            }
            "clipboard-yank" => {
                let main = self.sel.main();
                let text = self.range_text(main);
                self.host.clipboard_write(&text);
                Some(("Yanked main selection to + register".into(), false))
            }
            "clear-register" => {
                if args.len() > 1 {
                    return Some((
                        format!("Expected at most 1 argument, got {}", args.len()),
                        true,
                    ));
                }
                match args.first() {
                    None => {
                        for r in self.registers.names() {
                            self.yank_to(r, Vec::new());
                        }
                    }
                    Some(a) => {
                        let mut chars = a.chars();
                        match (chars.next(), chars.next()) {
                            (Some(c), None) => self.yank_to(c, Vec::new()),
                            _ => return Some((format!("invalid register {a}"), true)),
                        }
                    }
                }
                self.dispatch(Tr::default());
                None
            }
            "echo" => Some((args.join(" "), false)),
            "theme" => {
                if args.len() > 1 {
                    return Some((
                        format!("Expected at most 1 argument, got {}", args.len()),
                        true,
                    ));
                }
                let Some(name) = args.first() else {
                    return Some((self.theme.clone().unwrap_or_default(), false));
                };
                if !self.config.themes.contains(name) {
                    return Some((format!("'theme': Could not load theme: {name}"), true));
                }
                if self.theme.as_deref() != Some(name) {
                    self.theme = Some(name.clone());
                    self.emit(Effect::ChangeTheme(name.clone()));
                }
                None
            }
            "buffer-next" => {
                self.emit(Effect::External(External::BufferNext));
                None
            }
            "buffer-previous" => {
                self.emit(Effect::External(External::BufferPrevious));
                None
            }
            "buffer-close" => {
                self.emit(Effect::External(External::BufferClose(args.to_vec())));
                None
            }
            "write" => {
                self.emit(Effect::Save);
                None
            }
            "quit" => {
                self.emit(Effect::Quit);
                None
            }
            "write-quit" => {
                self.emit(Effect::Save);
                self.emit(Effect::Quit);
                None
            }
            name => {
                self.emit(Effect::HostCommand {
                    name: name.to_string(),
                    args: args.to_vec(),
                });
                None
            }
        }
    }
}
