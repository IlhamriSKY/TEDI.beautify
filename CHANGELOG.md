# Changelog

All notable changes to **Beautify**. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

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
