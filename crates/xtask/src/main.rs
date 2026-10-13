//! `cargo xtask trace`: every rule ID in EENKAH-SPEC.md must be named by at least one test,
//! or be listed in docs/DEFERRED.md with the milestone and reason. Fails otherwise.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn rule_ids(spec: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in spec.lines() {
        if let Some(rest) = line.strip_prefix("| ") {
            let id = rest.split(" |").next().unwrap_or("").trim();
            let (p, n) = id.split_once('-').unwrap_or(("", ""));
            if !p.is_empty() && p.chars().all(|c| c.is_ascii_uppercase()) && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
                out.insert(id.to_string());
            }
        }
    }
    out
}

/// Test code: files under `tests/`, and everything after `#[cfg(test)]` in source files.
fn test_text(dir: &Path, acc: &mut String) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "target") {
                test_text(&p, acc);
            }
        } else if p.extension().is_some_and(|x| x == "rs") {
            let s = std::fs::read_to_string(&p).unwrap_or_default();
            if p.components().any(|c| c.as_os_str() == "tests") {
                acc.push_str(&s);
            } else if let Some(i) = s.find("#[cfg(test)]") {
                acc.push_str(&s[i..]);
            }
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    match std::env::args().nth(1).as_deref() {
        Some("trace") => {}
        _ => {
            eprintln!("usage: cargo xtask trace");
            std::process::exit(2);
        }
    }
    let ids = rule_ids(&std::fs::read_to_string(root.join("EENKAH-SPEC.md")).expect("EENKAH-SPEC.md"));
    let mut tests = String::new();
    test_text(&root.join("crates"), &mut tests);
    let norm = tests.to_ascii_lowercase().replace('_', "-");
    let deferred: BTreeMap<String, String> = std::fs::read_to_string(root.join("docs/DEFERRED.md"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            l.strip_prefix("| ")
                .and_then(|r| r.split_once(" | "))
                .map(|(id, why)| (id.trim().to_string(), why.trim_end_matches(" |").to_string()))
        })
        .collect();
    let (mut ok, mut def, mut missing) = (0, 0, vec![]);
    for id in &ids {
        // accept "FC-2" in a comment or "fc2_" in a test name
        let compact = id.to_ascii_lowercase().replace('-', "");
        let tested = tests.contains(id.as_str()) || norm.contains(&id.to_ascii_lowercase()) || norm.contains(&format!("{compact}-")) || tests.contains(&format!("{compact}_"));
        if tested {
            ok += 1;
        } else if deferred.contains_key(id) {
            def += 1;
        } else {
            missing.push(id.clone());
        }
    }
    println!("{} rules: {ok} tested, {def} deferred (docs/DEFERRED.md), {} untested", ids.len(), missing.len());
    for id in &missing {
        println!("  UNTESTED {id}");
    }
    if !missing.is_empty() {
        std::process::exit(1);
    }
}
