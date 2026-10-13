// One floating tooltip for the whole page. Charts call tooltip.show()/hide(); the <Tooltip/>
// host renders it. If the host app does not mount <Tooltip/>, one is mounted on first use.

import { useLayoutEffect, useRef, useSyncExternalStore } from "react";
import { createRoot } from "react-dom/client";
import { fmtNum, unitSuffix } from "./format";

export interface TooltipRow {
  color: string;
  name: string;
  value: number | null | undefined;
  unit?: string;
  strong?: boolean;
  dashed?: boolean;
}

interface TooltipState {
  x: number;
  y: number;
  title: string;
  rows: TooltipRow[];
}

let state: TooltipState | null = null;
const subs = new Set<() => void>();
let mounted = 0;
let autoHost: HTMLElement | null = null;

const notify = (): void => subs.forEach((f) => f());

export interface PointerLike {
  clientX: number;
  clientY: number;
}

export const tooltip = {
  show(ev: PointerLike, title: string, rows: TooltipRow[]): void {
    if (!mounted && !autoHost) {
      autoHost = document.createElement("div");
      document.body.append(autoHost);
      createRoot(autoHost).render(<Tooltip />);
    }
    state = { x: ev.clientX, y: ev.clientY, title, rows };
    notify();
  },
  hide(): void {
    if (state) {
      state = null;
      notify();
    }
  },
};

const subscribe = (f: () => void): (() => void) => {
  subs.add(f);
  return () => {
    subs.delete(f);
  };
};
const snapshot = (): TooltipState | null => state;

/** Tooltip host. Mount once (the app does); charts drive it through `tooltip`. */
export function Tooltip() {
  const s = useSyncExternalStore(subscribe, snapshot, snapshot);
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    mounted++;
    return () => {
      mounted--;
    };
  }, []);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !s) return;
    const r = el.getBoundingClientRect();
    let x = s.x + 14;
    let y = s.y + 14;
    if (x + r.width > window.innerWidth - 8) x = s.x - r.width - 14;
    if (y + r.height > window.innerHeight - 8) y = s.y - r.height - 14;
    el.style.transform = `translate(${Math.max(4, x)}px, ${Math.max(4, y)}px)`;
  });
  if (!s) return <div className="tooltip" role="status" ref={ref} style={{ display: "none" }} />;
  const rows = s.rows.filter((r) => r.value != null || r.strong);
  return (
    <div className="tooltip" role="status" ref={ref} style={{ display: "block" }}>
      <div className="tt-title">{s.title}</div>
      {rows.map((r, i) => (
        <div key={i} className={"tt-row" + (r.strong ? " strong" : "")}>
          <span className={"key key-line" + (r.dashed ? " dashed" : "")} style={{ background: r.color, borderColor: r.color }} />
          <span className="tt-val">{r.value == null ? "—" : fmtNum(r.value) + (r.unit ? unitSuffix(r.unit) : "")}</span>
          <span className="tt-name">{r.name}</span>
        </div>
      ))}
      {!rows.length && <div className="tt-row muted">No data</div>}
    </div>
  );
}
