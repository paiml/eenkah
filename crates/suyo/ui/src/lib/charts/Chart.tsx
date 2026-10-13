// <Chart>: pure renderer for a /render?format=suyo response, any chart type, plus the data view.

import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTheme } from "../theme";
import type { ChartType, RenderResponse } from "../types";
import { Cartesian } from "./Cartesian";
import { Histogram, Slope } from "./Distribution";
import { Empty } from "./Empty";
import { Calendar, Hourly, Status, Waffle } from "./Grid";
import { isRef, model, readTokens, type ChartOpts, type Model } from "./model";
import { Stat } from "./Stat";
import { Streams } from "./Streams";
import { RawTable, SummaryTable } from "./Tables";
import { Quadrant, Scatter } from "./XY";

export const CHARTS: [Exclude<ChartType, "text">, string][] = [
  ["line", "Line"], ["area", "Area"], ["bar", "Bar"], ["stat", "Big number"], ["table", "Table"],
  ["scatter", "Scatter"], ["histogram", "Histogram"], ["slope", "Slope"], ["calendar", "Calendar"], ["hourly", "Hour × weekday"], ["waffle", "Waffle"],
  ["streams", "Streams"], ["quadrant", "Quadrant"], ["status", "Status lights"],
];

export interface ChartProps {
  data: RenderResponse;
  /** Chart type; defaults to options.chart, then "line". */
  chart?: ChartType;
  /** Panel options (yMin, yMax, unit, legend, labels, nulls, stat, compare, better, decimals, …). */
  options?: ChartOpts;
  /** "data" shows the raw table instead of the chart. */
  view?: "chart" | "data";
  /** Pixel height available for the plot. */
  height?: number;
  /** Drag-to-zoom on line/area/bar and "Zoom to selection" on streams: epoch seconds. */
  onZoom?: (from: number, until: number) => void;
  /** Controlled set of hidden series ids (legend toggles); uncontrolled when omitted. */
  hidden?: Set<number>;
  onToggle?: (id: number) => void;
  /** Called with the computed model (series colours) whenever it changes. */
  onModel?: (m: Model) => void;
  /** Called after each render with the drawn legend's height (null when there is none). */
  onLegendHeight?: (px: number | null) => void;
}

const EMPTY_OPTS: ChartOpts = {};

/** Width of an element, tracked with a debounced ResizeObserver. */
export function useWidth<T extends HTMLElement>(): [React.RefObject<T | null>, number] {
  const ref = useRef<T>(null);
  const [w, setW] = useState(0);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    let last = el.clientWidth;
    setW(last);
    let timer: ReturnType<typeof setTimeout> | undefined;
    const ro = new ResizeObserver(() => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        const nw = el.clientWidth;
        if (Math.abs(nw - last) > 4) {
          last = nw;
          setW(nw);
        }
      }, 120);
    });
    ro.observe(el);
    return () => {
      clearTimeout(timer);
      ro.disconnect();
    };
  }, []);
  return [ref, w];
}

function Legend({ m, o, kind, hidden, onToggle }: { m: Model; o: ChartOpts; kind: "line" | "rect"; hidden: Set<number>; onToggle: (id: number) => void }) {
  const items = m.series.filter((s) => !(isRef(s) && o.legendRefs === false));
  if (o.legend === "hide" || (o.legend !== "show" && items.length < 2)) return null;
  return (
    <div className="legend" role="list">
      {items.map((s) => (
        <button key={s.id} className={"legend-item" + (hidden.has(s.id) ? " off" : "")} role="listitem" title="Click to hide or show" onClick={() => onToggle(s.id)}>
          {isRef(s) ? (
            <span className="key key-ref" style={{ borderColor: s.color }} />
          ) : kind === "line" ? (
            <span className={"key key-line" + (s.style.dashed ? " dashed" : "")} style={{ background: s.color, borderColor: s.color }} />
          ) : (
            <span className="key key-rect" style={{ background: s.color }} />
          )}
          <span className="legend-label">{s.name}</span>
        </button>
      ))}
    </div>
  );
}

export function Chart({ data, chart, options, view = "chart", height, onZoom, hidden: hiddenProp, onToggle, onModel, onLegendHeight }: ChartProps) {
  const o = options ?? EMPTY_OPTS;
  const { version } = useTheme();
  const m = useMemo(() => model(data, readTokens()), [data, version]);
  const [ownHidden, setOwnHidden] = useState<Set<number>>(() => new Set());
  const hidden = hiddenProp ?? ownHidden;
  const toggle = (id: number) => {
    if (onToggle) onToggle(id);
    if (!hiddenProp) {
      setOwnHidden((h) => {
        const n = new Set(h);
        if (n.has(id)) n.delete(id);
        else n.add(id);
        return n;
      });
    }
  };
  const [ref, width] = useWidth<HTMLDivElement>();
  const onModelRef = useRef(onModel);
  onModelRef.current = onModel;
  useLayoutEffect(() => onModelRef.current?.(m), [m]);
  const onLegendRef = useRef(onLegendHeight);
  onLegendRef.current = onLegendHeight;
  useLayoutEffect(() => {
    const lg = ref.current?.querySelector<HTMLElement>(".legend");
    onLegendRef.current?.(lg ? lg.offsetHeight : null);
  });

  const typeOpt = typeof o.chart === "string" ? o.chart : undefined;
  const type = chart ?? (typeOpt as ChartType | undefined) ?? "line";
  const h = height ?? (typeof o.height === "number" ? o.height : undefined);

  let body: React.ReactNode = null;
  if (width > 0) {
    if (view === "data") body = <RawTable m={m} />;
    else if (!m.series.length) body = <Empty msg={m.errors.length ? "" : "No data in this range"} />;
    else {
      const p = { m, o, width, height: h, hidden };
      switch (type) {
        case "stat": body = <Stat m={m} o={o} width={width} height={h} />; break;
        case "table": body = <SummaryTable m={m} width={width} />; break;
        case "scatter": body = <Scatter {...p} />; break;
        case "quadrant": body = <Quadrant {...p} />; break;
        case "calendar": body = <Calendar m={m} width={width} />; break;
        case "hourly": body = <Hourly m={m} width={width} height={h} />; break;
        case "waffle": body = <Waffle m={m} width={width} height={h} />; break;
        case "status": body = <Status m={m} o={o} />; break;
        case "histogram":
          body = <><Legend m={m} o={o} kind="rect" hidden={hidden} onToggle={toggle} /><div className="chart-body"><Histogram {...p} /></div></>;
          break;
        case "slope":
          body = <><Legend m={m} o={o} kind="line" hidden={hidden} onToggle={toggle} /><div className="chart-body"><Slope {...p} /></div></>;
          break;
        case "streams":
          body = <><Legend m={m} o={{ ...o, legend: o.legend || "show" }} kind="line" hidden={hidden} onToggle={toggle} /><div className="chart-body"><Streams {...p} onZoom={onZoom} /></div></>;
          break;
        case "bar":
          body = <><Legend m={m} o={o} kind="rect" hidden={hidden} onToggle={toggle} /><div className="chart-body"><Cartesian {...p} chart="bar" onZoom={onZoom} /></div></>;
          break;
        case "area":
          body = <><Legend m={m} o={o} kind="line" hidden={hidden} onToggle={toggle} /><div className="chart-body"><Cartesian {...p} chart="area" onZoom={onZoom} /></div></>;
          break;
        default:
          body = <><Legend m={m} o={o} kind="line" hidden={hidden} onToggle={toggle} /><div className="chart-body"><Cartesian {...p} chart="line" onZoom={onZoom} /></div></>;
      }
    }
  }
  return <div ref={ref}>{body}</div>;
}
