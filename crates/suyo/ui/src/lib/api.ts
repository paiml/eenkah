// HTTP client for the suyo server. Every app path goes through url() so suyo works
// when mounted under a prefix (window.SUYO_BASE, e.g. "/suyo").

import { store } from "./store";
import type { DashboardSpec, DashboardSummary, FunctionDoc, RenderFormat, RenderParams, RenderResponse, SeriesMeta } from "./types";

let BASE = "";

/** Set the path prefix suyo is mounted under ("" when standalone). Trailing slashes are dropped. */
export function setBase(base: string): void {
  BASE = (base || "").replace(/\/+$/, "");
}

export function getBase(): string {
  return BASE;
}

/** Prefix an absolute app path with the base. Relative and already-prefixed paths pass through. */
export function url(path: string): string {
  return path.startsWith("/") && !(BASE && path.startsWith(BASE + "/")) ? BASE + path : path;
}

function errorText(body: unknown, fallback: string): string {
  if (body && typeof body === "object" && "error" in body) {
    const e = (body as { error: unknown }).error;
    if (typeof e === "string") return e;
  }
  if (typeof body === "string" && body) return body;
  return fallback;
}

/**
 * Fetch a suyo API path and decode JSON (or text). Adds the stored write token, and on a 401
 * asks for one once and retries. The result type is the caller's assertion about the endpoint.
 */
export async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  const token = store.get<string | null>("token", null);
  if (token) headers.set("Authorization", `Bearer ${token}`);
  const r = await fetch(url(path), { ...init, headers });
  const ct = r.headers.get("content-type") || "";
  const body: unknown = ct.includes("json") ? await r.json() : await r.text();
  if (r.status === 401) {
    const t = window.prompt("This suyo server requires a write token:");
    if (t) {
      store.set("token", t);
      return api<T>(path, init);
    }
  }
  if (!r.ok) throw new Error(errorText(body, r.statusText));
  return body as T;
}

/** Build a /render URL (base-prefixed). */
export function renderUrl(params: RenderParams, format: RenderFormat = "suyo"): string {
  const q = new URLSearchParams();
  for (const t of params.targets || []) if (t && t.trim()) q.append("target", t.trim());
  if (params.from) q.set("from", params.from);
  if (params.until) q.set("until", params.until);
  if (params.step) q.set("step", params.step);
  if (params.tz) q.set("tz", params.tz);
  if (params.maxDataPoints) q.set("maxDataPoints", String(params.maxDataPoints));
  q.set("format", format);
  return url("/render?" + q.toString());
}

export function fetchRender(params: RenderParams, init?: RequestInit): Promise<RenderResponse> {
  return api<RenderResponse>(renderUrl(params, "suyo"), init);
}

export function listDashboards(): Promise<DashboardSummary[]> {
  return api<DashboardSummary[]>("/api/dashboards");
}

export function getDashboard(name: string): Promise<DashboardSpec> {
  return api<DashboardSpec>(`/api/dashboards/${encodeURIComponent(name)}`);
}

export function saveDashboard(name: string, spec: DashboardSpec): Promise<{ ok: boolean; name: string }> {
  return api(`/api/dashboards/${encodeURIComponent(name)}`, { method: "PUT", body: JSON.stringify(spec) });
}

export function deleteDashboard(name: string): Promise<{ deleted: boolean | number }> {
  return api(`/api/dashboards/${encodeURIComponent(name)}`, { method: "DELETE" });
}

export function listSeries(query?: string): Promise<SeriesMeta[]> {
  return api<SeriesMeta[]>("/api/series" + (query ? "?query=" + encodeURIComponent(query) : ""));
}

export function listFunctions(): Promise<FunctionDoc[]> {
  return api<FunctionDoc[]>("/api/functions");
}

/** Dashboard slug from a free-form name: lower case, letters, digits, - and _. */
export const slugify = (s: string): string => s.trim().toLowerCase().replace(/[^a-z0-9_-]+/g, "-");
