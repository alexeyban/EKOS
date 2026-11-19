// RFC 0261 §2.7: light, dark or "follow the system", kept per person in this browser. index.html
// applies the stored choice before first paint; this module changes it.

export type ThemeChoice = "system" | "light" | "dark";

export const THEME_KEY = "ekos-theme";
const ORDER: ThemeChoice[] = ["system", "light", "dark"];

export function readTheme(): ThemeChoice {
  try {
    const v = window.localStorage.getItem(THEME_KEY);
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

export function applyTheme(choice: ThemeChoice): void {
  const root = document.documentElement;
  if (choice === "system") delete root.dataset.theme;
  else root.dataset.theme = choice;
  try {
    if (choice === "system") window.localStorage.removeItem(THEME_KEY);
    else window.localStorage.setItem(THEME_KEY, choice);
  } catch {
    // Private windows and blocked storage: the choice lasts for this page only.
  }
}

/** The next choice in the cycle the top bar's theme button walks through. */
export function nextTheme(choice: ThemeChoice): ThemeChoice {
  return ORDER[(ORDER.indexOf(choice) + 1) % ORDER.length];
}

export const THEME_LABEL: Record<ThemeChoice, string> = {
  system: "Theme: follow the system",
  light: "Theme: light",
  dark: "Theme: dark",
};
