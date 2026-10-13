import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The Rust server serves the built files under /static/ and answers every page route
// (/, /d, /d/:name, /docs, /embed) with dist/index.html.
const API = "http://127.0.0.1:8080";

export default defineConfig(({ command }) => ({
  plugins: [react()],
  // In dev the Vite server is the page host itself, so it serves from "/".
  base: command === "build" ? "/static/" : "/",
  build: {
    outDir: "dist",
    assetsDir: "assets",
    emptyOutDir: true,
    cssCodeSplit: false,
    rollupOptions: {
      output: {
        inlineDynamicImports: true,
        manualChunks: undefined,
        entryFileNames: "assets/suyo.js",
        assetFileNames: "assets/[name][extname]",
      },
    },
  },
  server: {
    proxy: {
      "/render": API,
      "/api": API,
      "/metrics": API,
    },
  },
}));
