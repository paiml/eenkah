//! Built-in starter dashboards.

pub const SYSTEM_DASHBOARD: &str = r##"{
  "title": "System",
  "description": "Host metrics from `suyo serve --collect 10s`.",
  "from": "-1h",
  "until": "now",
  "refresh": 30,
  "panels": [
    {"chart": "stat", "w": 3, "title": "CPU", "targets": ["averageSeries(host.*.cpu.percent)"], "options": {"stat": "last", "compare": "avg", "better": "down", "decimals": 1}},
    {"chart": "stat", "w": 3, "title": "Memory used", "targets": ["averageSeries(host.*.mem.used_percent)"], "options": {"stat": "last", "compare": "avg", "better": "down", "decimals": 1}},
    {"chart": "stat", "w": 3, "title": "Load (1m)", "targets": ["sumSeries(host.*.load.1m)"], "options": {"stat": "last", "compare": "avg", "better": "down", "decimals": 2}},
    {"chart": "stat", "w": 3, "title": "Network in", "targets": ["sumSeries(host.*.net.rx)"], "options": {"stat": "last", "compare": "avg", "decimals": 2}},
    {"chart": "line", "w": 12, "h": 3, "title": "CPU and memory, overlaid", "subtitle": "Both are percentages, so they share one axis",
     "targets": ["alias(host.*.cpu.percent, \"CPU\")", "alias(host.*.mem.used_percent, \"Memory\")"], "options": {"yMin": 0, "yMax": 100}},
    {"chart": "area", "w": 6, "h": 3, "title": "CPU breakdown", "targets": ["stacked(aliasByMetric(host.*.cpu.{user,system,iowait}))"]},
    {"chart": "line", "w": 6, "h": 3, "title": "Network", "targets": ["aliasByMetric(host.*.net.{rx,tx})"]},
    {"chart": "line", "w": 12, "h": 3, "from": "-7d", "title": "CPU this week vs last week",
     "targets": ["alias(host.*.cpu.percent, \"This week\")", "alias(dim(timeShift(host.*.cpu.percent, \"7d\")), \"Last week\")"]}
  ]
}"##;

pub const DEMO_DASHBOARD: &str = r##"{
  "title": "Demo",
  "description": "Generated data — every chart type, no data required.",
  "from": "-7d",
  "until": "now",
  "panels": [
    {"chart": "text", "w": 12, "text": "Every panel here is a set of Graphite-style targets plus a chart type. Click a panel title to open it in the composer, edit the functions, and save it into any dashboard."},
    {"chart": "stat", "w": 4, "title": "Requests / min", "targets": ["randomWalk(\"api.requests\")"], "options": {"compare": "avg", "better": "up"}},
    {"chart": "stat", "w": 4, "title": "p95 latency", "targets": ["scale(randomWalk(\"api.latency\"), 4)"], "options": {"compare": "avg", "better": "down", "unit": "ms"}},
    {"chart": "stat", "w": 4, "title": "Error rate", "targets": ["scale(randomWalk(\"api.errors\"), 0.02)"], "options": {"compare": "avg", "better": "down", "unit": "%", "decimals": 2}},
    {"chart": "line", "w": 8, "h": 3, "title": "Three services, one highlighted",
     "targets": ["highlight(group(alias(randomWalk(\"svc.checkout\"), \"checkout\"), alias(randomWalk(\"svc.search\"), \"search\"), alias(randomWalk(\"svc.auth\"), \"auth\")), \"checkout\")"]},
    {"chart": "histogram", "w": 4, "h": 3, "title": "Distribution", "targets": ["alias(randomWalk(\"svc.checkout\"), \"checkout\")"]},
    {"chart": "area", "w": 6, "h": 3, "title": "Stacked area", "targets": ["stacked(alias(randomWalk(\"a\"), \"A\"))", "stacked(alias(randomWalk(\"b\"), \"B\"))", "stacked(alias(randomWalk(\"c\"), \"C\"))"]},
    {"chart": "bar", "w": 6, "h": 3, "step": "1d", "title": "Daily totals vs the week before",
     "targets": ["alias(summarize(randomWalk(\"orders\"), \"1d\", \"sum\"), \"This week\")", "alias(dim(timeShift(summarize(randomWalk(\"orders\"), \"1d\", \"sum\"), \"7d\")), \"Last week\")"]},
    {"chart": "streams", "w": 8, "h": 6, "from": "-1d", "title": "Streams: one lane per unit, drag to measure a section",
     "targets": ["alias(scale(randomWalk(\"ride.power\"), 20), \"Power\")", "alias(offset(randomWalk(\"ride.hr\"), 120), \"Heart rate\")", "alias(offset(randomWalk(\"ride.cad\"), 85), \"Cadence\")"]},
    {"chart": "status", "w": 4, "h": 3, "title": "Status lights", "targets": ["alias(randomWalk(\"api.requests\"), \"Requests\")", "alias(randomWalk(\"api.latency\"), \"Latency\")", "alias(randomWalk(\"api.errors\"), \"Errors\")"], "options": {"better": "up,down,down"}},
    {"chart": "quadrant", "w": 4, "h": 3, "step": "1h", "title": "Quadrant: latency vs errors",
     "targets": ["alias(randomWalk(\"api.latency\"), \"Latency\")", "alias(randomWalk(\"api.errors\"), \"Errors\")"],
     "options": {"xBetter": "down", "yBetter": "down", "q1": "slow and failing", "q3": "fast and clean"}}
  ]
}"##;
