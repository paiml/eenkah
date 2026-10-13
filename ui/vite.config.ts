import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

// suyo's chart library (vendored in crates/suyo) is used from source, so eenkah and suyo always build together.
const suyoLib = fileURLToPath(new URL("../crates/suyo/ui/src/lib/index.ts", import.meta.url));
const API = "http://127.0.0.1:8090";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { "suyo-ui": suyoLib },
    // one React for both the app and the suyo library
    dedupe: ["react", "react-dom"],
  },
  build: {
    outDir: "dist",
    assetsDir: "assets",
    emptyOutDir: true,
    rollupOptions: { output: { inlineDynamicImports: true } },
  },
  server: {
    port: 5173,
    proxy: { "/api": API, "/suyo": API },
  },
});
