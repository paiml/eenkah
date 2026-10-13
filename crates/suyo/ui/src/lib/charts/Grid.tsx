// Cell charts: calendar heatmap, hour × weekday heatmap, waffle (parts of a whole), status lights.

import { useRef, type ReactElement } from "react";
import { fmtNum, fmtVal } from "../format";
import { fmtTime, tzParts } from "../time";
import { tooltip } from "../Tooltip";
import { Empty } from "./Empty";
import { isRef, lastIdx, mean, optNum, spanLabel, type ChartOpts, type Model, type Tokens } from "./model";

function seqColor(T: Tokens, v: number | null | undefined, breaks: number[]): string | null {
  if (v == null) return null;
  let k = 0;
  while (k < breaks.length && v > (breaks[k] as number)) k++;
  return T.seq[Math.min(5, k + 1)] ?? null;
}

function quantileBreaks(a: number[]): number[] {
  const s = [...a].sort((x, y) => x - y);
  if (!s.length) return [];
  return [0.2, 0.4, 0.6, 0.8].map((q) => s[Math.floor(q * (s.length - 1))] as number);
}

function HeatLegend({ T, breaks, unit }: { T: Tokens; breaks: number[]; unit: string }) {
  return (
    <div className="heat-legend">
      <span>Less</span>
      {[1, 2, 3, 4, 5].map((i) => <span key={i} className="cell" style={{ background: T.seq[i] }} />)}
      <span>More</span>
      {breaks.length > 0 && <span className="muted">{` · quintiles: ${breaks.map((b) => fmtVal(b)).join(" / ")}${unit ? " " + unit : ""}`}</span>}
    </div>
  );
}

export function Calendar({ m, width }: { m: Model; width: number }) {
  const T = m.T;
  const s = m.series.find((x) => !isRef(x));
  const days = s ? s.ts.map((t, i) => ({ t, v: s.v[i] ?? null, p: tzParts(t, m.tz) })) : [];
  const d0 = days[0];
  if (!s || !d0) return <Empty msg="No data" />;
  const breaks = quantileBreaks(days.map((d) => d.v).filter((v): v is number => v != null && v > 0));
  const offset = (d0.p.wd + 6) % 7; // Monday = 0
  const weeks = Math.ceil((days.length + offset) / 7);
  const W = Math.max(width, 240);
  const labelW = 28;
  const gap = W < 500 ? 1 : 2;
  const cell = Math.max(3, Math.min(16, Math.floor((W - labelW - 4) / weeks) - gap));
  const pitch = cell + gap;
  const H = 18 + 7 * pitch;
  const els: ReactElement[] = [];
  ["Mon", "", "Wed", "", "Fri", "", ""].forEach((d, i) => {
    if (d) els.push(<text key={`wd${i}`} x={0} y={18 + i * pitch + cell - 2} className="tick">{d}</text>);
  });
  let lastMonth = -1;
  const fillOf = (v: number | null): string => (v != null && v > 0 ? (seqColor(T, v, breaks) ?? T.grid) : T.grid);
  days.forEach((d, i) => {
    const k = i + offset, wk = Math.floor(k / 7), wd = k % 7;
    const x = labelW + wk * pitch, y = 18 + wd * pitch;
    if (d.p.m !== lastMonth && wd <= 1) {
      els.push(<text key={`m${i}`} x={x} y={11} className="tick">{d.p.monthShort}</text>);
      lastMonth = d.p.m;
    }
    els.push(<rect key={`c${i}`} x={x} y={y} width={cell} height={cell} rx={2} fill={fillOf(d.v)} data-i={i} />);
  });
  const SW = Math.min(W, labelW + weeks * pitch + 4);
  return (
    <>
      <svg className="chart-svg" width={SW} height={H} viewBox={`0 0 ${SW} ${H}`}
        onPointerMove={(ev) => {
          const el = ev.target instanceof Element ? ev.target.closest("[data-i]") : null;
          const d = el ? days[Number(el.getAttribute("data-i"))] : undefined;
          if (!d) return tooltip.hide();
          tooltip.show(ev, fmtTime(d.t, m.tz, "tip", 86400), [{ color: fillOf(d.v), name: s.name, value: d.v, unit: s.unit }]);
        }}
        onPointerLeave={() => tooltip.hide()}>
        {els}
      </svg>
      <HeatLegend T={T} breaks={breaks} unit={s.unit} />
    </>
  );
}

const NAMES = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

export function Hourly({ m, width, height }: { m: Model; width: number; height?: number }) {
  const T = m.T;
  const s = m.series.find((x) => !isRef(x));
  if (!s) return <Empty msg="No data" />;
  const sum = Array.from({ length: 7 }, () => new Array<number>(24).fill(0));
  const cnt = Array.from({ length: 7 }, () => new Array<number>(24).fill(0));
  s.ts.forEach((t, i) => {
    const v = s.v[i];
    if (v == null) return;
    const p = tzParts(t, m.tz);
    const wd = (p.wd + 6) % 7;
    const rs = sum[wd] as number[], rc = cnt[wd] as number[];
    rs[p.H] = (rs[p.H] ?? 0) + v;
    rc[p.H] = (rc[p.H] ?? 0) + 1;
  });
  const avg = sum.map((r, a) => r.map((x, b) => { const c = cnt[a]?.[b] ?? 0; return c ? x / c : null; }));
  const flat = avg.flat().filter((v): v is number => v != null);
  if (!flat.length) return <Empty msg="No data" />;
  const breaks = quantileBreaks(flat);
  const W = Math.max(width, 260), labelW = 34;
  const cw = Math.max(8, Math.floor((W - labelW - 4) / 24) - 2), pitchX = cw + 2;
  const ch = Math.max(12, Math.min(26, Math.floor(((height || 230) - 40) / 7) - 2)), pitchY = ch + 2;
  const H = 20 + 7 * pitchY;
  const els: ReactElement[] = [];
  NAMES.forEach((n, r) => els.push(<text key={`n${r}`} x={0} y={20 + r * pitchY + ch / 2 + 4} className="tick">{n}</text>));
  for (let hr = 0; hr < 24; hr += 3) els.push(<text key={`h${hr}`} x={labelW + hr * pitchX} y={12} className="tick">{String(hr).padStart(2, "0")}h</text>);
  avg.forEach((row, r) => row.forEach((v, c) => {
    els.push(<rect key={`c${r}-${c}`} x={labelW + c * pitchX} y={20 + r * pitchY} width={cw} height={ch} rx={2} fill={seqColor(T, v, breaks) || T.grid} data-r={r} data-c={c} />);
  }));
  const SW = Math.min(W, labelW + 24 * pitchX + 2);
  return (
    <>
      <svg className="chart-svg" width={SW} height={H} viewBox={`0 0 ${SW} ${H}`}
        onPointerMove={(ev) => {
          const el = ev.target instanceof Element ? ev.target.closest("[data-r]") : null;
          if (!el) return tooltip.hide();
          const r = Number(el.getAttribute("data-r")), c = Number(el.getAttribute("data-c"));
          const v = avg[r]?.[c] ?? null;
          tooltip.show(ev, `${NAMES[r] ?? ""} ${String(c).padStart(2, "0")}:00–${String(c + 1).padStart(2, "0")}:00`, [
            { color: seqColor(T, v, breaks) || T.grid, name: `avg ${s.name}`, value: v, unit: s.unit },
            { color: "transparent", name: "samples", value: cnt[r]?.[c] ?? 0, unit: "" },
          ]);
        }}
        onPointerLeave={() => tooltip.hide()}>
        {els}
      </svg>
      <HeatLegend T={T} breaks={breaks} unit={s.unit} />
    </>
  );
}

export function Waffle({ m, width, height }: { m: Model; width: number; height?: number }) {
  const ref = useRef<SVGSVGElement>(null);
  const list = m.series.filter((s) => !isRef(s));
  // latest timestamp where any series has data
  let t = -Infinity;
  for (const s of list) {
    const i = lastIdx(s);
    if (i >= 0) t = Math.max(t, s.ts[i] as number);
  }
  const parts = list.map((s) => ({ s, v: Math.max(0, s.v[s.ts.indexOf(t)] ?? 0) }));
  const total = parts.reduce((a, p) => a + p.v, 0);
  if (!total) return <Empty msg="No data" />;
  // largest-remainder rounding to exactly 100 cells
  const raw = parts.map((p) => (p.v / total) * 100);
  const cells = raw.map(Math.floor);
  let left = 100 - cells.reduce((a, b) => a + b, 0);
  raw.map((r, i): [number, number] => [r - Math.floor(r), i]).sort((a, b) => b[0] - a[0]).forEach(([, i]) => {
    if (left-- > 0) cells[i] = (cells[i] ?? 0) + 1;
  });
  const size = Math.max(120, Math.min((height || 230) - 20, width * 0.5, 240));
  const c = size / 10, gap = 2;
  const rects: ReactElement[] = [];
  let k = 0;
  cells.forEach((n, pi) => {
    for (let j = 0; j < n; j++, k++) {
      const col = Math.floor(k / 10), row = k % 10;
      rects.push(<rect key={k} x={col * c} y={(9 - row) * c} width={c - gap} height={c - gap} rx={2} fill={parts[pi]?.s.color} data-p={pi} />);
    }
  });
  return (
    <div className="waffle-wrap">
      <svg ref={ref} className="waffle" width={size} height={size} viewBox={`0 0 ${size} ${size}`}
        onPointerMove={(ev) => {
          const el = ev.target instanceof Element ? ev.target.closest("[data-p]") : null;
          const pi = el ? Number(el.getAttribute("data-p")) : -1;
          const p = parts[pi];
          if (!p) return tooltip.hide();
          tooltip.show(ev, p.s.name, [{ color: p.s.color, name: `${cells[pi]}% of total`, value: p.v, unit: p.s.unit }]);
        }}
        onPointerLeave={() => tooltip.hide()}>
        {rects}
      </svg>
      <div className="waffle-legend">
        <div className="muted small">{fmtTime(t, m.tz, "tip", list[0]?.step || 86400)}</div>
        {parts.map((p, i) => (
          <div key={p.s.id} className="waffle-row">
            <span className="key key-rect" style={{ background: p.s.color }} />
            <span className="legend-label">{p.s.name}</span>
            <span className="num">{`${cells[i]}%`}</span>
            <span className="muted num">{`${fmtNum(p.v)}${p.s.unit ? " " + p.s.unit : ""}`}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

// Status: one row per series, latest value against that series' own history in the range
// (mean and standard deviation of every earlier point). Options: better = "up" | "down" | "both",
// or a comma list matched to the targets; amber / red = z thresholds (default 1 and 2). Status is
// shown by colour AND by shape and word, so it reads without colour.
type Level = "ok" | "watch" | "alert" | "none";

export function Status({ m, o }: { m: Model; o: ChartOpts }) {
  const T = m.T;
  const list = m.series.filter((s) => !isRef(s));
  if (!list.length) return <Empty msg="No data" />;
  const dirs = String(o.better || "both").split(",").map((x) => x.trim());
  const amber = optNum(o, "amber") ?? 1, red = optNum(o, "red") ?? 2;
  const levels: Record<Level, [string, string, string]> = { ok: ["●", "normal", T.good], watch: ["▲", "watch", "var(--warn)"], alert: ["■", "alert", T.bad], none: ["○", "no data", T.muted] };
  return (
    <div className="status-list" role="list">
      {list.map((s, k) => {
        const dir = dirs[Math.min(k, dirs.length - 1)] || "both";
        const i = lastIdx(s);
        const hist = s.v.slice(0, Math.max(0, i)).filter((v): v is number => v != null);
        let lvl: Level = "none", z: number | null = null;
        const v = i >= 0 ? (s.v[i] ?? null) : null;
        if (v != null && hist.length >= 5) {
          const mu = mean(hist), sd = Math.sqrt(mean(hist.map((x) => (x - mu) ** 2)));
          z = sd > 0 ? (v - mu) / sd : 0;
          const badness = dir === "up" ? -z : dir === "down" ? z : Math.abs(z);
          lvl = badness >= red ? "alert" : badness >= amber ? "watch" : "ok";
        }
        const [shape, word, color] = levels[lvl];
        const u = s.unit ? ` ${s.unit}` : "";
        return (
          <div key={s.id} className={`status-row ${lvl}`} role="listitem">
            <span className="status-dot" style={{ color }} aria-hidden="true">{shape}</span>
            <span className="status-name">{s.name}</span>
            <span className="status-val num">{v == null ? "—" : fmtNum(v) + u}</span>
            <span className="status-word" style={{ color }}>{word}</span>
            <span className="status-z muted num">{z == null ? "" : `${z >= 0 ? "+" : "−"}${Math.abs(z).toFixed(1)} sd vs ${spanLabel(m)}`}</span>
          </div>
        );
      })}
    </div>
  );
}
