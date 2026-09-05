import react from "@vitejs/plugin-react";
// `vitest/config`'s defineConfig re-exports Vite's, extended with a `test` field — a drop-in
// replacement for `vite`'s own that keeps this one file as the source of truth for both.
import { defineConfig } from "vitest/config";

// The console API runs on :8000; proxy /api so the browser talks to one origin in dev.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: process.env.VITE_API_TARGET ?? "http://localhost:8000",
        changeOrigin: true,
      },
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    css: false,
    coverage: {
      provider: "v8",
      reporter: ["text", "lcov"],
      include: ["src/**/*.{ts,tsx}"],
      exclude: [
        "src/vite-env.d.ts",
        "src/api/schema.d.ts",
        "src/main.tsx",
        "src/test/**",
        "**/*.test.{ts,tsx}",
      ],
    },
  },
});
