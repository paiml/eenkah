// suyo's standalone app: one page, routed on the pathname (full page loads between routes).

import { getBase, ThemeProvider, Tooltip } from "../lib";
import { Composer } from "./Composer";
import { DashboardList } from "./DashboardList";
import { DashboardPage } from "./DashboardPage";
import { Docs } from "./Docs";
import { Embed } from "./Embed";
import { Header } from "./Header";

export type Route =
  | { page: "compose" }
  | { page: "list" }
  | { page: "dashboard"; name: string }
  | { page: "docs" }
  | { page: "embed" }
  | { page: "missing"; path: string };

export function route(pathname: string): Route {
  const base = getBase();
  const p = base && pathname.startsWith(base) ? pathname.slice(base.length) || "/" : pathname;
  if (p === "/" || p === "") return { page: "compose" };
  if (p === "/d" || p === "/d/") return { page: "list" };
  if (p.startsWith("/d/")) return { page: "dashboard", name: decodeURIComponent(p.slice(3).replace(/\/$/, "")) };
  if (p === "/docs") return { page: "docs" };
  // /render?format=html serves the same page as /embed
  if (p === "/embed" || p === "/render") return { page: "embed" };
  return { page: "missing", path: p };
}

function Page({ r }: { r: Route }) {
  switch (r.page) {
    case "compose": return <Composer />;
    case "list": return <DashboardList />;
    case "dashboard": return <DashboardPage name={r.name} />;
    case "docs": return <Docs />;
    case "embed": return <Embed />;
    case "missing":
      return (
        <>
          <Header active="" />
          <div className="docs"><h1>Not found</h1><p className="mono">{r.path}</p></div>
        </>
      );
  }
}

export function App() {
  return (
    <ThemeProvider>
      <Page r={route(window.location.pathname)} />
      <Tooltip />
    </ThemeProvider>
  );
}
