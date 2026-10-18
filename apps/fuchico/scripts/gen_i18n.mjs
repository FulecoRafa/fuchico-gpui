import fs from "node:fs";
const dir = "/Users/fuleco/Documents/Dev/obsidian-helix/src/lib/i18n/";
function load(file, name) {
  let src = fs.readFileSync(dir + file, "utf8");
  src = src.replace(/^import .*$/gm, "").replace(/export type .*$/gm, "")
    .replace(/: Record<MessageKey, string>/, "").replace(/ as const;/, ";")
    .replace(/export const /, "const ");
  return new Function(src + `\nreturn ${name};`)();
}
const en = load("en.ts", "en"), pt = load("ptBR.ts", "ptBR");
const keys = Object.keys(en);
const variant = k => k.split(/[.]/).map(p => p[0].toUpperCase() + p.slice(1)).join("");
const seen = new Set();
for (const k of keys) { const v = variant(k); if (seen.has(v)) throw new Error("dup " + v); seen.add(v); }
const lit = s => JSON.stringify(s); // JSON string escapes are valid Rust string escapes for our content
let out = `//! GENERATED from obsidian-helix \`src/lib/i18n/{en,ptBR}.ts\` by
//! \`scripts/gen_i18n.mjs\` — do not edit by hand; edit the generator input.
//!
//! Compile-time completeness: every language is an exhaustive \`match\` over
//! [\`Msg\`] with no wildcard arm, so adding a key without translating it in
//! every catalog is a compile error.

/// Every UI message key (\`{key}\` in the TS catalogs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Msg {
${keys.map(k => `    /// \`${k}\`\n    ${variant(k)},`).join("\n")}
}

impl Msg {
    /// All keys, in catalog order.
    pub const ALL: &'static [Msg] = &[
${keys.map(k => `        Msg::${variant(k)},`).join("\n")}
    ];

    /// The original dotted key, e.g. \`"common.loading"\`.
    pub fn key(self) -> &'static str {
        match self {
${keys.map(k => `            Msg::${variant(k)} => ${lit(k)},`).join("\n")}
        }
    }
}

/// English (source of truth).
pub fn en(m: Msg) -> &'static str {
    match m {
${keys.map(k => `        Msg::${variant(k)} => ${lit(en[k])},`).join("\n")}
    }
}

/// Brazilian Portuguese.
pub fn pt_br(m: Msg) -> &'static str {
    match m {
${keys.map(k => { if (!(k in pt)) throw new Error("missing pt " + k); return `        Msg::${variant(k)} => ${lit(pt[k])},`; }).join("\n")}
    }
}
`;
fs.writeFileSync(process.argv[2], out);
console.log(keys.length, "keys");
