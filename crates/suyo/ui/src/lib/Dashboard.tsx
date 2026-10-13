// <Dashboard>: a 12-column grid of panels with an optional edit mode
// (move / resize / duplicate / delete panels, JSON editor, save, delete dashboard).

import { useEffect, useState, type ReactNode } from "react";
import { deleteDashboard, getDashboard, saveDashboard, url } from "./api";
import { JsonModal } from "./JsonModal";
import { composerHref, Panel } from "./Panel";
import { errorMessage, toast } from "./store";
import type { DashboardSpec, PanelSpec, Range } from "./types";

/** Parse dashboard JSON text, checking the minimum shape the server requires. */
export function parseDashboard(text: string): DashboardSpec {
  const v: unknown = JSON.parse(text);
  if (!v || typeof v !== "object" || !Array.isArray((v as { panels?: unknown }).panels)) throw new Error('dashboard needs a "panels" array');
  return v as DashboardSpec;
}

export interface DashboardProps {
  /** Dashboard name: fetched when `spec` is not given; needed to save or delete. */
  name?: string;
  spec?: DashboardSpec;
  /** Current range; defaults to the dashboard's own (or -7d → now). */
  range?: Range;
  /** Ignore per-panel ranges (set after zooming). */
  lock?: boolean;
  /** Zoom on a panel calls this with lock = true. Uncontrolled when omitted. */
  onRangeChange?: (range: Range, lock: boolean) => void;
  /** Show the Edit button (requires `name`). */
  editable?: boolean;
  /** Change to refetch every panel. */
  refreshSignal?: number;
  /** Extra controls at the start of the edit bar. */
  headExtra?: ReactNode;
  /** Render the title / description / edit bar (default true). */
  showHead?: boolean;
  /** Content shown between the head and the panel grid (e.g. a host app's summary cards). */
  intro?: ReactNode;
  /** Content shown after the panel grid. */
  outro?: ReactNode;
  /** Link for a panel's title and its "Edit" tool; null for none. Default: suyo's composer. */
  panelHref?: (panel: PanelSpec, index: number | null, range: Range) => string | null;
  /** "+ Add panel" link in edit mode; null hides it. Default: suyo's composer. */
  addPanelHref?: string | null;
  onSaved?: (spec: DashboardSpec) => void;
  onDeleted?: () => void;
}

export function Dashboard(props: DashboardProps) {
  const { name, spec: specProp } = props;
  const [fetched, setFetched] = useState<DashboardSpec | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    if (specProp || !name) return;
    let live = true;
    getDashboard(name)
      .then((s) => live && setFetched(s))
      .catch((e: unknown) => live && setErr(errorMessage(e)));
    return () => {
      live = false;
    };
  }, [name, specProp]);
  const spec = specProp ?? fetched;
  if (!spec) return err ? <div className="panel-error" style={{ padding: 16 }}>{err}</div> : null;
  return <DashboardView {...props} spec={spec} />;
}

const defaultPanelHref = (name: string | undefined) => (p: PanelSpec, i: number | null, r: Range): string =>
  composerHref(p, r) + (name && i != null ? `&dash=${encodeURIComponent(name)}&panel=${i}` : "");

function DashboardView({ name, spec: initial, range: rangeProp, lock: lockProp, onRangeChange, editable, refreshSignal, headExtra, showHead = true, intro, outro, panelHref, addPanelHref, onSaved, onDeleted }: DashboardProps & { spec: DashboardSpec }) {
  const [saved, setSaved] = useState<DashboardSpec>(initial);
  const [spec, setSpec] = useState<DashboardSpec>(initial);
  const [editing, setEditing] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [json, setJson] = useState(false);
  const [own, setOwn] = useState<{ range: Range; lock: boolean } | null>(null);

  const initialKey = JSON.stringify(initial);
  useEffect(() => {
    setSaved(initial);
    setSpec(initial);
    setDirty(false);
  }, [initialKey]); // eslint-disable-line react-hooks/exhaustive-deps

  const R: Range = rangeProp ?? own?.range ?? { from: saved.from || "-7d", until: saved.until || "now" };
  const lock = lockProp ?? own?.lock ?? false;
  const href = panelHref ?? defaultPanelHref(name);

  const zoom = (r: Range) => {
    if (onRangeChange) onRangeChange(r, true);
    else setOwn({ range: r, lock: true });
  };

  const update = (fn: (panels: PanelSpec[]) => PanelSpec[]) => {
    setSpec((s) => ({ ...s, panels: fn([...s.panels]) }));
    setDirty(true);
  };

  const save = async (next: DashboardSpec = spec): Promise<boolean> => {
    if (!name) return false;
    try {
      await saveDashboard(name, next);
      setSaved(next);
      setSpec(next);
      setDirty(false);
      toast("Saved");
      onSaved?.(next);
      return true;
    } catch (e) {
      toast(errorMessage(e), "error");
      return false;
    }
  };

  const toggleEdit = () => {
    if (editing && dirty) {
      if (!window.confirm("Discard unsaved layout changes?")) return;
      setSpec(saved);
      setDirty(false);
      setEditing(false);
      return;
    }
    setEditing(!editing);
  };

  const remove = async () => {
    if (!name || !window.confirm(`Delete dashboard “${name}”?`)) return;
    try {
      await deleteDashboard(name);
      if (onDeleted) onDeleted();
      else window.location.href = url("/d");
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  };

  const tools = (i: number): ReactNode => {
    if (!editing) return null;
    const p = spec.panels[i];
    if (!p) return null;
    const n = spec.panels.length;
    const set = (patch: Partial<PanelSpec>) => update((ps) => ps.map((x, j) => (j === i ? { ...x, ...patch } : x)));
    const swap = (j: number) =>
      update((ps) => {
        const a = ps[i], b = ps[j];
        if (a && b) { ps[i] = b; ps[j] = a; }
        return ps;
      });
    const b = (label: string, title: string, fn: () => void) => (
      <button key={title} className="btn ghost small" title={title} onClick={fn}>{label}</button>
    );
    const edit = href(p, i, R);
    return [
      b("◀", "Move earlier", () => i > 0 && swap(i - 1)),
      b("▶", "Move later", () => i < n - 1 && swap(i + 1)),
      b("−", "Narrower", () => set({ w: Math.max(2, (p.w || 6) - 1) })),
      b("+", "Wider", () => set({ w: Math.min(12, (p.w || 6) + 1) })),
      b("↕", "Taller / shorter", () => {
        const hs = [1, 2, 3, 4, 5, 6];
        const cur = p.h || (p.chart === "stat" ? 2 : 3);
        set({ h: hs[(hs.indexOf(cur) + 1) % hs.length] });
      }),
      edit ? <a key="edit" className="btn ghost small" href={edit} title="Edit in composer">Edit</a> : null,
      b("⧉", "Duplicate", () => update((ps) => { ps.splice(i + 1, 0, structuredClone(p)); return ps; })),
      b("✕", "Remove", () => update((ps) => { ps.splice(i, 1); return ps; })),
    ];
  };

  const addHref = addPanelHref === undefined ? (name ? url(`/?dash=${encodeURIComponent(name)}&from=${encodeURIComponent(R.from)}`) : null) : addPanelHref;
  return (
    <>
      {showHead && (
        <div className="dash-head">
          <div className="grow">
            <h1 className="dash-title">{spec.title || name}</h1>
            {spec.description && <p className="dash-desc">{spec.description}</p>}
          </div>
          <div className="edit-bar">
            {headExtra}
            {editing && addHref && <a className="btn" href={addHref}>+ Add panel</a>}
            {editing && <button className="btn" onClick={() => setJson(true)}>JSON</button>}
            {editing && <button className="btn danger" onClick={remove}>Delete</button>}
            {dirty && <button className="btn primary" onClick={() => void save()}>Save</button>}
            {editable && name && <button className="btn" onClick={toggleEdit}>{editing ? "Done" : "Edit"}</button>}
          </div>
        </div>
      )}
      {intro}
      <div className="grid">
        {spec.panels.map((p, i) => {
          const eff = lock ? { ...p, from: undefined, until: undefined } : p;
          const w = p.w || (p.chart === "stat" ? 3 : 6);
          const hh = p.h || (p.chart === "stat" ? 2 : p.chart === "text" ? 1 : 3);
          const cls = [w <= 3 ? "w-small" : "", editing ? "editing" : ""].filter(Boolean).join(" ");
          return (
            <Panel key={i} spec={eff} range={R} tools={tools(i)} refreshSignal={refreshSignal} titleHref={href(eff, null, R)}
              onZoom={zoom} className={cls} style={{ gridColumn: `span ${w}`, gridRow: `span ${hh}` }} />
          );
        })}
        {!spec.panels.length && (
          <div className="card panel" style={{ gridColumn: "span 12" }}>
            <div className="empty">This dashboard is empty. Click Edit → + Add panel, or build a chart in Compose and save it here.</div>
          </div>
        )}
      </div>
      {outro}
      {json && (
        <JsonModal text={JSON.stringify(spec, null, 2)} onClose={() => setJson(false)}
          onSave={async (text) => {
            const next = parseDashboard(text);
            const ok = await save(next);
            if (ok) setEditing(false);
            return ok;
          }} />
      )}
    </>
  );
}
