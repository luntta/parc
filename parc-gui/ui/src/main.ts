import { initTheme } from "./lib/theme.ts";
import { initRouter } from "./lib/router.ts";
import { initKeyboard, registerShortcut } from "./lib/keyboard.ts";
import "./components/app-shell.ts";
import "./components/command-palette.ts";
import "./components/shortcut-help.ts";

initTheme();
initRouter();
initKeyboard();

// Register global shortcuts
registerShortcut({
  key: "k",
  ctrl: true,
  description: "Open command palette",
  handler: () => {
    const palette = document.querySelector("command-palette") as import("./components/command-palette.ts").CommandPalette | null;
    palette?.toggle();
  },
});

registerShortcut({
  key: "?",
  ctrl: true,
  description: "Show keyboard shortcuts",
  handler: () => {
    const help = document.querySelector("shortcut-help") as import("./components/shortcut-help.ts").ShortcutHelp | null;
    help?.toggle();
  },
});

registerShortcut({
  key: "n",
  ctrl: true,
  description: "Create new fragment",
  handler: () => {
    window.location.hash = "new";
  },
});

registerShortcut({
  key: "/",
  description: "Focus search",
  handler: () => {
    const palette = document.querySelector("command-palette") as import("./components/command-palette.ts").CommandPalette | null;
    palette?.show();
  },
});

registerShortcut({
  key: "Escape",
  description: "Close modal / go back",
  handler: () => {
    const palette = document.querySelector("command-palette") as import("./components/command-palette.ts").CommandPalette | null;
    const help = document.querySelector("shortcut-help") as import("./components/shortcut-help.ts").ShortcutHelp | null;
    if (palette?.classList.contains("visible")) palette.hide();
    else if (help?.classList.contains("visible")) help.hide();
  },
});

// List navigation shortcuts
registerShortcut({ key: "j", description: "Next item", handler: () => { /* handled by views */ } });
registerShortcut({ key: "k", description: "Previous item", handler: () => { /* handled by views */ } });
registerShortcut({ key: "Enter", description: "Open selected", handler: () => { /* handled by views */ } });

// Editor shortcuts
registerShortcut({
  key: "s",
  ctrl: true,
  description: "Save (in editor)",
  handler: () => {
    const saveBtn = document.querySelector("fragment-editor")?.shadowRoot?.getElementById("btn-save") as HTMLElement | null;
    saveBtn?.click();
  },
});
