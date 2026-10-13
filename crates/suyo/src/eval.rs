//! Evaluates target expressions into series. Function names and semantics
//! follow Graphite where one exists; a few additions (indexed, zscore,
//! highlight, threshold, unit) exist to support honest single-axis charts.

use crate::parser::{Arg, Expr};
use crate::series::{align, Series, Style};
use crate::store::{AggFn, SeriesMeta, Store};
use crate::timeutil::{Bucket, Span, Zone};
use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use std::collections::BTreeMap;

pub struct Ctx<'a> {
    pub store: &'a Store,
    pub index: &'a [SeriesMeta],
    pub from: i64,
    pub until: i64,
    pub zone: Zone,
    /// Requested consolidation bucket (auto from maxDataPoints or `step=`).
    pub bucket: Bucket,
    /// When set (`step=` given), every series uses exactly `bucket`.
    pub forced: bool,
    pub depth: u32,
}

impl<'a> Ctx<'a> {
    fn shifted(&self, from: i64, until: i64) -> Ctx<'a> {
        Ctx {
            store: self.store,
            index: self.index,
            from,
            until,
            zone: self.zone,
            bucket: self.bucket,
            forced: self.forced,
            depth: self.depth + 1,
        }
    }
}

pub fn evaluate(expr: &Expr, ctx: &Ctx) -> Result<Vec<Series>> {
    if ctx.depth > 64 {
        bail!("expression nested too deeply");
    }
    match expr {
        Expr::Path(p) => fetch_path(p, ctx),
        Expr::Call(name, args) => call(name, args, ctx).with_context(|| format!("in {name}()")),
        Expr::Num(n) => Ok(vec![constant(&format_num(*n), *n, ctx)]),
        Expr::Str(s) => bail!("expected a series but found the string \"{s}\""),
        Expr::Bool(_) => bail!("expected a series but found a boolean"),
    }
}

// ---------------------------------------------------------------- paths

pub fn glob_regex(pattern: &str) -> Result<Regex> {
    let mut re = String::from("^");
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' if chars.get(i + 1) == Some(&'*') => {
                re.push_str(".*");
                i += 1;
            }
            '*' => re.push_str("[^.]*"),
            '?' => re.push_str("[^.]"),
            '{' => re.push_str("(?:"),
            '}' => re.push(')'),
            ',' => re.push('|'),
            '[' => {
                let end = chars[i..]
                    .iter()
                    .position(|c| *c == ']')
                    .map(|p| p + i)
                    .ok_or_else(|| anyhow!("unclosed '[' in {pattern}"))?;
                re.push('[');
                re.extend(chars[i + 1..end].iter());
                re.push(']');
                i = end;
            }
            c => re.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    re.push('$');
    Ok(Regex::new(&re)?)
}

pub fn match_paths<'m>(pattern: &str, index: &'m [SeriesMeta]) -> Result<Vec<&'m SeriesMeta>> {
    if !pattern.contains(['*', '?', '{', '[']) {
        return Ok(index.iter().filter(|m| m.name == pattern).collect());
    }
    let re = glob_regex(pattern)?;
    Ok(index.iter().filter(|m| re.is_match(&m.name)).collect())
}

fn fetch_path(pattern: &str, ctx: &Ctx) -> Result<Vec<Series>> {
    let mut out = Vec::new();
    for m in match_paths(pattern, ctx.index)? {
        let bucket = if ctx.forced {
            ctx.bucket
        } else {
            match Bucket::from_interval(m.interval) {
                Some(native) if native.nominal() > ctx.bucket.nominal() => native,
                _ => ctx.bucket,
            }
        };
        let rows = ctx
            .store
            .fetch(m.id, ctx.from, ctx.until, bucket, ctx.zone)?;
        let ts = rows.iter().map(|r| r.0).collect();
        let stats: Vec<_> = rows.iter().map(|r| r.1).collect();
        let vals = stats.iter().map(|a| a.map(|a| a.value(m.agg))).collect();
        out.push(Series {
            name: m.name.clone(),
            path: m.name.clone(),
            ts,
            vals,
            stats: Some(stats),
            bucket,
            unit: m.unit.clone(),
            agg: m.agg,
            style: Style::default(),
        });
    }
    Ok(out)
}

/// Coarsest native interval among the paths an expression reads; used to
/// size prefetch windows for point-count arguments.
fn max_interval(expr: &Expr, ctx: &Ctx) -> i64 {
    match expr {
        Expr::Path(p) => match_paths(p, ctx.index)
            .map(|v| v.iter().map(|m| m.interval).max().unwrap_or(0))
            .unwrap_or(0),
        Expr::Call(_, args) => args
            .iter()
            .map(|a| max_interval(&a.val, ctx))
            .max()
            .unwrap_or(0),
        _ => 0,
    }
}

// ---------------------------------------------------------------- args

struct Args<'e, 'c, 'a> {
    name: &'e str,
    args: &'e [Arg],
    ctx: &'c Ctx<'a>,
}

impl<'e> Args<'e, '_, '_> {
    fn positional(&self, i: usize) -> Option<&'e Expr> {
        self.args
            .iter()
            .filter(|a| a.key.is_none())
            .nth(i)
            .map(|a| &a.val)
    }
    fn get(&self, i: usize, key: &str) -> Option<&'e Expr> {
        self.args
            .iter()
            .find(|a| a.key.as_deref() == Some(key))
            .map(|a| &a.val)
            .or_else(|| self.positional(i))
    }
    fn series(&self, i: usize) -> Result<Vec<Series>> {
        let e = self
            .positional(i)
            .ok_or_else(|| anyhow!("{}: missing series argument {}", self.name, i + 1))?;
        evaluate(e, self.ctx)
    }
    /// All positional args from `i` on, flattened (for sumSeries(a, b, c)).
    fn series_from(&self, i: usize) -> Result<Vec<Series>> {
        let mut out = Vec::new();
        let mut j = i;
        while let Some(e) = self.positional(j) {
            out.extend(evaluate(e, self.ctx)?);
            j += 1;
        }
        Ok(out)
    }
    fn num_opt(&self, i: usize, key: &str) -> Result<Option<f64>> {
        match self.get(i, key) {
            None => Ok(None),
            Some(Expr::Num(n)) => Ok(Some(*n)),
            Some(Expr::Str(s)) => s
                .trim()
                .parse()
                .map(Some)
                .map_err(|_| anyhow!("{}: '{key}' must be a number", self.name)),
            Some(_) => bail!("{}: '{key}' must be a number", self.name),
        }
    }
    fn num(&self, i: usize, key: &str) -> Result<f64> {
        self.num_opt(i, key)?
            .ok_or_else(|| anyhow!("{}: missing '{key}'", self.name))
    }
    fn str_opt(&self, i: usize, key: &str) -> Option<String> {
        match self.get(i, key) {
            Some(Expr::Str(s)) => Some(s.clone()),
            Some(Expr::Path(p)) => Some(p.clone()),
            Some(Expr::Num(n)) => Some(format_num(*n)),
            Some(Expr::Bool(b)) => Some(b.to_string()),
            _ => None,
        }
    }
    fn str(&self, i: usize, key: &str) -> Result<String> {
        self.str_opt(i, key)
            .ok_or_else(|| anyhow!("{}: missing '{key}'", self.name))
    }
    fn bool(&self, i: usize, key: &str, default: bool) -> bool {
        match self.get(i, key) {
            Some(Expr::Bool(b)) => *b,
            Some(Expr::Num(n)) => *n != 0.0,
            Some(Expr::Str(s)) => matches!(s.as_str(), "true" | "1" | "yes"),
            _ => default,
        }
    }
    fn agg(&self, i: usize, key: &str) -> Result<Option<AggFn>> {
        self.str_opt(i, key).map(|s| AggFn::parse(&s)).transpose()
    }
}

#[derive(Clone, Copy)]
enum Window {
    Points(usize),
    Time(Span),
}

impl Window {
    fn label(&self) -> String {
        match self {
            Window::Points(n) => n.to_string(),
            Window::Time(s) => format!("\"{s}\""),
        }
    }
}

fn window_arg(a: &Args, i: usize) -> Result<Window> {
    match a.get(i, "windowSize").or_else(|| a.get(i, "window")) {
        Some(Expr::Num(n)) if *n >= 1.0 => Ok(Window::Points(*n as usize)),
        Some(Expr::Str(s)) => {
            let sp = Span::parse(s)?;
            Ok(Window::Time(Span {
                n: sp.n.abs(),
                unit: sp.unit,
            }))
        }
        _ => bail!(
            "{}: window must be a point count or a time like \"7d\"",
            a.name
        ),
    }
}

// ---------------------------------------------------------------- dispatch

fn call(name: &str, args: &[Arg], ctx: &Ctx) -> Result<Vec<Series>> {
    let a = Args { name, args, ctx };
    let lname = name.to_ascii_lowercase();
    let n = lname.as_str();
    Ok(match n {
        // ---- combine
        "sumseries" | "sum" => vec![combine(&a.series_from(0)?, "sumSeries", ctx, |v| {
            Some(v.iter().sum())
        })?],
        "averageseries" | "avg" | "avgseries" => {
            vec![combine(&a.series_from(0)?, "averageSeries", ctx, |v| {
                Some(v.iter().sum::<f64>() / v.len() as f64)
            })?]
        }
        "minseries" => vec![combine(&a.series_from(0)?, "minSeries", ctx, |v| {
            v.iter().copied().reduce(f64::min)
        })?],
        "maxseries" => vec![combine(&a.series_from(0)?, "maxSeries", ctx, |v| {
            v.iter().copied().reduce(f64::max)
        })?],
        "rangeseries" => vec![combine(&a.series_from(0)?, "rangeSeries", ctx, |v| {
            Some(
                v.iter().copied().fold(f64::MIN, f64::max)
                    - v.iter().copied().fold(f64::MAX, f64::min),
            )
        })?],
        "countseries" => vec![combine(&a.series_from(0)?, "countSeries", ctx, |v| {
            Some(v.len() as f64)
        })?],
        "multiplyseries" => vec![combine(&a.series_from(0)?, "multiplySeries", ctx, |v| {
            Some(v.iter().product())
        })?],
        "stddevseries" => vec![combine(&a.series_from(0)?, "stddevSeries", ctx, |v| {
            Some(stddev(v))
        })?],
        "medianseries" => vec![combine(&a.series_from(0)?, "medianSeries", ctx, |v| {
            Some(percentile(v, 50.0))
        })?],
        "diffseries" => {
            let list = a.series_from(0)?;
            vec![combine_ordered(&list, "diffSeries", ctx, |v| {
                let first = v[0]?;
                Some(first - v[1..].iter().flatten().sum::<f64>())
            })?]
        }
        "divideseries" => {
            let dividend = a.series(0)?;
            let divisor = a.series(1)?;
            if divisor.len() != 1 {
                bail!(
                    "divideSeries: divisor must be exactly one series (got {})",
                    divisor.len()
                );
            }
            dividend
                .iter()
                .map(|s| {
                    let pair = vec![s.clone(), divisor[0].clone()];
                    let mut r = combine_ordered(&pair, "", ctx, |v| match (v[0], v[1]) {
                        (Some(x), Some(y)) if y != 0.0 => Some(x / y),
                        _ => None,
                    })?;
                    r.name = format!("divideSeries({},{})", s.name, divisor[0].name);
                    r.unit = String::new();
                    Ok(r)
                })
                .collect::<Result<_>>()?
        }
        "aggregate" => {
            let list = a.series(0)?;
            let f = a.str(1, "func")?;
            aggregate(&list, &f, &format!("{f}Series"), ctx)?
        }
        "percentileofseries" => {
            let list = a.series(0)?;
            let p = a.num(1, "n")?;
            let mut s = combine(&list, "percentileOfSeries", ctx, |v| Some(percentile(v, p)))?;
            s.name = format!("percentileOfSeries({},{})", names(&list), format_num(p));
            vec![s]
        }
        "group" => a.series_from(0)?,
        "aspercent" => as_percent(&a, ctx)?,
        "groupbynode" | "groupbynodes" => {
            let list = a.series(0)?;
            let mut nodes = Vec::new();
            let mut j = 1;
            let mut func = "avg".to_string();
            while let Some(e) = a.positional(j) {
                match e {
                    Expr::Num(x) => nodes.push(*x as i64),
                    Expr::Str(s) => func = s.clone(),
                    _ => {}
                }
                j += 1;
            }
            if let Some(f) = a.str_opt(99, "callback") {
                func = f;
            }
            let mut groups: BTreeMap<String, Vec<Series>> = BTreeMap::new();
            for s in list {
                groups.entry(node_key(&s.path, &nodes)).or_default().push(s);
            }
            let mut out = Vec::new();
            for (k, g) in groups {
                let mut r = aggregate(&g, &func, &func, ctx)?.remove(0);
                r.name = k.clone();
                r.path = k;
                out.push(r);
            }
            out
        }

        // ---- transform
        "scale" => {
            let f = a.num(1, "factor")?;
            map_each(a.series(0)?, |s| {
                s.map(format!("scale({},{})", s.name, format_num(f)), |v| {
                    Some(v * f)
                })
            })
        }
        "offset" => {
            let f = a.num(1, "factor")?;
            map_each(a.series(0)?, |s| {
                s.map(format!("offset({},{})", s.name, format_num(f)), |v| {
                    Some(v + f)
                })
            })
        }
        "absolute" => map_each(a.series(0)?, |s| {
            s.map(format!("absolute({})", s.name), |v| Some(v.abs()))
        }),
        "invert" => map_each(a.series(0)?, |s| {
            s.map(format!("invert({})", s.name), |v| {
                (v != 0.0).then(|| 1.0 / v)
            })
        }),
        "pow" => {
            let p = a.num(1, "factor")?;
            map_each(a.series(0)?, |s| {
                s.map(format!("pow({},{})", s.name, format_num(p)), |v| {
                    Some(v.powf(p))
                })
            })
        }
        "squareroot" => map_each(a.series(0)?, |s| {
            s.map(format!("squareRoot({})", s.name), |v| {
                (v >= 0.0).then(|| v.sqrt())
            })
        }),
        "log" => {
            let b = a.num_opt(1, "base")?.unwrap_or(10.0);
            map_each(a.series(0)?, |s| {
                s.map(format!("log({},{})", s.name, format_num(b)), |v| {
                    (v > 0.0).then(|| v.log(b))
                })
            })
        }
        "round" => {
            let p = a.num_opt(1, "precision")?.unwrap_or(0.0) as i32;
            let m = 10f64.powi(p);
            map_each(a.series(0)?, |s| {
                s.map(format!("round({},{})", s.name, p), |v| {
                    Some((v * m).round() / m)
                })
            })
        }
        "removebelowvalue" => {
            let n = a.num(1, "n")?;
            map_each(a.series(0)?, |s| {
                s.map(
                    format!("removeBelowValue({},{})", s.name, format_num(n)),
                    |v| (v >= n).then_some(v),
                )
            })
        }
        "removeabovevalue" => {
            let n = a.num(1, "n")?;
            map_each(a.series(0)?, |s| {
                s.map(
                    format!("removeAboveValue({},{})", s.name, format_num(n)),
                    |v| (v <= n).then_some(v),
                )
            })
        }
        "clamp" => {
            let lo = a.num(1, "min")?;
            let hi = a.num(2, "max")?;
            map_each(a.series(0)?, |s| {
                s.map(format!("clamp({})", s.name), |v| Some(v.clamp(lo, hi)))
            })
        }
        "derivative" => map_each(a.series(0)?, |s| derivative(s, "derivative", false, false)),
        "nonnegativederivative" => map_each(a.series(0)?, |s| {
            derivative(s, "nonNegativeDerivative", true, false)
        }),
        "persecond" => map_each(a.series(0)?, |s| derivative(s, "perSecond", true, true)),
        "integral" | "cumulativesum" => map_each(a.series(0)?, |s| {
            let mut acc = 0.0;
            let vals = s
                .vals
                .iter()
                .map(|v| {
                    v.map(|x| {
                        acc += x;
                        acc
                    })
                })
                .collect();
            s.with_vals(format!("integral({})", s.name), vals)
        }),
        "delay" => {
            let k = a.num(1, "steps")? as i64;
            map_each(a.series(0)?, |s| {
                let n = s.vals.len() as i64;
                let vals = (0..n)
                    .map(|i| {
                        if i - k >= 0 && i - k < n {
                            s.vals[(i - k) as usize]
                        } else {
                            None
                        }
                    })
                    .collect();
                s.with_vals(format!("delay({},{k})", s.name), vals)
            })
        }
        "movingaverage" | "movingmedian" | "movingsum" | "movingmin" | "movingmax"
        | "movingwindow" | "stdev" => {
            let (func, fname) = match n {
                "movingaverage" => ("avg".to_string(), "movingAverage"),
                "movingmedian" => ("median".to_string(), "movingMedian"),
                "movingsum" => ("sum".to_string(), "movingSum"),
                "movingmin" => ("min".to_string(), "movingMin"),
                "movingmax" => ("max".to_string(), "movingMax"),
                "stdev" => ("stddev".to_string(), "stdev"),
                _ => (
                    a.str_opt(2, "func").unwrap_or_else(|| "avg".into()),
                    "movingWindow",
                ),
            };
            let w = window_arg(&a, 1)?;
            moving(&a, w, &func, fname)?
        }
        "exponentialmovingaverage" | "ema" => {
            let w = window_arg(&a, 1)?;
            let (list, _) = prefetch(&a, w)?;
            list.into_iter()
                .map(|mut s| {
                    let k = match w {
                        Window::Points(p) => p as f64,
                        Window::Time(sp) => {
                            (sp.approx_secs() as f64 / s.bucket.nominal() as f64).max(1.0)
                        }
                    };
                    let alpha = 2.0 / (k + 1.0);
                    let mut ema: Option<f64> = None;
                    let vals = s
                        .vals
                        .iter()
                        .map(|v| {
                            if let Some(x) = v {
                                ema = Some(match ema {
                                    Some(e) => alpha * x + (1.0 - alpha) * e,
                                    None => *x,
                                });
                            }
                            v.and(ema)
                        })
                        .collect();
                    s = s.with_vals(
                        format!("exponentialMovingAverage({},{})", s.name, w.label()),
                        vals,
                    );
                    s.trim(s.bucket.start_of(ctx.from, ctx.zone), ctx.until);
                    s
                })
                .collect()
        }
        "summarize" | "smartsummarize" => {
            let interval = a.str(1, "intervalString")?;
            let bucket = Bucket::parse(&interval)?;
            let f = a.agg(2, "func")?;
            let list = a.series(0)?;
            list.into_iter()
                .map(|s| {
                    let f = f.unwrap_or(s.agg);
                    let mut r = s.rebucket(bucket, f, ctx.zone, ctx.from, ctx.until);
                    r.trim(bucket.start_of(ctx.from, ctx.zone), ctx.until);
                    r.name = format!("summarize({},\"{}\",\"{}\")", s.name, interval, f.name());
                    r
                })
                .collect()
        }
        "consolidateby" | "cumulative" => {
            let f = if n == "cumulative" {
                AggFn::Sum
            } else {
                AggFn::parse(&a.str(1, "consolidationFunc")?)?
            };
            map_each(a.series(0)?, |s| {
                let mut r = s.clone();
                if let Some(st) = &s.stats {
                    r.vals = st.iter().map(|x| x.map(|x| x.value(f))).collect();
                }
                r.agg = f;
                r.name = format!("consolidateBy({},\"{}\")", s.name, f.name());
                r
            })
        }
        "keeplastvalue" => {
            let limit = a
                .num_opt(1, "limit")?
                .map(|x| x as usize)
                .unwrap_or(usize::MAX);
            map_each(a.series(0)?, |s| {
                let mut last = None;
                let mut gap = 0usize;
                let vals = s
                    .vals
                    .iter()
                    .map(|v| match v {
                        Some(x) => {
                            last = Some(*x);
                            gap = 0;
                            Some(*x)
                        }
                        None => {
                            gap += 1;
                            if gap <= limit {
                                last
                            } else {
                                None
                            }
                        }
                    })
                    .collect();
                s.with_vals(format!("keepLastValue({})", s.name), vals)
            })
        }
        "interpolate" => map_each(a.series(0)?, |s| {
            let mut vals = s.vals.clone();
            let known: Vec<usize> = (0..vals.len()).filter(|i| vals[*i].is_some()).collect();
            for w in known.windows(2) {
                let (i, j) = (w[0], w[1]);
                let (x, y) = (vals[i].unwrap(), vals[j].unwrap());
                for (k, v) in vals.iter_mut().enumerate().take(j).skip(i + 1) {
                    *v = Some(x + (y - x) * (k - i) as f64 / (j - i) as f64);
                }
            }
            s.with_vals(format!("interpolate({})", s.name), vals)
        }),
        "transformnull" => {
            let d = a.num_opt(1, "default")?.unwrap_or(0.0);
            map_each(a.series(0)?, |s| {
                s.with_vals(
                    format!("transformNull({},{})", s.name, format_num(d)),
                    s.vals.iter().map(|v| Some(v.unwrap_or(d))).collect(),
                )
            })
        }
        "isnonnull" => map_each(a.series(0)?, |s| {
            s.with_vals(
                format!("isNonNull({})", s.name),
                s.vals
                    .iter()
                    .map(|v| Some(if v.is_some() { 1.0 } else { 0.0 }))
                    .collect(),
            )
        }),
        "timeshift" => {
            let raw = a.str(1, "timeShift")?;
            let sp = Span::parse(&raw)?;
            let shift = if raw.trim().starts_with('+') {
                sp
            } else {
                Span {
                    n: -sp.n.abs(),
                    unit: sp.unit,
                }
            };
            let back = Span {
                n: -shift.n,
                unit: shift.unit,
            };
            let inner = ctx.shifted(
                shift.add_to(ctx.from, ctx.zone),
                shift.add_to(ctx.until, ctx.zone),
            );
            let e = a
                .positional(0)
                .ok_or_else(|| anyhow!("timeShift: missing series"))?;
            let label = raw.trim().trim_start_matches(['+', '-']).to_string();
            evaluate(e, &inner)?
                .into_iter()
                .map(|mut s| {
                    s.ts = s.ts.iter().map(|t| back.add_to(*t, ctx.zone)).collect();
                    s.name = format!("timeShift({},\"{}\")", s.name, label);
                    s.style.dashed = true;
                    s
                })
                .collect()
        }
        "timeslice" => {
            let start = crate::timeutil::parse_time(
                &a.str(1, "startSliceAt")?,
                crate::timeutil::now(),
                ctx.zone,
                false,
            )?;
            let end = match a.str_opt(2, "endSliceAt") {
                Some(e) => crate::timeutil::parse_time(&e, crate::timeutil::now(), ctx.zone, true)?,
                None => i64::MAX,
            };
            map_each(a.series(0)?, |s| {
                let vals =
                    s.ts.iter()
                        .zip(&s.vals)
                        .map(|(t, v)| if *t >= start && *t < end { *v } else { None })
                        .collect();
                s.with_vals(format!("timeSlice({})", s.name), vals)
            })
        }
        "linearregression" | "trend" => map_each(a.series(0)?, |s| {
            let pts: Vec<(f64, f64)> =
                s.ts.iter()
                    .zip(&s.vals)
                    .filter_map(|(t, v)| v.map(|v| (*t as f64, v)))
                    .collect();
            let fit = linfit(&pts);
            let vals =
                s.ts.iter()
                    .map(|t| fit.map(|(m, b)| m * *t as f64 + b))
                    .collect();
            let mut r = s.with_vals(format!("linearRegression({})", s.name), vals);
            r.style.dashed = true;
            r
        }),
        "indexed" | "indexby" => {
            let base = a.num_opt(1, "base")?.unwrap_or(100.0);
            map_each(a.series(0)?, |s| match s.first() {
                Some(f) if f != 0.0 => {
                    let mut r = s.map(format!("indexed({})", s.name), |v| Some(v / f * base));
                    r.unit = "index".into();
                    r
                }
                _ => s.with_vals(format!("indexed({})", s.name), vec![None; s.vals.len()]),
            })
        }
        "zscore" => map_each(a.series(0)?, |s| {
            let v: Vec<f64> = s.non_null().collect();
            let (m, sd) = (mean(&v), stddev(&v));
            let mut r = s.map(format!("zscore({})", s.name), |x| {
                (sd > 0.0).then(|| (x - m) / sd)
            });
            r.unit = "σ".into();
            r
        }),
        "npercentile" => {
            let p = a.num(1, "n")?;
            map_each(a.series(0)?, |s| {
                let v: Vec<f64> = s.non_null().collect();
                let q = (!v.is_empty()).then(|| percentile(&v, p));
                let mut r = s.with_vals(
                    format!("nPercentile({},{})", s.name, format_num(p)),
                    s.ts.iter().map(|_| q).collect(),
                );
                r.style.kind = Some("threshold".into());
                r
            })
        }

        // ---- filter & sort
        "highestaverage" | "highestmax" | "highestcurrent" | "highestmin" | "lowestaverage"
        | "lowestmax" | "lowestcurrent" | "lowestmin" => {
            let k = a.num_opt(1, "n")?.unwrap_or(1.0) as usize;
            let stat = stat_fn(&n[if n.starts_with("highest") { 7 } else { 6 }..]);
            let mut list = a.series(0)?;
            list.sort_by(|x, y| {
                stat(x)
                    .unwrap_or(f64::MIN)
                    .total_cmp(&stat(y).unwrap_or(f64::MIN))
            });
            if n.starts_with("highest") {
                list.reverse();
            }
            list.truncate(k);
            list
        }
        "currentabove" | "currentbelow" | "averageabove" | "averagebelow" | "maximumabove"
        | "maximumbelow" | "minimumabove" | "minimumbelow" => {
            let thr = a.num(1, "n")?;
            let above = n.ends_with("above");
            let stat = stat_fn(n.trim_end_matches("above").trim_end_matches("below"));
            a.series(0)?
                .into_iter()
                .filter(|s| stat(s).is_some_and(|v| if above { v > thr } else { v < thr }))
                .collect()
        }
        "limit" => {
            let mut l = a.series(0)?;
            l.truncate(a.num(1, "n")? as usize);
            l
        }
        "exclude" => {
            let re = Regex::new(&a.str(1, "pattern")?)?;
            a.series(0)?
                .into_iter()
                .filter(|s| !re.is_match(&s.name))
                .collect()
        }
        "grep" => {
            let re = Regex::new(&a.str(1, "pattern")?)?;
            a.series(0)?
                .into_iter()
                .filter(|s| re.is_match(&s.name))
                .collect()
        }
        "removeemptyseries" => a
            .series(0)?
            .into_iter()
            .filter(|s| s.vals.iter().any(|v| v.is_some()))
            .collect(),
        "sortbyname" => {
            let mut l = a.series(0)?;
            l.sort_by(|x, y| x.name.cmp(&y.name));
            if a.bool(2, "reverse", false) {
                l.reverse();
            }
            l
        }
        "sortbymaxima" | "sortbyminima" | "sortbytotal" => {
            let mut l = a.series(0)?;
            let key = |s: &Series| {
                match n {
                    "sortbymaxima" => s.max(),
                    "sortbyminima" => s.min(),
                    _ => s.sum(),
                }
                .unwrap_or(f64::MIN)
            };
            l.sort_by(|x, y| key(y).total_cmp(&key(x)));
            if n == "sortbyminima" {
                l.reverse();
            }
            l
        }

        // ---- naming
        "alias" => {
            let new = a.str(1, "newName")?;
            map_each(a.series(0)?, |s| Series {
                name: new.replace("%s", &s.name),
                ..s.clone()
            })
        }
        "aliasbynode" | "aliasbynodes" => {
            let mut nodes = Vec::new();
            let mut j = 1;
            while let Some(Expr::Num(x)) = a.positional(j) {
                nodes.push(*x as i64);
                j += 1;
            }
            map_each(a.series(0)?, |s| Series {
                name: node_key(&s.path, &nodes),
                ..s.clone()
            })
        }
        "aliasbymetric" => map_each(a.series(0)?, |s| Series {
            name: s.path.rsplit('.').next().unwrap_or(&s.path).to_string(),
            ..s.clone()
        }),
        "aliassub" => {
            let re = Regex::new(&a.str(1, "search")?)?;
            let rep = a.str(2, "replace")?;
            let rep = Regex::new(r"\\(\d)")
                .unwrap()
                .replace_all(&rep, "$${$1}")
                .to_string();
            map_each(a.series(0)?, |s| Series {
                name: re.replace_all(&s.name, rep.as_str()).to_string(),
                ..s.clone()
            })
        }

        // ---- style
        "color" => {
            let c = a.str(1, "theColor")?;
            map_each(a.series(0)?, |s| styled(s, |st| st.color = Some(c.clone())))
        }
        "dashed" => map_each(a.series(0)?, |s| styled(s, |st| st.dashed = true)),
        "linewidth" => {
            let w = a.num(1, "width")?;
            map_each(a.series(0)?, |s| styled(s, |st| st.width = Some(w)))
        }
        "stacked" => map_each(a.series(0)?, |s| styled(s, |st| st.stack = true)),
        "asthreshold" | "reference" => map_each(a.series(0)?, |s| {
            styled(s, |st| st.kind = Some("threshold".into()))
        }),
        "dim" | "muted" => map_each(a.series(0)?, |s| styled(s, |st| st.emphasis = -1)),
        "highlight" => {
            let pat = a.str_opt(1, "pattern");
            let re = pat.as_deref().map(Regex::new).transpose()?;
            map_each(a.series(0)?, |s| {
                let hit = re.as_ref().map(|r| r.is_match(&s.name)).unwrap_or(true);
                styled(s, |st| st.emphasis = if hit { 1 } else { -1 })
            })
        }
        "unit" => {
            let u = a.str(1, "unit")?;
            map_each(a.series(0)?, |s| Series {
                unit: u.clone(),
                ..s.clone()
            })
        }

        // ---- generators
        "constantline" => {
            let v = a.num(0, "value")?;
            let mut s = constant(&format_num(v), v, ctx);
            s.style.kind = Some("threshold".into());
            vec![s]
        }
        "threshold" => {
            let v = a.num(0, "value")?;
            let label = a.str_opt(1, "label").unwrap_or_else(|| format_num(v));
            let mut s = constant(&label, v, ctx);
            s.style.kind = Some("threshold".into());
            s.style.color = a.str_opt(2, "color");
            vec![s]
        }
        "randomwalk" | "randomwalkfunction" => {
            let name = a.str_opt(0, "name").unwrap_or_else(|| "randomWalk".into());
            let step = a.num_opt(1, "step")?.unwrap_or(60.0).max(1.0) as i64;
            vec![random_walk(&name, step, ctx)]
        }
        "sinfunction" => {
            let name = a.str_opt(0, "name").unwrap_or_else(|| "sin".into());
            let amp = a.num_opt(1, "amplitude")?.unwrap_or(1.0);
            let step = a.num_opt(2, "step")?.unwrap_or(60.0).max(1.0) as i64;
            let b = gen_bucket(step, ctx);
            let ts = b.range(ctx.from, ctx.until, ctx.zone);
            let vals = ts
                .iter()
                .map(|t| Some(amp * (*t as f64 / 3600.0).sin()))
                .collect();
            vec![Series::new(name, ts, vals, b)]
        }
        _ => bail!("unknown function '{name}' — see /docs for the function reference"),
    })
}

// ---------------------------------------------------------------- helpers

fn map_each(list: Vec<Series>, f: impl Fn(&Series) -> Series) -> Vec<Series> {
    list.iter().map(f).collect()
}

fn styled(s: &Series, f: impl FnOnce(&mut Style)) -> Series {
    let mut r = s.clone();
    f(&mut r.style);
    r
}

fn names(list: &[Series]) -> String {
    list.iter()
        .map(|s| s.name.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

fn common_unit(list: &[Series]) -> String {
    match list.first() {
        Some(f) if list.iter().all(|s| s.unit == f.unit) => f.unit.clone(),
        _ => String::new(),
    }
}

/// Combine across series per timestamp, ignoring nulls.
fn combine(
    list: &[Series],
    label: &str,
    ctx: &Ctx,
    f: impl Fn(&[f64]) -> Option<f64>,
) -> Result<Series> {
    let (ts, cols, bucket) = align(list, ctx.zone, ctx.from, ctx.until);
    let vals = (0..ts.len())
        .map(|i| {
            let row: Vec<f64> = cols.iter().filter_map(|c| c[i]).collect();
            if row.is_empty() {
                None
            } else {
                f(&row).filter(|x| x.is_finite())
            }
        })
        .collect();
    let mut s = Series::new(format!("{label}({})", names(list)), ts, vals, bucket);
    s.unit = common_unit(list);
    s.agg = list.first().map(|x| x.agg).unwrap_or(AggFn::Avg);
    Ok(s)
}

/// Combine keeping positional order and nulls (for diff/divide).
fn combine_ordered(
    list: &[Series],
    label: &str,
    ctx: &Ctx,
    f: impl Fn(&[Option<f64>]) -> Option<f64>,
) -> Result<Series> {
    if list.is_empty() {
        bail!("no series matched");
    }
    let (ts, cols, bucket) = align(list, ctx.zone, ctx.from, ctx.until);
    let vals = (0..ts.len())
        .map(|i| f(&cols.iter().map(|c| c[i]).collect::<Vec<_>>()).filter(|x| x.is_finite()))
        .collect();
    let mut s = Series::new(format!("{label}({})", names(list)), ts, vals, bucket);
    s.unit = common_unit(list);
    Ok(s)
}

fn aggregate(list: &[Series], func: &str, label: &str, ctx: &Ctx) -> Result<Vec<Series>> {
    let f = func.trim_matches('"').to_ascii_lowercase();
    let s = match f.as_str() {
        "sum" | "total" => combine(list, label, ctx, |v| Some(v.iter().sum()))?,
        "avg" | "average" | "mean" => combine(list, label, ctx, |v| Some(mean(v)))?,
        "min" => combine(list, label, ctx, |v| v.iter().copied().reduce(f64::min))?,
        "max" => combine(list, label, ctx, |v| v.iter().copied().reduce(f64::max))?,
        "median" => combine(list, label, ctx, |v| Some(percentile(v, 50.0)))?,
        "count" => combine(list, label, ctx, |v| Some(v.len() as f64))?,
        "range" => combine(list, label, ctx, |v| {
            Some(
                v.iter().copied().fold(f64::MIN, f64::max)
                    - v.iter().copied().fold(f64::MAX, f64::min),
            )
        })?,
        "stddev" => combine(list, label, ctx, |v| Some(stddev(v)))?,
        "multiply" => combine(list, label, ctx, |v| Some(v.iter().product()))?,
        "diff" => combine_ordered(list, label, ctx, |v| {
            Some(v[0]? - v[1..].iter().flatten().sum::<f64>())
        })?,
        o => bail!("unknown aggregation '{o}'"),
    };
    Ok(vec![s])
}

fn as_percent(a: &Args, ctx: &Ctx) -> Result<Vec<Series>> {
    let list = a.series(0)?;
    let mut all = list.clone();
    match a.positional(1) {
        Some(Expr::Num(t)) => {
            let t = *t;
            return Ok(map_each(list, |s| {
                let mut r = s.map(format!("asPercent({},{})", s.name, format_num(t)), |v| {
                    (t != 0.0).then(|| v / t * 100.0)
                });
                r.unit = "%".into();
                r
            }));
        }
        Some(e) => {
            let t = evaluate(e, ctx)?;
            let tot = combine(&t, "sumSeries", ctx, |v| Some(v.iter().sum()))?;
            all.push(tot);
        }
        None => {
            let tot = combine(&list, "sumSeries", ctx, |v| Some(v.iter().sum()))?;
            all.push(tot);
        }
    }
    let (ts, cols, bucket) = align(&all, ctx.zone, ctx.from, ctx.until);
    let tot = cols.last().unwrap().clone();
    Ok(list
        .iter()
        .enumerate()
        .map(|(k, s)| {
            let vals = (0..ts.len())
                .map(|i| match (cols[k][i], tot[i]) {
                    (Some(v), Some(t)) if t != 0.0 => Some(v / t * 100.0),
                    _ => None,
                })
                .collect();
            let mut r = Series::new(format!("asPercent({})", s.name), ts.clone(), vals, bucket);
            r.path = s.path.clone();
            r.unit = "%".into();
            r.style = s.style.clone();
            r
        })
        .collect())
}

fn derivative(s: &Series, label: &str, non_negative: bool, per_second: bool) -> Series {
    let mut prev: Option<(i64, f64)> = None;
    let vals =
        s.ts.iter()
            .zip(&s.vals)
            .map(|(t, v)| {
                let out = match (prev, v) {
                    (Some((pt, pv)), Some(x)) => {
                        let d = x - pv;
                        let d = if per_second {
                            d / (*t - pt).max(1) as f64
                        } else {
                            d
                        };
                        if non_negative && d < 0.0 {
                            None
                        } else {
                            Some(d)
                        }
                    }
                    _ => None,
                };
                if let Some(x) = v {
                    prev = Some((*t, *x));
                }
                out
            })
            .collect();
    let mut r = s.with_vals(format!("{label}({})", s.name), vals);
    if per_second && !r.unit.is_empty() {
        r.unit = format!("{}/s", r.unit);
    }
    r
}

/// Evaluate the first argument with the start pulled back by the window so
/// the first visible points have a full window behind them.
fn prefetch(a: &Args, w: Window) -> Result<(Vec<Series>, i64)> {
    let ctx = a.ctx;
    let e = a
        .positional(0)
        .ok_or_else(|| anyhow!("{}: missing series", a.name))?;
    let back = match w {
        Window::Time(sp) => sp.approx_secs(),
        Window::Points(p) => p as i64 * ctx.bucket.nominal().max(max_interval(e, ctx)),
    };
    let inner = ctx.shifted(ctx.from - back, ctx.until);
    Ok((evaluate(e, &inner)?, back))
}

fn moving(a: &Args, w: Window, func: &str, fname: &str) -> Result<Vec<Series>> {
    let ctx = a.ctx;
    let (list, _) = prefetch(a, w)?;
    let reduce = |v: &mut Vec<f64>| -> Option<f64> {
        if v.is_empty() {
            return None;
        }
        Some(match func {
            "avg" | "average" => mean(v),
            "median" => percentile(v, 50.0),
            "sum" => v.iter().sum(),
            "min" => v.iter().copied().fold(f64::MAX, f64::min),
            "max" => v.iter().copied().fold(f64::MIN, f64::max),
            "stddev" => stddev(v),
            _ => mean(v),
        })
    };
    Ok(list
        .into_iter()
        .map(|s| {
            let n = s.vals.len();
            let mut out = Vec::with_capacity(n);
            let mut lo = 0usize;
            for i in 0..n {
                match w {
                    Window::Points(p) => lo = (i + 1).saturating_sub(p),
                    Window::Time(sp) => {
                        let start = s.ts[i] - sp.approx_secs();
                        while lo < i && s.ts[lo] <= start {
                            lo += 1;
                        }
                    }
                }
                let mut win: Vec<f64> = s.vals[lo..=i].iter().flatten().copied().collect();
                out.push(reduce(&mut win).filter(|x| x.is_finite()));
            }
            let mut r = s.with_vals(format!("{fname}({},{})", s.name, w.label()), out);
            r.trim(r.bucket.start_of(ctx.from, ctx.zone), ctx.until);
            r
        })
        .collect())
}

fn stat_fn(name: &str) -> fn(&Series) -> Option<f64> {
    match name {
        "max" | "maximum" => Series::max,
        "min" | "minimum" => Series::min,
        "current" => Series::last,
        _ => Series::avg,
    }
}

fn node_key(path: &str, nodes: &[i64]) -> String {
    let parts: Vec<&str> = path.split('.').collect();
    nodes
        .iter()
        .filter_map(|n| {
            let i = if *n < 0 { parts.len() as i64 + n } else { *n };
            parts.get(i as usize).copied()
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn constant(name: &str, v: f64, ctx: &Ctx) -> Series {
    let ts = ctx.bucket.range(ctx.from, ctx.until, ctx.zone);
    let vals = vec![Some(v); ts.len()];
    Series::new(name, ts, vals, ctx.bucket)
}

/// Deterministic demo signal (daily + weekly cycles plus noise), stable
/// across reloads and consistent at any zoom level.
fn random_walk(name: &str, step: i64, ctx: &Ctx) -> Series {
    let b = gen_bucket(step, ctx);
    let ts = b.range(ctx.from, ctx.until, ctx.zone);
    let seed = name.bytes().fold(1469598103934665603u64, |h, c| {
        (h ^ c as u64).wrapping_mul(1099511628211)
    });
    let phase = |k: u64| ((seed >> k) & 0xffff) as f64 / 65535.0 * std::f64::consts::TAU;
    let noise = |t: i64| {
        let mut x = seed ^ (t as u64).wrapping_mul(0x9E3779B97F4A7C15);
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51afd7ed558ccd);
        x ^= x >> 33;
        (x % 2001) as f64 / 1000.0 - 1.0
    };
    let level = 40.0 + (seed % 40) as f64;
    let vals = ts
        .iter()
        .map(|t| {
            let x = *t as f64;
            let day = (x / 86400.0 * std::f64::consts::TAU + phase(3)).sin();
            let week = (x / (7.0 * 86400.0) * std::f64::consts::TAU + phase(19)).sin();
            let drift = (x / (29.0 * 86400.0) * std::f64::consts::TAU + phase(37)).sin();
            let v = level
                + 14.0 * day
                + 6.0 * week
                + 8.0 * drift
                + 4.0 * noise(t.div_euclid(b.nominal().max(60)));
            Some((v.max(0.0) * 100.0).round() / 100.0)
        })
        .collect();
    Series::new(name, ts, vals, b)
}

/// Generators use their own step unless the view needs coarser buckets.
fn gen_bucket(step: i64, ctx: &Ctx) -> Bucket {
    let own = Bucket::secs(step).normalized();
    if ctx.forced || ctx.bucket.nominal() > own.nominal() {
        ctx.bucket
    } else {
        own
    }
}

pub fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

pub fn stddev(v: &[f64]) -> f64 {
    if v.len() < 2 {
        return 0.0;
    }
    let m = mean(v);
    (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
}

pub fn percentile(v: &[f64], p: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    if s.is_empty() {
        return f64::NAN;
    }
    let rank = (p.clamp(0.0, 100.0) / 100.0) * (s.len() - 1) as f64;
    let (lo, hi) = (rank.floor() as usize, rank.ceil() as usize);
    s[lo] + (s[hi] - s[lo]) * (rank - lo as f64)
}

pub fn linfit(pts: &[(f64, f64)]) -> Option<(f64, f64)> {
    if pts.len() < 2 {
        return None;
    }
    let n = pts.len() as f64;
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    if sxx == 0.0 {
        return None;
    }
    let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let m = sxy / sxx;
    Some((m, my - m * mx))
}

pub fn format_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}
