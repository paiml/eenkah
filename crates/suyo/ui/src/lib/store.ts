// Best-effort localStorage under the "suyo." prefix (private mode just forgets).

export const store = {
  get<T>(key: string, fallback: T): T {
    try {
      const v = localStorage.getItem("suyo." + key);
      return v == null ? fallback : (JSON.parse(v) as T);
    } catch {
      return fallback;
    }
  },
  set(key: string, value: unknown): void {
    try {
      localStorage.setItem("suyo." + key, JSON.stringify(value));
    } catch {
      /* private mode */
    }
  },
};

/** Debounce a function (trailing call). */
export function debounce<A extends unknown[]>(fn: (...a: A) => void, ms = 300): (...a: A) => void {
  let t: ReturnType<typeof setTimeout> | undefined;
  return (...a: A) => {
    clearTimeout(t);
    t = setTimeout(() => fn(...a), ms);
  };
}

/** A short status message at the bottom of the page. */
export function toast(msg: string, kind: "" | "error" = ""): void {
  const el = document.createElement("div");
  el.className = "toast " + kind;
  el.setAttribute("role", "status");
  el.textContent = msg;
  document.body.append(el);
  setTimeout(() => el.classList.add("show"), 10);
  setTimeout(() => {
    el.classList.remove("show");
    setTimeout(() => el.remove(), 300);
  }, 2600);
}

export const errorMessage = (e: unknown): string => (e instanceof Error ? e.message : String(e));
