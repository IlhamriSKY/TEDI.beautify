// Beautify — format flow module. Bundled into extension.js by build.mjs.
//
// The Beautify-click handler: read the active editor buffer, POST it to the
// sidecar `/format` route, and replace the buffer with the result (user Ctrl+S
// to persist). Guarded by the shared `busy` flag so rapid clicks don't overlap.
import { safeToast } from "./dom.js";
import { LANG_LABELS, baseName, langForPath } from "./lang.js";
import { busy, ctx, setBusy } from "./runtime.js";
import { fetchJson } from "./sidecar.js";

// --------------------------------- Format ------------------------------------

export async function runFormat() {
  if (busy) return;
  if (!ctx) return;
  setBusy(true);
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
    setBusy(false);
  }
}
