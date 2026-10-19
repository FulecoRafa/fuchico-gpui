//! `commands_markdown()`: the Vim key and Ex command tables as Markdown
//! (same table layout as `fuchico_helix::commands_markdown`).

use crate::ex::{DEFAULT_EX_COMMANDS, EXTRA_EX_COMMANDS};
use crate::keymap::{CmdType, Command, Context, MotionKind, OperatorKind};

/// `MoveByWords` → `moveByWords`.
fn camel(debug: &str) -> String {
    let mut c = debug.chars();
    match c.next() {
        Some(f) => f.to_ascii_lowercase().to_string() + c.as_str(),
        None => String::new(),
    }
}

fn esc(s: &str) -> String {
    s.replace('|', "\\|")
}

fn command_name(c: &Command) -> String {
    match c.ty {
        CmdType::KeyToKey => "keyToKey".into(),
        CmdType::Motion => c.motion.map(|m| camel(&format!("{m:?}"))).unwrap_or_default(),
        CmdType::Operator => c.operator.map(|o| o.name().to_string()).unwrap_or_default(),
        CmdType::OperatorMotion => {
            let o = c.operator.map(|o| o.name()).unwrap_or("");
            let m = c.motion.map(|m| camel(&format!("{m:?}"))).unwrap_or_default();
            format!("{o} + {m}")
        }
        CmdType::Action => c.action.map(|a| camel(&format!("{a:?}"))).unwrap_or_default(),
        CmdType::Search => "search".into(),
        CmdType::Ex => "ex".into(),
        CmdType::KeyToEx => "keyToEx".into(),
        CmdType::Idle => "idle".into(),
    }
}

fn description(c: &Command) -> String {
    match c.ty {
        CmdType::KeyToKey => format!("same as `{}`", c.to_keys.as_deref().unwrap_or("")),
        CmdType::KeyToEx => format!("runs `:{}`", c.ex_input.as_deref().unwrap_or("")),
        CmdType::Ex => "opens the `:` command line".into(),
        CmdType::Search => {
            let fwd = c.search_args.as_ref().is_some_and(|a| a.forward);
            if fwd { "search forward".into() } else { "search backward".into() }
        }
        CmdType::Motion => "motion".into(),
        CmdType::Operator => "operator (waits for a motion)".into(),
        CmdType::OperatorMotion => "operator with a built-in motion".into(),
        CmdType::Action => "action".into(),
        CmdType::Idle => "ignored".into(),
    }
}

/// Known deviations from cm-vim.
fn status(c: &Command) -> &'static str {
    match (c.motion, c.operator) {
        (Some(MotionKind::MoveToMatchedSymbol), _) => "quirk",
        (Some(MotionKind::MoveByPage | MotionKind::MoveByScroll), _) => "quirk",
        (_, Some(OperatorKind::IndentAuto)) => "quirk",
        _ => "parity",
    }
}

fn undo(c: &Command) -> &'static str {
    let op_edit = matches!(c.ty, CmdType::OperatorMotion | CmdType::Operator)
        && !matches!(c.operator, Some(OperatorKind::Yank));
    if op_edit || c.is_edit { "yes" } else { "" }
}

/// The default keymap and Ex commands as Markdown.
pub fn commands_markdown() -> String {
    let keymap = crate::keymap::default_keymap();
    let tables: [(&str, Option<Context>); 5] = [
        ("all modes", None),
        ("normal", Some(Context::Normal)),
        ("visual", Some(Context::Visual)),
        ("operator pending", Some(Context::OperatorPending)),
        ("insert", Some(Context::Insert)),
    ];
    let mut out = String::new();
    for (label, ctx) in tables {
        out.push_str(&format!("### {label}\n\n"));
        out.push_str("| Key | Command | Status | Undo | Description |\n|---|---|---|---|---|\n");
        for c in keymap.iter().filter(|c| c.context == ctx) {
            out.push_str(&format!(
                "| `{}` | `{}` | {} | {} | {} |\n",
                esc(&c.keys),
                command_name(c),
                status(c),
                undo(c),
                esc(&description(c))
            ));
        }
        out.push('\n');
    }
    out.push_str("### ex commands\n\n");
    out.push_str("| Key | Command | Status | Undo | Description |\n|---|---|---|---|---|\n");
    for &(name, short) in DEFAULT_EX_COMMANDS {
        let key = match short {
            Some(s) if s.len() < name.len() => format!(":{s}[{}]", &name[s.len()..]),
            _ => format!(":{name}"),
        };
        let extra = EXTRA_EX_COMMANDS.contains(&name);
        let edits = matches!(
            name,
            "substitute" | "sort" | "delete" | "join" | "normal" | "global" | "vglobal" | "undo" | "redo"
        );
        let desc = match name {
            "write" => "save (`Effect::Save`)",
            "quit" => "close (`Effect::Quit`; needs `app_commands`)",
            "wq" | "xit" => "save and close (needs `app_commands`)",
            "colorscheme" => "show / change the theme (`Effect::ChangeTheme`)",
            "version" => "show the cm-vim version",
            _ => "",
        };
        out.push_str(&format!(
            "| `{}` | `{name}` | {} | {} | {} |\n",
            esc(&key),
            if extra { "extension" } else { "parity" },
            if edits { "yes" } else { "" },
            esc(desc)
        ));
    }
    out.push('\n');
    out
}
