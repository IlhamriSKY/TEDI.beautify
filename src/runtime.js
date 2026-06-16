// Beautify — runtime module. Bundled into extension.js by build.mjs.
// Shared mutable singletons + app constants. Other modules import the live
// bindings for reads and call the setters here for writes (esbuild preserves
// ESM live-binding semantics across the bundle).

export const CMD_FORMAT = "tedi.beautify.format";
export const BUTTON_ID = "format";

// Tight enough that the READY line lands "instantly" from a user POV,
// loose enough that a slow first-time process spawn (Defender / Gatekeeper
// scanning the unsigned helper) does not trip the timeout.
export const READY_TIMEOUT_MS = 12_000;
export const READY_POLL_MS = 80;

// ----------------------------- Module state ----------------------------------

export let ctx = null;
export let sidecar = null; // { handle, port, token, baseUrl }
export let bootInFlight = null;
export let busy = false;
/** Mirrors whether the header item is currently mounted, so we don't
 *  spam setItem / removeItem on every context change. */
export let buttonShown = false;
/** Disposer returned by `ctx.app.onContextChange`. */
export let unsubscribeContext = null;

export function setCtx(value) {
  ctx = value;
}

export function setSidecar(value) {
  sidecar = value;
}

export function setBootInFlight(value) {
  bootInFlight = value;
}

export function setBusy(value) {
  busy = value;
}

export function setButtonShown(value) {
  buttonShown = value;
}

export function setUnsubscribeContext(value) {
  unsubscribeContext = value;
}

export function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}
