import sproytUiCss from "@sproyt/ui/styles.css";
import bridgeCss from "./legacy-bridge.css";
import reactAppCss from "./react-app.css";

const styleId = "sproyt-editorial-design-system";
const themeStorageKey = "sproyt.theme.v1";

export type SproytThemeMode = "light" | "dark" | "system";

/** Installs the shared and transitional styles once for either application surface. */
export function installSproytStyles(): void {
  if (document.getElementById(styleId)) return;
  const style = document.createElement("style");
  style.id = styleId;
  style.textContent = `${sproytUiCss}\n${reactAppCss}\n${bridgeCss}`;
  document.head.append(style);
}

/**
 * Applies the shared UI package to the transitional DOM client.
 *
 * The live client is still migrated view by view, so this owns only the
 * design-system boundary: a single scoped Theme region and a single injected
 * stylesheet. State, routing and browser effects remain with the client.
 */
export function installSproytDesignSystem(root: HTMLElement): SproytThemeMode {
  installSproytStyles();
  root.classList.add("sp-theme", "sproyt-editorial-app");
  root.dataset.accent = "citron";
  root.dataset.density = "comfortable";
  const mode = storedTheme();
  applyTheme(root, mode);

  return mode;
}

export function setSproytTheme(root: HTMLElement, mode: SproytThemeMode): void {
  applyTheme(root, mode);
  try { window.localStorage.setItem(themeStorageKey, mode); } catch { /* optional persistence */ }
}

function applyTheme(root: HTMLElement, mode: SproytThemeMode): void {
  root.dataset.theme = mode;
  document.documentElement.dataset.theme = mode;
}

function storedTheme(): SproytThemeMode {
  try {
    const value = window.localStorage.getItem(themeStorageKey);
    if (value === "light" || value === "dark" || value === "system") return value;
  } catch { /* optional persistence */ }
  return "system";
}
