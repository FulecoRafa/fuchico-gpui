//! The `:` command line: typable commands, numeric goto preview,
//! completions, history, host commands.

use super::*;
use crate::effect::{External, Scroll};
use crate::{CommandInfo, ExternalCommands, PromptKind};

fn run(e: &mut Editor, line: &str) -> Vec<Effect> {
    let mut fx = e.handle_keys(":");
    fx.extend(e.prompt_input(line));
    fx.extend(e.prompt_close(true));
    fx
}

#[test]
fn colon_opens_command_line() {
    let mut e = ed("|a^b");
    let fx = e.handle_keys(":");
    assert!(fx.contains(&Effect::OpenCommandLine { placeholder: None }));
    assert_eq!(e.prompt_kind(), Some(PromptKind::Command));
    e.prompt_input("echo hi");
    e.prompt_close(true);
    let fx = e.handle_keys(":");
    assert!(fx.contains(&Effect::OpenCommandLine {
        placeholder: Some("echo hi".into())
    }));
}

#[test]
fn echo() {
    let mut e = ed("|a^b");
    let fx = run(&mut e, "echo hello   world");
    assert_eq!(messages(&fx), vec![("hello world".to_string(), false)]);
    assert_eq!(
        e.registers().get(':').unwrap(),
        &vec!["echo hello   world".to_string()]
    );
}

#[test]
fn unknown_command() {
    let mut e = ed("|a^b");
    let fx = run(&mut e, "frob x");
    assert_eq!(
        messages(&fx),
        vec![("no such command: 'frob'".to_string(), true)]
    );
}

#[test]
fn empty_accept_reruns_last_command() {
    let mut e = ed("|a^b");
    run(&mut e, "echo again");
    e.handle_keys(":");
    let fx = e.prompt_close(true);
    assert_eq!(messages(&fx), vec![("again".to_string(), false)]);
}

#[test]
fn empty_accept_without_history_does_nothing() {
    let mut e = ed("|a^b");
    e.handle_keys(":");
    let fx = e.prompt_close(true);
    assert!(messages(&fx).is_empty());
    assert!(e.registers().get(':').is_none());
}

#[test]
fn goto_command() {
    let mut e = ed("|a^\nb\nc\nd");
    run(&mut e, "goto 3");
    assert_eq!(render(&e), "a\nb\n|c\nd");
    run(&mut e, "g 1");
    assert_eq!(render(&e), "|a\nb\nc\nd");
    run(&mut e, "g 99");
    assert_eq!(render(&e), "a\nb\nc\n|d");
    run(&mut e, "g 2.7");
    assert_eq!(render(&e), "a\n|b\nc\nd");
}

#[test]
fn goto_errors() {
    let mut e = ed("|a^\nb");
    assert_eq!(
        messages(&run(&mut e, "goto")),
        vec![("Line number required".to_string(), true)]
    );
    assert_eq!(
        messages(&run(&mut e, "goto x")),
        vec![("Invalid line number".to_string(), true)]
    );
    assert_eq!(
        messages(&run(&mut e, "goto 0")),
        vec![("Invalid line number".to_string(), true)]
    );
    assert_eq!(
        messages(&run(&mut e, "goto -2")),
        vec![("Invalid line number".to_string(), true)]
    );
    assert_eq!(render(&e), "|a^\nb");
}

#[test]
fn numeric_line_preview_and_cancel() {
    let mut e = ed("|a^\nb\nc");
    e.handle_keys(":");
    let fx = e.prompt_input("2");
    assert_eq!(render(&e), "a\n|b\nc");
    assert!(
        fx.iter()
            .any(|f| matches!(f, Effect::Scroll(Scroll::To { .. })))
    );
    e.prompt_input("9"); // out of range: stays
    assert_eq!(render(&e), "a\n|b\nc");
    let fx = e.prompt_close(false);
    assert_eq!(render(&e), "|a^\nb\nc");
    assert!(fx.contains(&Effect::Scroll(Scroll::RestorePromptStart)));
}

#[test]
fn numeric_line_accept() {
    let mut e = ed("|a^\nb\nc");
    e.handle_keys(":");
    e.prompt_input("3");
    e.prompt_close(true);
    assert_eq!(render(&e), "a\nb\n|c");
    assert_eq!(e.registers().get(':').unwrap(), &vec!["3".to_string()]);
    // execute_command without a preview still moves.
    e.execute_command("1");
    assert_eq!(render(&e), "|a\nb\nc");
}

#[test]
fn clear_register() {
    let mut e = editing::with_reg("|a^b", 'a', &["x"]);
    let fx = run(&mut e, "clear-register a b");
    assert_eq!(
        messages(&fx),
        vec![("Expected at most 1 argument, got 2".to_string(), true)]
    );
    let fx = run(&mut e, "clear-register ab");
    assert_eq!(
        messages(&fx),
        vec![("invalid register ab".to_string(), true)]
    );
    run(&mut e, "clear-register a");
    assert!(e.registers().get('a').is_none_or(|v| v.is_empty()));
}

#[test]
fn clear_all_registers() {
    let mut e = editing::with_reg("|a^b", 'a', &["x"]);
    e.handle_keys("y");
    run(&mut e, "clear-register");
    assert!(e.registers().get('a').is_none_or(|v| v.is_empty()));
    assert!(e.registers().get('"').is_none_or(|v| v.is_empty()));
}

#[test]
fn clipboard_yank_command() {
    let mut e = ed("^abc|d");
    let fx = run(&mut e, "clipboard-yank");
    assert_eq!(
        messages(&fx),
        vec![("Yanked main selection to + register".to_string(), false)]
    );
    assert_eq!(e.prompt_read_register('+').unwrap(), "abc");
}

#[test]
fn completions_prefix_and_help() {
    let e = {
        let mut e = ed("|a^b");
        e.handle_keys(":");
        e
    };
    let c = e.prompt_completions("c");
    assert_eq!(
        c.options,
        vec!["clipboard-yank".to_string(), "clear-register".to_string()]
    );
    assert_eq!(c.help, None);
    let c = e.prompt_completions("g");
    assert_eq!(c.options, vec!["goto".to_string()]);
    assert_eq!(c.help.as_deref(), Some("Goto line number\nAliases: g"));
    let c = e.prompt_completions("echo");
    assert_eq!(
        c.help.as_deref(),
        Some("Prints the given argumens to the statusline.")
    );
    assert_eq!(e.prompt_completions("zz").options, Vec::<String>::new());
    // app commands with the default config
    let mut e = ed_ext("|a^b");
    e.handle_keys(":");
    assert_eq!(
        e.prompt_completions("w").options,
        vec!["write".to_string(), "write-quit".to_string()]
    );
    assert_eq!(
        e.prompt_completions("wq").help.as_deref(),
        Some("Write changes to disk and close the view\nAliases: wq, x")
    );
}

#[test]
fn accept_completion_replaces_last_word() {
    let mut e = ed("|a^b");
    e.handle_keys(":");
    assert_eq!(
        e.prompt_accept_completion("cl", "clear-register"),
        "clear-register"
    );
    assert_eq!(
        e.prompt_accept_completion("theme  so", "solarized"),
        "theme  solarized"
    );
}

#[test]
fn command_history() {
    let mut e = ed("|a^b");
    run(&mut e, "echo 1");
    run(&mut e, "echo 2");
    e.handle_keys(":");
    assert_eq!(e.prompt_history(true).as_deref(), Some("echo 2"));
    assert_eq!(e.prompt_history(true).as_deref(), Some("echo 1"));
    assert_eq!(e.prompt_history(true), None);
    assert_eq!(e.prompt_history(false).as_deref(), Some("echo 2"));
    assert_eq!(e.prompt_history(false), None);
}

fn themed() -> Editor {
    let mut cfg = Config::parity();
    cfg.themes = vec!["dark".into(), "dawn".into(), "light".into()];
    cfg.theme = Some("dark".into());
    ed_with("|a^b", cfg)
}

#[test]
fn theme_command() {
    let mut e = themed();
    assert_eq!(
        messages(&run(&mut e, "theme")),
        vec![("dark".to_string(), false)]
    );
    let fx = run(&mut e, "theme light");
    assert!(fx.contains(&Effect::ChangeTheme("light".into())));
    assert_eq!(e.theme(), Some("light"));
    let fx = run(&mut e, "theme light");
    assert!(!fx.iter().any(|f| matches!(f, Effect::ChangeTheme(_))));
    assert_eq!(
        messages(&run(&mut e, "theme nope")),
        vec![("'theme': Could not load theme: nope".to_string(), true)]
    );
    assert_eq!(
        messages(&run(&mut e, "theme a b")),
        vec![("Expected at most 1 argument, got 2".to_string(), true)]
    );
    // without themes the command does not exist
    let mut e = ed("|a^b");
    assert_eq!(
        messages(&run(&mut e, "theme")),
        vec![("no such command: 'theme'".to_string(), true)]
    );
}

#[test]
fn theme_autocomplete() {
    let mut e = themed();
    e.handle_keys(":");
    let c = e.prompt_completions("theme da");
    assert_eq!(c.options, vec!["dark".to_string(), "dawn".to_string()]);
    assert!(c.help.unwrap().starts_with("Change the editor theme"));
    assert_eq!(e.prompt_completions("theme ").options.len(), 3);
    assert!(e.prompt_completions("theme dark x").options.is_empty());
}

#[test]
fn buffer_commands_need_externals() {
    let mut e = ed("|a^b");
    assert_eq!(
        messages(&run(&mut e, "bn")),
        vec![("no such command: 'bn'".to_string(), true)]
    );
    let mut cfg = Config::parity();
    cfg.externals = ExternalCommands {
        buffer_next: true,
        buffer_previous: true,
        buffer_close: true,
        ..Default::default()
    };
    let mut e = ed_with("|a^b", cfg);
    assert!(run(&mut e, "bn").contains(&Effect::External(External::BufferNext)));
    assert!(run(&mut e, "bprev").contains(&Effect::External(External::BufferPrevious)));
    assert!(
        run(&mut e, "bc x y").contains(&Effect::External(External::BufferClose(vec![
            "x".into(),
            "y".into()
        ])))
    );
    assert!(run(&mut e, "buffer-close").contains(&Effect::External(External::BufferClose(vec![]))));
}

#[test]
fn app_commands() {
    let mut cfg = Config::parity();
    cfg.app_commands = false;
    let mut e = ed_with("|a^b", cfg);
    assert_eq!(
        messages(&run(&mut e, "w")),
        vec![("no such command: 'w'".to_string(), true)]
    );
    let mut e = ed("|a^b");
    assert!(run(&mut e, "w").contains(&Effect::Save));
    assert!(run(&mut e, "q").contains(&Effect::Quit));
    let fx = run(&mut e, "x");
    assert!(fx.contains(&Effect::Save) && fx.contains(&Effect::Quit));
}

#[test]
fn host_commands() {
    let mut e = ed("|a^b");
    e.register_command(CommandInfo::new(
        "open-daily",
        &["daily"],
        "Open today's note",
    ));
    let fx = run(&mut e, "daily 2026-01-01");
    assert!(fx.contains(&Effect::HostCommand {
        name: "open-daily".into(),
        args: vec!["2026-01-01".into()]
    }));
    assert!(e.commands().iter().any(|c| c.name == "open-daily"));
    // re-registering replaces
    e.register_command(CommandInfo::new("open-daily", &[], "x"));
    assert_eq!(
        e.commands()
            .iter()
            .filter(|c| c.name == "open-daily")
            .count(),
        1
    );
    let fx = e.execute_command("open-daily");
    assert!(fx.contains(&Effect::HostCommand {
        name: "open-daily".into(),
        args: vec![]
    }));
    assert_eq!(
        e.registers().get(':').unwrap(),
        &vec!["open-daily".to_string()]
    );
}

#[test]
fn trailing_spaces_are_trimmed() {
    let mut e = ed("|a^b");
    let fx = run(&mut e, "echo a  ");
    assert_eq!(messages(&fx), vec![("a".to_string(), false)]);
}

#[test]
fn prompt_read_register() {
    let mut e = editing::with_reg("|a^b", 'a', &["one", "two"]);
    e.handle_keys(":");
    assert_eq!(e.prompt_read_register('a').unwrap(), "one");
    assert_eq!(e.prompt_read_register('z').unwrap(), "");
}
