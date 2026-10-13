import { useState, type ReactNode } from "react";
import { Dashboard, getDashboard, RangePicker, type DashboardSpec, type Range } from "suyo-ui";
import { useLoad } from "../hooks";

/** A eenkah view backed by an editable suyo dashboard, with its own time range. */
export function DashboardPage({ name, intro, outro }: { name: string; intro?: ReactNode; outro?: ReactNode }) {
  const [spec, reload] = useLoad<DashboardSpec>(() => getDashboard(name), [name]);
  const [range, setRange] = useState<Range | null>(null);
  const [lock, setLock] = useState(false);
  if (spec.state === "loading") return <main className="page muted">Loading…</main>;
  if (spec.state === "error") return <main className="page error">Could not load the “{name}” view: {spec.error}</main>;
  const own: Range = { from: spec.data.from ?? "-30d", until: spec.data.until ?? "now" };
  const current = range ?? own;
  return (
    <main className="page">
      <Dashboard
        intro={intro}
        outro={outro}
        name={name}
        spec={spec.data}
        editable
        range={current}
        lock={lock}
        onRangeChange={(r, l) => {
          setRange(r);
          setLock(l);
        }}
        onSaved={reload}
        headExtra={
          <>
            {range && (
              <button
                className="btn small"
                onClick={() => {
                  setRange(null);
                  setLock(false);
                }}
              >
                Reset range
              </button>
            )}
            <RangePicker
              from={current.from}
              until={current.until}
              onChange={(from, until) => {
                setRange({ from, until });
                setLock(false);
              }}
            />
          </>
        }
      />
    </main>
  );
}
