#!/usr/bin/env node
/**
 * `langForPath` decides whether the wand appears and which formatter runs, and
 * it is the one piece of extension JS with real branching. It is pure, so a
 * handful of asserts covers it without a running host.
 *
 * The sidecar's own formatting is covered by `cargo test` in `sidecar-src/`.
 *
 *   node scripts/verify.mjs
 */
import assert from "node:assert/strict";
import { LANG_BY_EXT, LANG_BY_NAME, LANG_LABELS, baseName, langForPath } from "../src/lang.js";

let ran = 0;
const test = (name, fn) => {
  fn();
  ran += 1;
  console.log(`  ok  ${name}`);
};

test("every mapped language has a toast label", () => {
  // A missing label renders "Formatted (undefined)".
  for (const lang of new Set([...Object.values(LANG_BY_EXT), ...Object.values(LANG_BY_NAME)])) {
    assert.ok(LANG_LABELS[lang], `no LANG_LABELS entry for ${lang}`);
  }
});

test("resolves both path separators and a bare file name", () => {
  assert.equal(langForPath("C:\\src\\a.ts"), "typescript");
  assert.equal(langForPath("/home/u/a.ts"), "typescript");
  assert.equal(langForPath("a.ts"), "typescript");
  assert.equal(baseName("C:\\src\\a.ts"), "a.ts");
  assert.equal(baseName("/home/u/a.ts"), "a.ts");
});

test("extension matching is case-insensitive", () => {
  assert.equal(langForPath("A.TS"), "typescript");
  assert.equal(langForPath("Component.VUE"), "vue");
});

test("the JSON family splits strict from comment-tolerant", () => {
  assert.equal(langForPath("tsconfig.json"), "json");
  assert.equal(langForPath("a.jsonc"), "jsonc");
  assert.equal(langForPath("a.json5"), "jsonc");
  // Dotfiles: `.babelrc` has no extension as far as lastIndexOf('.') is
  // concerned, so it has to match on the whole name.
  assert.equal(langForPath(".babelrc"), "jsonc");
  assert.equal(langForPath("/repo/.swcrc"), "jsonc");
  assert.equal(langForPath("C:\\repo\\.BABELRC"), "jsonc");
});

test("JSON-or-YAML dotfiles get no formatter rather than the wrong one", () => {
  // Prettier, ESLint and stylelint all accept either format in these
  // extension-less files, and the name does not say which.
  for (const f of [".prettierrc", ".eslintrc", ".stylelintrc"]) {
    assert.equal(langForPath(f), null, f);
  }
  // The same tools' *extension-carrying* config files are unambiguous.
  assert.equal(langForPath(".prettierrc.json"), "json");
  assert.equal(langForPath(".prettierrc.yaml"), "yaml");
});

test("MDX gets no formatter - the markdown one only damages it", () => {
  // dprint-plugin-markdown has no MDX mode: it reads a JSX block as an HTML
  // block and strips the indentation off its props. Measured on a real MDX
  // file, that de-indent was the ONLY edit it made.
  assert.equal(langForPath("docs/page.mdx"), null);
  assert.equal(langForPath("page.md"), "markdown");
  assert.equal(langForPath("page.markdown"), "markdown");
});

test("unformattable files return null so the wand stays hidden", () => {
  for (const f of ["a.png", "a.exe", "Makefile", "a.", ".", "", "a.rs", "a.go", "a.py"]) {
    assert.equal(langForPath(f), null, JSON.stringify(f));
  }
  assert.equal(langForPath(null), null);
  assert.equal(langForPath(undefined), null);
});

test("a dotted path does not swallow the real extension", () => {
  assert.equal(langForPath("/a.b.c/my.file.ts"), "typescript");
  // A directory with a dot and an extension-less file is not a .ts file.
  assert.equal(langForPath("/a.ts/README"), null);
});

test("every language id the map produces exists in the sidecar", () => {
  // Mirrors the `match lang` arms in sidecar-src/src/format.rs. A typo on
  // either side is a runtime "unsupported language" toast, not a build error.
  const SIDECAR = new Set([
    "json", "jsonc", "css", "scss", "less", "sass", "html", "vue", "svelte",
    "astro", "angular", "jinja", "vento", "mustache", "xml", "javascript",
    "jsx", "typescript", "tsx", "markdown", "yaml", "toml", "sql",
  ]);
  for (const lang of [...Object.values(LANG_BY_EXT), ...Object.values(LANG_BY_NAME)]) {
    assert.ok(SIDECAR.has(lang), `sidecar has no arm for "${lang}"`);
  }
});

console.log(`\n${ran} checks passed`);
