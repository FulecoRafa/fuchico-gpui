//! Language tables: the 143 `@codemirror/language-data` entries (for fenced
//! code info strings), the file languages of `languageDefinitions.ts` (for
//! non-Markdown files), and the fence-toolbar language list.

/// One `@codemirror/language-data` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageDesc {
    /// Display name, e.g. `"C++"`.
    pub name: &'static str,
    /// `LanguageDescription.alias`: the explicit aliases followed by the
    /// lower-cased name (the exact array CodeMirror builds, in that order).
    pub alias: &'static [&'static str],
    /// Name of the syntect grammar used to highlight it: a two-face (bat)
    /// grammar or one of the legacy-mode ports in `grammars/`. Every entry
    /// has one (`tests/languages.rs`); `None` would render monospace.
    pub grammar: Option<&'static str>,
}

macro_rules! langs {
    ($( $name:literal [$($a:literal),*] $g:expr ;)*) => {
        &[ $( LanguageDesc { name: $name, alias: &[$($a),*], grammar: $g } ),* ]
    };
}

const JS: Option<&str> = Some("JavaScript (Babel)");
const SQL: Option<&str> = Some("SQL");

/// The 143 language-data descriptions, in CodeMirror's order (order matters
/// for [`match_language_name`]).
pub static LANGUAGES: &[LanguageDesc] = langs! {
    "C" ["c"] Some("C");
    "C++" ["cpp", "c++"] Some("C++");
    "CQL" ["cassandra", "cql"] SQL;
    "CSS" ["css"] Some("CSS");
    "Go" ["go"] Some("Go");
    "HTML" ["xhtml", "html"] Some("HTML");
    "Java" ["java"] Some("Java");
    "JavaScript" ["ecmascript", "js", "node", "javascript"] JS;
    "Jinja" ["jinja"] Some("Jinja2");
    "JSON" ["json5", "json"] Some("JSON");
    "JSX" ["jsx"] JS;
    "LESS" ["less"] Some("Less");
    "Liquid" ["liquid"] Some("HTML");
    "MariaDB SQL" ["mariadb sql"] SQL;
    "Markdown" ["markdown"] Some("Markdown");
    "MS SQL" ["ms sql"] SQL;
    "MySQL" ["mysql"] SQL;
    "PHP" ["php"] Some("PHP");
    "PLSQL" ["plsql"] SQL;
    "PostgreSQL" ["postgresql"] SQL;
    "Python" ["python"] Some("Python");
    "Rust" ["rust"] Some("Rust");
    "Sass" ["sass"] Some("Sass");
    "SCSS" ["scss"] Some("SCSS");
    "SQL" ["sql"] SQL;
    "SQLite" ["sqlite"] SQL;
    "TSX" ["tsx"] Some("TypeScriptReact");
    "TypeScript" ["ts", "typescript"] Some("TypeScript");
    "WebAssembly" ["webassembly"] Some("WebAssembly");
    "XML" ["rss", "wsdl", "xsd", "xml"] Some("XML");
    "YAML" ["yml", "yaml"] Some("YAML");
    "APL" ["apl"] Some("APL");
    "PGP" ["asciiarmor", "pgp"] Some("PGP");
    "ASN.1" ["asn.1"] Some("ASN.1");
    "Asterisk" ["asterisk"] Some("Asterisk");
    "Brainfuck" ["brainfuck"] Some("Brainfuck");
    "Cobol" ["cobol"] Some("Cobol");
    "C#" ["csharp", "cs", "c#"] Some("C#");
    "Clojure" ["clojure"] Some("Clojure");
    "ClojureScript" ["clojurescript"] Some("Clojure");
    "Closure Stylesheets (GSS)" ["closure stylesheets (gss)"] Some("CSS");
    "CMake" ["cmake"] Some("CMake");
    "CoffeeScript" ["coffee", "coffee-script", "coffeescript"] Some("CoffeeScript");
    "Common Lisp" ["lisp", "common lisp"] Some("Lisp");
    "Cypher" ["cypher"] Some("Cypher");
    "Cython" ["cython"] Some("Python");
    "Crystal" ["crystal"] Some("Crystal");
    "D" ["d"] Some("D");
    "Dart" ["dart"] Some("Dart");
    "diff" ["diff"] Some("Diff");
    "Dockerfile" ["dockerfile"] Some("Dockerfile");
    "DTD" ["dtd"] Some("XML");
    "Dylan" ["dylan"] Some("Dylan");
    "EBNF" ["ebnf"] Some("EBNF");
    "ECL" ["ecl"] Some("ECL");
    "edn" ["edn"] Some("Clojure");
    "Eiffel" ["eiffel"] Some("Eiffel");
    "Elm" ["elm"] Some("Elm");
    "Erlang" ["erlang"] Some("Erlang");
    "Esper" ["esper"] Some("Esper");
    "Factor" ["factor"] Some("Factor");
    "FCL" ["fcl"] Some("FCL");
    "Forth" ["forth"] Some("Forth");
    "Fortran" ["fortran"] Some("Fortran (Modern)");
    "F#" ["fsharp", "f#"] Some("F#");
    "Gas" ["gas"] Some("x86_64 Assembly");
    "Gherkin" ["gherkin"] Some("Gherkin");
    "Groovy" ["groovy"] Some("Groovy");
    "Haskell" ["haskell"] Some("Haskell");
    "Haxe" ["haxe"] Some("Haxe");
    "HXML" ["hxml"] Some("HXML");
    "HTTP" ["http"] Some("HTTP Request and Response");
    "IDL" ["idl"] Some("IDL");
    "JSON-LD" ["jsonld", "json-ld"] Some("JSON");
    "Julia" ["julia"] Some("Julia");
    "Kotlin" ["kotlin"] Some("Kotlin");
    "LiveScript" ["ls", "livescript"] Some("LiveScript");
    "Lua" ["lua"] Some("Lua");
    "mIRC" ["mirc"] Some("mIRC");
    "Mathematica" ["mathematica"] Some("Mathematica");
    "Modelica" ["modelica"] Some("Modelica");
    "MUMPS" ["mumps"] Some("MUMPS");
    "Mbox" ["mbox"] Some("Email");
    "Nginx" ["nginx"] Some("nginx");
    "NSIS" ["nsis"] Some("NSIS");
    "NTriples" ["ntriples"] Some("NTriples");
    "Objective-C" ["objective-c", "objc", "objective-c"] Some("Objective-C");
    "Objective-C++" ["objective-c++", "objc++", "objective-c++"] Some("Objective-C++");
    "OCaml" ["ocaml"] Some("OCaml");
    "Octave" ["octave"] Some("MATLAB");
    "Oz" ["oz"] Some("Oz");
    "Pascal" ["pascal"] Some("Pascal");
    "Perl" ["perl"] Some("Perl");
    "Pig" ["pig"] Some("Pig");
    "PowerShell" ["powershell"] Some("PowerShell");
    "Properties files" ["ini", "properties", "properties files"] Some("INI");
    "ProtoBuf" ["protobuf"] Some("Protocol Buffer");
    "Pug" ["jade", "pug"] Some("Pug");
    "Puppet" ["puppet"] Some("Puppet");
    "Q" ["q"] Some("Q");
    "R" ["rscript", "r"] Some("R");
    "RPM Changes" ["rpm changes"] Some("RPM Changes");
    "RPM Spec" ["rpm spec"] Some("RPM Spec");
    "Ruby" ["jruby", "macruby", "rake", "rb", "rbx", "ruby"] Some("Ruby");
    "SAS" ["sas"] Some("SAS");
    "Scala" ["scala"] Some("Scala");
    "Scheme" ["scheme"] Some("Lisp");
    "Shell" ["bash", "sh", "zsh", "shell"] Some("Bourne Again Shell (bash)");
    "Sieve" ["sieve"] Some("Sieve");
    "Smalltalk" ["smalltalk"] Some("Smalltalk");
    "Solr" ["solr"] Some("Solr");
    "SML" ["sml"] Some("SML");
    "SPARQL" ["sparul", "sparql"] Some("SPARQL");
    "Spreadsheet" ["excel", "formula", "spreadsheet"] Some("Spreadsheet");
    "Squirrel" ["squirrel"] Some("Squirrel");
    "Stylus" ["stylus"] Some("Stylus");
    "Swift" ["swift"] Some("Swift");
    "sTeX" ["stex"] Some("LaTeX");
    "LaTeX" ["tex", "latex"] Some("LaTeX");
    "SystemVerilog" ["systemverilog"] Some("SystemVerilog");
    "Tcl" ["tcl"] Some("Tcl");
    "Textile" ["textile"] Some("Textile");
    "TiddlyWiki" ["tiddlywiki"] Some("TiddlyWiki");
    "Tiki wiki" ["tiki wiki"] Some("Tiki wiki");
    "TOML" ["toml"] Some("TOML");
    "Troff" ["troff"] Some("Groff/troff");
    "TTCN" ["ttcn"] Some("TTCN");
    "TTCN_CFG" ["ttcn_cfg"] Some("TTCN_CFG");
    "Turtle" ["turtle"] Some("Turtle");
    "Web IDL" ["web idl"] Some("Web IDL");
    "VB.NET" ["vb.net"] Some("VB.NET");
    "VBScript" ["vbscript"] Some("VBScript");
    "Velocity" ["velocity"] Some("Velocity");
    "Verilog" ["verilog"] Some("Verilog");
    "VHDL" ["vhdl"] Some("VHDL");
    "XQuery" ["xquery"] Some("XQuery");
    "Yacas" ["yacas"] Some("Yacas");
    "Z80" ["z80"] Some("Z80");
    "MscGen" ["mscgen"] Some("MscGen");
    "Xù" ["xù"] Some("Xù");
    "MsGenny" ["msgenny"] Some("MsGenny");
    "Vue" ["vue"] Some("Vue Component");
    "Angular Template" ["angular template"] Some("HTML");
};

fn is_js_word(c: Option<char>) -> bool {
    // JS `/\w/.test(undefined)` tests the string "undefined", which is true:
    // a missing neighbour counts as a word character.
    c.is_none_or(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Port of `LanguageDescription.matchLanguageName(languages, name, fuzzy)`:
/// exact (case-insensitive) alias match first, in list order; then, if
/// `fuzzy`, the first alias contained in `name` that is longer than two
/// characters or is delimited by non-word characters on both sides.
pub fn match_language_name(name: &str, fuzzy: bool) -> Option<&'static LanguageDesc> {
    let name = name.to_lowercase();
    if let Some(d) = LANGUAGES.iter().find(|d| d.alias.iter().any(|a| *a == name)) {
        return Some(d);
    }
    if !fuzzy {
        return None;
    }
    for d in LANGUAGES {
        for a in d.alias {
            if let Some(found) = name.find(a) {
                let before = name[..found].chars().next_back();
                let after = name[found + a.len()..].chars().next();
                // `a.length` is UTF-16 length; every alias is ASCII except "xù".
                if a.chars().count() > 2 || (!is_js_word(before) && !is_js_word(after)) {
                    return Some(d);
                }
            }
        }
    }
    None
}

/// Resolves a fenced-code info string the way lang-markdown's
/// `getCodeParser` did: keep only the first whitespace-free word, then
/// [`match_language_name`] with fuzzy matching.
pub fn language_for_info(info: &str) -> Option<&'static LanguageDesc> {
    let word = info.split(char::is_whitespace).find(|_| true).unwrap_or("");
    if info.is_empty() || word.is_empty() && info.trim().is_empty() {
        return None;
    }
    // `/\S*/.exec(info)[0]` takes the leading run of non-space characters,
    // which is empty when the info string starts with whitespace; an empty
    // name still goes through matchLanguageName (and matches nothing).
    let lead: String = info.chars().take_while(|c| !c.is_whitespace()).collect();
    match_language_name(&lead, true)
}

/// The fence toolbar's language list (`<select class="cm-fence-lang">`),
/// after the leading `plain` (value `""`).
pub const TOOLBAR_LANGUAGES: &[&str] = &[
    "typescript", "javascript", "tsx", "jsx", "python", "rust", "go", "java", "c", "cpp", "css", "html", "json",
    "yaml", "toml", "xml", "sql", "bash", "sh", "markdown", "dockerfile",
];

/// Options shown by the toolbar for a block whose current info string is
/// `current` (trimmed, lower-cased): `""` (plain) first, then `current` if it
/// is not in the list, then [`TOOLBAR_LANGUAGES`].
pub fn toolbar_options(current: &str) -> Vec<String> {
    let mut v = vec![String::new()];
    if !current.is_empty() && !TOOLBAR_LANGUAGES.contains(&current) {
        v.push(current.to_string());
    }
    v.extend(TOOLBAR_LANGUAGES.iter().map(|s| s.to_string()));
    v
}

/// A file language from `languageDefinitions.ts` (non-Markdown files).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileLanguage {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    /// syntect grammar used to highlight it.
    pub grammar: &'static str,
    /// Live preview (decorations) applies only to Markdown.
    pub is_markdown: bool,
}

pub static FILE_LANGUAGES: &[FileLanguage] = &[
    FileLanguage { name: "JavaScript", extensions: &["js", "cjs", "mjs"], grammar: "JavaScript (Babel)", is_markdown: false },
    FileLanguage { name: "TypeScript", extensions: &["ts", "cts", "mts"], grammar: "TypeScript", is_markdown: false },
    FileLanguage { name: "JavaScript React", extensions: &["jsx"], grammar: "JavaScript (Babel)", is_markdown: false },
    FileLanguage { name: "TypeScript React", extensions: &["tsx"], grammar: "TypeScriptReact", is_markdown: false },
    FileLanguage { name: "Rust", extensions: &["rs"], grammar: "Rust", is_markdown: false },
    FileLanguage { name: "Python", extensions: &["py"], grammar: "Python", is_markdown: false },
    FileLanguage { name: "JSON", extensions: &["json", "jsonc"], grammar: "JSON", is_markdown: false },
    FileLanguage { name: "Markdown", extensions: &["md", "markdown"], grammar: "Markdown", is_markdown: true },
    FileLanguage { name: "HTML", extensions: &["html", "htm"], grammar: "HTML", is_markdown: false },
    FileLanguage { name: "CSS", extensions: &["css"], grammar: "CSS", is_markdown: false },
];

/// `resolveLanguage(path)`: lower-cased basename; exact filename match (none
/// are defined), then the substring after the last `.`. A name with no dot or
/// ending in a dot has no extension.
pub fn resolve_file_language(path: &str) -> Option<&'static FileLanguage> {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path).to_lowercase();
    let dot = base.rfind('.')?;
    let ext = &base[dot + 1..];
    if ext.is_empty() {
        return None;
    }
    FILE_LANGUAGES.iter().find(|l| l.extensions.contains(&ext))
}

/// `resolveDisplayName(filename)`: `None` → `"Plain Text"`; a known language
/// → its name; otherwise the basename with its first letter capitalised.
pub fn resolve_display_name(filename: Option<&str>) -> String {
    let Some(f) = filename else { return "Plain Text".into() };
    if let Some(l) = resolve_file_language(f) {
        return l.name.into();
    }
    let base = f.rsplit(['/', '\\']).next().unwrap_or(f);
    let mut c = base.chars();
    match c.next() {
        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_143_languages() {
        assert_eq!(LANGUAGES.len(), 143);
    }

    #[test]
    fn exact_aliases() {
        assert_eq!(match_language_name("js", true).unwrap().name, "JavaScript");
        assert_eq!(match_language_name("TS", true).unwrap().name, "TypeScript");
        assert_eq!(match_language_name("c++", true).unwrap().name, "C++");
        assert_eq!(match_language_name("cs", true).unwrap().name, "C#");
        assert_eq!(match_language_name("yml", true).unwrap().name, "YAML");
        assert_eq!(match_language_name("zsh", true).unwrap().name, "Shell");
        assert_eq!(match_language_name("rb", true).unwrap().name, "Ruby");
        assert_eq!(match_language_name("tex", true).unwrap().name, "LaTeX");
        assert_eq!(match_language_name("ini", true).unwrap().name, "Properties files");
        assert_eq!(match_language_name("objc", true).unwrap().name, "Objective-C");
    }

    #[test]
    fn fuzzy_quirks() {
        // "text" contains the alias "tex" (length 3) -> LaTeX.
        assert_eq!(match_language_name("text", true).unwrap().name, "LaTeX");
        // "javascript-x" contains "java" first (Java precedes JavaScript in
        // the list), exactly like the original.
        assert_eq!(match_language_name("javascript-x", true).unwrap().name, "Java");
        // Short aliases need non-word chars on *both* sides; a string edge
        // counts as a word char (JS `/\w/.test(undefined)`), so "c-lang"
        // does not match C ... but it does contain "lang"? no alias -> None.
        assert!(match_language_name("c-lang", true).is_none());
        assert_eq!(match_language_name("x.c.y", true).unwrap().name, "C");
        assert!(match_language_name("text", false).is_none());
        assert!(match_language_name("mermaid", true).is_none());
    }

    #[test]
    fn info_uses_first_word() {
        assert_eq!(language_for_info("rust ignore").unwrap().name, "Rust");
        assert!(language_for_info("").is_none());
        assert!(language_for_info(" rust").is_none());
    }

    #[test]
    fn every_toolbar_language_resolves_with_a_grammar() {
        for l in TOOLBAR_LANGUAGES {
            let d = match_language_name(l, true).unwrap_or_else(|| panic!("{l} unresolved"));
            assert!(d.grammar.is_some(), "{l} has no grammar");
        }
    }

    #[test]
    fn toolbar_inserts_unknown_current() {
        let o = toolbar_options("mermaid");
        assert_eq!(o[0], "");
        assert_eq!(o[1], "mermaid");
        assert_eq!(o[2], "typescript");
        assert_eq!(toolbar_options("rust").len(), 22);
        assert_eq!(toolbar_options("").len(), 22);
    }

    #[test]
    fn file_languages() {
        assert_eq!(resolve_file_language("/a/b/Main.RS").unwrap().name, "Rust");
        assert_eq!(resolve_file_language("x.mjs").unwrap().name, "JavaScript");
        assert!(resolve_file_language("Makefile").is_none());
        assert!(resolve_file_language("weird.").is_none());
        assert_eq!(resolve_display_name(None), "Plain Text");
        assert_eq!(resolve_display_name(Some("notes.md")), "Markdown");
        assert_eq!(resolve_display_name(Some("/x/makefile")), "Makefile");
    }
}
