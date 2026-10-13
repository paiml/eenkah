// /d/:name — a dashboard with range picker, auto-refresh, reload, theme, and edit mode.
// ?from=&until= override the dashboard's range; lock=1 also overrides per-panel ranges (zoom).

import { useEffect, useState } from "react";
import { Dashboard, getDashboard, RangePicker, ThemePicker, url, type DashboardSpec, type Range } from "../lib";
import { Header } from "./Header";

const REFRESH: [string, string][] = [["0", "Auto-refresh off"], ["30", "Every 30s"], ["60", "Every 1m"], ["300", "Every 5m"], ["900", "Every 15m"]];

export function DashboardPage({ name }: { name: string }) {
  const [spec, setSpec] = useState<DashboardSpec | null>(null);
  const [missing, setMissing] = useState(false);
  useEffect(() => {
    getDashboard(name)
      .then((s) => {
        setSpec(s);
        document.title = `${s.title || name} · suyo`;
      })
      .catch(() => setMissing(true));
  }, [name]);
  if (missing)
    return (
      <>
        <Header active="/d" />
        <div className="docs">
          <h1>{`No dashboard “${name}”`}</h1>
          <p><a href={url("/d")}>See all dashboards</a></p>
        </div>
      </>
    );
  if (!spec) return <Header active="/d" />;
  return <Loaded name={name} spec={spec} />;
}

function Loaded({ name, spec }: { name: string; spec: DashboardSpec }) {
  const q = new URLSearchParams(window.location.search);
  const dFrom = spec.from || "-7d", dUntil = spec.until || "now";
  const [R, setR] = useState<{ range: Range; lock: boolean }>(() => ({
    range: { from: q.get("from") || dFrom, until: q.get("until") || dUntil },
    lock: q.get("lock") === "1",
  }));
  const [showReset, setShowReset] = useState(() => Boolean(q.get("from") || q.get("lock")));
  const [refresh, setRefresh] = useState(() => {
    const v = String(q.get("refresh") || spec.refresh || 0);
    return REFRESH.some(([x]) => x === v) ? v : "0";
  });
  const [signal, setSignal] = useState(0);

  useEffect(() => {
    const s = Number(refresh);
    if (!s) return;
    const t = setInterval(() => setSignal((n) => n + 1), s * 1000);
    return () => clearInterval(t);
  }, [refresh]);

  const setRange = (range: Range, lock: boolean) => {
    const p = new URLSearchParams(window.location.search);
    const isDefault = range.from === dFrom && range.until === dUntil && !lock;
    if (isDefault) {
      p.delete("from");
      p.delete("until");
      p.delete("lock");
    } else {
      p.set("from", range.from);
      p.set("until", range.until);
      if (lock) p.set("lock", "1");
      else p.delete("lock");
    }
    history.replaceState(null, "", window.location.pathname + (p.toString() ? "?" + p.toString() : ""));
    setShowReset(!isDefault);
    setR({ range, lock });
  };

  return (
    <>
      <Header active="/d">
        <RangePicker from={R.range.from} until={R.range.until} onChange={(from, until) => setRange({ from, until }, false)} />
        <select className="select" title="Auto refresh" aria-label="Auto refresh" value={refresh} onChange={(e) => setRefresh(e.target.value)}>
          {REFRESH.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
        </select>
        <button className="btn" title="Refresh now" onClick={() => setSignal((n) => n + 1)}>↻</button>
        <ThemePicker />
      </Header>
      <Dashboard name={name} spec={spec} range={R.range} lock={R.lock} onRangeChange={setRange} editable refreshSignal={signal}
        headExtra={showReset && (
          <button className="btn small" title="Back to the dashboard's own time ranges" onClick={() => setRange({ from: dFrom, until: dUntil }, false)}>Reset range</button>
        )} />
    </>
  );
}
