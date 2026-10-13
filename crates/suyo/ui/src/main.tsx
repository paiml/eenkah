import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { setBase } from "./lib";
import { App } from "./app/App";

// The server injects window.SUYO_BASE when suyo is mounted under a path prefix.
setBase(typeof window.SUYO_BASE === "string" ? window.SUYO_BASE : "");

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
