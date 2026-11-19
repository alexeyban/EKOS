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

/** Colours a chart needs, read from the tokens so a chart follows the theme like the CSS does. */
export interface ChartColors {
  a: string;
  b: string;
  /** Secondary text: axis labels, muted lines. */
  text: string;
  /** Main text: labels drawn on a canvas, the selected node. */
  ink: string;
  grid: string;
  surface: string;
  /** The page ground, which a canvas paints behind its drawing. */
  bg: string;
}

const CHART_FALLBACK: ChartColors = {
  a: "#7c3aed",
  b: "#047857",
  text: "#5c5578",
  ink: "#171326",
  grid: "#e3ddf5",
  surface: "#ffffff",
  bg: "#f6f4fc",
};

export function readChartColors(root: Element = document.documentElement): ChartColors {
  const css = getComputedStyle(root);
  const pick = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
  return {
    a: pick("--accent", CHART_FALLBACK.a),
    b: pick("--ok", CHART_FALLBACK.b),
    text: pick("--text-2", CHART_FALLBACK.text),
    ink: pick("--text", CHART_FALLBACK.ink),
    grid: pick("--border", CHART_FALLBACK.grid),
    surface: pick("--surface", CHART_FALLBACK.surface),
    bg: pick("--bg", CHART_FALLBACK.bg),
  };
}

/** `color` (#rgb, #rrggbb or rgb()/rgba()) at the given opacity, for a canvas fill or stroke. */
export function withAlpha(color: string, alpha: number): string {
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(color.trim());
  if (hex) {
    const h = hex[1].length === 3 ? [...hex[1]].map((x) => x + x).join("") : hex[1];
    const [r, g, b] = [0, 2, 4].map((i) => Number.parseInt(h.slice(i, i + 2), 16));
    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
  }
  const rgb = /^rgba?\(\s*(\d+)[\s,]+(\d+)[\s,]+(\d+)/i.exec(color.trim());
  return rgb ? `rgba(${rgb[1]}, ${rgb[2]}, ${rgb[3]}, ${alpha})` : color;
}
