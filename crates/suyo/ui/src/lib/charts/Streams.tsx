// Streams: one lane per series, each with its own y-scale and unit, so different units never
// share an axis. Hover syncs a crosshair across lanes (and other charts); dragging selects a
// window and the stats table below is recomputed for exactly that window. Threshold series are
// drawn in the lane with the same unit.

import { useMemo, useRef, useState, type PointerEvent, type ReactElement } from "react";
import { fmtDuration, fmtNum, fmtVal, niceTicks, textWidth } from "../format";
import { fmtTime, timeTicks } from "../time";
import { useHover } from "../hover";
import { tooltip } from "../Tooltip";
import { Empty } from "./Empty";
import { f1, isRef, lastVal, mean, vals, type MSeries, type Model } from "./model";

export interface StreamsProps {
  m: Model;
  width: number;
  height?: number;
  hidden: Set<number>;
  onZoom?: (from: number, until: number) => void;
}

export function Streams({ m, width, height, hidden, onZoom }: StreamsProps) {
  const L = useMemo(() => {
    const T = m.T;
    const lanes = m.series.filter((s) => !isRef(s) && !hidden.has(s.id));
    const refs = m.series.filter((s) => isRef(s) && !hidden.has(s.id));
    if (!lanes.length) return null;
    const W = Math.max(width, 260);
    const statsH = 60 + (lanes.length + 1) * 31; // head line + table
    const H = Math.max(lanes.length * 72 + 24, (height || 320) - statsH);
    const gap = 12, bottom = 22, top = 4, labelH = 16; // labelH: room above each lane's plot for its name
    const laneH = (H - top - bottom - gap * (lanes.length - 1)) / lanes.length;
    const scales = lanes.map((s) => {
      const a = vals(s).concat(refs.filter((r) => r.unit === s.unit).flatMap(vals));
      let lo = a.length ? Math.min(...a) : 0, hi = a.length ? Math.max(...a) : 1;
      if (hi <= lo) hi = lo + (Math.abs(lo) || 1);
      const ticks = niceTicks(lo, hi, Math.max(2, Math.floor((laneH - labelH) / 26)));
      return { lo: Math.min(lo, ticks[0] ?? lo), hi: Math.max(hi, ticks[ticks.length - 1] ?? hi), ticks };
    });
    const left = Math.max(...scales.flatMap((sc) => sc.ticks.map((t) => textWidth(fmtVal(t), 11)))) + 14;
    const x0 = left, x1 = W - 12;
    const sx = (t: number) => x0 + ((t - m.from) / Math.max(1, m.until - m.from)) * (x1 - x0);
    const inv = (p: number) => Math.round(m.from + ((p - x0) / (x1 - x0)) * (m.until - m.from));
    const laneTop = (k: number) => top + k * (laneH + gap);
    const sy = (k: number, v: number) => {
      const sc = scales[k] ?? { lo: 0, hi: 1 };
      return laneTop(k) + laneH - ((v - sc.lo) / (sc.hi - sc.lo)) * (laneH - labelH);
    };
    const els: ReactElement[] = [];
    lanes.forEach((s, k) => {
      const y0 = laneTop(k), sc = scales[k];
      sc?.ticks.forEach((t, j) => {
        const y = +f1(sy(k, t));
        els.push(<line key={`g${k}-${j}`} x1={x0} x2={x1} y1={y} y2={y} stroke={T.grid} />);
        els.push(<text key={`gl${k}-${j}`} x={x0 - 8} y={y + 4} className="tick" textAnchor="end">{fmtVal(t)}</text>);
      });
      els.push(
        <text key={`ln${k}`} x={x0 + 6} y={y0 + 12} className="lane-label" fill={s.color}>
          {s.name}
          {s.unit ? <> <tspan className="muted">{s.unit}</tspan></> : null}
        </text>,
      );
      refs.filter((r) => r.unit === s.unit).forEach((r) => {
        const v = lastVal(r);
        if (v != null) els.push(<line key={`r${k}-${r.id}`} x1={x0} x2={x1} y1={f1(sy(k, v))} y2={f1(sy(k, v))} stroke={r.color} strokeDasharray="5 4" />);
      });
      let d = "", pen = false;
      s.ts.forEach((t, i) => {
        const v = s.v[i];
        if (v == null) { pen = false; return; }
        d += `${pen ? "L" : "M"}${f1(sx(t))},${f1(sy(k, v))}`;
        pen = true;
      });
      els.push(<path key={`p${k}`} d={d} fill="none" stroke={s.color} strokeWidth={1.5} strokeLinejoin="round" />);
    });
    const yEnd = laneTop(lanes.length - 1) + laneH;
    els.push(<line key="axis" x1={x0} x2={x1} y1={yEnd} y2={yEnd} stroke={T.axis} />);
    timeTicks(m.from, m.until, m.tz, Math.max(2, Math.floor((x1 - x0) / 84))).forEach(([t, label], i) => {
      const x = sx(t);
      if (x > x0 + 10 && x < x1 - 10) els.push(<text key={`x${i}`} x={f1(x)} y={yEnd + 16} className="tick" textAnchor="middle">{label}</text>);
    });
    const nearestI = (s: MSeries, t: number): number => {
      const ts = s.ts;
      if (!ts.length) return -1;
      let lo = 0, hi = ts.length - 1;
      while (hi - lo > 1) {
        const mid = (lo + hi) >> 1;
        if ((ts[mid] as number) < t) lo = mid;
        else hi = mid;
      }
      return Math.abs((ts[lo] as number) - t) <= Math.abs((ts[hi] as number) - t) ? lo : hi;
    };
    return { T, W, H, x0, x1, top, yEnd, lanes, sx, sy, inv, nearestI, staticLayer: <>{els}</> };
  }, [m, width, height, hidden]);

  const svgRef = useRef<SVGSVGElement>(null);
  const [hov, setHov] = useState<number | null>(null);
  const [drag, setDrag] = useState<[number, number] | null>(null); // pixels while dragging
  const [win, setWin] = useState<[number, number] | null>(null); // selected window, epoch seconds
  const dragStart = useRef<number | null>(null);
  const emit = useHover((t) => setHov(t == null || t < m.from || t > m.until ? null : t));

  if (!L) return <Empty msg="No streams selected" />;
  const { T, W, H, x0, x1, top, yEnd, lanes, sx, sy, inv, nearestI } = L;

  const px = (ev: PointerEvent) => {
    const r = svgRef.current?.getBoundingClientRect();
    return Math.min(x1, Math.max(x0, ev.clientX - (r?.left ?? 0)));
  };
  const rowsAt = (t: number) =>
    lanes.map((s) => {
      const i = nearestI(s, t);
      return { color: s.color, name: s.name, value: i < 0 ? null : (s.v[i] ?? null), unit: s.unit };
    });
  const onMove = (ev: PointerEvent<SVGRectElement>) => {
    const p = px(ev);
    if (dragStart.current != null) setDrag([dragStart.current, p]);
    const t = inv(p);
    setHov(t);
    tooltip.show(ev, fmtTime(t, m.tz, "tip", m.step), rowsAt(t));
    emit(t);
  };
  const onLeave = () => {
    setHov(null);
    tooltip.hide();
    emit(null);
  };
  const onDown = (ev: PointerEvent<SVGRectElement>) => {
    dragStart.current = px(ev);
    ev.currentTarget.setPointerCapture(ev.pointerId);
  };
  const onUp = (ev: PointerEvent<SVGRectElement>) => {
    const d = dragStart.current;
    if (d == null) return;
    const a = Math.min(d, px(ev)), b = Math.max(d, px(ev));
    dragStart.current = null;
    setDrag(null);
    if (b - a < 6) return;
    setWin([inv(a), inv(b)]);
  };
  const clear = () => setWin(null);

  // selection rectangle: live drag, else the stored window
  const selPx: [number, number] | null = drag ? [Math.min(drag[0], drag[1]), Math.abs(drag[1] - drag[0])] : win ? [sx(win[0]), sx(win[1]) - sx(win[0])] : null;
  const hx = hov == null ? null : +f1(sx(hov));

  // stats for the window (whole range when none)
  const [a, b] = win ?? [null, null];
  const inWin = (t: number) => a == null || b == null || (t >= a && t <= b);
  return (
    <>
      <svg ref={svgRef} className="chart-svg" width={W} height={H} viewBox={`0 0 ${W} ${H}`}>
        {L.staticLayer}
        <rect className="win-sel" x={selPx?.[0] ?? 0} y={top} width={selPx?.[1] ?? 0} height={yEnd - top} visibility={selPx ? "visible" : "hidden"} />
        <line className="xhair" x1={hx ?? 0} x2={hx ?? 0} y1={top} y2={yEnd} stroke={T.muted} visibility={hx == null ? "hidden" : "visible"} />
        <g className="hover-dots">
          {hov != null &&
            lanes.map((s, k) => {
              const i = nearestI(s, hov);
              const v = i < 0 ? null : s.v[i];
              if (v == null) return null;
              return <circle key={s.id} cx={f1(sx(s.ts[i] as number))} cy={f1(sy(k, v))} r={3.5} fill={s.color} stroke={T.surface} strokeWidth={2} />;
            })}
        </g>
        <rect className="hit" x={x0} y={top} width={x1 - x0} height={yEnd - top} fill="transparent"
          onPointerMove={onMove} onPointerLeave={onLeave} onPointerDown={onDown} onPointerUp={onUp} onDoubleClick={clear} />
      </svg>
      <div className="stream-stats">
        <div className="stream-stats-head">
          <span className="strong">{a == null || b == null ? "Whole range" : `${fmtTime(a, m.tz, "tip", m.step)} → ${fmtTime(b, m.tz, "tip", m.step)}`}</span>
          {a == null || b == null ? (
            <span className="muted"> · drag across the chart to measure a section</span>
          ) : (
            <>
              <span className="muted">{` · ${fmtDuration(b - a)}`}</span>{" "}
              <button className="btn ghost small" onClick={clear}>Clear</button>
              {onZoom && <button className="btn ghost small" onClick={() => onZoom(a, b)}>Zoom to selection</button>}
            </>
          )}
        </div>
        <div className="table-scroll">
          <table className="data-table">
            <thead>
              <tr>
                <th>Stream</th>
                <th className="num">Min</th>
                <th className="num">Average</th>
                <th className="num">Max</th>
                <th className="num">Change</th>
              </tr>
            </thead>
            <tbody>
              {lanes.map((s) => {
                const a2: number[] = [];
                s.ts.forEach((tt, i) => { const v = s.v[i]; if (v != null && inWin(tt)) a2.push(v); });
                const u = s.unit ? ` ${s.unit}` : "";
                const f = (v: number | null) => (v == null ? "—" : fmtNum(v) + u);
                const ch = a2.length > 1 ? (a2[a2.length - 1] as number) - (a2[0] as number) : null;
                return (
                  <tr key={s.id}>
                    <td><span className="key key-line" style={{ background: s.color }} /><span className="nowrap">{s.name}</span></td>
                    <td className="num">{f(a2.length ? Math.min(...a2) : null)}</td>
                    <td className="num strong">{f(a2.length ? mean(a2) : null)}</td>
                    <td className="num">{f(a2.length ? Math.max(...a2) : null)}</td>
                    <td className="num">{ch == null ? "—" : `${ch >= 0 ? "+" : "−"}${fmtNum(Math.abs(ch))}${u}`}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>
    </>
  );
}
