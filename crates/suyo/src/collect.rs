//! Built-in Linux host collector: CPU, memory, load and network from /proc.
//! Series live under `host.<hostname>.*`.

use crate::store::{AggFn, MetaHint, Store};
use std::collections::HashMap;
use std::time::Duration;

fn read(p: &str) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

pub fn hostname() -> String {
    let h = read("/etc/hostname");
    let h = if h.trim().is_empty() {
        read("/proc/sys/kernel/hostname")
    } else {
        h
    };
    crate::store::sanitize_segment(if h.trim().is_empty() {
        "localhost"
    } else {
        h.trim()
    })
}

#[derive(Clone, Copy, Default)]
struct Cpu {
    user: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    total: u64,
}

fn cpu() -> Option<Cpu> {
    let s = read("/proc/stat");
    let line = s.lines().next()?;
    let v: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    if v.len() < 5 {
        return None;
    }
    Some(Cpu {
        user: v[0] + v[1],
        system: v[2] + v.get(5).unwrap_or(&0) + v.get(6).unwrap_or(&0),
        idle: v[3],
        iowait: v[4],
        total: v.iter().take(8).sum(),
    })
}

fn net_bytes() -> (u64, u64) {
    let mut rx = 0;
    let mut tx = 0;
    for l in read("/proc/net/dev").lines().skip(2) {
        let Some((iface, rest)) = l.split_once(':') else {
            continue;
        };
        if iface.trim() == "lo" {
            continue;
        }
        let v: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|x| x.parse().ok())
            .collect();
        if v.len() >= 9 {
            rx += v[0];
            tx += v[8];
        }
    }
    (rx, tx)
}

pub fn spawn(db: String, every: Duration) {
    std::thread::spawn(move || {
        let host = hostname();
        let p = |s: &str| format!("host.{host}.{s}");
        let mut hints = HashMap::new();
        for (k, u) in [
            ("cpu.percent", "%"),
            ("cpu.user", "%"),
            ("cpu.system", "%"),
            ("cpu.iowait", "%"),
            ("mem.used_percent", "%"),
            ("mem.used", "GiB"),
            ("mem.available", "GiB"),
            ("swap.used", "GiB"),
            ("load.1m", ""),
            ("load.5m", ""),
            ("load.15m", ""),
            ("net.rx", "MiB/s"),
            ("net.tx", "MiB/s"),
        ] {
            hints.insert(
                p(k),
                MetaHint {
                    unit: Some(u.into()),
                    agg: Some(AggFn::Avg),
                    interval: Some(every.as_secs().max(1) as i64),
                    description: None,
                },
            );
        }
        let mut store = match Store::open(&db) {
            Ok(s) => s,
            Err(e) => return eprintln!("collector: {e:#}"),
        };
        let mut prev = cpu();
        let mut prev_net = (net_bytes(), std::time::Instant::now());
        eprintln!(
            "collecting host metrics as host.{host}.* every {}s",
            every.as_secs()
        );
        loop {
            std::thread::sleep(every);
            let ts = crate::timeutil::now();
            let mut pts = Vec::new();
            if let (Some(a), Some(b)) = (prev, cpu()) {
                let dt = (b.total.saturating_sub(a.total)).max(1) as f64;
                let pct = |x: u64, y: u64| (y.saturating_sub(x)) as f64 / dt * 100.0;
                pts.push((
                    p("cpu.percent"),
                    ts,
                    100.0 - pct(a.idle + a.iowait, b.idle + b.iowait),
                ));
                pts.push((p("cpu.user"), ts, pct(a.user, b.user)));
                pts.push((p("cpu.system"), ts, pct(a.system, b.system)));
                pts.push((p("cpu.iowait"), ts, pct(a.iowait, b.iowait)));
                prev = Some(b);
            }
            let mi: HashMap<String, f64> = read("/proc/meminfo")
                .lines()
                .filter_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    Some((
                        k.to_string(),
                        v.split_whitespace().next()?.parse::<f64>().ok()? / 1048576.0,
                    ))
                })
                .collect();
            if let (Some(t), Some(av)) = (mi.get("MemTotal"), mi.get("MemAvailable")) {
                pts.push((p("mem.used_percent"), ts, (t - av) / t * 100.0));
                pts.push((p("mem.used"), ts, t - av));
                pts.push((p("mem.available"), ts, *av));
            }
            if let (Some(t), Some(f)) = (mi.get("SwapTotal"), mi.get("SwapFree")) {
                pts.push((p("swap.used"), ts, t - f));
            }
            let la: Vec<f64> = read("/proc/loadavg")
                .split_whitespace()
                .take(3)
                .filter_map(|x| x.parse().ok())
                .collect();
            for (i, k) in ["load.1m", "load.5m", "load.15m"].iter().enumerate() {
                if let Some(v) = la.get(i) {
                    pts.push((p(k), ts, *v));
                }
            }
            let now_net = (net_bytes(), std::time::Instant::now());
            let secs = now_net
                .1
                .duration_since(prev_net.1)
                .as_secs_f64()
                .max(0.001);
            pts.push((
                p("net.rx"),
                ts,
                now_net.0 .0.saturating_sub(prev_net.0 .0) as f64 / secs / 1048576.0,
            ));
            pts.push((
                p("net.tx"),
                ts,
                now_net.0 .1.saturating_sub(prev_net.0 .1) as f64 / secs / 1048576.0,
            ));
            prev_net = now_net;
            if let Err(e) = store.write(&pts, &hints) {
                eprintln!("collector write failed: {e:#}");
            }
        }
    });
}
