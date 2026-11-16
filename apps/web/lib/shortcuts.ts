import { useEffect, useRef } from "react";

/** What a key press can do, and the shell state it may act on. */
export interface ShellShortcuts {
  canShell: boolean;
  shellOpen: boolean;
  conversationOpen: boolean;
  subagentOpen: boolean;
  onShowSessions: () => void;
  onOpenShell: () => void;
  onNewShell: () => void;
  onFocusComposer: () => void;
  onClearConversation: () => void;
  onCloseSubagent: () => void;
}

/** True while a field owns the keyboard, where Escape belongs to the field. */
function typing(): boolean {
  const element = document.activeElement;
  if (!element) return false;
  const tag = element.tagName;
  return (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT" ||
    (element as HTMLElement).isContentEditable
  );
}

/**
 * The desktop shell's keyboard map.
 *
 * `mod` is Command on macOS and Control elsewhere. A combination carrying Shift
 * or Alt is not ours (`mod+shift+1` stays the browser's), and a held key fires
 * once. The listener is installed once; a ref carries the latest callbacks so a
 * re-render does not rebind it. Callbacks only run when the action can apply,
 * so `mod+2` without a worker and `mod+k` without a conversation do nothing.
 * A read-only subagent transcript is not a conversation for `mod+k`: it has no
 * composer, so the shell must not focus one that was never drawn.
 */
export function useShellShortcuts(shortcuts: ShellShortcuts): void {
  const latest = useRef(shortcuts);
  latest.current = shortcuts;

  useEffect(() => {
    if (typeof window === "undefined") return;

    function onKeyDown(event: KeyboardEvent): void {
      if (event.defaultPrevented || event.repeat) return;
      const current = latest.current;
      const mod = event.metaKey || event.ctrlKey;
      const plain = !event.shiftKey && !event.altKey;

      if (mod && plain) {
        switch (event.key) {
          case "1":
            current.onShowSessions();
            event.preventDefault();
            return;
          case "2":
            if (!current.canShell) return;
            current.onOpenShell();
            event.preventDefault();
            return;
          case "n":
          case "N":
            if (!current.canShell) return;
            current.onNewShell();
            event.preventDefault();
            return;
          case "k":
          case "K":
            if (!current.conversationOpen || current.subagentOpen) return;
            current.onFocusComposer();
            event.preventDefault();
            return;
        }
      }

      if (
        event.key !== "Escape" ||
        event.metaKey ||
        event.ctrlKey ||
        event.altKey ||
        event.shiftKey ||
        typing()
      ) {
        return;
      }

      if (current.shellOpen) {
        current.onShowSessions();
        event.preventDefault();
      } else if (current.subagentOpen) {
        current.onCloseSubagent();
        event.preventDefault();
      } else if (current.conversationOpen) {
        current.onClearConversation();
        event.preventDefault();
      }
    }

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
