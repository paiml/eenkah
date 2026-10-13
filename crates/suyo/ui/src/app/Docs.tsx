import { useEffect, useState } from "react";
import { listFunctions, ThemePicker, url, type FunctionDoc } from "../lib";
import { Header } from "./Header";

const Code = ({ children }: { children: string }) => <code>{children}</code>;
const TryLink = ({ t }: { t: string }) => <a href={url("/?target=" + encodeURIComponent(t) + "&from=-90d")}><code>{t}</code></a>;

const RENDER_API = `GET /render?target=<expr>[&target=...]&from=-7d&until=now&format=json

format    json (Graphite-compatible) · suyo (richer JSON for the UI) · csv · svg · html
from      -7d · -1mon · now-1M/M · now/w · today · yesterday · 2026-01-31 · 2026-01-31 08:00 · epoch
until     same syntax; rounding (/d /w /M /y) rounds up to the end of the unit
step      1min 5min 1h 1d 1w 1mon 1y — fixed bucket size (default: auto from maxDataPoints)
tz        IANA zone for calendar buckets, e.g. Europe/Madrid (default: server --tz)
maxDataPoints  approximate points per series when step is auto (default 800)

format=svg extras: chart=line|area|bar|stacked  title=  width=  height=  theme=light|dark|paper|nord|midnight  yMin= yMax= legend=hide`;

const CHART_TYPES = `line, area, bar      time series; stacked() series stack; asThreshold()/threshold() draw reference lines
stat                  big number + delta + sparkline   options: stat=last|avg|sum|max|min compare=avg|first|series|none better=up|down decimals=
table                 latest / average / min / max / change per target
scatter               target A on x, target B on y, joined on time — with fit line and r
histogram             distribution of values
slope                 first vs last bucket per series (e.g. step=1mon over two months)
calendar              one cell per day (step=1d)
hourly                hour-of-day × weekday averages (step=1h)
waffle                shares of the latest bucket (parts of a whole — instead of a pie)`;

const EMBEDDING = `<iframe src="http://host:8080/embed?target=health.daily.rhr&from=-90d&chart=line&title=Resting%20HR&theme=dark"></iframe>
<img src="http://host:8080/render?target=health.daily.steps&from=-30d&step=1d&format=svg&chart=bar&title=Steps">`;

const DATA_IN = `# HTTP JSON (ts: epoch s/ms or date string; unit/agg optional)
curl -X POST localhost:8080/api/ingest -d '{"series":"home.temp.office","ts":1760000000,"value":21.5,"unit":"°C"}'
curl -X POST localhost:8080/api/ingest -d '[{"series":"x.y","points":[[1760000000,1],[1760000060,2]],"agg":"sum"}]'

# Graphite plaintext (HTTP, stdin, or TCP with serve --carbon 0.0.0.0:2003)
echo "host.web1.requests 42 $(date +%s)" | curl -X POST --data-binary @- localhost:8080/api/ingest/plaintext
echo "host.web1.requests 42 $(date +%s)" | suyo ingest

# CSV: a ts/date column plus one column per series
suyo import-csv budget.csv --prefix finance --unit spend=EUR --agg spend=sum

# Any SQLite database, incrementally. sources.toml:
#   [[source]]
#   name = "orders"
#   db = "~/shop.db"
#   prefix = "shop"
#   query = "SELECT created AS ts, total, items FROM orders WHERE created >= :since"
#   units = { total = "EUR" }
#   agg = { total = "sum", items = "sum" }
suyo sync --sources sources.toml
suyo serve --sources sources.toml --sync-every 15m

# This machine's CPU / memory / load / network
suyo serve --collect 10s`;

export function Docs() {
  const [funcs, setFuncs] = useState<FunctionDoc[]>([]);
  const [search, setSearch] = useState("");
  useEffect(() => {
    document.title = "Docs · suyo";
    listFunctions().then(setFuncs).catch(() => setFuncs([]));
  }, []);
  const s = search.toLowerCase();
  const shown = funcs.filter((f) => !s || f.name.toLowerCase().includes(s) || f.doc.toLowerCase().includes(s) || f.group.toLowerCase().includes(s));
  return (
    <>
      <Header active="/docs">
        <ThemePicker />
      </Header>
      <div className="docs">
        <h1>suyo reference</h1>
        <p>Every chart is a URL. Targets are Graphite-style expressions — series paths with globs, wrapped in functions — and every other query argument controls the time range, bucketing and rendering.</p>
        <h2>Render API</h2>
        <pre>{RENDER_API}</pre>
        <p>
          Paths: <Code>health.daily.rhr</Code> · <Code>health.daily.*</Code> · <Code>{"health.sleep.{deep,rem}"}</Code> · <Code>host.*.cpu.percent</Code> · <Code>health.**</Code> (any depth).
        </p>
        <p>
          Buckets are calendar-aware in the view's timezone: a 1d bucket is local midnight to midnight, 1w starts Monday, 1mon is the calendar month. Each series has a default consolidation (avg, or sum for counts like steps) used when many points fall in one bucket; change it with <Code>consolidateBy</Code> or per series with <Code>suyo meta NAME --agg sum</Code>.
        </p>
        <h2>Chart types</h2>
        <pre>{CHART_TYPES}</pre>
        <p>Different units never share a second axis. Put them in separate panels, or bring them to one scale with <Code>indexed()</Code> or <Code>zscore()</Code>.</p>
        <h2>Embedding</h2>
        <pre>{EMBEDDING}</pre>
        <h2>Getting data in</h2>
        <pre>{DATA_IN}</pre>
        <h2>Graphite compatibility</h2>
        <p>
          <Code>/render?format=json</Code>, <Code>/metrics/find</Code>, <Code>/metrics/expand</Code> and <Code>/metrics/index.json</Code> follow Graphite's shapes, so Graphite clients and Grafana's Graphite data source can read from suyo.
        </p>
        <h2>Functions</h2>
        <input className="input" placeholder="Search functions…" style={{ width: "100%", margin: "8px 0" }} value={search} onChange={(e) => setSearch(e.target.value)} />
        <div className="card">
          {shown.map((f, i) => (
            <div key={f.name}>
              {f.group !== shown[i - 1]?.group && <div className="fn-group">{f.group}</div>}
              <div className="fn" id={f.name}>
                <div className="sig">{f.signature}</div>
                <div>{f.doc}</div>
                <div className="ex">Try: <TryLink t={f.example} /></div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </>
  );
}
