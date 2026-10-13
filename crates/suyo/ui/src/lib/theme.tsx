// Themes are CSS token sets (see suyo.css). The chosen theme lives in localStorage
// "suyo.theme" and is applied as data-theme on <html>; "auto" follows the OS.
// Works without a provider; ThemeProvider only exists for apps that want one at the root.

import { createContext, useContext, useEffect, useSyncExternalStore, type ReactNode } from "react";
import { store } from "./store";

export type ThemeName = "auto" | "light" | "dark" | "paper" | "nord" | "midnight" | "canvas";

export const THEMES: [ThemeName, string][] = [["auto", "Auto"], ["light", "Light"], ["dark", "Dark"], ["paper", "Paper"], ["nord", "Nord"], ["midnight", "Midnight"], ["canvas", "Canvas"]];

const isTheme = (v: unknown): v is ThemeName => typeof v === "string" && THEMES.some(([t]) => t === v);

/** True when the theme came from the URL or the user's own choice, not a default. */
let explicit = false;

function initialTheme(): ThemeName {
  if (typeof window === "undefined") return "auto";
  const q = new URLSearchParams(window.location.search).get("theme");
  if (isTheme(q)) {
    explicit = true;
    return q;
  }
  const s = store.get<unknown>("theme", null);
  if (isTheme(s)) {
    explicit = true;
    return s;
  }
  return "auto";
}

/** Apply a theme to <html> without storing it. */
export function applyTheme(name: ThemeName): void {
  if (typeof document === "undefined") return;
  if (name === "auto") document.documentElement.removeAttribute("data-theme");
  else document.documentElement.setAttribute("data-theme", name);
}

interface ThemeSnapshot {
  theme: ThemeName;
  /** Bumps whenever the effective colours may have changed (theme or OS scheme). */
  version: number;
}

let snap: ThemeSnapshot = { theme: initialTheme(), version: 0 };
applyTheme(snap.theme);
const subs = new Set<() => void>();
const notify = (): void => subs.forEach((f) => f());
let mqlBound = false;

function subscribe(f: () => void): () => void {
  subs.add(f);
  if (!mqlBound && typeof window !== "undefined" && window.matchMedia) {
    mqlBound = true;
    window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
      snap = { ...snap, version: snap.version + 1 };
      notify();
    });
  }
  return () => {
    subs.delete(f);
  };
}

/** The app's theme when the user has not chosen one (and none is in the URL). Not stored. */
export function setDefaultTheme(name: ThemeName): void {
  if (explicit || snap.theme === name) return;
  applyTheme(name);
  snap = { theme: name, version: snap.version + 1 };
  notify();
}

/** Choose and remember a theme. */
export function setTheme(name: ThemeName): void {
  store.set("theme", name);
  applyTheme(name);
  snap = { theme: name, version: snap.version + 1 };
  notify();
}

const getSnap = (): ThemeSnapshot => snap;

export interface ThemeApi extends ThemeSnapshot {
  setTheme: (name: ThemeName) => void;
}

const Ctx = createContext<ThemeApi | null>(null);

export function useTheme(): ThemeApi {
  const ctx = useContext(Ctx);
  const s = useSyncExternalStore(subscribe, getSnap, getSnap);
  return ctx ?? { ...s, setTheme };
}

export function ThemeProvider({ children, theme }: { children: ReactNode; theme?: ThemeName }) {
  const s = useSyncExternalStore(subscribe, getSnap, getSnap);
  useEffect(() => {
    if (theme && theme !== snap.theme) {
      applyTheme(theme);
      snap = { theme, version: snap.version + 1 };
      notify();
    }
  }, [theme]);
  return <Ctx.Provider value={{ ...s, setTheme }}>{children}</Ctx.Provider>;
}

export function ThemePicker({ onChange }: { onChange?: (t: ThemeName) => void }) {
  const { theme, setTheme: set } = useTheme();
  return (
    <select
      className="select"
      aria-label="Theme"
      title="Theme"
      value={theme}
      onChange={(e) => {
        const v = e.target.value;
        if (isTheme(v)) {
          set(v);
          onChange?.(v);
        }
      }}
    >
      {THEMES.map(([v, l]) => (
        <option key={v} value={v}>
          {l}
        </option>
      ))}
    </select>
  );
}
