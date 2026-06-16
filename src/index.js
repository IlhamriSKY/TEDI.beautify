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

import { safeToast } from "./dom.js";
import { runFormat } from "./format.js";
import { mountHeaderButton, syncHeaderButton, unmountHeaderButton } from "./header.js";
import {
  CMD_FORMAT,
  buttonShown,
  ctx,
  setCtx,
  setSidecar,
  setUnsubscribeContext,
  sidecar,
  unsubscribeContext,
} from "./runtime.js";
import { fetchJson } from "./sidecar.js";

export async function activate(context) {
  setCtx(context);

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
    setUnsubscribeContext(ctx.app.onContextChange((snapshot) => {
      syncHeaderButton(snapshot);
    }));
  } catch (err) {
    ctx?.logger?.warn?.("context subscribe failed; showing button unconditionally", err);
    mountHeaderButton();
  }
}

export async function deactivate() {
  try {
    if (typeof unsubscribeContext === "function") {
      try {
        unsubscribeContext();
      } catch {
        /* empty */
      }
      setUnsubscribeContext(null);
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
    setSidecar(null);
    setCtx(null);
  }
}
