// Beautify — language detection module. Bundled into extension.js by build.mjs.
//
// File-path → formatter-language-id mapping plus the pretty labels surfaced in
// toasts. Pure helpers (no host calls), shared by the header (to decide when to
// show the wand) and the format flow (to pick the formatter + label).

/** File-extension to formatter language id. Lowercased on lookup. Keep in
 *  sync with the `match lang` arms in `sidecar-src/src/main.rs`. Covers
 *  the same language set Prettier ships in VSCode plus SQL and TOML. */
export const LANG_BY_EXT = {
  // JSON family. JSONC / JSON5 reuse the JSON path; comments and trailing
  // commas only survive when the input happens to be strict JSON.
  json: "json",
  jsonc: "json",
  json5: "json",
  // CSS family via malva (one crate, four dialects).
  css: "css",
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
  // XML / SVG via the custom depth-based indenter (markup_fmt is too
  // HTML-flavoured and would reorder attributes / rewrite void tags).
  xml: "xml",
  svg: "xml",
  // JS / TS / JSX / TSX via dprint-plugin-typescript.
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "jsx",
  ts: "typescript",
  cts: "typescript",
  mts: "typescript",
  tsx: "tsx",
  // Markdown via dprint-plugin-markdown.
  md: "markdown",
  markdown: "markdown",
  mdx: "markdown",
  // Data formats.
  yaml: "yaml",
  yml: "yaml",
  toml: "toml",
  sql: "sql",
};

/** Pretty labels surfaced in toasts. */
export const LANG_LABELS = {
  json: "JSON",
  css: "CSS",
  scss: "SCSS",
  less: "LESS",
  sass: "Sass",
  html: "HTML",
  vue: "Vue",
  svelte: "Svelte",
  astro: "Astro",
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
  const ext = extOf(filePath);
  if (!ext) return null;
  return LANG_BY_EXT[ext] ?? null;
}

export function baseName(p) {
  if (!p) return "";
  return p.split(/[\\/]/).pop() ?? p;
}
