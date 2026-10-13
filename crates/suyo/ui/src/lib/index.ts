// suyo UI library: typed API client, chart renderer, panels, dashboards and controls.
// Importing it also brings in suyo.css (theme tokens and component styles).

import "./suyo.css";

export type {
  ChartType,
  DashboardSpec,
  DashboardSummary,
  DataPoint,
  FunctionDoc,
  OptionValue,
  PanelOptions,
  PanelSpec,
  Range,
  RenderError,
  RenderFormat,
  RenderParams,
  RenderResponse,
  RenderSeries,
  SeriesMeta,
  SeriesStyle,
  SuyoHost,
} from "./types";

export {
  api,
  deleteDashboard,
  fetchRender,
  getBase,
  getDashboard,
  listDashboards,
  listFunctions,
  listSeries,
  renderUrl,
  saveDashboard,
  setBase,
  slugify,
  url,
} from "./api";

export { Chart, CHARTS, useWidth, type ChartProps } from "./charts/Chart";
export { model, readTokens, type MSeries, type Model, type Tokens } from "./charts/model";
export { Panel, composerHref, type PanelProps } from "./Panel";
export { Dashboard, parseDashboard, type DashboardProps } from "./Dashboard";
export { RangePicker, type RangePickerProps } from "./RangePicker";
export { JsonModal, type JsonModalProps } from "./JsonModal";
export { ThemePicker, ThemeProvider, useTheme, setTheme, setDefaultTheme, applyTheme, THEMES, type ThemeName, type ThemeApi } from "./theme";
export { useHover, hoverBus, type HoverEvent } from "./hover";
export { Tooltip, tooltip, type TooltipRow } from "./Tooltip";
export { fmtVal, fmtNum, fmtDuration, niceTicks, unitSuffix } from "./format";
export { absTime, fmtTime, rangeLabel, RANGES, timeTicks, tzParts, tzTime } from "./time";
export { store, toast, debounce, errorMessage } from "./store";
