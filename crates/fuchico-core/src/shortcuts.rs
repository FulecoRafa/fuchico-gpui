//! Port of `formatBinding` from `src/modules/settings/lib/fixedShortcuts.ts`.

use std::sync::LazyLock;

use regex::Regex;

static MOD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(Mod|Meta|Ctrl|Alt|Shift)-(.+)$").unwrap());

fn mac_mod(m: &str) -> &'static str {
    match m {
        "Mod" | "Meta" => "⌘",
        "Ctrl" => "⌃",
        "Alt" => "⌥",
        _ => "⇧",
    }
}

fn other_mod(m: &str) -> &'static str {
    match m {
        "Mod" | "Ctrl" => "Ctrl",
        "Meta" => "Win",
        "Alt" => "Alt",
        _ => "Shift",
    }
}

fn format_chunk(chunk: &str, is_mac: bool) -> String {
    let mut mods = Vec::new();
    let mut rest = chunk;
    while let Some(m) = MOD_RE.captures(rest) {
        mods.push(m.get(1).unwrap().as_str());
        rest = m.get(2).unwrap().as_str();
    }
    if mods.is_empty() {
        return chunk.to_string();
    }
    let key = if rest.chars().count() == 1 { rest.to_uppercase() } else { rest.to_string() };
    if is_mac {
        format!("{}{key}", mods.iter().map(|m| mac_mod(m)).collect::<String>())
    } else {
        let mut parts: Vec<String> = mods.iter().map(|m| other_mod(m).to_string()).collect();
        parts.push(key);
        parts.join("+")
    }
}

/// Renders "Mod-Shift-p" as "⌘⇧P" (macOS) or "Ctrl+Shift+P"; " / " separates
/// alternatives.
pub fn format_binding(binding: &str, is_mac: bool) -> String {
    binding.split(" / ").map(|c| format_chunk(c, is_mac)).collect::<Vec<_>>().join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_for_macos() {
        assert_eq!(format_binding("Mod-Shift-p", true), "⌘⇧P");
        assert_eq!(format_binding("Mod-s", true), "⌘S");
        assert_eq!(format_binding("Mod-Shift-Enter", true), "⌘⇧Enter");
        assert_eq!(format_binding("Alt-ArrowUp", true), "⌥ArrowUp");
    }

    #[test]
    fn formats_for_windows_linux() {
        assert_eq!(format_binding("Mod-Shift-p", false), "Ctrl+Shift+P");
        assert_eq!(format_binding("Mod-.", false), "Ctrl+.");
    }

    #[test]
    fn keeps_the_minus_key() {
        assert_eq!(format_binding("Mod--", true), "⌘-");
        assert_eq!(format_binding("Mod-Shift--", false), "Ctrl+Shift+-");
    }

    #[test]
    fn passes_plain_keys_and_alternatives() {
        assert_eq!(format_binding("F2", true), "F2");
        assert_eq!(format_binding("Right-click", true), "Right-click");
        assert_eq!(format_binding("Mod-= / Mod-- / Mod-0", false), "Ctrl+= / Ctrl+- / Ctrl+0");
    }
}
