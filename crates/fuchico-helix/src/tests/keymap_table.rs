//! Binding table integrity and `COMMANDS.md` freshness.

use crate::keymap::{lookup, normal_alias};
use crate::{BindingStatus, KeyEvent, KeymapTable, bindings, commands_markdown};
use std::collections::HashSet;

const BEGIN: &str = "<!-- BEGIN GENERATED BINDINGS -->\n";
const END: &str = "<!-- END GENERATED BINDINGS -->";

#[test]
fn every_key_parses_and_roundtrips() {
    for b in bindings() {
        let ev = KeyEvent::parse(b.key).unwrap_or_else(|| panic!("{:?} does not parse", b.key));
        assert_eq!(ev.name(), b.key, "name roundtrip for {:?}", b.key);
    }
}

#[test]
fn keys_are_unique_per_table() {
    let mut seen = HashSet::new();
    for b in bindings() {
        assert!(
            seen.insert((b.table, b.key)),
            "duplicate binding {:?} {:?}",
            b.table,
            b.key
        );
    }
}

#[test]
fn aliases_resolve() {
    for b in bindings()
        .iter()
        .filter(|b| b.description.starts_with("alias"))
    {
        assert_eq!(b.table, KeymapTable::Normal);
        let target = normal_alias(b.key).unwrap_or_else(|| panic!("{} has no alias target", b.key));
        assert!(
            b.description.ends_with(target),
            "{:?} describes {:?}",
            b.key,
            target
        );
        let resolved = lookup(KeymapTable::Normal, b.key, false).expect("alias resolves");
        assert_eq!(resolved.key, target);
        assert_eq!(resolved.command, b.command);
    }
}

#[test]
fn extensions_are_hidden_in_parity() {
    for b in bindings()
        .iter()
        .filter(|b| !b.description.starts_with("alias"))
    {
        let parity = lookup(b.table, b.key, false);
        let ext = lookup(b.table, b.key, true).expect("visible with extensions");
        assert_eq!(ext.command, b.command);
        if b.status == BindingStatus::Extension {
            assert!(
                parity.is_none(),
                "{:?} {:?} leaks into parity",
                b.table,
                b.key
            );
        } else {
            assert_eq!(parity.unwrap().command, b.command);
        }
    }
}

#[test]
fn every_table_has_bindings() {
    for t in [
        KeymapTable::Insert,
        KeymapTable::Normal,
        KeymapTable::Goto,
        KeymapTable::Match,
        KeymapTable::Space,
        KeymapTable::LeftBracket,
        KeymapTable::RightBracket,
        KeymapTable::View,
    ] {
        assert!(bindings().iter().any(|b| b.table == t), "{t:?}");
    }
}

/// `COMMANDS.md` embeds [`commands_markdown`]. Regenerate with
/// `UPDATE_COMMANDS_MD=1 cargo test -p fuchico-helix commands_md`.
#[test]
fn commands_md_is_up_to_date() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/COMMANDS.md");
    let doc = std::fs::read_to_string(path).expect("COMMANDS.md exists");
    let start = doc.find(BEGIN).expect("begin marker") + BEGIN.len();
    let end = doc.find(END).expect("end marker");
    let generated = commands_markdown();
    if std::env::var_os("UPDATE_COMMANDS_MD").is_some() {
        let updated = format!("{}{}{}", &doc[..start], generated, &doc[end..]);
        std::fs::write(path, updated).unwrap();
        return;
    }
    assert!(
        doc[start..end] == generated,
        "COMMANDS.md is stale; rerun with UPDATE_COMMANDS_MD=1"
    );
}
