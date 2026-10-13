// <Panel>: a panel = targets + chart type + options. Fetches /render and draws it.
// Used by dashboards, the composer preview and /embed.

import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { fetchRender, renderUrl, url } from "./api";
import { Chart } from "./charts/Chart";
import type { Model } from "./charts/model";
import { errorMessage } from "./store";
import { absTime, rangeLabel } from "./time";
import type { PanelSpec, Range, RenderResponse } from "./types";

const PX_PER_ROW = 64, GAP = 12;

/** Composer URL (base-prefixed) that reproduces a panel. */
export function composerHref(spec: PanelSpec, range: Partial<Range> = {}): string {
  const q = new URLSearchParams();
  for (const t of spec.targets || []) q.append("target", t);
  const from = spec.from || range.from, until = spec.until || range.until;
  if (from) q.set("from", from);
  if (until && until !== "now") q.set("until", until);
  if (spec.chart && spec.chart !== "line") q.set("chart", spec.chart);
  if (spec.step) q.set("step", spec.step);
  if (spec.title) q.set("title", spec.title);
  if (spec.subtitle) q.set("subtitle", spec.subtitle);
  for (const [k, v] of Object.entries(spec.options || {})) if (v != null && v !== "") q.set(k, String(v));
  return url("/?" + q.toString());
}

export interface PanelProps {
  spec: PanelSpec;
  /** Range used when the panel has no from/until of its own (default -1d → now). */
  range?: Partial<Range>;
  /** Drag-to-zoom: the selection as absolute times in the data's zone, plus epoch seconds. */
  onZoom?: (range: Range, epoch: [number, number]) => void;
  /** Extra buttons in the panel's hover toolbar (dashboard edit tools). */
  tools?: ReactNode;
  /** Title, toolbar and data/CSV/SVG links (default true). */
  chrome?: boolean;
  /** Draw the card background and border (default true). */
  card?: boolean;
  /** Fixed plot height in px; otherwise derived from spec.h grid rows. */
  height?: number;
  /** Change to refetch. */
  refreshSignal?: number;
  /** IANA zone for calendar buckets (default: the server's). */
  tz?: string;
  /** Title link; default opens the panel in the composer, null for none. */
  titleHref?: string | null;
  className?: string;
  style?: CSSProperties;
  onData?: (data: RenderResponse | null) => void;
  onModel?: (m: Model) => void;
}

function TextPanel({ spec, refreshSignal, className, style }: PanelProps) {
  const [lines, setLines] = useState<string[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    if (!spec.src) return;
    let live = true;
    // `src` is a host-absolute URL: fetched as is, never base-prefixed.
    fetch(spec.src)
      .then((r) => r.text())
      .then((t) => {
        if (!live) return;
        setLines(t.split("\n").filter((l) => l.trim()));
        setErr(null);
      })
      .catch((e: unknown) => live && setErr(String(e)));
    return () => {
      live = false;
    };
  }, [spec.src, refreshSignal]);
  return (
    <section className={"panel text-panel" + (className ? " " + className : "")} style={style}>
      {spec.title && <div className="panel-title">{spec.title}</div>}
      <div>{err ?? (lines ? lines.map((l, i) => <p key={i}>{l}</p>) : spec.text || "")}</div>
    </section>
  );
}

export function Panel(props: PanelProps) {
  if (props.spec.chart === "text") return <TextPanel {...props} />;
  return <ChartPanel {...props} />;
}

function ChartPanel({ spec, range, onZoom, tools, chrome = true, card = true, height, refreshSignal, tz, titleHref, className, style, onData, onModel }: PanelProps) {
  const chart = spec.chart || "line";
  const [view, setView] = useState<"chart" | "data">("chart");
  const [data, setData] = useState<RenderResponse | null>(null);
  const [errors, setErrors] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const headRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const [headH, setHeadH] = useState(0);
  const [legendMeasured, setLegendMeasured] = useState<number | null>(null);
  const seq = useRef(0);
  const onDataRef = useRef(onData);
  onDataRef.current = onData;

  const from = spec.from || range?.from || "-1d";
  const until = spec.until || range?.until || "now";
  const targetsKey = JSON.stringify(spec.targets || []);
  const optsKey = JSON.stringify(spec.options || {});
  // keyed by content so a re-created spec object doesn't redraw the chart
  const options = useMemo(() => spec.options ?? {}, [optsKey]); // eslint-disable-line react-hooks/exhaustive-deps

  useLayoutEffect(() => {
    const h = headRef.current?.offsetHeight ?? 0;
    if (h !== headH) setHeadH(h);
  });

  useEffect(() => {
    const targets: string[] = JSON.parse(targetsKey) as string[];
    if (!targets.length) {
      setData(null);
      setErrors(null);
      onDataRef.current?.(null);
      return;
    }
    const my = ++seq.current;
    setLoading(true);
    const width = Math.max(200, bodyRef.current?.clientWidth || 800);
    fetchRender({ targets, from, until, step: spec.step, tz, maxDataPoints: Math.round(width / (chart === "bar" ? 8 : 1.6)) })
      .then((d) => {
        if (my !== seq.current) return;
        setData(d);
        const errs = d.errors || [];
        setErrors(errs.length ? errs.map((e) => `${e.target}\n  ${e.error}`).join("\n") : null);
        onDataRef.current?.(d);
      })
      .catch((e: unknown) => {
        if (my !== seq.current) return;
        setErrors(errorMessage(e));
        setData(null);
        onDataRef.current?.(null);
      })
      .finally(() => {
        if (my === seq.current) setLoading(false);
      });
  }, [targetsKey, from, until, spec.step, tz, chart, refreshSignal]);

  const plotHeight = (() => {
    if (height) return height;
    const rows = spec.h || (chart === "stat" ? 2 : 3);
    // the rendered legend's height (plus its 6px margin) once drawn, else a guess
    const legendH = legendMeasured != null ? legendMeasured + 6 : ["line", "area", "bar"].includes(chart) && (spec.targets || []).length > 1 ? 26 : 0;
    // 36 = panel padding (14 + 12) + border (2) + the head's bottom margin (8)
    return Math.max(70, rows * PX_PER_ROW + (rows - 1) * GAP - headH - 36 - legendH);
  })();

  const zoom = onZoom && data ? (a: number, b: number) => onZoom({ from: absTime(a, data.tz), until: absTime(b, data.tz) }, [a, b]) : undefined;
  const linkParams = { targets: spec.targets, from, until, step: spec.step };
  const href = titleHref === undefined ? composerHref(spec, range) : titleHref;

  return (
    <section className={(card ? "card panel" : "panel") + (className ? " " + className : "")} style={style}>
      {chrome && (
        <div className="panel-head" ref={headRef}>
          <div className="grow">
            <div className="panel-title">
              {spec.title ? href ? <a href={href} title="Open in composer">{spec.title}</a> : spec.title : null}{" "}
              {spec.from && (
                <span className="badge" title={`This panel uses its own time range: ${spec.from} → ${spec.until || "now"}`}>
                  {rangeLabel(spec.from, spec.until)}
                </span>
              )}
            </div>
            {spec.subtitle && <div className="panel-sub">{spec.subtitle}</div>}
          </div>
          <div className="panel-tools">
            {tools}
            <button className="btn ghost small" title="Toggle data table" onClick={() => setView((v) => (v === "chart" ? "data" : "chart"))}>
              {view === "chart" ? "Data" : "Chart"}
            </button>
            <a className="btn ghost small" target="_blank" rel="noopener" title="Open as CSV" href={renderUrl(linkParams, "csv")}>CSV</a>
            <a className="btn ghost small" target="_blank" rel="noopener" title="Open as SVG" href={renderUrl(linkParams, "svg")}>SVG</a>
          </div>
        </div>
      )}
      <div className="panel-error" hidden={!errors}>{errors}</div>
      <div className={"panel-body" + (loading ? " loading" : "")} ref={bodyRef}>
        {!spec.targets?.length ? (
          <div className="empty">Add a target to plot</div>
        ) : data ? (
          <Chart data={data} chart={chart} options={options} view={view} height={plotHeight} onZoom={zoom} onModel={onModel} onLegendHeight={setLegendMeasured} />
        ) : null}
      </div>
    </section>
  );
}
