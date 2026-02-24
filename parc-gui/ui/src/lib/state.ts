export interface NavState {
  currentView: string;
  selectedFragmentId: string | null;
  listViewMode: "card" | "table";
  tagViewMode: "cloud" | "list";
}

export interface PrefState {
  theme: "light" | "dark" | "system";
  editorFontSize: number;
  showPreview: boolean;
}

const NAV_KEY = "parc-nav-state";
const PREF_KEY = "parc-prefs";

const defaultNav: NavState = {
  currentView: "all",
  selectedFragmentId: null,
  listViewMode: "card",
  tagViewMode: "cloud",
};

const defaultPref: PrefState = {
  theme: "system",
  editorFontSize: 14,
  showPreview: true,
};

function load<T>(key: string, defaults: T): T {
  try {
    const stored = localStorage.getItem(key);
    if (stored) return { ...defaults, ...JSON.parse(stored) };
  } catch { /* ignore */ }
  return { ...defaults };
}

function save(key: string, value: unknown): void {
  localStorage.setItem(key, JSON.stringify(value));
}

let navState = load(NAV_KEY, defaultNav);
let prefState = load(PREF_KEY, defaultPref);

export function getNav(): NavState {
  return { ...navState };
}

export function setNav(partial: Partial<NavState>): void {
  navState = { ...navState, ...partial };
  save(NAV_KEY, navState);
}

export function getPrefs(): PrefState {
  return { ...prefState };
}

export function setPrefs(partial: Partial<PrefState>): void {
  prefState = { ...prefState, ...partial };
  save(PREF_KEY, prefState);
}
