# Changelog

All notable changes to **Beautify**. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## [0.2.1] - 2026-09-02

### Fixed

- **The format shortcut is `Mod+Alt+F`, not `Mod+Alt+B`.** `Mod+Alt+B` is TEDI's own "toggle right panel", so the two were bound to the same chord and which one ran came down to the order the two capture-phase listeners happened to be registered in - stable until you rebound anything in Settings > Shortcuts, after which the extension started eating the panel toggle. TEDI now keeps a chord its own catalog claims and warns in the console instead, so the old binding would simply have stopped working. `F` for Format; the wand icon in the header is unchanged.

## [0.2.0] - 2026-08-25

### Added

- **Embedded code is formatted, not passed through.** A `<script>` or `<style>` block in HTML / Vue / Svelte / Astro, a Vue or Angular binding attribute, a fenced code block in Markdown, and a `` css`…` `` / `` html`…` `` / `` sql`…` `` / `` styled.div`…` `` tagged template in JS or TS all go back through the matching formatter, at whatever print width is left after their parent's indentation. Until now every one of those callbacks returned the original bytes, so a `.vue` file came back with its `<template>` tidied and its `<script setup>` exactly as it was. A block whose language has no formatter here, or that does not parse, is still left byte-for-byte alone, so a half-written `<script>` never costs you the rest of the document. Recursion is capped at four levels.
- **JSONC really works now, and so do the extension-less config files.** `.jsonc` and `.json5` map to a comment- and trailing-comma-preserving mode instead of being aliases for strict JSON, which `serde_json` rejected outright the moment a `//` appeared. `.babelrc`, `.swcrc`, `.jscsrc` and `.jshintrc` are matched by file name - the wand used to refuse to appear for any of them, because a leading dot reads as "no extension". `.prettierrc`, `.eslintrc` and `.stylelintrc` are deliberately left out: those tools accept JSON *or* YAML in the same extension-less file, so the wand would offer to format a YAML one and then fail.
- **More languages:** Jinja / Twig / Nunjucks (`.jinja`, `.jinja2`, `.j2`, `.twig`, `.njk`, `.nunjucks`), Vento (`.vto`), Mustache / Handlebars (`.mustache`, `.hbs`, `.handlebars`), PostCSS (`.pcss`, `.postcss`), and the rest of the XML family (`.xsl`, `.xslt`, `.xsd`, `.wsdl`, `.rss`, `.atom`, `.plist`).
- **CRLF and BOM survive a format.** The line ending and any UTF-8 BOM are detached before parsing and re-applied after printing. Formatting a CRLF file no longer rewrites every line and turns a two-line edit into a whole-file diff.
- **ARM builds.** `windows-aarch64` and `linux-aarch64` are built and shipped. The extension already resolved those paths, so on an ARM machine it failed with "reinstall the extension to repopulate sidecar/" - which reinstalling could not fix, because the binary had never been built.

### Fixed

- **JSON no longer re-sorts your keys or rewrites your numbers.** The old path parsed into a `serde_json::Value` and printed it back out. `serde_json`'s default map is a `BTreeMap`, so every object came back in alphabetical order - reformatting a `package.json` reordered the whole file. Numbers were stored as i64 / u64 / f64, so ids inside integer range survived but anything outside them did not: measured against the shipped 0.1.6, `123456789012345678901234567890` came back as `1.2345678901234568e+29`, `1.0000000000000001` as `1.0`, and `-99999999999999999999` as `-1e+20`. Both paths now go through `dprint-plugin-json`, which is document-ordered and keeps every number literal byte-for-byte.
- **XML no longer edits attribute values.** The hand-rolled indenter ended a tag at the first `>` it saw, which lands inside `<a title="x > y">` and inside `<!-- p > q -->`, and it then inserted a newline there. That is a change to the document, not to its whitespace. `markup_fmt` gained a real XML mode in 0.27, so the whole hand-rolled path is gone.
- **A crashed sidecar is no longer permanent.** The cached `baseUrl` outlived the process it pointed at, so once the helper died every later click failed with `TypeError: Failed to fetch` until the extension was toggled off and on. A transport-level failure now discards the handle and boots a fresh helper, once.
- **A parser panic no longer kills the sidecar.** `panic = "abort"` is off and formatting runs on a blocking thread inside `catch_unwind`, so a pathological file returns a 400 for that file and the helper stays up.
- **Files over 2 MB format.** axum's default body limit is 2 MB, which is smaller than the minified bundles a beautifier is most useful on; the ceiling is now 64 MB. Verified end to end against a 5.7 MB JSON document.
- **The `engines.tedi` note in the README said `>= 0.2.27`** while the manifest has required `>= 0.3.9` since 0.1.2.

- **`.mdx` no longer claims a formatter.** `dprint-plugin-markdown` has no MDX mode: it reads a JSX block as an HTML block and strips the indentation off its props. Diffed against a real MDX file, that de-indent was the *only* edit the formatter made - all cost, no benefit - and re-running does not put the indentation back. `.md` and `.markdown` are unaffected.

### Changed

- **Security.** The bearer token is compared in constant time, and the request body is read as bytes and parsed only *after* the token check, so an unauthenticated caller cannot make the helper parse a 64 MB payload.
- **The helper exits after 30 minutes idle.** If TEDI is killed rather than closed, `deactivate` never runs; on macOS and Linux nothing else would have reaped the process, and it would have sat on a loopback port indefinitely.
- **Dependencies.** axum 0.7 → 0.8, tower-http 0.6 → 0.7, tokio 1.40 → 1.53, malva 0.15 → 0.16, sqlformat 0.3 → 0.5, toml_edit 0.22 → 0.25; `rand` is replaced by a direct `getrandom` call, and `dprint-plugin-json` is added. The release binary is ~5.6 MB.
- **`scripts/verify.mjs`** - 9 assertions over `langForPath`, the function that decides whether the wand appears and which formatter runs, including a drift guard that every language id the map can produce has a matching arm in the sidecar. Runs in CI and as `npm run verify`.
- **CI compiles the sidecar on every push.** `release.yml` was the only workflow that ran `cargo`, so a `sidecar-src/` change that did not build stayed green until somebody pushed a tag - at which point the release, not the commit, was what broke. `build-check.yml` now runs `cargo fmt --check`, `clippy -D warnings` and `cargo test` (11 tests) alongside the bundle build.

## [0.1.6] - 2026-08-24

### Changed

- **Built against TEDI's published extension types.** TEDI 0.4.26 ships `tedi.d.ts`, a standalone typed contract for `ctx`, and a JSON Schema for `manifest.json`. Both now live in this repo, written by `tedi ext types`, alongside a `jsconfig.json` that turns type checking on for plain JavaScript. A misspelled `ctx.*` call is an editor error now rather than a `TypeError` raised inside an async handler, where it surfaces as an unhandled rejection nobody sees. `build.mjs` is the canonical copy shared across the TEDI extensions: it reads its entry point, output path and banner from `manifest.json`, so it holds nothing specific to this extension. The manifest gains a `$schema` line, which every parser ignores and which gives the file completion while it is edited. No behaviour changes; the bundle esbuild produces is byte-identical apart from its banner comment.

## [0.1.5] - 2026-07-18

### Changed

- **The header button uses a `lucide:` icon ref** (`lucide:WandSparkles`) instead of the legacy `hugeicon:MagicWand01Icon`. The host still resolves the old ref through its back-compat alias table, so this is the same glyph with no visual change, just the current icon API.
- **Documentation.** Project links point at the TEDI website (https://tedi.ilhamriski.com/) in both `manifest.json` and the README, the README follows the structure shared across the TEDI extensions, and "How it works" is rendered as a Mermaid diagram.

## [0.1.4] - 2026-06-16

### Changed

- **Internal refactor.** The single `src/index.js` is split into small, cohesive modules (each ≤ 300 lines), matching the project's module convention. No behaviour change — the built `extension.js` is functionally identical (verified: same string-literal set, same exports).

## [0.1.3] - 2026-06-16

### Changed

- **Build pipeline.** The extension is now authored as `src/index.js` and bundled into `extension.js` with esbuild (`npm run build`); the built bundle is **no longer committed** — CI (`release.yml`) builds it into the release `.zip` that users install. No behaviour change. CI actions bumped to `@v5` (Node 24).

## [0.1.2] - 2026-06-10

### Fixed

- **Beautify now actually formats the buffer instead of always failing with an error toast.** The sidecar serves `POST /format` over loopback HTTP, which WebView2 guards with a CORS preflight because the request carries `Content-Type: application/json` + `Authorization`. The CORS layer replied `Access-Control-Allow-Headers: *`, but per the Fetch spec the `*` wildcard does not authorize the `Authorization` header, so the preflight was rejected and every click surfaced `TypeError: Failed to fetch`. The sidecar now lists `authorization` and `content-type` explicitly ([`sidecar-src/src/main.rs`](sidecar-src/src/main.rs)).

### Changed

- **`engines.tedi` raised to `>=0.3.9`.** The host now enforces this constraint at install time, so older TEDI builds refuse to install the extension and surface a "needs TEDI X.Y.Z" message rather than letting it run against a host that predates the current API surface.

## [0.1.1] - 2026-05-26

### Changed

- **Header button is contextual.** The wand only mounts when an editor tab is focused on a file the sidecar can actually format (`langForPath` returns non-null). On terminal / SSH / diff / preview / extension tabs, or on editor tabs holding an unsupported extension, the button unmounts. Avoids the dead-click case where pressing the wand on a `.png` or terminal tab would just toast a warning. Subscribes via `ctx.app.onContextChange`; falls back to "always show" if the subscribe call throws.

## [0.1.0] - 2026-05-26

### Added

- **First release.** Zero-config beautify for the active editor buffer. Click the wand icon in the header (left of the markdown-preview toggle) or press `Mod+Alt+B`; the result lands in the editor as a dirty buffer, Ctrl+S to persist. Same language surface VSCode's built-in formatters + Prettier extension cover, packaged as a single offline extension.
- **Languages supported.**
  - JSON / JSONC / JSON5 via `serde_json` (2-space indent; JSONC / JSON5 comments only survive on strict-JSON input).
  - JavaScript / TypeScript / JSX / TSX via `dprint-plugin-typescript` (the Prettier-compatible printer dprint ships).
  - CSS / SCSS / LESS / Sass via `malva` (g-plane).
  - HTML / Vue / Svelte / Astro via `markup_fmt` (g-plane). Angular component templates supported in code; no canonical extension exists so open them as `.html` to pick up formatting.
  - Markdown via `dprint-plugin-markdown` (re-flows paragraphs, normalises list markers, fences code blocks consistently).
  - YAML via `pretty_yaml` (g-plane, preserves comments).
  - TOML via `toml_edit` (preserves comments).
  - SQL via `sqlformat` (generic dialect, 2-space, uppercase keywords).
  - XML / SVG via a depth-based reindenter (keeps attribute order intact, idempotent on already-pretty input).
- **Sidecar architecture.** `sidecar/<platform>-<arch>/tedi-beautify-helper` is a small Rust binary built per (target_os, target_arch) by the new release workflow, mirroring `tedi.screenshot` / `tedi.sql-explorer`. The sidecar spawns once per session (lazy on first click), binds `127.0.0.1` on an OS-assigned port, and authenticates every request with a per-boot 32-byte hex bearer token. TEDI core stays generic; uninstalling Beautify removes every formatter dep with it.
- **Manifest permissions.** `headerbar:write`, `ui:toast`, `editor:read`, `editor:write`, `invoke:shell_bg_spawn_direct`, `invoke:shell_bg_logs`, `invoke:shell_bg_kill`. No filesystem, network, or keychain permissions. The sidecar binds loopback only and rejects every request without the bearer token, so no other machine on the LAN can reach it.
- **Release CI** mirrors `tedi.sql-explorer`: matrix-builds the sidecar across `windows-latest` / `macos-latest` (x86_64 + aarch64) / `ubuntu-latest`, uploads each as an artifact, then a second job downloads all four, flattens the layout, zips the runtime tree, and uploads to the GitHub release. No platform-specific apt packages required -- every formatter crate compiles from pure Rust sources.

### Known limitations

- **JS / TS bundle size.** `dprint-plugin-typescript` pulls in `swc_ecma_parser`, which adds ~10-15 MB to the release binary. The trade-off is "matches Prettier output exactly" vs "smaller sidecar"; users who never edit JS / TS pay the size cost regardless. Future versions may gate the JS / TS modules behind a separate sidecar feature.
- **HTML embedded code blocks pass through.** `markup_fmt` calls back for `<script>` / `<style>` bodies; v0.1.0 returns the original code unchanged. A future release will route them back through the JS / TS / CSS helpers in the same process.
- **Markdown code-block bodies pass through** for the same reason.
- **JSONC / JSON5 comments are dropped** when re-emitting -- the `serde_json::Value` path has no representation for them. Use the TEDI core formatter's external-command setting and point it at `prettier --parser jsonc` if comment-preserving JSONC matters.
