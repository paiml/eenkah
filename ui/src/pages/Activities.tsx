import { useMemo, useState } from "react";
import { api, type ActivityRow } from "../api";
import { fmt, useLoad } from "../hooks";
import { Link } from "../router";
import { AgoLocal } from "../components/When";

type SortKey = "start_utc" | "moving_min" | "distance_km" | "ascent_m" | "avg_hr" | "load" | "decoupling_pct";

const COLUMNS: { key: SortKey; label: string; digits: number; unit: string }[] = [
  { key: "moving_min", label: "Moving", digits: 0, unit: "min" },
  { key: "distance_km", label: "Distance", digits: 1, unit: "km" },
  { key: "ascent_m", label: "Climb", digits: 0, unit: "m" },
  { key: "avg_hr", label: "Avg HR", digits: 0, unit: "" },
  { key: "load", label: "Load", digits: 0, unit: "" },
  { key: "decoupling_pct", label: "Decoupling", digits: 1, unit: "%" },
];

export const kindOf = (a: ActivityRow): string => (a.type ?? a.sport ?? "other").replace(/_/g, " ");

export function ActivitiesPage() {
  const [list] = useLoad(() => api.activities());
  const [kind, setKind] = useState("all");
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: "start_utc", desc: true });

  const rows = useMemo(() => {
    if (list.state !== "ok") return [];
    const r = list.data.filter((a) => kind === "all" || kindOf(a) === kind);
    const v = (a: ActivityRow) => a[sort.key] ?? -Infinity;
    return [...r].sort((a, b) => (sort.desc ? v(b) - v(a) : v(a) - v(b)));
  }, [list, kind, sort]);

  const kinds = useMemo(() => {
    if (list.state !== "ok") return [];
    const c = new Map<string, number>();
    for (const a of list.data) c.set(kindOf(a), (c.get(kindOf(a)) ?? 0) + 1);
    return [...c.entries()].sort((a, b) => b[1] - a[1]);
  }, [list]);

  const head = (key: SortKey, label: string, num = true) => (
    <th className={num ? "num" : ""}>
      <button className="th-sort" onClick={() => setSort({ key, desc: sort.key === key ? !sort.desc : true })} aria-sort={sort.key === key ? (sort.desc ? "descending" : "ascending") : "none"}>
        {label}
        {sort.key === key ? (sort.desc ? " ▼" : " ▲") : ""}
      </button>
    </th>
  );

  return (
    <main className="page">
      <div className="page-head">
        <div>
          <h1>What did that session do?</h1>
          <p className="muted">Every session, newest first. Open one to see its streams; drag across them to measure a section. Load is heart-rate impulse; decoupling is how far heart rate drifted against output.</p>
        </div>
        <label className="row gap">
          <span className="small muted">Type</span>
          <select className="select" value={kind} onChange={(e) => setKind(e.target.value)}>
            <option value="all">All ({list.state === "ok" ? list.data.length : "…"})</option>
            {kinds.map(([k, n]) => (
              <option key={k} value={k}>
                {k} ({n})
              </option>
            ))}
          </select>
        </label>
      </div>
      {list.state === "error" && <p className="error">{list.error}</p>}
      {list.state === "loading" && <p className="muted">Loading…</p>}
      {list.state === "ok" && (
        <section className="card panel">
          <div className="table-scroll">
            <table className="data-table">
              <thead>
                <tr>
                  {head("start_utc", "Start", false)}
                  <th>Type</th>
                  <th>Session</th>
                  <th>Name</th>
                  {COLUMNS.map((c) => head(c.key, c.label))}
                </tr>
              </thead>
              <tbody>
                {rows.map((a) => (
                  <tr key={a.id}>
                    <td className="nowrap">
                      <Link to={`/activity/${a.id}`}>{a.start_local ? <AgoLocal s={a.start_local} /> : "?"}</Link>
                    </td>
                    <td>{kindOf(a)}</td>
                    <td>{a.session_type ? <span className={"type-badge" + (a.session_type_source === "you" ? " you" : "")}>{a.session_type}</span> : <span className="muted">—</span>}</td>
                    <td>{a.name ?? ""}</td>
                    {COLUMNS.map((c) => (
                      <td key={c.key} className={"num" + (c.key === "load" ? " strong" : "")}>
                        {fmt(a[c.key], c.digits, c.unit)}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}
    </main>
  );
}
