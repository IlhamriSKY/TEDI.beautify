// Beautify — language detection module. Bundled into extension.js by build.mjs.
//
// File-path → formatter-language-id mapping plus the pretty labels surfaced in
// toasts. Pure helpers (no host calls), shared by the header (to decide when to
// show the wand) and the format flow (to pick the formatter + label).

/** File-extension to formatter language id. Lowercased on lookup. Keep in
 *  sync with the `match lang` arms in `sidecar-src/src/format.rs`. Covers
 *  the same language set Prettier ships in VSCode plus SQL, TOML, XML and the
 *  template dialects markup_fmt understands. */
export const LANG_BY_EXT = {
  // JSON family. `jsonc` is its own id, not an alias for `json`: it selects
  // the sidecar's comment- and trailing-comma-preserving mode. Before that
  // split, any .jsonc file with a comment in it simply failed to parse.
  json: "json",
  jsonc: "jsonc",
  json5: "jsonc",
  // CSS family via malva (one crate, four dialects).
  css: "css",
  pcss: "css",
  postcss: "css",
  scss: "scss",
  less: "less",
  sass: "sass",
  // HTML family via markup_fmt.
  html: "html",
  htm: "html",
  xhtml: "html",
  vue: "vue",
  svelte: "svelte",
  astro: "astro",
  // Template dialects, same crate.
  jinja: "jinja",
  jinja2: "jinja",
  j2: "jinja",
  twig: "jinja",
  njk: "jinja",
  nunjucks: "jinja",
  vto: "vento",
  mustache: "mustache",
  hbs: "mustache",
  handlebars: "mustache",
  // XML family, also markup_fmt (it gained a real XML mode in 0.27; this used
  // to be a hand-rolled indenter that split tags at the first `>`).
  xml: "xml",
  svg: "xml",
  xsl: "xml",
  xslt: "xml",
  xsd: "xml",
  wsdl: "xml",
  rss: "xml",
  atom: "xml",
  plist: "xml",
  // JS / TS / JSX / TSX via dprint-plugin-typescript.
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "jsx",
  ts: "typescript",
  cts: "typescript",
  mts: "typescript",
  tsx: "tsx",
  // Markdown via dprint-plugin-markdown. `.mdx` is NOT here: dprint has no MDX
  // mode, so it reads a JSX block as an HTML block and can strip the
  // indentation off its props. On a realistic MDX file that de-indent is the
  // only edit it makes - all cost, no benefit, and re-running does not undo it.
  md: "markdown",
  markdown: "markdown",
  // Data formats.
  yaml: "yaml",
  yml: "yaml",
  toml: "toml",
  sql: "sql",
};

/** Extension-less config files, matched on the whole file name. `extOf` reads
 *  `.babelrc` as "no extension", so without this table the tool-config files
 *  people most often want tidied are the ones the wand refuses to appear for.
 *  All of these accept comments in practice, hence `jsonc`.
 *
 *  Only names whose tool documents a JSON-family format are listed.
 *  `.prettierrc`, `.eslintrc` and `.stylelintrc` are deliberately absent: their
 *  tools accept JSON *or* YAML in the same extension-less file, and nothing in
 *  the name says which, so the wand would offer to format a YAML one and then
 *  fail on the first `:`. No formatter beats the wrong formatter. */
export const LANG_BY_NAME = {
  ".babelrc": "jsonc",
  ".swcrc": "jsonc",
  ".jscsrc": "jsonc",
  ".jshintrc": "jsonc",
};

/** Pretty labels surfaced in toasts. */
export const LANG_LABELS = {
  json: "JSON",
  jsonc: "JSONC",
  css: "CSS",
  scss: "SCSS",
  less: "LESS",
  sass: "Sass",
  html: "HTML",
  vue: "Vue",
  svelte: "Svelte",
  astro: "Astro",
  jinja: "Jinja",
  vento: "Vento",
  mustache: "Mustache",
  xml: "XML",
  javascript: "JavaScript",
  jsx: "JSX",
  typescript: "TypeScript",
  tsx: "TSX",
  markdown: "Markdown",
  yaml: "YAML",
  toml: "TOML",
  sql: "SQL",
};

function extOf(filePath) {
  if (!filePath) return null;
  const base = filePath.split(/[\\/]/).pop() ?? "";
  const i = base.lastIndexOf(".");
  if (i <= 0 || i === base.length - 1) return null;
  return base.slice(i + 1).toLowerCase();
}

export function langForPath(filePath) {
  const byName = LANG_BY_NAME[baseName(filePath).toLowerCase()];
  if (byName) return byName;
  const ext = extOf(filePath);
  if (!ext) return null;
  return LANG_BY_EXT[ext] ?? null;
}

export function baseName(p) {
  if (!p) return "";
  return p.split(/[\\/]/).pop() ?? p;
}
