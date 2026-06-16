// Beautify — header-bar button module. Bundled into extension.js by build.mjs.
//
// Mounts / unmounts the wand button in the file-view-mode cluster, tracking the
// focused tab so it only appears when there's a formattable editor buffer.
import { langForPath } from "./lang.js";
import { runFormat } from "./format.js";
import { BUTTON_ID, buttonShown, ctx, setButtonShown } from "./runtime.js";

export function syncHeaderButton(snapshot) {
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
export function mountHeaderButton() {
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
    setButtonShown(true);
  } catch (err) {
    ctx?.logger?.warn?.("headerBar.setItem failed", err);
  }
}

export function unmountHeaderButton() {
  try {
    ctx?.headerBar?.removeItem?.(BUTTON_ID);
  } catch (err) {
    ctx?.logger?.warn?.("headerBar.removeItem failed", err);
  }
  setButtonShown(false);
}
