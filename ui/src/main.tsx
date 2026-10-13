import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { setBase, setDefaultTheme, ThemeProvider } from "suyo-ui";
import { App } from "./App";
import { RouterProvider } from "./router";
import "./eenkah.css";

// suyo (charts, dashboards, render API) is mounted under /suyo on the eenkah server.
setBase("/suyo");
// DESIGN.md: a dark-only system. Used unless the athlete picks another theme in the menu.
setDefaultTheme("canvas");

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");
createRoot(root).render(
  <StrictMode>
    <ThemeProvider>
      <RouterProvider>
        <App />
      </RouterProvider>
    </ThemeProvider>
  </StrictMode>,
);
