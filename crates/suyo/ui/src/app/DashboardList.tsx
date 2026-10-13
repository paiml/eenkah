import { useEffect, useState } from "react";
import { JsonModal, listDashboards, parseDashboard, saveDashboard, slugify, ThemePicker, url, type DashboardSummary } from "../lib";
import { Header } from "./Header";

export function DashboardList() {
  const [list, setList] = useState<DashboardSummary[] | null>(null);
  const [importing, setImporting] = useState(false);
  useEffect(() => {
    document.title = "Dashboards · suyo";
    listDashboards().then(setList).catch(() => setList([]));
  }, []);

  const create = async () => {
    const n = window.prompt("Dashboard name (letters, digits, - and _):");
    if (!n) return;
    const slug = slugify(n);
    await saveDashboard(slug, { title: n.trim(), from: "-7d", until: "now", panels: [] });
    window.location.href = url(`/d/${slug}`);
  };

  return (
    <>
      <Header active="/d">
        <ThemePicker />
      </Header>
      <div className="dash-head">
        <h1 className="dash-title">Dashboards</h1>
        <div className="topbar-right">
          <button className="btn" onClick={() => setImporting(true)}>Import JSON</button>
          <button className="btn primary" onClick={() => void create()}>+ New dashboard</button>
        </div>
      </div>
      <div className="dash-list">
        {list && !list.length && <div className="muted">No dashboards yet.</div>}
        {list?.map((d) => (
          <a key={d.name} className="card dash-card" href={url(`/d/${encodeURIComponent(d.name)}`)}>
            <h3>{d.title || d.name}</h3>
            <div className="small muted">{d.description || ""}</div>
            <div className="small muted">{`${d.panels} panels · /d/${d.name}`}</div>
          </a>
        ))}
      </div>
      {importing && (
        <JsonModal text="" onClose={() => setImporting(false)}
          onSave={async (text) => {
            const spec = parseDashboard(text);
            const slug = (window.prompt("Save as (name):", slugify(spec.title || "imported")) || "").trim();
            if (!slug) return false;
            await saveDashboard(slug, spec);
            window.location.href = url(`/d/${slug}`);
            return true;
          }} />
      )}
    </>
  );
}
