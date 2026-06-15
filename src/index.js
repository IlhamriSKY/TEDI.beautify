// Beautify - format the active editor buffer via a native sidecar.
//
// Architecture mirrors tedi.sql-explorer:
//   `sidecar/<platform>-<arch>/tedi-beautify-helper` is a small Rust binary
//   that links serde_json / lightningcss / sqlformat / etc. behind an
//   axum HTTP server on `127.0.0.1:<random-port>` with a per-boot bearer
//   token. The extension JS layer:
//
//     1. picks the helper for the current OS / arch (`ctx.os`)
//     2. spawns it via `shell_bg_spawn_direct` (no shell wrapper, so the
//        tracked PID is the helper itself)
//     3. polls its stdout via `shell_bg_logs` until the `READY {port,token}`
//        line lands (same handshake the SQL Explorer extension uses)
//     4. on each Beautify click, POSTs `{lang, content}` to `/format` over
//        the loopback HTTP channel with the bearer token, reads the
//        formatted text back, and replaces the editor buffer via
//        `ctx.editor.setActiveContent` (user Ctrl+S to persist)
//     5. on `deactivate`, best-effort `POST /shutdown` then `shell_bg_kill`
//
// Why HTTP-over-loopback instead of stdin: `shell_bg_spawn_direct` ties
// stdin to `Stdio::null()` so there's no way to pipe the buffer in. The
// alternative (write to a temp file + pass --input <path>) requires
// `invoke:fs_write_file`, which is flagged HIGH risk in the install
// dialog. Loopback HTTP with a per-boot 32-byte bearer token keeps the
// extension's permission surface modest (just shell_bg_*) and reuses the
// same pattern the SQL Explorer extension already audited.
//
// Why sidecar (instead of TEDI-core Tauri commands): formatter dependencies
// (lightningcss, serde_json, sqlformat, ...) stay inside this extension's
// release artifact. The TEDI core binary stays generic; uninstalling
// Beautify removes every native dep with it.

const EXT_ID = "tedi.beautify";
const CMD_FORMAT = "tedi.beautify.format";
const BUTTON_ID = "format";

// Tight enough that the READY line lands "instantly" from a user POV,
// loose enough that a slow first-time process spawn (Defender / Gatekeeper
// scanning the unsigned helper) does not trip the timeout.
const READY_TIMEOUT_MS = 12_000;
const READY_POLL_MS = 80;

/** File-extension to formatter language id. Lowercased on lookup. Keep in
 *  sync with the `match lang` arms in `sidecar-src/src/main.rs`. Covers
 *  the same language set Prettier ships in VSCode plus SQL and TOML. */
const LANG_BY_EXT = {
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
const LANG_LABELS = {
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

let ctx = null;
let sidecar = null; // { handle, port, token, baseUrl }
let bootInFlight = null;
let busy = false;
/** Mirrors whether the header item is currently mounted, so we don't
 *  spam setItem / removeItem on every context change. */
let buttonShown = false;
/** Disposer returned by `ctx.app.onContextChange`. */
let unsubscribeContext = null;

function platformDir(os) {
  const arch = os?.arch || "x86_64";
  if (os?.platform === "windows") return arch === "aarch64" ? "windows-aarch64" : "windows-x86_64";
  if (os?.platform === "macos") return arch === "aarch64" ? "macos-aarch64" : "macos-x86_64";
  if (os?.platform === "linux") return arch === "aarch64" ? "linux-aarch64" : "linux-x86_64";
  return null;
}

function helperPath(installPath, os) {
  if (typeof installPath !== "string" || !installPath) return null;
  if (!os || typeof os.platform !== "string") return null;
  const dir = platformDir(os);
  if (!dir) return null;
  const exe = os.platform === "windows" ? "tedi-beautify-helper.exe" : "tedi-beautify-helper";
  return `${installPath.replace(/\\/g, "/")}/sidecar/${dir}/${exe}`;
}

function extOf(filePath) {
  if (!filePath) return null;
  const base = filePath.split(/[\\/]/).pop() ?? "";
  const i = base.lastIndexOf(".");
  if (i <= 0 || i === base.length - 1) return null;
  return base.slice(i + 1).toLowerCase();
}

function langForPath(filePath) {
  const ext = extOf(filePath);
  if (!ext) return null;
  return LANG_BY_EXT[ext] ?? null;
}

export async function activate(context) {
  ctx = context;

  // Probe host APIs up front. Missing anything -> we're on an older TEDI
  // than the manifest engines constraint; surface one warning toast and
  // stay activated-but-idle so disable/uninstall still tears down cleanly.
  const missing = [];
  if (typeof ctx.invoke !== "function") missing.push("ctx.invoke");
  if (typeof ctx.os?.platform !== "string") missing.push("ctx.os.platform");
  if (typeof ctx.installPath !== "string") missing.push("ctx.installPath");
  if (typeof ctx.headerBar?.setItem !== "function") missing.push("ctx.headerBar");
  if (typeof ctx.app?.onContextChange !== "function") missing.push("ctx.app.onContextChange");
  if (typeof ctx.editor?.getActive !== "function") missing.push("ctx.editor.getActive");
  if (typeof ctx.editor?.setActiveContent !== "function") missing.push("ctx.editor.setActiveContent");
  if (missing.length > 0) {
    const msg = `Beautify needs a newer TEDI (missing: ${missing.join(", ")}).`;
    ctx?.logger?.warn?.(msg);
    safeToast(msg, "warning");
    return;
  }

  ctx.registerCommandHandler(CMD_FORMAT, () => {
    runFormat().catch((err) => ctx?.logger?.error?.("format failed", err));
  });

  // Mount / unmount the header button based on the focused tab so the
  // wand only appears when there's something to format. Initial sync via
  // `getContext()` covers the case where TEDI already had an editor tab
  // focused at extension boot.
  try {
    syncHeaderButton(ctx.app.getContext());
    unsubscribeContext = ctx.app.onContextChange((snapshot) => {
      syncHeaderButton(snapshot);
    });
  } catch (err) {
    ctx?.logger?.warn?.("context subscribe failed; showing button unconditionally", err);
    mountHeaderButton();
  }
}

function syncHeaderButton(snapshot) {
  const shouldShow =
    snapshot?.activeTabKind === "editor" &&
    langForPath(snapshot.activeFileName) !== null;
  if (shouldShow && !buttonShown) {
    mountHeaderButton();
  } else if (!shouldShow && buttonShown) {
    unmountHeaderButton();
  }
}

// Header button. `placement: "left"` lands it in the file-view-mode
// cluster (immediately before the markdown-preview toggle) so the
// wand groups with the other "render this file as X" toggles.
function mountHeaderButton() {
  try {
    ctx?.headerBar?.setItem?.({
      id: BUTTON_ID,
      placement: "left",
      icon: "hugeicon:MagicWand01Icon",
      tooltip: "Beautify (Ctrl+Alt+B)",
      onClick: () => {
        void runFormat();
      },
    });
    buttonShown = true;
  } catch (err) {
    ctx?.logger?.warn?.("headerBar.setItem failed", err);
  }
}

function unmountHeaderButton() {
  try {
    ctx?.headerBar?.removeItem?.(BUTTON_ID);
  } catch (err) {
    ctx?.logger?.warn?.("headerBar.removeItem failed", err);
  }
  buttonShown = false;
}

export async function deactivate() {
  try {
    if (typeof unsubscribeContext === "function") {
      try {
        unsubscribeContext();
      } catch {
        /* empty */
      }
      unsubscribeContext = null;
    }
    if (buttonShown) {
      unmountHeaderButton();
    }
    if (sidecar?.baseUrl) {
      // Best effort - the helper exits as soon as the route runs.
      await fetchJson("/shutdown", { method: "POST", body: {} }).catch(() => {});
    }
    if (sidecar?.handle != null) {
      await ctx.invoke("shell_bg_kill", { handle: sidecar.handle }).catch(() => {});
    }
  } finally {
    sidecar = null;
    ctx = null;
  }
}

// --------------------------------- Sidecar -----------------------------------

async function ensureSidecar() {
  if (sidecar?.baseUrl) return sidecar;
  if (bootInFlight) return bootInFlight;
  bootInFlight = bootSidecar().finally(() => {
    bootInFlight = null;
  });
  return bootInFlight;
}

async function bootSidecar() {
  const program = helperPath(ctx.installPath, ctx.os);
  if (!program) {
    throw new Error(`unsupported platform ${ctx.os?.platform}/${ctx.os?.arch}`);
  }
  let handle;
  try {
    handle = await ctx.invoke("shell_bg_spawn_direct", { program, args: [] });
  } catch (err) {
    const msg = err?.message ?? String(err);
    if (/os error 2|no such file|cannot find/i.test(msg)) {
      throw new Error(
        `Sidecar binary missing for ${ctx.os?.platform}-${ctx.os?.arch}. Reinstall the extension to repopulate sidecar/. (${msg})`,
      );
    }
    throw new Error(`spawn failed: ${msg}`);
  }

  // Drain stdout until READY lands. `shell_bg_logs` returns new bytes since
  // the last sinceOffset; the helper prints `READY <json>` on its first
  // line, then idles waiting for HTTP requests.
  const deadline = Date.now() + READY_TIMEOUT_MS;
  let offset = 0;
  let buf = "";
  while (true) {
    if (Date.now() > deadline) {
      await ctx.invoke("shell_bg_kill", { handle }).catch(() => {});
      throw new Error("sidecar handshake timed out");
    }
    const resp = await ctx.invoke("shell_bg_logs", { handle, sinceOffset: offset });
    if (resp?.bytes) buf += resp.bytes;
    offset = typeof resp?.next_offset === "number" ? resp.next_offset : offset;
    if (resp?.exited) {
      throw new Error(`sidecar exited before READY (exit ${resp.exit_code ?? "?"})`);
    }
    const line = extractReady(buf);
    if (line) {
      sidecar = {
        handle,
        port: line.port,
        token: line.token,
        baseUrl: `http://127.0.0.1:${line.port}`,
      };
      ctx?.logger?.info?.(`beautify sidecar ready on ${sidecar.baseUrl}`);
      return sidecar;
    }
    await sleep(READY_POLL_MS);
  }
}

function extractReady(buf) {
  // Strict prefix match. Anything before the keyword is throwaway stderr.
  const idx = buf.indexOf("READY ");
  if (idx < 0) return null;
  const nl = buf.indexOf("\n", idx);
  if (nl < 0) return null;
  const jsonText = buf.slice(idx + "READY ".length, nl).trim();
  try {
    return JSON.parse(jsonText);
  } catch {
    return null;
  }
}

async function fetchJson(path, opts = {}) {
  if (!sidecar?.baseUrl) await ensureSidecar();
  const url = `${sidecar.baseUrl}${path}`;
  const headers = {
    "Content-Type": "application/json",
    Authorization: `Bearer ${sidecar.token}`,
  };
  const init = {
    method: opts.method ?? "GET",
    headers,
  };
  if (opts.body !== undefined) init.body = JSON.stringify(opts.body);
  const res = await fetch(url, init);
  let text = "";
  try {
    text = await res.text();
  } catch {
    /* empty */
  }
  let json = null;
  try {
    json = text ? JSON.parse(text) : null;
  } catch {
    /* empty */
  }
  if (!res.ok) {
    const msg = json?.error?.message ?? text ?? `HTTP ${res.status}`;
    throw new Error(msg);
  }
  return json;
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

// --------------------------------- Format ------------------------------------

async function runFormat() {
  if (busy) return;
  if (!ctx) return;
  busy = true;
  try {
    const snapshot = ctx.editor.getActive();
    if (!snapshot) {
      safeToast("Beautify: focus an editor tab first.", "warning");
      return;
    }
    const lang = langForPath(snapshot.path);
    if (!lang) {
      safeToast(
        `Beautify: no formatter for "${baseName(snapshot.path)}" yet.`,
        "warning",
      );
      return;
    }
    if (!snapshot.content) {
      safeToast(`Beautify: file is empty.`, "info");
      return;
    }

    let result;
    try {
      result = await fetchJson("/format", {
        method: "POST",
        body: { lang, content: snapshot.content },
      });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      safeToast(`Beautify failed: ${msg}`, "error");
      return;
    }
    const formatted = typeof result?.content === "string" ? result.content : null;
    if (formatted === null) {
      safeToast("Beautify: helper returned no payload.", "error");
      return;
    }
    if (formatted === snapshot.content) {
      safeToast(`Already formatted (${LANG_LABELS[lang]}).`, "info");
      return;
    }
    const ok = ctx.editor.setActiveContent(formatted);
    if (!ok) {
      safeToast("Beautify: active editor went away mid-format.", "warning");
      return;
    }
    safeToast(`Formatted (${LANG_LABELS[lang]}). Press Ctrl+S to save.`, "success");
  } finally {
    busy = false;
  }
}

function baseName(p) {
  if (!p) return "";
  return p.split(/[\\/]/).pop() ?? p;
}

function safeToast(message, variant) {
  try {
    ctx?.ui?.toast?.(message, { variant });
  } catch {
    ctx?.logger?.info?.(message);
  }
}
