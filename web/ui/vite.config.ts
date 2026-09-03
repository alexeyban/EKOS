import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

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
});
