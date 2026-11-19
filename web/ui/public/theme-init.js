// RFC 0261 §2.7: apply a chosen light/dark theme before first paint (see src/theme.ts).
try {
  var t = window.localStorage.getItem("ekos-theme");
  if (t === "light" || t === "dark") document.documentElement.setAttribute("data-theme", t);
} catch (e) {
  // Storage blocked: follow the system.
}
