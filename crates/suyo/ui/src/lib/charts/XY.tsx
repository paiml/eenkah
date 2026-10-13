// Two series against each other: scatter (with fit line) and quadrant (split at medians).

import { useRef, type PointerEvent, type ReactElement } from "react";
import { fmtNum, fmtVal, niceTicks, textWidth } from "../format";
import { fmtTime } from "../time";
import { tooltip } from "../Tooltip";
import { Empty } from "./Empty";
import { f1, first, isRef, last, mean, median, optNum, optStr, type ChartOpts, type MSeries, type Model } from "./model";

type Pt = [number, number, number]; // x, y, t

function joinPoints(a: MSeries, b: MSeries): Pt[] {
  const ymap = new Map(b.ts.map((t, i) => [t, b.v[i]]));
  const out: Pt[] = [];
  a.ts.forEach((t, i) => {
    const x = a.v[i], y = ymap.get(t);
    if (x != null && y != null) out.push([x, y, t]);
  });
  return out;
}

function strength(r: number): string {
  const a = Math.abs(r);
  const s = a < 0.1 ? "none" : a < 0.3 ? "weak" : a < 0.5 ? "moderate" : "strong";
  return a < 0.1 ? "no clear relationship" : `${s} ${r > 0 ? "positive" : "negative"}`;
}

interface Frame {
  W: number;
  H: number;
  x0: number;
  x1: number;
  y0: number;
  y1: number;
  left: number;
  xt: number[];
  yt: number[];
  X: (v: number) => number;
  Y: (v: number) => number;
}

function frame(pts: Pt[], width: number, H: number): Frame {
  const W = Math.max(width, 240);
  const xt = niceTicks(Math.min(...pts.map((p) => p[0])), Math.max(...pts.map((p) => p[0])), Math.max(3, Math.floor(W / 90)));
  const yt = niceTicks(Math.min(...pts.map((p) => p[1])), Math.max(...pts.map((p) => p[1])), Math.max(3, Math.floor(H / 50)));
  const left = Math.max(...yt.map((t) => textWidth(fmtVal(t), 11))) + 16, bottom = 38, top = 24, right = 16;
  const x0 = left, x1 = W - right, y0 = top, y1 = H - bottom;
  const X = (v: number) => x0 + ((v - first(xt)) / (last(xt) - first(xt) || 1)) * (x1 - x0);
  const Y = (v: number) => y1 - ((v - first(yt)) / (last(yt) - first(yt) || 1)) * (y1 - y0);
  return { W, H, x0, x1, y0, y1, left, xt, yt, X, Y };
}

function useNearestTip(pts: Pt[], F: Frame | null, m: Model, xs: MSeries | undefined, ys: MSeries | undefined) {
  const ref = useRef<SVGSVGElement>(null);
  const onMove = (ev: PointerEvent<SVGSVGElement>) => {
    if (!F || !xs || !ys) return;
    const rc = ref.current?.getBoundingClientRect();
    const mxp = ev.clientX - (rc?.left ?? 0), myp = ev.clientY - (rc?.top ?? 0);
    let best: Pt | null = null, bd = 24 * 24;
    for (const p of pts) {
      const d = (F.X(p[0]) - mxp) ** 2 + (F.Y(p[1]) - myp) ** 2;
      if (d < bd) { bd = d; best = p; }
    }
    if (!best) return tooltip.hide();
    const c = m.T.pal[0] ?? "";
    tooltip.show(ev, fmtTime(best[2], m.tz, "tip", xs.step), [{ color: c, name: xs.name, value: best[0], unit: xs.unit }, { color: c, name: ys.name, value: best[1], unit: ys.unit }]);
  };
  return { ref, onMove, onLeave: () => tooltip.hide() };
}

export function Scatter({ m, o, width, height }: { m: Model; o: ChartOpts; width: number; height?: number }) {
  void o;
  const T = m.T;
  const [sxS, syS] = m.series.filter((s) => !isRef(s));
  const pts = sxS && syS ? joinPoints(sxS, syS) : [];
  const F = pts.length >= 2 ? frame(pts, width, (height || 260) - 22) : null;
  const tip = useNearestTip(pts, F, m, sxS, syS);
  if (!sxS || !syS) return <Empty msg="Scatter needs two targets: x first, then y" />;
  if (!F) return <Empty msg="Not enough overlapping points" />;
  const { W, H, x0, x1, y0, y1, left, xt, yt, X, Y } = F;
  const n = pts.length, mx = mean(pts.map((p) => p[0])), my = mean(pts.map((p) => p[1]));
  let sxx = 0, sxy = 0, syy = 0;
  for (const p of pts) { sxx += (p[0] - mx) ** 2; sxy += (p[0] - mx) * (p[1] - my); syy += (p[1] - my) ** 2; }
  const slope = sxx ? sxy / sxx : 0, icpt = my - slope * mx, r = sxx && syy ? sxy / Math.sqrt(sxx * syy) : 0;
  const a = first(xt), b = last(xt);
  return (
    <>
      <div className="chart-note">{`r = ${r.toFixed(2)} · ${strength(r)} · ${n} points`}</div>
      <svg ref={tip.ref} className="chart-svg" width={W} height={H} viewBox={`0 0 ${W} ${H}`} onPointerMove={tip.onMove} onPointerLeave={tip.onLeave}>
        {yt.map((t, i) => (
          <g key={`y${i}`}>
            <line x1={x0} x2={x1} y1={f1(Y(t))} y2={f1(Y(t))} stroke={T.grid} />
            <text x={x0 - 8} y={f1(Y(t) + 4)} className="tick" textAnchor="end">{fmtVal(t)}</text>
          </g>
        ))}
        {xt.map((t, i) => (
          <g key={`x${i}`}>
            <line x1={f1(X(t))} x2={f1(X(t))} y1={y0} y2={y1} stroke={T.grid} />
            <text x={f1(X(t))} y={y1 + 16} className="tick" textAnchor="middle">{fmtVal(t)}</text>
          </g>
        ))}
        <text x={x1} y={H - 6} className="axis-title" textAnchor="end">{sxS.name} →</text>
        <text x={x0 - left + 2} y={y0 - 8} className="axis-title">↑ {syS.name}</text>
        {pts.map((p, i) => (
          <circle key={i} cx={f1(X(p[0]))} cy={f1(Y(p[1]))} r={4} fill={T.pal[0]} fillOpacity={0.75} stroke={T.surface} strokeWidth={1.5} />
        ))}
        {sxx ? <path d={`M${f1(X(a))},${f1(Y(slope * a + icpt))}L${f1(X(b))},${f1(Y(slope * b + icpt))}`} stroke={T.ink2} strokeWidth={1.5} strokeDasharray="6 4" fill="none" /> : null}
      </svg>
    </>
  );
}

// Quadrant: x first, then y, split at their own medians over the range. Options: xBetter/yBetter
// ("up" | "down") tint the good/bad quadrants; q1..q4 label them (q1 = top-right, q2 = top-left,
// q3 = bottom-left, q4 = bottom-right); recent = how many of the newest points to emphasise (7).
export function Quadrant({ m, o, width, height }: { m: Model; o: ChartOpts; width: number; height?: number }) {
  const T = m.T;
  const [qx, qy] = m.series.filter((s) => !isRef(s));
  const pts = qx && qy ? joinPoints(qx, qy) : [];
  const F = pts.length >= 4 ? frame(pts, Math.max(width, 260), (height || 280) - 22) : null;
  const tip = useNearestTip(pts, F, m, qx, qy);
  if (!qx || !qy) return <Empty msg="Quadrant needs two targets: x first, then y" />;
  if (!F) return <Empty msg="Not enough overlapping points" />;
  const { W, H, x0, x1, y0, y1, left, xt, yt, X, Y } = F;
  const mx = median(pts.map((p) => p[0])), my = median(pts.map((p) => p[1]));
  const good = (dir: string | undefined, hiSide: boolean): boolean | null => (dir === "up" ? hiSide : dir === "down" ? !hiSide : null);
  const quads: [boolean, boolean, string | undefined][] = [[true, true, optStr(o, "q1")], [false, true, optStr(o, "q2")], [false, false, optStr(o, "q3")], [true, false, optStr(o, "q4")]];
  const els: ReactElement[] = [];
  quads.forEach(([xh, yh, label], k) => {
    const gx = good(optStr(o, "xBetter"), xh), gy = good(optStr(o, "yBetter"), yh);
    const tint = gx === true && gy === true ? T.good : gx === false && gy === false ? T.bad : null;
    const rx = xh ? X(mx) : x0, ry = yh ? y0 : Y(my), rw = xh ? x1 - X(mx) : X(mx) - x0, rh = yh ? Y(my) - y0 : y1 - Y(my);
    if (tint) els.push(<rect key={`q${k}`} x={f1(rx)} y={f1(ry)} width={f1(Math.max(0, rw))} height={f1(Math.max(0, rh))} fill={tint} fillOpacity={0.07} />);
    if (label) els.push(<text key={`ql${k}`} x={f1(xh ? x1 - 6 : x0 + 6)} y={f1(yh ? y0 + 14 : y1 - 8)} className="quad-label" textAnchor={xh ? "end" : "start"}>{label}</text>);
  });
  const recent = optNum(o, "recent") ?? 7;
  const sorted = [...pts].sort((a, b) => a[2] - b[2]);
  const cut = sorted.length - recent;
  const lp = last(sorted);
  const where = quads.find(([xh, yh]) => (lp[0] >= mx) === xh && (lp[1] >= my) === yh);
  const unitOf = (s: MSeries) => (s.unit ? ` (${s.unit})` : "");
  return (
    <>
      <div className="chart-note">{`Lines are your medians (${fmtNum(mx)} · ${fmtNum(my)}) · ${pts.length} points · newest ${Math.min(recent, pts.length)} in colour${where?.[2] ? ` · latest: ${where[2]}` : ""}`}</div>
      <svg ref={tip.ref} className="chart-svg" width={W} height={H} viewBox={`0 0 ${W} ${H}`} onPointerMove={tip.onMove} onPointerLeave={tip.onLeave}>
        {els}
        {yt.map((t, i) => (
          <g key={`y${i}`}>
            <line x1={x0} x2={x1} y1={f1(Y(t))} y2={f1(Y(t))} stroke={T.grid} />
            <text x={x0 - 8} y={f1(Y(t) + 4)} className="tick" textAnchor="end">{fmtVal(t)}</text>
          </g>
        ))}
        {xt.map((t, i) => (
          <text key={`x${i}`} x={f1(X(t))} y={y1 + 16} className="tick" textAnchor="middle">{fmtVal(t)}</text>
        ))}
        <line x1={f1(X(mx))} x2={f1(X(mx))} y1={y0} y2={y1} stroke={T.ink2} strokeDasharray="4 4" />
        <line x1={x0} x2={x1} y1={f1(Y(my))} y2={f1(Y(my))} stroke={T.ink2} strokeDasharray="4 4" />
        <text x={x1} y={H - 6} className="axis-title" textAnchor="end">{qx.name}{unitOf(qx)} →</text>
        <text x={x0 - left + 2} y={y0 - 8} className="axis-title">↑ {qy.name}{unitOf(qy)}</text>
        {sorted.map((p, i) => {
          const isNew = i >= cut, isLast = i === sorted.length - 1;
          return <circle key={i} cx={f1(X(p[0]))} cy={f1(Y(p[1]))} r={isLast ? 6 : 4} fill={isNew ? T.pal[0] : T.dim} fillOpacity={isNew ? 0.9 : 0.6} stroke={T.surface} strokeWidth={1.5} />;
        })}
        <text x={f1(X(lp[0]) + 9)} y={f1(Y(lp[1]) - 7)} className="end-label">latest</text>
      </svg>
    </>
  );
}
