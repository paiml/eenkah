// Wire types shared by the library and the app. They mirror the Rust server
// (src/render.rs `suyo_json`, src/store.rs `SeriesMeta`, src/catalog.rs `FnDoc`).

export type ChartType =
  | "line"
  | "area"
  | "bar"
  | "stat"
  | "table"
  | "scatter"
  | "histogram"
  | "slope"
  | "calendar"
  | "hourly"
  | "waffle"
  | "streams"
  | "quadrant"
  | "status"
  | "text";

/** A value in a panel's `options` map. */
export type OptionValue = string | number | boolean;
export type PanelOptions = Record<string, OptionValue | undefined>;

/** Per-series style set by functions such as highlight(), dashed(), stacked(), threshold(). */
export interface SeriesStyle {
  color?: string;
  dashed?: boolean;
  width?: number;
  /** 1 = highlighted, -1 = de-emphasised, 0 / absent = normal. */
  emphasis?: number;
  stack?: boolean;
  /** "threshold" for reference lines. */
  kind?: string;
}

/** [value, epochSeconds] */
export type DataPoint = [number | null, number];

export interface RenderSeries {
  target: string;
  path: string;
  unit: string;
  agg: string;
  step: string;
  stepSeconds: number;
  style: SeriesStyle;
  datapoints: DataPoint[];
}

export interface RenderError {
  target: string;
  error: string;
}

/** Response of GET /render?format=suyo */
export interface RenderResponse {
  from: number;
  until: number;
  tz: string;
  step: string;
  stepSeconds: number;
  series: RenderSeries[];
  errors: RenderError[];
}

export interface Range {
  from: string;
  until: string;
}

export interface PanelSpec {
  chart?: ChartType;
  title?: string;
  subtitle?: string;
  targets?: string[];
  from?: string;
  until?: string;
  step?: string;
  /** Width in columns of a 12-column grid. */
  w?: number;
  /** Height in 64px grid rows. */
  h?: number;
  options?: PanelOptions;
  /** Static text for chart "text". */
  text?: string;
  /** Host-absolute URL of plain text for chart "text" (one paragraph per line); never base-prefixed. */
  src?: string;
}

export interface DashboardSpec {
  title?: string;
  description?: string;
  from?: string;
  until?: string;
  refresh?: number;
  panels: PanelSpec[];
}

/** Row of GET /api/dashboards */
export interface DashboardSummary {
  name: string;
  title?: string | null;
  description?: string | null;
  panels: number;
  updated: number;
}

/** Row of GET /api/series */
export interface SeriesMeta {
  id: number;
  name: string;
  unit: string;
  agg: string;
  interval: number;
  description: string;
  first: number | null;
  last: number | null;
  points?: number;
}

/** Row of GET /api/functions */
export interface FunctionDoc {
  name: string;
  signature: string;
  group: string;
  doc: string;
  example: string;
}

/** Parameters of a /render request. */
export interface RenderParams {
  targets?: string[];
  from?: string;
  until?: string;
  step?: string;
  tz?: string;
  maxDataPoints?: number;
}

export type RenderFormat = "suyo" | "json" | "csv" | "svg" | "html";

/** Host branding injected by the server as window.SUYO_HOST. */
export interface SuyoHost {
  brand?: [string, string] | null;
  nav?: [string, string][];
}

declare global {
  interface Window {
    SUYO_BASE?: string;
    SUYO_HOST?: SuyoHost;
  }
}
