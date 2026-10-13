// Histogram (distribution of values) and slope (first vs last bucket per series).

import { useRef, type ReactElement } from "react";
import { fmtNum, fmtVal, niceTicks, textWidth } from "../format";
import { fmtTime } from "../time";
import { tooltip } from "../Tooltip";
import { Empty } from "./Empty";
import { barPath, f1, isRef, last, sharedUnit, vals, type MSeries, type Model } from "./model";

export function Histogram({ m, width, height, hidden }: { m: Model; width: number; height?: number; hidden: Set<number> }) {
  const T = m.T;
  const svgRef = useRef<SVGSVGElement>(null);
  const list = m.series.filter((s) => !isRef(s) && !hidden.has(s.id)).slice(0, 3);
  const all = list.flatMap(vals);
  if (all.length < 2) return <Empty msg="Not enough data" />;
  const lo = Math.min(...all), hi = Math.max(...all);
  const nb = Math.max(5, Math.min(40, Math.ceil(Math.sqrt(all.length))));
  const bt = niceTicks(lo, hi, nb);
  const bw = (bt[1] ?? 0) - (bt[0] ?? 0) || 1;
  const start = Math.floor(lo / bw) * bw;
  const bins = Math.max(1, Math.ceil((hi - start) / bw + 1e-9)) + (hi === start + Math.ceil((hi - start) / bw) * bw ? 1 : 0);
  const counts = list.map((s) => {
    const c = new Array<number>(bins).fill(0);
    for (const v of vals(s)) {
      const i = Math.min(bins - 1, Math.floor((v - start) / bw));
      c[i] = (c[i] ?? 0) + 1;
    }
    return c;
  });
  const maxC = Math.max(...counts.flat());
  const W = Math.max(width, 240), H = height || 260;
  const yt = niceTicks(0, maxC, Math.max(2, Math.floor(H / 50)));
  const left = Math.max(...yt.map((t) => textWidth(fmtVal(t), 11))) + 14;
  const x0 = left, x1 = W - 14, y0 = 10, y1 = H - 24;
  const X = (v: number) => x0 + ((v - start) / (bins * bw)) * (x1 - x0);
  const Y = (c: number) => y1 - (c / last(yt)) * (y1 - y0);
  const els: ReactElement[] = [];
  yt.forEach((t, i) => {
    els.push(<line key={`g${i}`} x1={x0} x2={x1} y1={f1(Y(t))} y2={f1(Y(t))} stroke={t === 0 ? T.axis : T.grid} />);
    els.push(<text key={`gl${i}`} x={x0 - 8} y={f1(Y(t) + 4)} className="tick" textAnchor="end">{fmtVal(t)}</text>);
  });
  const every = Math.ceil((bins + 1) / Math.max(2, Math.floor((x1 - x0) / 56)));
  for (let i = 0; i <= bins; i += every) els.push(<text key={`x${i}`} x={f1(X(start + i * bw))} y={y1 + 16} className="tick" textAnchor="middle">{fmtVal(start + i * bw)}</text>);
  const unit = sharedUnit(list);
  list.forEach((s: MSeries, k) => {
    const c = counts[k] ?? [];
    if (list.length === 1) {
      c.forEach((n, i) => {
        if (!n) return;
        const x = X(start + i * bw) + 1, w = Math.max(1, X(start + (i + 1) * bw) - X(start + i * bw) - 2);
        els.push(<path key={`b${k}-${i}`} d={barPath(x, Y(n), w, y1 - Y(n), Math.min(4, w / 2), true)} fill={s.color} />);
      });
    } else {
      let d = `M${X(start)},${y1}`;
      c.forEach((n, i) => { d += `V${f1(Y(n))}H${f1(X(start + (i + 1) * bw))}`; });
      d += `V${y1}`;
      els.push(<path key={`s${k}`} d={d} fill={s.color} fillOpacity={0.1} stroke={s.color} strokeWidth={2} strokeLinejoin="round" />);
    }
  });
  return (
    <svg ref={svgRef} className="chart-svg" width={W} height={H} viewBox={`0 0 ${W} ${H}`}>
      {els}
      <rect className="hit" x={x0} y={y0} width={x1 - x0} height={y1 - y0} fill="transparent"
        onPointerMove={(ev) => {
          const r = svgRef.current?.getBoundingClientRect();
          const i = Math.floor(((ev.clientX - (r?.left ?? 0) - x0) / (x1 - x0)) * bins);
          if (i < 0 || i >= bins) return tooltip.hide();
          const a = start + i * bw;
          tooltip.show(ev, `${fmtVal(a)}–${fmtVal(a + bw)}${unit ? " " + unit : ""}`, list.map((s, k) => ({ color: s.color, name: s.name, value: counts[k]?.[i] ?? null, unit: "points" })));
        }}
        onPointerLeave={() => tooltip.hide()} />
    </svg>
  );
}

interface SlopeRow {
  s: MSeries;
  a: number;
  b: number;
  ta: number;
  tb: number;
}

export function Slope({ m, width, height, hidden }: { m: Model; width: number; height?: number; hidden: Set<number> }) {
  const T = m.T;
  const list = m.series.filter((s) => !isRef(s) && !hidden.has(s.id));
  const rows: SlopeRow[] = [];
  for (const s of list) {
    const idx = s.v.map((v, i) => (v != null ? i : -1)).filter((i) => i >= 0);
    if (idx.length < 2) continue;
    const i0 = idx[0] as number, i1 = last(idx);
    rows.push({ s, a: s.v[i0] as number, b: s.v[i1] as number, ta: s.ts[i0] as number, tb: s.ts[i1] as number });
  }
  const r0 = rows[0];
  if (!r0) return <Empty msg="Slope needs two periods — set a range covering two buckets (e.g. step=1mon over two months)" />;
  const W = Math.max(width, 260), H = height || 260;
  const all = rows.flatMap((r) => [r.a, r.b]);
  let lo = Math.min(...all), hi = Math.max(...all);
  if (lo === hi) { lo -= 1; hi += 1; }
  const lw = Math.min(170, Math.max(...rows.map((r) => textWidth(`${fmtNum(r.a)} ${r.s.name}`, 11.5))) + 14);
  const x0 = lw + 4, x1 = W - lw - 4, y0 = 30, y1 = H - 10 - 7 * Math.max(0, rows.length - 1);
  const Y = (v: number) => y1 - ((v - lo) / (hi - lo)) * (y1 - y0);
  // keep labels apart and inside the plot
  const place = (ys: number[]): number[] => {
    const order: [number, number][] = ys.map((y, i) => [y, i]);
    order.sort((a, b) => a[0] - b[0]);
    for (let k = 1; k < order.length; k++) (order[k] as [number, number])[0] = Math.max((order[k] as [number, number])[0], (order[k - 1] as [number, number])[0] + 14);
    const over = order.length ? last(order)[0] - (H - 6) : 0;
    if (over > 0) for (const o2 of order) o2[0] -= over;
    for (let k = order.length - 2; k >= 0; k--) (order[k] as [number, number])[0] = Math.min((order[k] as [number, number])[0], (order[k + 1] as [number, number])[0] - 14);
    const out: number[] = [];
    for (const [y, i] of order) out[i] = y;
    return out;
  };
  const la = place(rows.map((r) => Y(r.a))), lb = place(rows.map((r) => Y(r.b)));
  return (
    <svg className="chart-svg" width={W} height={H} viewBox={`0 0 ${W} ${H}`}>
      <text x={x0} y={14} className="tick strong" textAnchor="middle">{fmtTime(r0.ta, m.tz, "bar", r0.s.step)}</text>
      <text x={x1} y={14} className="tick strong" textAnchor="middle">{fmtTime(r0.tb, m.tz, "bar", r0.s.step)}</text>
      <line x1={x0} x2={x0} y1={y0 - 8} y2={y1} stroke={T.axis} />
      <line x1={x1} x2={x1} y1={y0 - 8} y2={y1} stroke={T.axis} />
      {rows.map((r, i) => {
        const c = r.s.color;
        const pct = r.a ? ((r.b - r.a) / Math.abs(r.a)) * 100 : 0;
        return (
          <g key={r.s.id}>
            <path d={`M${x0},${f1(Y(r.a))}L${x1},${f1(Y(r.b))}`} stroke={c} strokeWidth={2.5} fill="none" />
            <circle cx={x0} cy={f1(Y(r.a))} r={4.5} fill={c} stroke={T.surface} strokeWidth={2} />
            <circle cx={x1} cy={f1(Y(r.b))} r={4.5} fill={c} stroke={T.surface} strokeWidth={2} />
            <text x={x0 - 10} y={f1((la[i] ?? 0) + 4)} className="end-label" textAnchor="end">
              {r.s.name} <tspan className="v">{fmtNum(r.a)}</tspan>
            </text>
            <text x={x1 + 10} y={f1((lb[i] ?? 0) + 4)} className="end-label">
              <tspan className="v">{fmtNum(r.b)}</tspan> <tspan className="muted">{`${pct >= 0 ? "+" : "−"}${Math.abs(pct).toFixed(0)}%`}</tspan>
            </text>
          </g>
        );
      })}
    </svg>
  );
}
