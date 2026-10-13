// A small typed router over the History API: the app has six pages and needs no more.

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type MouseEvent, type ReactNode } from "react";

export type Route =
  | { page: "today" }
  | { page: "week" }
  | { page: "trends" }
  | { page: "activities" }
  | { page: "activity"; id: number }
  | { page: "model" }
  | { page: "data" }
  | { page: "missing"; path: string };

export function parse(path: string): Route {
  const p = path.replace(/\/+$/, "") || "/";
  if (p === "/") return { page: "today" };
  if (p === "/week") return { page: "week" };
  if (p === "/trends") return { page: "trends" };
  if (p === "/activities") return { page: "activities" };
  if (p === "/model") return { page: "model" };
  if (p === "/data") return { page: "data" };
  const m = /^\/activity\/(-?\d+)$/.exec(p);
  if (m?.[1]) return { page: "activity", id: Number(m[1]) };
  return { page: "missing", path: p };
}

export function href(r: Route): string {
  switch (r.page) {
    case "today":
      return "/";
    case "activity":
      return `/activity/${r.id}`;
    case "missing":
      return r.path;
    default:
      return `/${r.page}`;
  }
}

interface Nav {
  route: Route;
  go: (to: string) => void;
}

const NavContext = createContext<Nav>({ route: { page: "today" }, go: () => undefined });

export function RouterProvider({ children }: { children: ReactNode }) {
  const [path, setPath] = useState(() => location.pathname);
  useEffect(() => {
    const on = () => setPath(location.pathname);
    addEventListener("popstate", on);
    return () => removeEventListener("popstate", on);
  }, []);
  const go = useCallback((to: string) => {
    history.pushState(null, "", to);
    setPath(location.pathname);
    scrollTo(0, 0);
  }, []);
  const value = useMemo(() => ({ route: parse(path), go }), [path, go]);
  return <NavContext.Provider value={value}>{children}</NavContext.Provider>;
}

export const useRoute = () => useContext(NavContext);

/** An in-app link: plain <a> semantics (middle-click, copy link) with client-side navigation. */
export function Link({ to, className, children, title }: { to: string; className?: string; children: ReactNode; title?: string }) {
  const { go } = useRoute();
  const onClick = (e: MouseEvent<HTMLAnchorElement>) => {
    if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    e.preventDefault();
    go(to);
  };
  return (
    <a href={to} className={className} onClick={onClick} title={title}>
      {children}
    </a>
  );
}
