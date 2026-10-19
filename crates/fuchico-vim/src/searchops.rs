//! Search state and options: `parseQuery`, `updateSearchQuery`,
//! `findNext`, `findNextFromAndToInclusive`, the highlight overlay and
//! `getOption` / `setOption`.

use crate::editor::Editor;
use crate::pos::{INF, Pos};
use crate::search::{Query, SearchCursor, find_unescaped_separators, translate_regex};
use crate::state::OptionValue;

/// Option scope for `:set` / `:setlocal` / `:setglobal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    Any,
    Local,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptType {
    Bool,
    Number,
    Str,
}

/// The defined option names (for completion).
pub(crate) fn option_names() -> Vec<&'static str> {
    vec!["pcre", "insertModeEscKeysTimeout", "textwidth", "filetype"]
}

/// `options[name]` after alias resolution.
pub(crate) fn option_def(name: &str) -> Option<(&'static str, OptType)> {
    Some(match name {
        "pcre" => ("pcre", OptType::Bool),
        "insertModeEscKeysTimeout" => ("insertModeEscKeysTimeout", OptType::Number),
        "textwidth" | "tw" => ("textwidth", OptType::Number),
        "filetype" | "ft" => ("filetype", OptType::Str),
        _ => return None,
    })
}

impl Query {
    /// `query.toString()` (`/source/flags`).
    pub fn display(&self) -> String {
        format!("/{}/{}", self.source, if self.ignore_case { "im" } else { "m" })
    }
}

impl Editor {
    // ---- options ------------------------------------------------------------

    /// `getOption(name, cm, cfg)`.
    pub(crate) fn get_option(&self, name: &str, scope: Scope) -> Result<Option<OptionValue>, String> {
        let Some((name, _)) = option_def(name) else {
            return Err(format!("Unknown option: {name}"));
        };
        // callback options (textwidth, filetype) are local proxies
        match name {
            "textwidth" => {
                if scope == Scope::Global {
                    return Ok(None);
                }
                return Ok(Some(
                    self.vim
                        .options
                        .get("textwidth")
                        .cloned()
                        .unwrap_or(OptionValue::Number(80)),
                ));
            }
            "filetype" => {
                if scope == Scope::Global {
                    return Ok(None);
                }
                return Ok(Some(
                    self.vim
                        .options
                        .get("filetype")
                        .cloned()
                        .unwrap_or(OptionValue::Str("markdown".into())),
                ));
            }
            _ => {}
        }
        if scope != Scope::Global
            && let Some(v) = self.vim.options.get(name)
        {
            return Ok(Some(v.clone()));
        }
        if scope != Scope::Local {
            return Ok(self.g.options.get(name).cloned());
        }
        Ok(None)
    }

    pub(crate) fn get_option_bool(&self, name: &str) -> bool {
        matches!(self.get_option(name, Scope::Any), Ok(Some(OptionValue::Bool(true))))
    }

    pub(crate) fn get_option_number(&self, name: &str) -> i64 {
        match self.get_option(name, Scope::Any) {
            Ok(Some(OptionValue::Number(n))) => n,
            _ => 0,
        }
    }

    /// `setOption(name, value, cm, cfg)`. `value` is the raw `:set`
    /// argument (`None` = no `=value`, `Some(false)` style booleans are
    /// passed as `OptionValue::Bool`).
    pub(crate) fn set_option(
        &mut self,
        name: &str,
        value: Option<OptionValue>,
        scope: Scope,
    ) -> Result<(), String> {
        let Some((canon, ty)) = option_def(name) else {
            return Err(format!("Unknown option: {name}"));
        };
        let value = match ty {
            OptType::Bool => match value {
                None | Some(OptionValue::Bool(true)) => OptionValue::Bool(true),
                Some(OptionValue::Bool(false)) => OptionValue::Bool(false),
                Some(OptionValue::Str(s)) if s.is_empty() => OptionValue::Bool(true),
                Some(v) => {
                    let shown = match v {
                        OptionValue::Number(n) => n.to_string(),
                        OptionValue::Str(s) => s,
                        OptionValue::Bool(b) => b.to_string(),
                    };
                    return Err(format!("Invalid argument: {name}={shown}"));
                }
            },
            OptType::Number => match value {
                Some(OptionValue::Number(n)) => OptionValue::Number(n),
                Some(OptionValue::Str(s)) => {
                    // JS: numbers are parsed with `parseInt` by :set
                    OptionValue::Number(s.trim().parse::<f64>().map(|f| f.round() as i64).unwrap_or(0))
                }
                Some(OptionValue::Bool(b)) => OptionValue::Number(i64::from(b)),
                None => OptionValue::Number(0),
            },
            OptType::Str => match value {
                Some(OptionValue::Str(s)) => OptionValue::Str(s),
                Some(OptionValue::Number(n)) => OptionValue::Str(n.to_string()),
                Some(OptionValue::Bool(b)) => OptionValue::Str(b.to_string()),
                None => OptionValue::Str(String::new()),
            },
        };
        match canon {
            "textwidth" => {
                if scope != Scope::Global
                    && let OptionValue::Number(n) = value
                    && n > 1
                {
                    self.vim.options.insert("textwidth".into(), OptionValue::Number(n));
                }
            }
            "filetype" => {
                if scope != Scope::Global {
                    self.vim.options.insert("filetype".into(), value);
                }
            }
            _ => {
                if scope != Scope::Local {
                    self.g.options.insert(canon.to_string(), value.clone());
                }
                if scope != Scope::Global {
                    self.vim.options.insert(canon.to_string(), value);
                }
            }
        }
        Ok(())
    }

    // ---- queries --------------------------------------------------------------

    /// `parseQuery(query, ignoreCase, smartCase)`. `Ok(None)` = blank.
    pub(crate) fn parse_query(
        &mut self,
        query: &str,
        ignore_case: bool,
        smart_case: bool,
    ) -> Result<Option<Query>, String> {
        self.g
            .register_controller
            .get_register(Some('/'))
            .set_text(query, false, false);
        let slashes = find_unescaped_separators(query, '/');
        let mut force_ignore_case = false;
        let mut regex_part: String = if slashes.is_empty() {
            query.to_string()
        } else {
            let flags: String = query.chars().skip(slashes[0]).collect();
            force_ignore_case = flags.contains('i');
            query.chars().take(slashes[0]).collect()
        };
        if regex_part.is_empty() {
            return Ok(None);
        }
        if !self.get_option_bool("pcre") {
            regex_part = translate_regex(&regex_part);
        }
        let mut ignore_case = ignore_case;
        if smart_case {
            ignore_case = !regex_part.chars().any(|c| c.is_ascii_uppercase());
        }
        Query::new(&regex_part, ignore_case || force_ignore_case).map(Some)
    }

    /// `updateSearchQuery`: `Err` = invalid regex.
    pub(crate) fn update_search_query(
        &mut self,
        raw: &str,
        ignore_case: bool,
        smart_case: bool,
    ) -> Result<Option<Query>, String> {
        if raw.is_empty() {
            return Ok(None);
        }
        let Some(q) = self.parse_query(raw, ignore_case, smart_case)? else {
            return Ok(None);
        };
        self.highlight_search_matches(&q);
        if self.g.search_query.as_ref() != Some(&q) {
            self.g.search_query = Some(q.clone());
        }
        Ok(Some(q))
    }

    /// `highlightSearchMatches` (the 50ms debounce is dropped).
    pub(crate) fn highlight_search_matches(&mut self, q: &Query) {
        if self.highlight.as_ref() != Some(q) {
            self.highlight = Some(q.clone());
            self.emit(fuchico_helix::Effect::StatusChanged);
        }
        self.vim.search_overlay = true;
    }

    /// `clearSearchHighlight`.
    pub(crate) fn clear_search_highlight(&mut self) {
        if self.highlight.is_some() {
            self.emit(fuchico_helix::Effect::StatusChanged);
        }
        self.highlight = None;
        self.vim.search_overlay = false;
    }

    fn wrap_cursor(&self, q: &Query, prev: bool) -> SearchCursor {
        let p = if prev {
            Pos::new(self.last_line(), INF)
        } else {
            Pos::new(self.first_line(), 0)
        };
        SearchCursor::new(q, self.index_from_pos(p))
    }

    /// `findNext(cm, prev, query, repeat)`.
    pub(crate) fn find_next(&mut self, prev: bool, q: &Query, repeat: i64) -> Option<Pos> {
        let repeat = if repeat == 0 { 1 } else { repeat };
        let pos = self.index_from_pos(self.get_cursor("head"));
        let mut cursor = SearchCursor::new(q, pos);
        for i in 0..repeat {
            let mut found = cursor.find(&self.doc, prev);
            if i == 0 && found.is_some() && cursor.from() == Some(pos) {
                let last_end = if prev { cursor.from() } else { cursor.to() }.unwrap_or(0);
                found = cursor.find(&self.doc, prev);
                if found.as_deref() == Some("") && cursor.from() == Some(last_end) {
                    let le = self.pos_from_index(last_end);
                    if self.line_len(le.line) == le.ch {
                        found = cursor.find(&self.doc, prev);
                    }
                }
            }
            if found.is_none() {
                cursor = self.wrap_cursor(q, prev);
                cursor.find(&self.doc, prev)?;
            }
        }
        cursor.from().map(|o| self.pos_from_index(o))
    }

    /// `findNextFromAndToInclusive`.
    pub(crate) fn find_next_from_and_to_inclusive(
        &mut self,
        prev: bool,
        q: &Query,
        repeat: i64,
    ) -> Option<(Pos, Pos)> {
        let repeat = if repeat == 0 { 1 } else { repeat };
        let pos = self.index_from_pos(self.get_cursor("head"));
        let mut cursor = SearchCursor::new(q, pos);
        let found = cursor.find(&self.doc, !prev);
        if !self.vim.visual_mode && found.is_some() && cursor.from() == Some(pos) {
            cursor.find(&self.doc, !prev);
        }
        for _ in 0..repeat {
            if cursor.find(&self.doc, prev).is_none() {
                cursor = self.wrap_cursor(q, prev);
                cursor.find(&self.doc, prev)?;
            }
        }
        let from = cursor.from()?;
        let to = cursor.to()?;
        Some((self.pos_from_index(from), self.pos_from_index(to)))
    }
}
