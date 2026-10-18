//! Builds the bundled syntax set: two-face's extended (bat) set plus the
//! grammars in `grammars/` (ports of the CodeMirror legacy modes that have
//! no syntect grammar), linked together and dumped compressed into
//! `OUT_DIR/syntaxes.packdump`, which `highlight::syntax_set` loads lazily.
//!
//! A grammar that fails to load is skipped with a cargo warning; the
//! `languages` test then reports its language as missing from the set.

use std::path::PathBuf;

use syntect::parsing::SyntaxDefinition;

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("grammars");
    println!("cargo:rerun-if-changed=grammars");
    println!("cargo:rerun-if-changed=build.rs");
    let mut builder = two_face::syntax::extra_newlines().into_builder();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("grammars/ directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "sublime-syntax"))
        .collect();
    files.sort();
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        let src = std::fs::read_to_string(&path).expect("read grammar");
        let fallback = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string);
        match SyntaxDefinition::load_from_str(&src, true, fallback.as_deref()) {
            Ok(def) => builder.add(def),
            Err(e) => println!("cargo:warning=skipping {}: {e}", path.display()),
        }
    }
    let set = builder.build();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("syntaxes.packdump");
    syntect::dumps::dump_to_uncompressed_file(&set, &out).expect("dumping syntax set");
}
