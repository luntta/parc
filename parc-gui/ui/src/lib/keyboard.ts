type ShortcutHandler = (e: KeyboardEvent) => void;

interface Shortcut {
  key: string;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  handler: ShortcutHandler;
  description: string;
  context?: string;
}

const shortcuts: Shortcut[] = [];

export function registerShortcut(shortcut: Shortcut): () => void {
  shortcuts.push(shortcut);
  return () => {
    const idx = shortcuts.indexOf(shortcut);
    if (idx >= 0) shortcuts.splice(idx, 1);
  };
}

export function getShortcuts(): Shortcut[] {
  return [...shortcuts];
}

export function initKeyboard(): void {
  document.addEventListener("keydown", (e: KeyboardEvent) => {
    // Don't intercept when typing in inputs
    const target = e.target as HTMLElement;
    if (
      target.tagName === "INPUT" ||
      target.tagName === "TEXTAREA" ||
      target.isContentEditable
    ) {
      // Only allow Escape and Ctrl shortcuts in inputs
      if (e.key !== "Escape" && !e.ctrlKey && !e.metaKey) return;
    }

    for (const s of shortcuts) {
      const ctrlMatch = (s.ctrl ?? false) === (e.ctrlKey || e.metaKey);
      const shiftMatch = (s.shift ?? false) === e.shiftKey;
      const altMatch = (s.alt ?? false) === e.altKey;
      if (e.key === s.key && ctrlMatch && shiftMatch && altMatch) {
        e.preventDefault();
        s.handler(e);
        return;
      }
    }
  });
}
