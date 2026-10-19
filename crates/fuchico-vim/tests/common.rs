#![allow(dead_code)]
use fuchico_vim::{Config, Editor, Effect};

pub fn ed(text: &str) -> Editor {
    Editor::new(text)
}

pub fn ed_app(text: &str) -> Editor {
    let cfg = Config { app_commands: true, ..Config::default() };
    Editor::with_config(text, cfg)
}

/// Run keys, return the text.
pub fn run(text: &str, keys: &str) -> String {
    let mut e = ed(text);
    e.handle_keys(keys);
    e.text()
}

/// Run keys, return (text, (line, col)) — 1-based.
pub fn run_c(text: &str, keys: &str) -> (String, (usize, usize)) {
    let mut e = ed(text);
    e.handle_keys(keys);
    (e.text(), e.line_col())
}

pub fn cur(text: &str, keys: &str) -> (usize, usize) {
    run_c(text, keys).1
}

pub fn msg(e: &Editor) -> String {
    e.message().map(|m| m.0.to_string()).unwrap_or_default()
}

pub fn has(effs: &[Effect], f: impl Fn(&Effect) -> bool) -> bool {
    effs.iter().any(f)
}
