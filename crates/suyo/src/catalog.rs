//! Function reference served at /api/functions (autocomplete + /docs).

use serde::Serialize;

#[derive(Serialize)]
pub struct FnDoc {
    pub name: &'static str,
    pub signature: &'static str,
    pub group: &'static str,
    pub doc: &'static str,
    pub example: &'static str,
}

macro_rules! f {
    ($n:expr, $s:expr, $g:expr, $d:expr, $e:expr) => {
        FnDoc {
            name: $n,
            signature: $s,
            group: $g,
            doc: $d,
            example: $e,
        }
    };
}

pub const FUNCTIONS: &[FnDoc] = &[
    // Combine
    f!("sumSeries", "sumSeries(*seriesLists)", "Combine", "Adds series together point by point. Alias: sum.", "sumSeries(host.*.cpu.percent)"),
    f!("averageSeries", "averageSeries(*seriesLists)", "Combine", "Mean of the series at each point. Alias: avg.", "averageSeries(host.*.mem.used_percent)"),
    f!("minSeries", "minSeries(*seriesLists)", "Combine", "Lowest value across series at each point.", "minSeries(health.daily.{hr_min,rhr})"),
    f!("maxSeries", "maxSeries(*seriesLists)", "Combine", "Highest value across series at each point.", "maxSeries(health.daily.hr_max)"),
    f!("medianSeries", "medianSeries(*seriesLists)", "Combine", "Median across series at each point.", "medianSeries(host.*.load.1m)"),
    f!("rangeSeries", "rangeSeries(*seriesLists)", "Combine", "Max minus min across series at each point.", "rangeSeries(health.daily.{bb_max,bb_min})"),
    f!("diffSeries", "diffSeries(minuend, *subtrahends)", "Combine", "First series minus the rest.", "diffSeries(health.daily.bb_max, health.daily.bb_min)"),
    f!("divideSeries", "divideSeries(dividend, divisor)", "Combine", "Divides each dividend series by a single divisor series.", "divideSeries(health.daily.calories_active, health.daily.calories_total)"),
    f!("multiplySeries", "multiplySeries(*seriesLists)", "Combine", "Multiplies series point by point.", "multiplySeries(a.b, c.d)"),
    f!("stddevSeries", "stddevSeries(*seriesLists)", "Combine", "Standard deviation across series at each point.", "stddevSeries(host.*.cpu.percent)"),
    f!("countSeries", "countSeries(*seriesLists)", "Combine", "How many series have data at each point.", "countSeries(host.*.cpu.percent)"),
    f!("percentileOfSeries", "percentileOfSeries(seriesList, n)", "Combine", "The n-th percentile across series at each point.", "percentileOfSeries(host.*.cpu.percent, 95)"),
    f!("aggregate", "aggregate(seriesList, func)", "Combine", "Combine with a named function: sum, avg, min, max, median, count, range, stddev, multiply, diff.", "aggregate(host.*.cpu.percent, \"max\")"),
    f!("groupByNode", "groupByNode(seriesList, nodeNum, func=\"avg\")", "Combine", "Groups series by a path segment and combines each group.", "groupByNode(host.*.cpu.*, 3, \"avg\")"),
    f!("group", "group(*seriesLists)", "Combine", "Puts several series lists into one list.", "group(health.daily.rhr, health.daily.hr_min)"),
    f!("asPercent", "asPercent(seriesList, total=None)", "Combine", "Each series as a percent of a number, a total series, or the sum of the list.", "asPercent(health.sleep.{deep_h,light_h,rem_h,awake_h})"),
    // Transform
    f!("summarize", "summarize(seriesList, interval, func=<series agg>)", "Transform", "Re-buckets into calendar-aware intervals (1h, 1d, 1w, 1mon, 1y) in the view's timezone. Uses exact raw aggregates when available.", "summarize(health.daily.steps, \"1w\", \"sum\")"),
    f!("consolidateBy", "consolidateBy(seriesList, func)", "Transform", "How raw points collapse into each bucket: avg, sum, min, max, count, last.", "consolidateBy(health.hr, \"max\")"),
    f!("movingAverage", "movingAverage(seriesList, window)", "Transform", "Rolling mean over a point count or time (\"7d\"). Prefetches the window so the first point is complete.", "movingAverage(health.daily.rhr, \"7d\")"),
    f!("movingMedian", "movingMedian(seriesList, window)", "Transform", "Rolling median — robust to spikes.", "movingMedian(health.hr, \"1h\")"),
    f!("movingSum", "movingSum(seriesList, window)", "Transform", "Rolling sum.", "movingSum(health.daily.intensity_minutes, \"7d\")"),
    f!("movingMin", "movingMin(seriesList, window)", "Transform", "Rolling minimum.", "movingMin(health.daily.rhr, \"30d\")"),
    f!("movingMax", "movingMax(seriesList, window)", "Transform", "Rolling maximum.", "movingMax(health.daily.hr_max, \"30d\")"),
    f!("movingWindow", "movingWindow(seriesList, window, func=\"avg\")", "Transform", "Rolling window with any of avg, median, sum, min, max, stddev.", "movingWindow(health.stress, \"2h\", \"max\")"),
    f!("exponentialMovingAverage", "exponentialMovingAverage(seriesList, window)", "Transform", "EMA with alpha = 2/(n+1). Alias: ema.", "exponentialMovingAverage(health.daily.rhr, 14)"),
    f!("stdev", "stdev(seriesList, window)", "Transform", "Rolling standard deviation.", "stdev(health.daily.rhr, \"14d\")"),
    f!("timeShift", "timeShift(seriesList, shift)", "Transform", "Shows the series as it was `shift` ago, aligned to now — compare this month with last year. Calendar-aware.", "timeShift(health.daily.steps, \"1y\")"),
    f!("timeSlice", "timeSlice(seriesList, start, end=\"now\")", "Transform", "Nulls out data outside a time window.", "timeSlice(health.hr, \"-2h\")"),
    f!("scale", "scale(seriesList, factor)", "Transform", "Multiplies every value.", "scale(health.daily.distance, 0.621371)"),
    f!("offset", "offset(seriesList, amount)", "Transform", "Adds a constant.", "offset(a.b, -100)"),
    f!("absolute", "absolute(seriesList)", "Transform", "Absolute value.", "absolute(a.b)"),
    f!("invert", "invert(seriesList)", "Transform", "1/x.", "invert(a.b)"),
    f!("pow", "pow(seriesList, power)", "Transform", "Raises to a power.", "pow(a.b, 2)"),
    f!("squareRoot", "squareRoot(seriesList)", "Transform", "Square root.", "squareRoot(a.b)"),
    f!("log", "log(seriesList, base=10)", "Transform", "Logarithm.", "log(a.b, 2)"),
    f!("round", "round(seriesList, precision=0)", "Transform", "Rounds values.", "round(health.daily.distance, 1)"),
    f!("clamp", "clamp(seriesList, min, max)", "Transform", "Clamps values to a range.", "clamp(health.stress, 0, 100)"),
    f!("derivative", "derivative(seriesList)", "Transform", "Change from the previous point.", "derivative(health.weight.kg)"),
    f!("nonNegativeDerivative", "nonNegativeDerivative(seriesList)", "Transform", "Derivative that drops negative steps (counter resets).", "nonNegativeDerivative(net.bytes_in)"),
    f!("perSecond", "perSecond(seriesList)", "Transform", "Rate per second from a counter.", "perSecond(net.bytes_in)"),
    f!("integral", "integral(seriesList)", "Transform", "Running total.", "integral(health.daily.steps)"),
    f!("delay", "delay(seriesList, steps)", "Transform", "Shifts values forward by N points.", "delay(a.b, 1)"),
    f!("keepLastValue", "keepLastValue(seriesList, limit=inf)", "Transform", "Fills gaps with the last seen value.", "keepLastValue(health.weight.kg)"),
    f!("interpolate", "interpolate(seriesList)", "Transform", "Fills gaps by linear interpolation.", "interpolate(health.weight.kg)"),
    f!("transformNull", "transformNull(seriesList, default=0)", "Transform", "Replaces nulls with a value.", "transformNull(health.daily.steps, 0)"),
    f!("isNonNull", "isNonNull(seriesList)", "Transform", "1 where data exists, 0 otherwise — shows coverage.", "isNonNull(health.hr)"),
    f!("removeBelowValue", "removeBelowValue(seriesList, n)", "Transform", "Drops points below n.", "removeBelowValue(health.stress, 0)"),
    f!("removeAboveValue", "removeAboveValue(seriesList, n)", "Transform", "Drops points above n.", "removeAboveValue(health.hr, 220)"),
    f!("linearRegression", "linearRegression(seriesList)", "Transform", "Least-squares trend line over the view. Alias: trend.", "linearRegression(health.daily.rhr)"),
    f!("indexed", "indexed(seriesList, base=100)", "Transform", "Rebases each series so its first value is 100 — compare series with different units on one axis instead of a dual axis.", "indexed(group(health.daily.rhr, health.daily.stress_avg))"),
    f!("zscore", "zscore(seriesList)", "Transform", "Standard score — how unusual each point is for that series.", "zscore(health.daily.rhr)"),
    f!("nPercentile", "nPercentile(seriesList, n)", "Transform", "A flat line at the series' n-th percentile.", "nPercentile(health.hr, 95)"),
    // Filter
    f!("highestAverage", "highestAverage(seriesList, n=1)", "Filter", "Top n series by average. Also highestMax, highestCurrent, highestMin.", "highestAverage(host.*.cpu.percent, 3)"),
    f!("lowestAverage", "lowestAverage(seriesList, n=1)", "Filter", "Bottom n by average. Also lowestMax, lowestCurrent, lowestMin.", "lowestAverage(host.*.cpu.percent, 3)"),
    f!("currentAbove", "currentAbove(seriesList, n)", "Filter", "Series whose latest value is above n. Also currentBelow, averageAbove/Below, maximumAbove/Below, minimumAbove/Below.", "currentAbove(host.*.cpu.percent, 80)"),
    f!("limit", "limit(seriesList, n)", "Filter", "First n series.", "limit(host.*.cpu.percent, 5)"),
    f!("exclude", "exclude(seriesList, regex)", "Filter", "Drops series whose name matches.", "exclude(health.daily.*, \"goal\")"),
    f!("grep", "grep(seriesList, regex)", "Filter", "Keeps series whose name matches.", "grep(health.daily.*, \"^health.daily.bb\")"),
    f!("removeEmptySeries", "removeEmptySeries(seriesList)", "Filter", "Drops series without data in the view.", "removeEmptySeries(health.activity.*.distance)"),
    f!("sortByName", "sortByName(seriesList, natural=false, reverse=false)", "Filter", "Sorts by name.", "sortByName(host.*.cpu.percent)"),
    f!("sortByMaxima", "sortByMaxima(seriesList)", "Filter", "Sorts by peak, highest first. Also sortByMinima, sortByTotal.", "sortByMaxima(host.*.cpu.percent)"),
    // Naming
    f!("alias", "alias(seriesList, name)", "Naming", "Renames; %s is replaced by the old name.", "alias(health.daily.rhr, \"Resting HR\")"),
    f!("aliasByNode", "aliasByNode(seriesList, *nodes)", "Naming", "Names each series by path segments (0-based, negatives from the end).", "aliasByNode(health.daily.*, -1)"),
    f!("aliasByMetric", "aliasByMetric(seriesList)", "Naming", "Names each series by its last path segment.", "aliasByMetric(health.sleep.*)"),
    f!("aliasSub", "aliasSub(seriesList, regex, replace)", "Naming", "Regex rename; \\1 refers to groups.", "aliasSub(health.sleep.*, \"health.sleep.(.*)_h\", \"\\1\")"),
    // Style
    f!("color", "color(seriesList, color)", "Style", "Fixes a series color (hex or CSS name). Prefer the theme palette.", "color(health.daily.rhr, \"#e34948\")"),
    f!("highlight", "highlight(seriesList, regex=None)", "Style", "Emphasises matching series and grays out the rest — focus attention on one story.", "highlight(aliasByNode(health.daily.{rhr,hr_min,hr_max},-1), \"rhr\")"),
    f!("dim", "dim(seriesList)", "Style", "Draws series in the muted gray (context, not focus).", "dim(timeShift(health.daily.rhr, \"1y\"))"),
    f!("dashed", "dashed(seriesList)", "Style", "Dashed line.", "dashed(linearRegression(health.daily.rhr))"),
    f!("lineWidth", "lineWidth(seriesList, width)", "Style", "Line thickness in px.", "lineWidth(health.daily.rhr, 3)"),
    f!("stacked", "stacked(seriesList)", "Style", "Stacks these series (area or bar charts).", "stacked(health.sleep.{deep_h,light_h,rem_h})"),
    f!("asThreshold", "asThreshold(seriesList)", "Style", "Draws a series as a reference line (e.g. a daily goal) on bar and line charts.", "asThreshold(health.daily.step_goal)"),
    f!("unit", "unit(seriesList, unit)", "Style", "Sets the unit shown on axes and tooltips.", "unit(scale(health.daily.distance, 0.621), \"mi\")"),
    // Generators
    f!("threshold", "threshold(value, label=None, color=None)", "Generate", "A labelled reference line (goal, limit).", "threshold(10000, \"Step goal\")"),
    f!("constantLine", "constantLine(value)", "Generate", "A flat line.", "constantLine(60)"),
    f!("randomWalk", "randomWalk(name, step=60)", "Generate", "Deterministic demo data.", "randomWalk(\"demo\")"),
    f!("sinFunction", "sinFunction(name, amplitude=1, step=60)", "Generate", "A sine wave for testing.", "sinFunction(\"wave\", 10)"),
];
