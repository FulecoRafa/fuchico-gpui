//! Zed colour themes (`themes/*.json`, schema
//! <https://zed.dev/schema/themes/v0.2.0.json>) mapped onto Fuchico's 22
//! colour tokens.
//!
//! A theme file is a *family*: `{ "name", "author", "themes": [ { "name",
//! "appearance": "light" | "dark", "style": { … } } ] }`. `style` holds flat
//! dotted keys (`"editor.background": "#282c34ff"`), a `syntax` map of
//! `{ "color", "font_style", "font_weight" }` objects, a `players` list and an
//! `accents` list. Any value may be `null` or missing.
//!
//! # Mapping (first present key wins; otherwise the Ayu light/dark token of
//! the theme's appearance is kept)
//!
//! | Fuchico token            | Zed style keys                                                   |
//! |--------------------------|------------------------------------------------------------------|
//! | `--background`           | `editor.background`, `background`                                |
//! | `--foreground`           | `editor.foreground`, `text`                                      |
//! | `--card`                 | `panel.background`, `tab_bar.background`, `surface.background`, `background` |
//! | `--card-foreground`      | `text`, `editor.foreground`                                      |
//! | `--primary`              | `text.accent`, `icon.accent`, `accents[0]`, `players[0].cursor`   |
//! | `--primary-foreground`   | `editor.background`, `background`                                |
//! | `--secondary`            | `element.background`, `surface.background`                       |
//! | `--secondary-foreground` | `text`                                                           |
//! | `--muted`                | `surface.background`, `element.background`                       |
//! | `--muted-foreground`     | `text.muted`, `editor.line_number`                               |
//! | `--accent`               | `element.selected`, `ghost_element.selected`, `element.hover`    |
//! | `--accent-foreground`    | `text`                                                           |
//! | `--destructive`          | `error`, `version_control.deleted`, `deleted`                    |
//! | `--border`               | `border.variant`, `border`                                       |
//! | `--input`                | `border`, `border.variant`                                       |
//! | `--ring`                 | `border.focused`, `text.accent`                                  |
//! | `--syntax-keyword`       | `syntax.keyword`                                                 |
//! | `--syntax-string`        | `syntax.string`                                                  |
//! | `--syntax-number`        | `syntax.number`, `syntax.constant`, `syntax.boolean`             |
//! | `--syntax-function`      | `syntax.function`, `syntax.link_text`, `syntax.title`            |
//! | `--syntax-type`          | `syntax.type`, `syntax.constructor`                              |
//! | `--syntax-comment`       | `syntax.comment`, `syntax.comment.doc`                           |
//!
//! Beyond the tokens ([`ThemeExtras`], library widgets only): the caret is
//! `players[0].cursor`, the text selection `players[0].selection`, and the
//! `success` / `warning` / `info` status colours replace the
//! syntax-derived ones. Markdown headings use `--foreground` and links
//! `--syntax-function`, as with the built-in palettes. See
//! `docs/ZED_THEMES.md`.

use std::collections::HashMap;

use gpui_kit::Hsla;
use serde_json::Value;

use crate::theme::color::parse_color;
use crate::theme::{Appearance, ColorTokens, ThemeExtras};

/// One theme of a family.
#[derive(Clone, Debug, PartialEq)]
pub struct ZedColorTheme {
    pub name: String,
    pub appearance: Appearance,
    pub style: ZedStyle,
}

/// The colours of a theme's `style`, parsed (unparsable values dropped).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ZedStyle {
    flat: HashMap<String, Hsla>,
    syntax: HashMap<String, Hsla>,
    /// `players[0]`: (cursor, selection).
    player: (Option<Hsla>, Option<Hsla>),
    accents: Vec<Hsla>,
}

/// A parsed theme family file.
#[derive(Clone, Debug, PartialEq)]
pub struct ZedThemeFamily {
    pub name: Option<String>,
    pub themes: Vec<ZedColorTheme>,
}

/// Removes `//` and `/* */` comments and trailing commas outside strings
/// (Zed reads its theme files leniently).
pub fn strip_jsonc(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c as char);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1] as char);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => {
                in_str = true;
                out.push('"');
                i += 1;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b',' => {
                let mut j = i + 1;
                while j < b.len() && (b[j] as char).is_whitespace() {
                    j += 1;
                }
                if !matches!(b.get(j), Some(b']') | Some(b'}')) {
                    out.push(',');
                }
                i += 1;
            }
            _ => {
                // Copy the whole UTF-8 sequence starting here.
                let len = utf8_len(c);
                out.push_str(&src[i..(i + len).min(b.len())]);
                i += len;
            }
        }
    }
    out
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

/// Parses JSON leniently (comments, trailing commas).
pub fn parse_lenient(src: &str) -> Result<Value, String> {
    serde_json::from_str(src)
        .or_else(|_| serde_json::from_str(&strip_jsonc(src)))
        .map_err(|e| e.to_string())
}

fn color(v: &Value) -> Option<Hsla> {
    let s = v.as_str()?.trim();
    parse_color(s, &|_| None).map(|c| c.to_hsla())
}

fn appearance(v: &Value) -> Option<Appearance> {
    match v.as_str()?.to_ascii_lowercase().as_str() {
        "light" => Some(Appearance::Light),
        "dark" => Some(Appearance::Dark),
        _ => None,
    }
}

/// Whether a parsed JSON value looks like a colour theme family.
pub fn is_color_family(v: &Value) -> bool {
    v.get("themes")
        .and_then(Value::as_array)
        .is_some_and(|t| t.iter().any(|t| t.get("style").is_some()))
}

impl ZedThemeFamily {
    /// Parses a family; themes without a name, a valid appearance or a
    /// `style` object are skipped (logged). Errors when nothing usable.
    pub fn parse(json: &str) -> Result<ZedThemeFamily, String> {
        let v = parse_lenient(json)?;
        Self::from_value(&v)
    }

    pub fn from_value(v: &Value) -> Result<ZedThemeFamily, String> {
        let list = v
            .get("themes")
            .and_then(Value::as_array)
            .ok_or("no \"themes\" array")?;
        let mut themes = Vec::new();
        for (i, t) in list.iter().enumerate() {
            let (Some(name), Some(app), Some(style)) = (
                t.get("name").and_then(Value::as_str),
                t.get("appearance").and_then(appearance),
                t.get("style").and_then(Value::as_object),
            ) else {
                log::warn!("zed theme #{i}: missing name / appearance / style, skipped");
                continue;
            };
            themes.push(ZedColorTheme {
                name: name.to_string(),
                appearance: app,
                style: ZedStyle::from_map(style),
            });
        }
        if themes.is_empty() {
            return Err("no usable themes".into());
        }
        Ok(ZedThemeFamily {
            name: v.get("name").and_then(Value::as_str).map(str::to_string),
            themes,
        })
    }
}

impl ZedStyle {
    fn from_map(style: &serde_json::Map<String, Value>) -> ZedStyle {
        let mut s = ZedStyle::default();
        for (k, v) in style {
            match k.as_str() {
                "syntax" => {
                    if let Some(m) = v.as_object() {
                        for (sk, sv) in m {
                            if let Some(c) = sv.get("color").and_then(color) {
                                s.syntax.insert(sk.clone(), c);
                            }
                        }
                    }
                }
                "players" => {
                    if let Some(p) = v.as_array().and_then(|a| a.first()) {
                        s.player = (
                            p.get("cursor").and_then(color),
                            p.get("selection").and_then(color),
                        );
                    }
                }
                "accents" => {
                    if let Some(a) = v.as_array() {
                        s.accents = a.iter().filter_map(color).collect();
                    }
                }
                _ => {
                    if let Some(c) = color(v) {
                        s.flat.insert(k.clone(), c);
                    }
                }
            }
        }
        s
    }

    fn first(&self, keys: &[&str]) -> Option<Hsla> {
        keys.iter().find_map(|k| self.flat.get(*k).copied())
    }

    fn syn(&self, keys: &[&str]) -> Option<Hsla> {
        keys.iter().find_map(|k| self.syntax.get(*k).copied())
    }

    /// The mapped tokens over `base` (see the module table) and extras.
    pub fn tokens(&self, base: ColorTokens) -> (ColorTokens, ThemeExtras) {
        let mut t = base;
        let set = |field: &mut Hsla, v: Option<Hsla>| {
            if let Some(v) = v {
                *field = v;
            }
        };
        let text = self.first(&["text", "editor.foreground"]);
        set(
            &mut t.background,
            self.first(&["editor.background", "background"]),
        );
        set(
            &mut t.foreground,
            self.first(&["editor.foreground", "text"]),
        );
        set(
            &mut t.card,
            self.first(&[
                "panel.background",
                "tab_bar.background",
                "surface.background",
                "background",
            ]),
        );
        set(&mut t.card_foreground, text);
        set(
            &mut t.primary,
            self.first(&["text.accent", "icon.accent"])
                .or_else(|| self.accents.first().copied())
                .or(self.player.0),
        );
        set(
            &mut t.primary_foreground,
            self.first(&["editor.background", "background"]),
        );
        set(
            &mut t.secondary,
            self.first(&["element.background", "surface.background"]),
        );
        set(&mut t.secondary_foreground, text);
        set(
            &mut t.muted,
            self.first(&["surface.background", "element.background"]),
        );
        set(
            &mut t.muted_foreground,
            self.first(&["text.muted", "editor.line_number"]),
        );
        set(
            &mut t.accent,
            self.first(&[
                "element.selected",
                "ghost_element.selected",
                "element.hover",
            ]),
        );
        set(&mut t.accent_foreground, text);
        set(
            &mut t.destructive,
            self.first(&["error", "version_control.deleted", "deleted"]),
        );
        set(&mut t.border, self.first(&["border.variant", "border"]));
        set(&mut t.input, self.first(&["border", "border.variant"]));
        set(&mut t.ring, self.first(&["border.focused", "text.accent"]));
        set(&mut t.syntax_keyword, self.syn(&["keyword"]));
        set(&mut t.syntax_string, self.syn(&["string"]));
        set(
            &mut t.syntax_number,
            self.syn(&["number", "constant", "boolean"]),
        );
        set(
            &mut t.syntax_function,
            self.syn(&["function", "link_text", "title"]),
        );
        set(&mut t.syntax_type, self.syn(&["type", "constructor"]));
        set(&mut t.syntax_comment, self.syn(&["comment", "comment.doc"]));
        let extras = ThemeExtras {
            cursor: self.player.0,
            selection: self.player.1,
            success: self.first(&["success"]),
            warning: self.first(&["warning"]),
            info: self.first(&["info"]),
        };
        (t, extras)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::PaletteId;
    use crate::theme::hex;

    pub const ONE: &str = include_str!("fixtures/one/themes/one.json");

    #[test]
    fn parses_multi_theme_family_with_appearance() {
        let f = ZedThemeFamily::parse(ONE).unwrap();
        assert_eq!(f.name.as_deref(), Some("One"));
        let names: Vec<_> = f
            .themes
            .iter()
            .map(|t| (t.name.as_str(), t.appearance))
            .collect();
        assert_eq!(
            names,
            vec![
                ("One Dark", Appearance::Dark),
                ("One Light", Appearance::Light)
            ]
        );
    }

    #[test]
    fn maps_keys_onto_tokens() {
        let f = ZedThemeFamily::parse(ONE).unwrap();
        let dark = &f.themes[0];
        let base = ColorTokens::built_in(PaletteId::Ayu, Appearance::Dark);
        let (t, x) = dark.style.tokens(base);
        assert_eq!(hex(t.background), "#282c33");
        assert_eq!(hex(t.foreground), "#acb2be");
        assert_eq!(hex(t.card), "#2f343e");
        assert_eq!(hex(t.muted_foreground), "#a9afbc");
        assert_eq!(hex(t.primary), "#74ade8");
        assert_eq!(hex(t.border), "#363c46");
        assert_eq!(hex(t.ring), "#47679e");
        assert_eq!(hex(t.accent), "#454a56");
        assert_eq!(hex(t.destructive), "#d07277");
        assert_eq!(hex(t.syntax_keyword), "#b477cf");
        assert_eq!(hex(t.syntax_string), "#a1c181");
        assert_eq!(hex(t.syntax_function), "#73ade9");
        assert_eq!(hex(t.syntax_comment), "#5d636f");
        assert_eq!(x.cursor.map(hex).as_deref(), Some("#74ade8"));
        assert_eq!(x.selection.map(hex).as_deref(), Some("#74ade83d"));
    }

    #[test]
    fn missing_keys_keep_the_base_and_bad_values_are_dropped() {
        let f = ZedThemeFamily::parse(
            r##"{"name":"Tiny","themes":[
                {"name":"Tiny","appearance":"light","style":{
                    "background":"#ff0000ff","text":"not a colour","border":null,
                    "syntax":{"keyword":{"color":"#00ff00"}}}}]}"##,
        )
        .unwrap();
        let base = ColorTokens::built_in(PaletteId::Ayu, Appearance::Light);
        let (t, x) = f.themes[0].style.tokens(base);
        assert_eq!(hex(t.background), "#ff0000");
        assert_eq!(hex(t.syntax_keyword), "#00ff00");
        assert_eq!(t.foreground, base.foreground);
        assert_eq!(t.border, base.border);
        assert_eq!(t.syntax_string, base.syntax_string);
        assert_eq!(x, ThemeExtras::default());
    }

    #[test]
    fn skips_bad_entries_and_rejects_garbage() {
        let f = ZedThemeFamily::parse(
            r#"{"themes":[{"name":"No appearance","style":{}},
                {"name":"Ok","appearance":"DARK","style":{}},]}"#,
        )
        .unwrap();
        assert_eq!(f.themes.len(), 1);
        assert_eq!(f.themes[0].appearance, Appearance::Dark);
        assert!(ZedThemeFamily::parse("{nope").is_err());
        assert!(ZedThemeFamily::parse(r#"{"themes":[]}"#).is_err());
        assert!(ZedThemeFamily::parse(r#"{"name":"x"}"#).is_err());
    }

    #[test]
    fn jsonc_is_accepted() {
        let src = "{\n// comment\n\"a\": \"x // not a comment\", /* block */ \"b\": [1, 2,],\n}";
        let v = parse_lenient(src).unwrap();
        assert_eq!(v["a"], "x // not a comment");
        assert_eq!(v["b"], serde_json::json!([1, 2]));
    }
}
