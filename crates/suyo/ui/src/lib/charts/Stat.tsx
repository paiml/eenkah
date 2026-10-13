// Big number with its change on one line, over a filled step-area of the range with a dashed reference line.
// Heads-up style: the tile stays neutral unless the change is worse than this series' own usual
// variation ("worse": over 1 standard deviation of the reference values, "much worse": over 2),
// so colour only appears when something needs attention. Without `better`, it never colours.

import { useLayoutEffect, useRef, useState } from "react";
import { fmtNum, unitSuffix } from "../format";
import { fmtTime } from "../time";
import { tooltip } from "../Tooltip";
import { Empty } from "./Empty";
import { f1, firstVal, isRef, lastVal, mean, optNum, optStr, spanLabel, vals, type ChartOpts, type MSeries, type Model } from "./model";

function pick(ser: MSeries, how: string): number | null {
  const a = vals(ser);
  if (!a.length) return null;
  switch (how) {
    case "avg": return mean(a);
    case "sum": return a.reduce((x, y) => x + y, 0);
    case "max": return Math.max(...a);
    case "min": return Math.min(...a);
    case "first": return a[0] ?? null;
    default: return lastVal(ser);
  }
}

function sd(a: number[]): number | null {
  if (a.length < 3) return null;
  const m = mean(a);
  return Math.sqrt(a.reduce((x, v) => x + (v - m) ** 2, 0) / (a.length - 1));
}

type Level = "ok" | "worse" | "much-worse";

/** Height of the number row, px (matches .stat-top in suyo.css). */
const TOP = 36;

export function Stat({ m, o, width, height }: { m: Model; o: ChartOpts; width: number; height?: number }) {
  const T = m.T;
  const svgRef = useRef<SVGSVGElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  // the space the tile really has: the panel body's height (the grid row fixes it)
  const [avail, setAvail] = useState<number | null>(null);
  const [availW, setAvailW] = useState<number | null>(null);
  const s = m.series.find((x) => !isRef(x));
  const hasSeries = s != null;
  useLayoutEffect(() => {
    const body = rootRef.current?.closest<HTMLElement>(".panel-body");
    if (!body) return;
    const measure = () => {
      setAvail(body.clientHeight || null);
      setAvailW(body.clientWidth || null);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(body);
    return () => ro.disconnect();
  }, [hasSeries]);
  if (!s) return <Empty msg="No data" />;
  const how = optStr(o, "stat") || "last";
  const value = pick(s, how);
  const unit = o.unit != null ? String(o.unit) : s.unit;
  const dec = optNum(o, "decimals");

  // reference: another series, the range average (excluding the value itself) or the first value
  let ref: number | null = null, refLabel = "", spread: number | null = null;
  const cmp = optStr(o, "compare") || "avg";
  const other = m.series.filter((x) => !isRef(x))[1];
  if (cmp === "series" || (other && cmp !== "avg" && cmp !== "first" && cmp !== "none")) {
    if (other) { ref = pick(other, how); refLabel = `vs ${other.name}`; spread = sd(vals(other)); }
  } else if (cmp === "avg") {
    const a = vals(s);
    const past = how === "last" ? a.slice(0, -1) : a;
    if (a.length > 1) { ref = mean(past); refLabel = `vs ${spanLabel(m)} avg`; spread = sd(past); }
  } else if (cmp === "first") {
    ref = firstVal(s);
    refLabel = "vs start of range";
    spread = sd(vals(s));
  }

  let level: Level = "ok";
  let delta = null;
  if (value != null && ref != null && cmp !== "none") {
    const d = value - ref;
    const pct = ref !== 0 ? (d / Math.abs(ref)) * 100 : null;
    const worse = o.better === "down" ? d > 0 : o.better === "up" ? d < 0 : false;
    const z = spread ? Math.abs(d) / spread : 0;
    if (worse) level = z >= 2 ? "much-worse" : z >= 1 ? "worse" : "ok";
    const arrow = Math.abs(d) < 1e-9 ? "■" : d > 0 ? "▲" : "▼";
    const dd = dec != null ? dec : Math.abs(d) < 10 && !Number.isInteger(d) ? 1 : 0;
    const num = `${d > 0 ? "+" : d < 0 ? "−" : ""}${fmtNum(Math.abs(d), dd)}`;
    const abs = `${num}${unit ? unitSuffix(unit) : ""}`;
    const pctTxt = pct != null && Math.abs(pct) < 1000 ? ` (${d >= 0 ? "+" : "−"}${Math.abs(pct).toFixed(Math.abs(pct) < 10 ? 1 : 0)}%)` : "";
    const txt = abs + pctTxt;
    const word = level === "much-worse" ? "much worse than usual" : level === "worse" ? "worse than usual" : "";
    delta = (
      <div className={`stat-delta ${level}`} title={[txt, refLabel, word].filter(Boolean).join(" ")}>
        <span className="arrow" aria-hidden="true">{arrow}</span>
        {/* narrow tiles drop, in order, the reference label, the percentage and the unit (suyo.css) */}
        <span>
          {num}
          {unit ? <span className="d-unit">{unitSuffix(unit)}</span> : null}
          {pctTxt ? <span className="d-pct">{pctTxt}</span> : null}
        </span>
        <span className="stat-ref"> {refLabel}</span>
        {word && <span className="sr-only">, {word}</span>}
      </div>
    );
  }
  const color = level === "much-worse" ? T.bad : level === "worse" ? T.warn : T.ink2;

  // step-area over the whole range, filling the space left in the tile
  const pts: [number, number][] = [];
  s.ts.forEach((t, i) => { const v = s.v[i]; if (v != null) pts.push([t, v]); });
  let spark = null;
  // the area takes what is left of the tile under the number row; none if that is too little
  const room = Math.min(140, (avail ?? height ?? 110) - TOP - 6);
  if (pts.length > 2 && o.spark !== false && room >= 18) {
    const W = Math.max(60, Math.min(width, availW ?? width));
    const H = room;
    const a = pts.map((p) => p[1]);
    let lo = Math.min(...a), hi = Math.max(...a);
    if (ref != null) { lo = Math.min(lo, ref); hi = Math.max(hi, ref); }
    const pad = (hi - lo) * 0.08 || 1;
    lo -= pad; hi += pad;
    const t0 = pts[0]![0];
    const t1 = pts[pts.length - 1]![0];
    const sx = (t: number) => ((t - t0) / Math.max(1, t1 - t0)) * W;
    const sy = (v: number) => H - ((v - lo) / (hi - lo)) * H;
    let line = `M${f1(sx(t0))},${f1(sy(pts[0]![1]))}`;
    for (let i = 1; i < pts.length; i++) {
      const [t, v] = pts[i]!;
      line += `H${f1(sx(t))}V${f1(sy(v))}`;
    }
    const area = `${line}V${H}H0Z`;
    const lp = pts[pts.length - 1]!;
    spark = (
      // drawn at the measured width, then scaled by CSS to the tile's width so it can never overflow
      <svg ref={svgRef} className="spark" height={H} viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label={`${s.name} over ${spanLabel(m)}`}
        onPointerMove={(ev) => {
          const r = svgRef.current?.getBoundingClientRect();
          const t = t0 + ((ev.clientX - (r?.left ?? 0)) / Math.max(1, r?.width ?? W)) * (t1 - t0);
          let best = pts[0]!;
          for (const p of pts) if (Math.abs(p[0] - t) < Math.abs(best[0] - t)) best = p;
          tooltip.show(ev, fmtTime(best[0], m.tz, "tip", s.step), [{ color, name: s.name, value: best[1], unit }]);
        }}
        onPointerLeave={() => tooltip.hide()}>
        <path d={area} fill={color} fillOpacity={0.22} stroke="none" />
        <path d={line} fill="none" stroke={color} strokeWidth={1.5} strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
        {ref != null && <line x1={0} x2={W} y1={f1(sy(ref))} y2={f1(sy(ref))} stroke={T.ink2} strokeOpacity={0.7} strokeWidth={1} strokeDasharray="4 3" vectorEffect="non-scaling-stroke" />}
        <path d={`M${f1(Math.min(W - 1, sx(lp[0])))},${f1(sy(lp[1]))}h0`} stroke={color} strokeWidth={6} strokeLinecap="round" vectorEffect="non-scaling-stroke" />
      </svg>
    );
  }
  return (
    <div className={`stat stat-${level}`} ref={rootRef}>
      <div className="stat-top">
        <div className="stat-value">
          {value == null ? "—" : fmtNum(value, dec)}
          {unit ? <span className="stat-unit">{unitSuffix(unit)}</span> : null}
        </div>
        {delta}
      </div>
      {spark}
    </div>
  );
}
