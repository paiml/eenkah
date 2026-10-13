// Line, area and bar charts on a shared time axis: crosshair synced across charts,
// tooltip, direct end labels, reference lines, drag to zoom.

import { useMemo, useRef, useState, type PointerEvent, type ReactElement } from "react";
import { fmtVal, niceTicks, textWidth } from "../format";
import { fmtTime, timeTicks } from "../time";
import { useHover } from "../hover";
import { tooltip } from "../Tooltip";
import { barPath, f1, isRef, lastIdx, optNum, sharedUnit, type ChartOpts, type MSeries, type Model } from "./model";

export interface CartesianProps {
  m: Model;
  o: ChartOpts;
  chart: "line" | "area" | "bar";
  width: number;
  height?: number;
  hidden: Set<number>;
  onZoom?: (from: number, until: number) => void;
}

interface Label {
  s: MSeries;
  i: number;
  y: number;
}

export function Cartesian({ m, o, chart, width, height, hidden, onZoom }: CartesianProps) {
  const [clipId] = useState(() => "clip" + Math.random().toString(36).slice(2, 9));
  const L = useMemo(() => {
    const T = m.T;
    const visible = m.series.filter((s) => !hidden.has(s.id));
    const data = visible.filter((s) => !isRef(s));
    const refs = visible.filter(isRef);
    const isBar = chart === "bar";
    const stack = (chart === "area" || isBar) && data.some((s) => s.style.stack);

    // union of x positions
    const xsSet = new Set<number>();
    for (const s of visible) s.ts.forEach((t, i) => { if (s.v[i] != null || isBar) xsSet.add(t); });
    const xs = [...xsSet].sort((a, b) => a - b);
    const idx = new Map(m.series.map((s) => [s.id, new Map(s.ts.map((t, i) => [t, i]))]));
    const at = (s: MSeries, t: number): number | null => {
      const i = idx.get(s.id)?.get(t);
      return i == null ? null : (s.v[i] ?? null);
    };

    // stacking
    const base = new Map<number, Map<number, number>>();
    const stacked = new Map<number, Map<number, number>>();
    if (stack) {
      const acc = new Map(xs.map((t) => [t, 0]));
      for (const s of data.filter((d) => d.style.stack)) {
        const b = new Map<number, number>(), top = new Map<number, number>();
        for (const t of xs) {
          const v = at(s, t);
          if (v == null) continue;
          const a = acc.get(t) ?? 0;
          b.set(t, a);
          acc.set(t, a + v);
          top.set(t, a + v);
        }
        base.set(s.id, b);
        stacked.set(s.id, top);
      }
    }
    const yOf = (s: MSeries, t: number, v: number): number => stacked.get(s.id)?.get(t) ?? v;

    // y domain
    let lo = Infinity, hi = -Infinity;
    for (const s of visible) {
      const st = stacked.get(s.id);
      if (st) for (const v of st.values()) { hi = Math.max(hi, v); lo = Math.min(lo, 0); }
      for (const v of s.v) if (v != null) { lo = Math.min(lo, v); hi = Math.max(hi, v); }
    }
    if (!isFinite(lo)) { lo = 0; hi = 1; }
    if (isBar || chart === "area" || o.zero) lo = Math.min(lo, 0); // bars and areas encode length: always from zero
    const yMin = optNum(o, "yMin"), yMax = optNum(o, "yMax");
    if (yMin != null) lo = yMin;
    if (yMax != null) hi = yMax;
    if (hi <= lo) hi = lo + (Math.abs(lo) || 1);
    const W = Math.max(width, 240);
    const H = height || 260;
    const ticks = niceTicks(lo, hi + (o.yMax ? 0 : (hi - lo) * 0.04), Math.max(2, Math.floor((H - 40) / 46)));
    lo = Math.min(lo, ticks[0] ?? lo);
    hi = Math.max(hi, ticks[ticks.length - 1] ?? hi);
    const unit = o.unit != null ? String(o.unit) : sharedUnit(visible);
    const yLabels = ticks.map((t) => fmtVal(t));
    const left = Math.max(...yLabels.map((l) => textWidth(l, 11))) + 14;

    const top = unit ? 20 : 10, bottom = 24;
    const y0 = top, y1 = H - bottom;
    const sy = (v: number): number => y1 - ((v - lo) / (hi - lo)) * (y1 - y0);
    // Direct end labels for a few well-separated lines; skipped when they would collide.
    const wantLabels = !isBar && o.labels !== "none" && data.length >= 1 && data.length <= 4 && W > 420 && (data.length > 1 || refs.length > 0);
    const cands: Label[] = [];
    for (const s of [...(wantLabels ? data : []), ...refs]) {
      const i = lastIdx(s);
      if (i < 0) continue;
      const t = s.ts[i] as number;
      cands.push({ s, i, y: sy(yOf(s, t, s.v[i] as number)) });
    }
    cands.sort((a, b) => a.y - b.y);
    const collide = cands.some((c, i) => i > 0 && c.y - (cands[i - 1] as Label).y < 13);
    const labelled = cands.filter((c) => isRef(c.s) || !collide);
    const right = labelled.length ? Math.min(150, Math.max(...labelled.map((c) => textWidth(c.s.name, 11.5))) + 18) : 14;
    const x0 = left, x1 = W - right;
    // bars occupy whole buckets; the domain ends at the last bucket's end
    const stepS = Math.max(...data.map((s) => s.step || m.step || 60), m.step || 60);
    const xFrom = isBar && xs.length ? Math.min(xs[0] as number, m.from) : m.from;
    const xUntil = isBar && xs.length ? Math.max(m.until, (xs[xs.length - 1] as number) + stepS) : m.until;
    const sx = (t: number): number => x0 + ((t - xFrom) / Math.max(1, xUntil - xFrom)) * (x1 - x0);
    const slotW = (x1 - x0) * (stepS / Math.max(1, xUntil - xFrom));

    const els: ReactElement[] = [];
    // grid + y labels
    ticks.forEach((tk, i) => {
      const y = +f1(sy(tk));
      els.push(<line key={`g${i}`} x1={x0} x2={x1} y1={y} y2={y} stroke={tk === 0 ? T.axis : T.grid} strokeWidth={1} />);
      els.push(<text key={`gl${i}`} x={x0 - 8} y={y + 4} className="tick" textAnchor="end">{yLabels[i]}</text>);
    });
    if (unit) els.push(<text key="unit" x={x0 - 8} y={y0 - 10} className="tick unit" textAnchor="end">{unit}</text>);
    els.push(<line key="base" x1={x0} x2={x1} y1={y1} y2={y1} stroke={T.axis} strokeWidth={1} />);
    // x ticks
    if (isBar && xs.length && xs.length <= 40) {
      const every = Math.ceil(xs.length / Math.max(1, Math.floor((x1 - x0) / 64)));
      xs.forEach((t, i) => {
        if (i % every) return;
        els.push(<text key={`x${i}`} x={f1(sx(t) + slotW / 2)} y={y1 + 16} className="tick" textAnchor="middle">{fmtTime(t, m.tz, "bar", stepS)}</text>);
      });
    } else {
      timeTicks(xFrom, xUntil, m.tz, Math.max(2, Math.floor((x1 - x0) / 84))).forEach(([t, label], i) => {
        const x = sx(t);
        if (x < x0 + 10 || x > x1 - 10) return;
        els.push(<text key={`x${i}`} x={f1(x)} y={y1 + 16} className="tick" textAnchor="middle">{label}</text>);
      });
    }

    // marks, clipped to the plot so buckets that start before `from` don't spill over the axis
    const marks: ReactElement[] = [];
    if (isBar) {
      const groups = stack ? 1 : Math.max(1, data.length);
      const bw = Math.max(1, Math.min(24, (slotW * 0.72) / groups - (groups > 1 ? 2 : 0)));
      const gw = groups * bw + (groups - 1) * 2;
      const stackTops = new Map<number, number>();
      if (stack) for (const t of xs) stackTops.set(t, Math.max(...data.filter((d) => d.style.stack).map((d) => stacked.get(d.id)?.get(t) ?? -Infinity)));
      data.forEach((s, k) => {
        for (const t of xs) {
          const v = at(s, t);
          if (v == null) continue;
          const cx = sx(t) + slotW / 2;
          const x = stack ? cx - bw / 2 : cx - gw / 2 + k * (bw + 2);
          let ya: number, yb: number;
          const b0 = base.get(s.id);
          if (stack && b0) {
            const b = b0.get(t) ?? 0;
            ya = sy(b + v);
            yb = sy(b);
            if (b > 0) yb -= 2;
          } else {
            ya = sy(Math.max(v, 0));
            yb = sy(Math.min(v, 0));
          }
          const hh = Math.max(0.5, yb - ya);
          const r = Math.min(4, bw / 2, hh);
          const isTop = !stack || stacked.get(s.id)?.get(t) === stackTops.get(t);
          marks.push(<path key={`b${s.id}-${t}`} d={barPath(x, ya, bw, hh, isTop ? r : 0, v >= 0)} fill={s.color} />);
        }
      });
    } else {
      // dim series first so they sit underneath
      const order = [...data].sort((a, b) => (a.style.emphasis || 0) - (b.style.emphasis || 0));
      for (const s of order) {
        const pts: ([number, number, number] | null)[] = [];
        const st = stacked.get(s.id), bs = base.get(s.id);
        for (let i = 0; i < s.ts.length; i++) {
          const v = s.v[i];
          const t = s.ts[i] as number;
          if (v == null) {
            if (o.nulls === "connect") continue;
            pts.push(null);
            continue;
          }
          pts.push([sx(t), sy(st?.get(t) ?? v), st ? sy(bs?.get(t) ?? 0) : sy(Math.max(lo, 0))]);
        }
        const segs: [number, number, number][][] = [];
        let cur: [number, number, number][] = [];
        for (const p of pts) {
          if (p) cur.push(p);
          else if (cur.length) { segs.push(cur); cur = []; }
        }
        if (cur.length) segs.push(cur);
        const w = s.style.width || ((s.style.emphasis ?? 0) > 0 ? 2.5 : (s.style.emphasis ?? 0) < 0 ? 1.5 : 2);
        segs.forEach((seg, j) => {
          if (chart === "area") {
            const d = "M" + seg.map((p) => `${f1(p[0])},${f1(p[1])}`).join("L") + "L" + [...seg].reverse().map((p) => `${f1(p[0])},${f1(p[2])}`).join("L") + "Z";
            marks.push(<path key={`a${s.id}-${j}`} d={d} fill={s.color} fillOpacity={stack ? 0.5 : 0.1} />);
          }
          const p0 = seg[0] as [number, number, number];
          if (seg.length === 1) marks.push(<circle key={`p${s.id}-${j}`} cx={f1(p0[0])} cy={f1(p0[1])} r={2.5} fill={s.color} />);
          else
            marks.push(
              <path key={`l${s.id}-${j}`} d={"M" + seg.map((p) => `${f1(p[0])},${f1(p[1])}`).join("L")} fill="none" stroke={s.color} strokeWidth={w}
                strokeLinejoin="round" strokeLinecap="round" strokeDasharray={s.style.dashed ? "6 4" : undefined} />,
            );
        });
      }
    }
    // reference lines (thresholds / goals)
    for (const s of refs) {
      const pts: [number, number][] = [];
      s.ts.forEach((t, i) => {
        const v = s.v[i];
        if (v != null) pts.push([isBar ? sx(t) + slotW / 2 : sx(t), sy(v)]);
      });
      if (!pts.length) continue;
      const py = (pts[0] as [number, number])[1];
      const flat = pts.every((p) => Math.abs(p[1] - py) < 0.5);
      const d = flat ? `M${x0},${f1(py)}L${x1},${f1(py)}` : "M" + pts.map((p) => `${f1(p[0])},${f1(p[1])}`).join("L");
      marks.push(<path key={`r${s.id}`} d={d} fill="none" stroke={s.color} strokeWidth={1.5} strokeDasharray="5 4" />);
    }
    els.push(
      <clipPath key="clip" id={clipId}>
        <rect x={x0 - 5} y={y0 - 6} width={x1 - x0 + 10} height={y1 - y0 + 12} />
      </clipPath>,
    );
    els.push(<g key="marks" clipPath={`url(#${clipId})`}>{marks}</g>);
    // end labels
    labelled.forEach((c) => {
      const t = c.s.ts[c.i] as number;
      const x = isRef(c.s) ? x1 : isBar ? sx(t) + slotW / 2 : sx(t);
      if (!isRef(c.s)) els.push(<circle key={`ed${c.s.id}`} cx={f1(x)} cy={f1(c.y)} r={4} fill={c.s.color} stroke={T.surface} strokeWidth={2} />);
      els.push(<text key={`el${c.s.id}`} x={(isRef(c.s) ? x1 : x) + 8} y={f1(c.y + 4)} className="end-label">{c.s.name}</text>);
    });
    const staticLayer = <>{els}</>;

    const nearest = (px: number): number | null => {
      if (!xs.length) return null;
      const t = xFrom + ((px - x0 - (isBar ? slotW / 2 : 0)) / (x1 - x0)) * (xUntil - xFrom);
      let a = 0, b = xs.length - 1;
      while (b - a > 1) {
        const mid = (a + b) >> 1;
        if ((xs[mid] as number) < t) a = mid;
        else b = mid;
      }
      return Math.abs((xs[a] as number) - t) <= Math.abs((xs[b] as number) - t) ? (xs[a] as number) : (xs[b] as number);
    };
    return { T, W, H, x0, x1, y0, y1, xFrom, xUntil, sx, sy, slotW, isBar, data, visible, at, yOf, stepS, nearest, staticLayer };
  }, [m, o, chart, width, height, hidden, clipId]);

  const svgRef = useRef<SVGSVGElement>(null);
  const [hov, setHov] = useState<number | null>(null);
  const [sel, setSel] = useState<[number, number] | null>(null);
  const drag = useRef<number | null>(null);
  const emit = useHover((t) => {
    if (t == null || t < L.xFrom || t > L.xUntil) setHov(null);
    else setHov(L.nearest(L.sx(t)));
  });

  const px = (ev: PointerEvent): number => {
    const r = svgRef.current?.getBoundingClientRect();
    return Math.min(L.x1, Math.max(L.x0, ev.clientX - (r?.left ?? 0)));
  };
  const onMove = (ev: PointerEvent<SVGRectElement>) => {
    const p = px(ev);
    if (drag.current != null) setSel([Math.min(drag.current, p), Math.abs(p - drag.current)]);
    const t = L.nearest(p);
    setHov(t);
    if (t != null) {
      const rows = L.visible.map((s) => ({ color: s.color, name: s.name, value: L.at(s, t), unit: s.unit, dashed: Boolean(s.style.dashed) || isRef(s) }));
      tooltip.show(ev, fmtTime(t, m.tz, "tip", L.stepS), rows);
    } else tooltip.hide();
    emit(t);
  };
  const onLeave = () => {
    setHov(null);
    tooltip.hide();
    emit(null);
  };
  const onDown = (ev: PointerEvent<SVGRectElement>) => {
    if (!onZoom) return;
    drag.current = px(ev);
    ev.currentTarget.setPointerCapture(ev.pointerId);
  };
  const onUp = (ev: PointerEvent<SVGRectElement>) => {
    if (drag.current == null) return;
    const a = Math.min(drag.current, px(ev)), b = Math.max(drag.current, px(ev));
    drag.current = null;
    setSel(null);
    if (b - a < 8 || !onZoom) return;
    const inv = (p: number) => Math.round(L.xFrom + ((p - L.x0) / (L.x1 - L.x0)) * (L.xUntil - L.xFrom));
    onZoom(inv(a), inv(b));
  };

  const hx = hov == null ? null : +f1(L.isBar ? L.sx(hov) + L.slotW / 2 : L.sx(hov));
  return (
    <svg ref={svgRef} className="chart-svg" width={L.W} height={L.H} viewBox={`0 0 ${L.W} ${L.H}`}>
      {L.staticLayer}
      <line className="xhair" x1={hx ?? 0} x2={hx ?? 0} y1={L.y0} y2={L.y1} stroke={L.T.muted} strokeWidth={1} visibility={hx == null ? "hidden" : "visible"} />
      <rect className="zoom-sel" x={sel?.[0] ?? 0} y={L.y0} width={sel?.[1] ?? 0} height={L.y1 - L.y0} visibility={sel ? "visible" : "hidden"} />
      <g className="hover-dots">
        {hov != null && hx != null && !L.isBar &&
          L.data.map((s) => {
            const v = L.at(s, hov);
            if (v == null) return null;
            return <circle key={s.id} cx={hx} cy={f1(L.sy(L.yOf(s, hov, v)))} r={4} fill={s.color} stroke={L.T.surface} strokeWidth={2} />;
          })}
      </g>
      <rect className="hit" x={L.x0} y={L.y0} width={Math.max(0, L.x1 - L.x0)} height={L.y1 - L.y0} fill="transparent"
        onPointerMove={onMove} onPointerLeave={onLeave} onPointerDown={onDown} onPointerUp={onUp} />
    </svg>
  );
}
