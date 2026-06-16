// Beautify — host UI helpers. Bundled into extension.js by build.mjs.
import { ctx } from "./runtime.js";

export function safeToast(message, variant) {
  try {
    ctx?.ui?.toast?.(message, { variant });
  } catch {
    ctx?.logger?.info?.(message);
  }
}
