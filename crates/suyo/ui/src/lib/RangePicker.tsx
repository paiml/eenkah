// Time range dropdown: preset rows plus a custom from/until footer.

import { useEffect, useRef, useState } from "react";
import { RANGES, rangeLabel } from "./time";

export interface RangePickerProps {
  from: string;
  until: string;
  onChange: (from: string, until: string) => void;
}

export function RangePicker({ from, until, onChange }: RangePickerProps) {
  const [open, setOpen] = useState(false);
  const [fIn, setFIn] = useState(from);
  const [uIn, setUIn] = useState(until || "now");
  const wrap = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const outside = (e: PointerEvent) => {
      if (e.target instanceof Node && !wrap.current?.contains(e.target)) setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);

  const toggle = () => {
    if (!open) {
      setFIn(from);
      setUIn(until || "now");
    }
    setOpen(!open);
  };
  const pick = (f: string, u: string) => {
    onChange(f, u);
    setOpen(false);
  };
  const apply = () => pick(fIn.trim() || "-1d", uIn.trim() || "now");

  return (
    <div className="range-wrap" ref={wrap}>
      <button className="btn range-btn" aria-haspopup="true" onClick={toggle}>
        <span className="icon" aria-hidden="true">◷</span>
        {rangeLabel(from, until)}
      </button>
      {open && (
        <div className="menu range-menu">
          <div className="menu-rows">
            {RANGES.map(([l, f, u]) => {
              const on = f === from && u === (until || "now");
              return (
                <button key={l} className={"menu-row" + (on ? " on" : "")} onClick={() => pick(f, u)}>
                  <span className="check" aria-hidden="true">{on ? "✓" : ""}</span>
                  {l}
                </button>
              );
            })}
          </div>
          <div className="menu-foot">
            <div className="small muted">Custom range</div>
            <div className="row gap">
              <input className="input mono" value={fIn} aria-label="From" placeholder="-7d, now-1M/M, 2026-01-01" onChange={(e) => setFIn(e.target.value)} onKeyDown={(e) => e.key === "Enter" && apply()} />
              <input className="input mono" value={uIn} aria-label="Until" placeholder="now" onChange={(e) => setUIn(e.target.value)} onKeyDown={(e) => e.key === "Enter" && apply()} />
              <button className="btn primary" onClick={apply}>Apply</button>
            </div>
            <div className="small muted">Relative: -7d · -1mon · now-1M/M · now/w  Absolute: 2026-01-31 or 2026-01-31 08:00</div>
          </div>
        </div>
      )}
    </div>
  );
}
