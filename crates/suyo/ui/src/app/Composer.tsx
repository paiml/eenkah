// "/" — ad-hoc query composer: series tree, targets with autocomplete and function picker,
// chart type, options, live preview, share links and save-to-dashboard.

import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import {
  CHARTS, composerHref, debounce, errorMessage, getDashboard, listDashboards, listFunctions, listSeries, Panel, RangePicker, renderUrl,
  saveDashboard, slugify, store, ThemePicker, toast, url,
  type ChartType, type DashboardSpec, type DashboardSummary, type FunctionDoc, type PanelOptions, type PanelSpec, type SeriesMeta,
} from "../lib";
import { Header } from "./Header";
import { OPTION_KEYS } from "./options";

const STEPS: [string, string][] = [["", "Auto"], ["1min", "1 min"], ["5min", "5 min"], ["15min", "15 min"], ["1h", "1 hour"], ["6h", "6 hours"], ["1d", "1 day"], ["1w", "1 week"], ["1mon", "1 month"], ["1y", "1 year"]];
const HINTS: Partial<Record<ChartType, string>> = {
  scatter: "First target is x, second is y; points are joined on time. Use step=1d for daily pairs.",
  slope: "Compares first and last bucket of each series: e.g. step 1mon with range “Last month → now”.",
  calendar: "One cell per day of the first target. Use step 1d.",
  hourly: "Averages the first target by hour of day and weekday. Use step 1h.",
  waffle: "Shares of the latest bucket across targets.",
  histogram: "Distribution of values over the range.",
  table: "Latest / average / min / max / change per target.",
};

interface Target {
  id: number;
  text: string;
  on: boolean;
}

let nextId = 1;
const newTarget = (text = ""): Target => ({ id: nextId++, text, on: true });

interface Committed {
  key: number;
  spec: PanelSpec;
  from: string;
  until: string;
  step: string;
  href: string;
}

const isChart = (v: string | null): v is ChartType => v != null && (CHARTS.some(([c]) => c === v) || v === "text");

export function Composer() {
  const q = useMemo(() => new URLSearchParams(window.location.search), []);
  const [targets, setTargets] = useState<Target[]>(() => {
    const t = q.getAll("target").map((x) => newTarget(x));
    return t.length ? t : [newTarget()];
  });
  const [from, setFrom] = useState(() => q.get("from") || store.get("composer.from", "-7d"));
  const [until, setUntil] = useState(() => q.get("until") || "now");
  const [chart, setChart] = useState<ChartType>(() => { const c = q.get("chart"); return isChart(c) ? c : "line"; });
  const [step, setStep] = useState(() => q.get("step") || "");
  const [title, setTitle] = useState(() => q.get("title") || "");
  const [subtitle, setSubtitle] = useState(() => q.get("subtitle") || "");
  const [options, setOptions] = useState<Record<string, string>>(() =>
    Object.fromEntries(OPTION_KEYS.filter((k) => q.get(k) != null).map((k) => [k, q.get(k) ?? ""])),
  );
  const dash = q.get("dash") || "";
  const panelIdx = q.get("panel");
  const [funcs, setFuncs] = useState<FunctionDoc[]>([]);
  const [series, setSeries] = useState<SeriesMeta[]>([]);
  const [colors, setColors] = useState<string[]>([]);
  const [focusId, setFocusId] = useState<number | null>(null);

  // "update" = commit the current state to the preview, URL and share links.
  const [ver, setVer] = useState(0);
  const update = () => setVer((v) => v + 1);
  const scheduleUpdate = useMemo(() => debounce(() => setVer((v) => v + 1), 700), []);
  const scheduleText = useMemo(() => debounce(() => setVer((v) => v + 1), 400), []);

  const spec = (): PanelSpec => ({
    targets: targets.filter((t) => t.on && t.text.trim()).map((t) => t.text.trim()),
    chart,
    step: step || undefined,
    title: title || undefined,
    subtitle: subtitle || undefined,
    options: Object.fromEntries(Object.entries(options).filter(([, v]) => v !== "" && v != null)) as PanelOptions,
  });

  const [committed, setCommitted] = useState<Committed | null>(null);
  useEffect(() => {
    const s = spec();
    const extra = new URLSearchParams();
    if (dash) extra.set("dash", dash);
    if (panelIdx != null) extra.set("panel", panelIdx);
    const href = composerHref({ ...s, from, until }) + (extra.toString() ? "&" + extra.toString() : "");
    history.replaceState(null, "", href);
    store.set("composer.from", from);
    setCommitted({ key: ver, spec: s, from, until, step, href });
  }, [ver]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    Promise.all([listFunctions(), listSeries()])
      .then(([f, s]) => {
        setFuncs(f);
        setSeries(s);
      })
      .catch((e: unknown) => toast(errorMessage(e), "error"));
  }, []);

  const setTarget = (id: number, patch: Partial<Target>) => setTargets((ts) => ts.map((t) => (t.id === id ? { ...t, ...patch } : t)));
  const addTarget = (text = "") => {
    const t = newTarget(text);
    setTargets((ts) => [...ts, t]);
    return t;
  };
  const pickSeries = (path: string) => {
    setTargets((ts) => {
      const i = ts.findIndex((t) => !t.text.trim());
      if (i >= 0) return ts.map((t, j) => (j === i ? { ...t, text: path } : t));
      return [...ts, newTarget(path)];
    });
    update();
  };

  // swatch k = colour of the k-th plotted series (one per visible, non-empty target)
  let k = 0;
  const swatches = targets.map((t) => (t.on && t.text.trim() ? (colors[k++] ?? "transparent") : "transparent"));

  return (
    <>
      <Header active="/">
        <RangePicker from={from} until={until} onChange={(f, u) => { setFrom(f); setUntil(u); update(); }} />
        <ThemePicker />
      </Header>
      <div className="composer">
        <SeriesTree series={series} onPick={pickSeries} />
        <main className="main">
          <div className="toolbar">
            <div className="seg" role="radiogroup" aria-label="Chart type">
              {CHARTS.map(([v, l]) => (
                <button key={v} className={v === chart ? "on" : ""} role="radio" aria-checked={v === chart} onClick={() => { setChart(v); update(); }}>{l}</button>
              ))}
            </div>
            <select className="select" aria-label="Bucket size" title="Bucket size (step)" value={step} onChange={(e) => { setStep(e.target.value); update(); }}>
              {STEPS.map(([v, l]) => <option key={v} value={v}>{l === "Auto" ? "Step: auto" : `Step: ${l}`}</option>)}
            </select>
          </div>
          <div className="card">
            <div className="targets">
              {targets.map((t, i) => (
                <TargetRow key={t.id} t={t} index={i} swatch={swatches[i] ?? "transparent"} funcs={funcs} series={series} autoFocus={focusId === t.id}
                  onText={(text) => { setTarget(t.id, { text }); scheduleUpdate(); }}
                  onCommit={update}
                  onToggle={() => { setTarget(t.id, { on: !t.on }); update(); }}
                  onRemove={() => { setTargets((ts) => { const n = ts.filter((x) => x.id !== t.id); return n.length ? n : [newTarget()]; }); update(); }}
                  onFunction={(f) => {
                    const inner = f.example.slice(f.example.indexOf("(") + 1, -1);
                    const args = splitTopArgs(inner);
                    if (f.group === "Generate" || !t.text.trim()) addTarget(f.example);
                    else setTarget(t.id, { text: `${f.name}(${[t.text.trim(), ...args.slice(1)].join(", ")})` });
                    update();
                  }} />
              ))}
              <div className="row gap">
                <button className="btn ghost small" onClick={() => setFocusId(addTarget().id)}>+ Add target</button>
                <span className="small muted">Tab completes series and functions · Ctrl+Enter refreshes · drag across a chart to zoom</span>
              </div>
            </div>
          </div>
          <div className="toolbar">
            <input className="input grow" placeholder="Chart title (optional)" aria-label="Title" value={title} onChange={(e) => { setTitle(e.target.value); scheduleText(); }} />
            <input className="input grow" placeholder="Subtitle — what should the reader notice?" aria-label="Subtitle" value={subtitle} onChange={(e) => { setSubtitle(e.target.value); scheduleText(); }} />
          </div>
          <div className="card preview">
            {committed && (
              <Panel key={committed.key} spec={{ ...committed.spec, h: 5 }} range={{ from: committed.from, until: committed.until }}
                height={committed.spec.chart === "stat" ? 140 : 340}
                onZoom={(r) => { setFrom(r.from); setUntil(r.until); update(); }}
                onModel={(m) => setColors(m.series.map((s) => s.color))} />
            )}
          </div>
          <Options key={chart} chart={chart} options={options} onChange={(key, v) => { setOptions((o) => ({ ...o, [key]: v })); update(); }} />
          {committed && <Share c={committed} />}
          <SaveBox chart={chart} dash={dash} panelIdx={panelIdx} build={() => ({ spec: spec(), from, until, step, title, subtitle })} />
        </main>
      </div>
    </>
  );
}

// ------------------------------------------------------------------ targets

interface AcItem {
  insert: string;
  label: string;
  doc: string;
  leaf?: boolean;
}

function tokenAt(value: string, pos: number): { start: number; end: number; tok: string } {
  const before = value.slice(0, pos);
  const m = /[A-Za-z0-9_.*{}\-:@%]*$/.exec(before);
  const tok = m ? m[0] : "";
  return { start: pos - tok.length, end: pos, tok };
}

function suggestions(tok: string, series: SeriesMeta[], funcs: FunctionDoc[]): AcItem[] {
  if (!tok) return [];
  const items: AcItem[] = [];
  // series: complete the next path segment
  const segs = tok.split(".");
  const prefix = segs.slice(0, -1).join(".");
  const partial = (segs[segs.length - 1] ?? "").toLowerCase();
  const seen = new Set<string>();
  for (const s of series) {
    if (prefix && !s.name.startsWith(prefix + ".")) continue;
    const rest = prefix ? s.name.slice(prefix.length + 1) : s.name;
    const next = rest.split(".")[0] ?? "";
    if (!next.toLowerCase().startsWith(partial) || seen.has(next)) continue;
    seen.add(next);
    const full = (prefix ? prefix + "." : "") + next;
    const leaf = full === s.name;
    items.push({ insert: full + (leaf ? "" : "."), label: full + (leaf ? "" : ".…"), doc: leaf ? [s.unit, s.description].filter(Boolean).join(" · ") : "", leaf });
    if (items.length > 40) break;
  }
  if (!tok.includes(".")) for (const f of funcs) if (f.name.toLowerCase().startsWith(tok.toLowerCase())) items.push({ insert: f.name + "(", label: f.signature, doc: f.doc });
  return items.slice(0, 50);
}

function splitTopArgs(s: string): string[] {
  const out: string[] = [];
  let depth = 0, cur = "", quote: string | null = null;
  for (const c of s) {
    if (quote) { cur += c; if (c === quote) quote = null; continue; }
    if (c === '"' || c === "'") { quote = c; cur += c; continue; }
    if (c === "(" || c === "{" || c === "[") depth++;
    if (c === ")" || c === "}" || c === "]") depth--;
    if (c === "," && depth === 0) { out.push(cur.trim()); cur = ""; continue; }
    cur += c;
  }
  if (cur.trim()) out.push(cur.trim());
  return out;
}

interface TargetRowProps {
  t: Target;
  index: number;
  swatch: string;
  funcs: FunctionDoc[];
  series: SeriesMeta[];
  autoFocus: boolean;
  onText: (text: string) => void;
  onCommit: () => void;
  onToggle: () => void;
  onRemove: () => void;
  onFunction: (f: FunctionDoc) => void;
}

function TargetRow({ t, index, swatch, funcs, series, autoFocus, onText, onCommit, onToggle, onRemove, onFunction }: TargetRowProps) {
  const input = useRef<HTMLInputElement>(null);
  const acBox = useRef<HTMLDivElement>(null);
  const [ac, setAc] = useState<{ items: AcItem[]; active: number } | null>(null);
  const [fnOpen, setFnOpen] = useState(false);
  const caret = useRef<number | null>(null);

  useEffect(() => { if (autoFocus) input.current?.focus(); }, [autoFocus]);
  useLayoutEffect(() => {
    if (caret.current != null && input.current) {
      input.current.setSelectionRange(caret.current, caret.current);
      caret.current = null;
    }
  });
  useEffect(() => {
    acBox.current?.querySelector(".ac-item.on")?.scrollIntoView({ block: "nearest" });
  }, [ac?.active]);

  const suggest = (value: string, pos: number) => {
    const items = suggestions(tokenAt(value, pos).tok, series, funcs);
    setAc(items.length ? { items, active: 0 } : null);
  };
  const accept = (it: AcItem) => {
    const el = input.current;
    if (!el) return;
    const { start, end } = tokenAt(el.value, el.selectionStart ?? el.value.length);
    const value = el.value.slice(0, start) + it.insert + el.value.slice(end);
    const pos = start + it.insert.length;
    caret.current = pos;
    onText(value);
    if (it.leaf) setAc(null);
    else suggest(value, pos); // offer the next path segment
  };
  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") { onCommit(); return; }
    if (!ac) { if (e.key === "Enter") onCommit(); return; }
    const n = ac.items.length;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      setAc({ ...ac, active: (ac.active + (e.key === "ArrowDown" ? 1 : -1) + n) % n });
    } else if (e.key === "Tab" || e.key === "Enter") {
      const it = ac.items[ac.active];
      if (it) { e.preventDefault(); accept(it); }
    } else if (e.key === "Escape") setAc(null);
  };

  return (
    <div className={"target-row" + (t.on ? "" : " off")}>
      <span className="letter">{String.fromCharCode(65 + index)}</span>
      <span className="swatch" style={{ background: swatch }} />
      <input ref={input} className="input target-input" value={t.text} spellCheck={false} autoComplete="off" placeholder='e.g. movingAverage(health.daily.rhr, "7d")' aria-label={`Target ${index + 1}`}
        onChange={(e) => { onText(e.target.value); suggest(e.target.value, e.target.selectionStart ?? e.target.value.length); }}
        onKeyDown={onKey}
        onBlur={() => setTimeout(() => setAc(null), 150)} />
      <button className="btn small" title="Wrap in a function" onClick={(e) => { e.stopPropagation(); setFnOpen(true); }}>ƒ</button>
      <button className="btn ghost small" title={t.on ? "Hide this target" : "Show this target"} onClick={onToggle}>{t.on ? "Hide" : "Show"}</button>
      <button className="btn ghost small" title="Remove" aria-label="Remove target" onClick={onRemove}>✕</button>
      {ac && (
        <div className="ac" role="listbox" ref={acBox}>
          {ac.items.map((it, i) => (
            <div key={it.label + i} className={"ac-item" + (i === ac.active ? " on" : "")} role="option" aria-selected={i === ac.active}
              onMouseDown={(e) => { e.preventDefault(); accept(it); }}>
              <span className="mono">{it.label}</span>
              <span className="doc">{it.doc}</span>
            </div>
          ))}
        </div>
      )}
      {fnOpen && <FnMenu funcs={funcs} onClose={() => setFnOpen(false)} onPick={(f) => { setFnOpen(false); onFunction(f); }} />}
    </div>
  );
}

function FnMenu({ funcs, onPick, onClose }: { funcs: FunctionDoc[]; onPick: (f: FunctionDoc) => void; onClose: () => void }) {
  const [search, setSearch] = useState("");
  const box = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => {
    field.current?.focus();
    const off = (e: PointerEvent) => { if (e.target instanceof Node && !box.current?.contains(e.target)) onClose(); };
    const t = setTimeout(() => document.addEventListener("pointerdown", off), 0);
    return () => { clearTimeout(t); document.removeEventListener("pointerdown", off); };
  }, [onClose]);
  const qq = search.toLowerCase();
  const shown = funcs.filter((f) => !qq || f.name.toLowerCase().includes(qq) || f.doc.toLowerCase().includes(qq));
  return (
    <div className="menu fn-menu" ref={box}>
      <input ref={field} className="input" placeholder="Search functions…" style={{ width: "100%" }} value={search} onChange={(e) => setSearch(e.target.value)} />
      <div>
        {shown.map((f, i) => (
          <div key={f.name}>
            {f.group !== shown[i - 1]?.group && <div className="fn-group">{f.group}</div>}
            <div className="ac-item" onClick={() => onPick(f)}>
              <span className="mono">{f.name}</span>
              <span className="doc">{f.doc}</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ series tree

interface TNode {
  path: string;
  children?: Record<string, TNode>;
  meta?: SeriesMeta;
}

function buildTree(series: SeriesMeta[]): TNode {
  const root: TNode = { path: "" };
  for (const s of series) {
    let node = root;
    const parts = s.name.split(".");
    parts.forEach((seg, i) => {
      node.children = node.children || {};
      const child: TNode = node.children[seg] ?? { path: parts.slice(0, i + 1).join(".") };
      node.children[seg] = child;
      node = child;
      if (i === parts.length - 1) node.meta = s;
    });
  }
  return root;
}

const countLeaves = (n: TNode): number =>
  (n.children ? Object.values(n.children).reduce((a, c) => a + countLeaves(c), 0) : 0) + (n.meta && n.children ? 1 : n.children ? 0 : 1);

function SeriesTree({ series, onPick }: { series: SeriesMeta[]; onPick: (path: string) => void }) {
  const [filterText, setFilterText] = useState("");
  const [filter, setFilter] = useState("");
  const applyFilter = useMemo(() => debounce((v: string) => setFilter(v), 120), []);
  const [open, setOpen] = useState<Set<string>>(() => new Set(store.get<string[]>("composer.open", [])));
  const tree = useMemo(() => buildTree(series), [series]);
  const toggle = (path: string) => {
    setOpen((o) => {
      const n = new Set(o);
      if (n.has(path)) n.delete(path);
      else n.add(path);
      store.set("composer.open", [...n]);
      return n;
    });
  };
  const leaf = (path: string, meta: SeriesMeta | undefined, label?: string) => (
    <div key={"leaf:" + path + (label ?? "")} className="tree-node" role="treeitem" title={[path, meta?.description].filter(Boolean).join("\n")} onClick={() => onPick(path)}>
      <span className="leaf-dot" />
      {label || path}
      <span className="meta">{meta?.unit || ""}</span>
    </div>
  );
  const walk = (node: TNode): React.ReactNode[] => {
    const out: React.ReactNode[] = [];
    const entries = Object.entries(node.children || {}).sort((a, b) => (a[0] + "," < b[0] + "," ? -1 : 1));
    for (const [seg, child] of entries) {
      if (child.children) {
        const isOpen = open.has(child.path);
        out.push(
          <div key={"node:" + child.path} className="tree-node" role="treeitem" aria-expanded={isOpen} onClick={() => toggle(child.path)}>
            <span className="caret">{isOpen ? "▾" : "▸"}</span>
            {seg}
            <span className="meta">{String(countLeaves(child))}</span>
          </div>,
        );
        if (child.meta) out.push(leaf(child.path, child.meta, seg + " (series)"));
        if (isOpen) out.push(<div key={"kids:" + child.path} className="tree-children">{walk(child)}</div>);
      } else out.push(leaf(child.path, child.meta, seg));
    }
    return out;
  };
  const f = filter.trim().toLowerCase();
  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <div className="small muted">Series — click to plot</div>
        <input className="input" placeholder="Filter series…" aria-label="Filter series" value={filterText} onChange={(e) => { setFilterText(e.target.value); applyFilter(e.target.value); }} />
      </div>
      <div className="tree" role="tree">
        {!series.length ? (
          <div className="small muted" style={{ padding: 8 }}>No series yet. Import data with `suyo sync --sources`, `suyo import-csv`, POST /api/ingest, or try randomWalk("demo") as a target.</div>
        ) : f ? (
          series.filter((s) => s.name.toLowerCase().includes(f)).slice(0, 300).map((s) => leaf(s.name, s))
        ) : (
          walk(tree)
        )}
      </div>
    </aside>
  );
}

// ------------------------------------------------------------------ options

function OptInput({ value, placeholder, onCommit }: { value: string; placeholder: string; onCommit: (v: string) => void }) {
  const [v, setV] = useState(value);
  const commit = () => { if (v !== value) onCommit(v); };
  return <input className="input" value={v} placeholder={placeholder} onChange={(e) => setV(e.target.value)} onBlur={commit} onKeyDown={(e) => e.key === "Enter" && commit()} />;
}

function Options({ chart, options, onChange }: { chart: ChartType; options: Record<string, string>; onChange: (key: string, v: string) => void }) {
  const inp = (key: string, label: string, ph: string) => (
    <label key={key} className="field">{label}<OptInput value={options[key] ?? ""} placeholder={ph} onCommit={(v) => onChange(key, v)} /></label>
  );
  const sel = (key: string, label: string, opts: [string, string][]) => (
    <label key={key} className="field">
      {label}
      <select className="select" value={options[key] ?? ""} onChange={(e) => onChange(key, e.target.value)}>
        {opts.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
      </select>
    </label>
  );
  const kids: React.ReactNode[] = [];
  if (chart === "line" || chart === "area" || chart === "bar") {
    kids.push(inp("yMin", "Y min", "auto"), inp("yMax", "Y max", "auto"), inp("unit", "Unit", "auto"), sel("legend", "Legend", [["", "Auto"], ["show", "Show"], ["hide", "Hide"]]));
    if (chart !== "bar") kids.push(sel("labels", "End labels", [["", "Auto"], ["none", "Off"]]), sel("nulls", "Gaps", [["", "Show gaps"], ["connect", "Connect"]]));
  }
  if (chart === "stat") {
    kids.push(
      sel("stat", "Value", [["", "Latest"], ["avg", "Average"], ["sum", "Sum"], ["max", "Max"], ["min", "Min"]]),
      sel("compare", "Compare to", [["", "Range average"], ["first", "Start of range"], ["series", "Second target"], ["none", "Nothing"]]),
      sel("better", "Better when", [["", "Neutral"], ["up", "Higher"], ["down", "Lower"]]),
      inp("decimals", "Decimals", "auto"),
      inp("unit", "Unit", "auto"),
    );
  }
  const hint = HINTS[chart];
  if (hint) kids.push(<div key="hint" className="small muted">{hint}</div>);
  return <div className="card opts" hidden={!kids.length}>{kids}</div>;
}

// ------------------------------------------------------------------ share & save

function CopyRow({ label, value }: { label: string; value: string }) {
  const ref = useRef<HTMLInputElement>(null);
  return (
    <div className="share-row">
      <span className="small muted">{label}</span>
      <input ref={ref} className="input" readOnly value={value} />
      <button className="btn small" onClick={async () => {
        try { await navigator.clipboard.writeText(value); toast("Copied"); } catch { ref.current?.select(); }
      }}>Copy</button>
    </div>
  );
}

function Share({ c }: { c: Committed }) {
  const abs = (p: string) => window.location.origin + url(p);
  const base = { targets: c.spec.targets, from: c.from, until: c.until, step: c.step };
  const embedQ = new URLSearchParams(c.href.slice(c.href.indexOf("?") + 1));
  embedQ.delete("dash");
  embedQ.delete("panel");
  const embed = abs("/embed?" + embedQ.toString());
  const chart = c.spec.chart === "area" || c.spec.chart === "bar" ? c.spec.chart : "line";
  const svgUrl = abs(renderUrl(base, "svg")) + `&chart=${chart}${c.spec.title ? "&title=" + encodeURIComponent(c.spec.title) : ""}`;
  return (
    <div className="card share">
      <div className="panel-title">Share &amp; embed</div>
      <CopyRow label="This view" value={window.location.origin + c.href} />
      <CopyRow label="Embed page" value={embed} />
      <CopyRow label="iframe" value={`<iframe src="${embed}" width="800" height="360" frameborder="0"></iframe>`} />
      <CopyRow label="SVG image" value={svgUrl} />
      <CopyRow label="JSON (Graphite)" value={abs(renderUrl(base, "json"))} />
      <CopyRow label="CSV" value={abs(renderUrl(base, "csv"))} />
    </div>
  );
}

interface SaveInput {
  spec: PanelSpec;
  from: string;
  until: string;
  step: string;
  title: string;
  subtitle: string;
}

function SaveBox({ chart, dash, panelIdx, build }: { chart: ChartType; dash: string; panelIdx: string | null; build: () => SaveInput }) {
  const [dashes, setDashes] = useState<DashboardSummary[]>([]);
  const [sel, setSel] = useState(dash);
  const [name, setName] = useState("");
  const [w] = useState(() => (chart === "stat" ? 3 : 6));
  const [h] = useState(() => (chart === "stat" ? 2 : 3));
  const [width, setWidth] = useState(String(w));
  const [height, setHeight] = useState(String(h));
  useEffect(() => { listDashboards().then(setDashes).catch(() => { /* offline */ }); }, []);
  const editing = Boolean(dash && panelIdx != null);

  const save = async () => {
    const target = sel || slugify(name);
    if (!target) return toast("Pick or name a dashboard", "error");
    try {
      const exists = dashes.some((x) => x.name === target);
      const { spec: s, from, until, step, title, subtitle } = build();
      const d: DashboardSpec = exists ? await getDashboard(target) : { title: name.trim() || target, from, until, panels: [] };
      const opts = s.options ?? {};
      const p: PanelSpec = {
        title: title || s.targets?.[0] || "Untitled", subtitle: subtitle || undefined, chart: s.chart, targets: s.targets, step: step || undefined,
        w: Number(width), h: Number(height), options: Object.keys(opts).length ? opts : undefined,
      };
      if (from !== d.from || until !== (d.until || "now")) {
        p.from = from;
        if (until !== "now") p.until = until;
      }
      const idx = Number(panelIdx);
      const old = d.panels[idx];
      if (editing && target === dash && old) {
        p.w = old.w ?? p.w;
        p.h = old.h ?? p.h;
        d.panels[idx] = p;
      } else d.panels.push(p);
      await saveDashboard(target, d);
      window.location.href = url(`/d/${encodeURIComponent(target)}`);
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  };


  return (
    <div className="card share">
      <div className="panel-title">Save</div>
      <div className="row gap" style={{ flexWrap: "wrap" }}>
        <select className="select" value={sel} onChange={(e) => setSel(e.target.value)}>
          <option value="">New dashboard…</option>
          {dashes.map((d) => <option key={d.name} value={d.name}>{d.title || d.name}</option>)}
        </select>
        <input className="input" placeholder="new-dashboard-name" hidden={Boolean(sel)} value={name} onChange={(e) => setName(e.target.value)} />
        <select className="select" title="Width (of 12 columns)" value={width} onChange={(e) => setWidth(e.target.value)}>
          {([[3, "¼ width"], [4, "⅓ width"], [6, "½ width"], [8, "⅔ width"], [12, "Full width"]] as const).map(([v, l]) => <option key={v} value={String(v)}>{l}</option>)}
        </select>
        <select className="select" title="Height" value={height} onChange={(e) => setHeight(e.target.value)}>
          {([[2, "Short"], [3, "Medium"], [4, "Tall"], [6, "Very tall"]] as const).map(([v, l]) => <option key={v} value={String(v)}>{l}</option>)}
        </select>
        <button className="btn primary" onClick={() => void save()}>{editing ? `Update panel in “${dash}”` : "Add to dashboard"}</button>
      </div>
    </div>
  );
}
