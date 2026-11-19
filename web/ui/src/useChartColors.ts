// RFC 0261 §2.7: chart colours that change with the theme. SVG presentation attributes cannot read
// CSS variables, so the values are read from the tokens and read again when the theme changes
// (a person's choice on <html>, or the system's setting).

import { useEffect, useState } from "react";
import { type ChartColors, readChartColors } from "./theme";

export function useChartColors(): ChartColors {
  const [colors, setColors] = useState<ChartColors>(() => readChartColors());
  useEffect(() => {
    const refresh = () => setColors(readChartColors());
    const observer = new MutationObserver(refresh);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    media?.addEventListener?.("change", refresh);
    return () => {
      observer.disconnect();
      media?.removeEventListener?.("change", refresh);
    };
  }, []);
  return colors;
}
