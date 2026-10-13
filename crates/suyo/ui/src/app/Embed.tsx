// /embed?target=...&chart=...&title=...&theme=... — a single chart, no chrome.

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Panel, type ChartType, type PanelSpec } from "../lib";
import { OPTION_KEYS } from "./options";

export function Embed() {
  const q = new URLSearchParams(window.location.search);
  const [spec] = useState<PanelSpec>(() => ({
    targets: q.getAll("target"),
    chart: (q.get("chart") || "line") as ChartType,
    step: q.get("step") || undefined,
    options: Object.fromEntries(OPTION_KEYS.filter((k) => q.get(k) != null).map((k) => [k, q.get(k) ?? ""])),
  }));
  const title = q.get("title"), subtitle = q.get("subtitle");
  const range = { from: q.get("from") || "-1d", until: q.get("until") || "now" };
  const head = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(0);
  const [signal, setSignal] = useState(0);

  useLayoutEffect(() => {
    document.body.classList.add("embed");
    if (title) document.title = title;
    const measure = () => setHeight(Math.max(120, window.innerHeight - (head.current?.offsetHeight ?? 0) - 70));
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [title]);

  useEffect(() => {
    const every = Number(q.get("refresh"));
    if (!(every > 0)) return;
    const t = setInterval(() => setSignal((n) => n + 1), Math.max(5, every) * 1000);
    return () => clearInterval(t);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="embed-root">
      <div ref={head}>
        {title && <div className="panel-title">{title}</div>}
        {subtitle && <div className="panel-sub">{subtitle}</div>}
      </div>
      {height > 0 && <Panel spec={spec} range={range} chrome={false} card={false} height={height} refreshSignal={signal} style={{ padding: 0 }} />}
    </div>
  );
}
