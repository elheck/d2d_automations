/// <reference types="vitest/config" />
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The Rust server (check_stock_server) serves /api; during development this
// dev server forwards those requests to it, so pages and API share one origin
// (http://localhost:5173) exactly as in production.
// Port of the Rust server; dev.sh passes DEV_PORT on.
const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env ?? {};
const apiPort = env.CHECK_STOCK_API_PORT ?? "8080";

export default defineConfig({
  plugins: [react()],
  server: {
    host: "localhost",
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": {
        target: `http://127.0.0.1:${apiPort}`,
        // Keep the browser's Host header: the server checks it against its
        // allowlist (localhost is always allowed).
        changeOrigin: false,
      },
    },
  },
  build: {
    outDir: "dist",
    // Never inline files as data: URIs; the server's CSP only allows files
    // from its own origin.
    assetsInlineLimit: 0,
    sourcemap: false,
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    restoreMocks: true,
    unstubGlobals: true,
  },
});
