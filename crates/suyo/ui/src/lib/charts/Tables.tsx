// Summary table (latest / average / min / max / change / trend) and the raw data view.

import { fmtNum } from "../format";
import { fmtTime } from "../time";
import { Empty } from "./Empty";
import { f1, firstVal, isRef, last, lastVal, mean, vals, type Model } from "./model";

export function SummaryTable({ m, width }: { m: Model; width: number }) {
  const T = m.T;
  const list = m.series.filter((s) => !isRef(s));
  if (!list.length) return <Empty msg="No data" />;
  const wide = width > 560;
  return (
    <div className="table-scroll">
      <table className="data-table">
        <thead>
          <tr>
            <th>Series</th>
            <th className="num">Latest</th>
            <th className="num">Average</th>
            {wide && <th className="num">Min</th>}
            {wide && <th className="num">Max</th>}
            <th className="num">Change</th>
            <th>Trend</th>
          </tr>
        </thead>
        <tbody>
          {list.map((s) => {
            const a = vals(s);
            const f = firstVal(s), l = lastVal(s);
            const ch = f != null && l != null && f !== 0 ? ((l - f) / Math.abs(f)) * 100 : null;
            const pts: [number, number][] = [];
            s.ts.forEach((t, i) => { const v = s.v[i]; if (v != null) pts.push([t, v]); });
            let d: string | null = null;
            if (pts.length > 1) {
              const lo = Math.min(...a), hi = Math.max(...a);
              const t0 = (pts[0] as [number, number])[0], t1 = last(pts)[0];
              d = "M" + pts.map((p) => `${f1(2 + ((p[0] - t0) / Math.max(1, t1 - t0)) * 60)},${f1(20 - ((p[1] - lo) / (hi - lo || 1)) * 18)}`).join("L");
            }
            const u = s.unit ? ` ${s.unit}` : "";
            return (
              <tr key={s.id}>
                <td><span className="key key-line" style={{ background: s.color }} /><span className="nowrap">{s.name}</span></td>
                <td className="num strong">{l == null ? "—" : fmtNum(l) + u}</td>
                <td className="num">{a.length ? fmtNum(mean(a)) : "—"}</td>
                {wide && <td className="num">{a.length ? fmtNum(Math.min(...a)) : "—"}</td>}
                {wide && <td className="num">{a.length ? fmtNum(Math.max(...a)) : "—"}</td>}
                <td className="num">{ch == null ? "—" : `${ch >= 0 ? "+" : "−"}${Math.abs(ch).toFixed(1)}%`}</td>
                <td>
                  <svg className="spark" width={64} height={22} viewBox="0 0 64 22">
                    {d && <path d={d} fill="none" stroke={s.color === T.dim ? T.dim : s.color} strokeWidth={1.5} />}
                  </svg>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

/** Raw data view: every bucket for every series (accessibility / lookup), newest first. */
export function RawTable({ m }: { m: Model }) {
  const xs = [...new Set(m.series.flatMap((s) => s.ts))].sort((a, b) => b - a);
  const maps = m.series.map((s) => new Map(s.ts.map((tt, i) => [tt, s.v[i] ?? null])));
  return (
    <div className="table-scroll">
      <table className="data-table raw">
        <thead>
          <tr>
            <th>Time</th>
            {m.series.map((s) => <th key={s.id} className="num">{s.name + (s.unit ? ` (${s.unit})` : "")}</th>)}
          </tr>
        </thead>
        <tbody>
          {xs.slice(0, 2000).map((x) => (
            <tr key={x}>
              <td>{fmtTime(x, m.tz, "tip", m.step)}</td>
              {maps.map((mp, k) => {
                const v = mp.get(x);
                return <td key={k} className="num">{v == null ? "" : fmtNum(v)}</td>;
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
