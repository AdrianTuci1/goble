import { useEffect } from "react";

import { palette } from "@goble/common";

/**
 * The browser's single accessibility stylesheet.
 *
 * `:focus-visible` is the one rule every control needs but none should have to
 * repeat, so the shell injects it once instead of teaching each Pressable about
 * focus state. The reduced-motion block covers animated styles and CSS
 * transitions in one place; the app does not opt into motion the platform does
 * not want.
 */
const STYLE_ID = "goble-a11y-styles";

const STYLES = `
:focus-visible {
  outline: 2px solid ${palette.accent};
  outline-offset: 2px;
}

@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
    scroll-behavior: auto !important;
  }
}
`;

let consumers = 0;
let styleElement: HTMLStyleElement | null = null;

/** Add the shell stylesheet if it is not already in the document. */
function install(): void {
  if (typeof document === "undefined") return;
  if (styleElement?.isConnected) return;

  const existing = document.getElementById(STYLE_ID);
  if (existing instanceof HTMLStyleElement) {
    styleElement = existing;
    return;
  }

  const style = document.createElement("style");
  style.id = STYLE_ID;
  style.textContent = STYLES;
  document.head.appendChild(style);
  styleElement = style;
}

/** Mount the focus-ring and reduced-motion rules for as long as the shell is up. */
export function useFocusVisible(): void {
  useEffect(() => {
    if (typeof document === "undefined") return;

    consumers += 1;
    install();

    return () => {
      consumers -= 1;
      if (consumers > 0) return;
      styleElement?.remove();
      styleElement = null;
    };
  }, []);
}
