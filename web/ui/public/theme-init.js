// RFC 0261 §2.7: apply a chosen light/dark theme before first paint (see src/theme.ts).
try {
  const t = window.localStorage.getItem("ekos-theme");
  if (t === "light" || t === "dark") document.documentElement.dataset.theme = t;
} catch (e) {
  // Storage blocked: follow the system.
}
