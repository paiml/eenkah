//! Embed the built React app (`ui/dist`). If it has not been built, embed a page that says so
//! instead of failing, so `cargo test` works without Node.

use std::fmt::Write;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, root, out);
        } else {
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            out.push((rel, p));
        }
    }
}

fn main() {
    let dist = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../ui/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    let mut files = vec![];
    walk(&dist, &dist, &mut files);
    for (_, p) in &files {
        println!("cargo:rerun-if-changed={}", p.display());
    }
    let built = files.iter().any(|(n, _)| n == "index.html");
    let mut s = String::new();
    writeln!(s, "pub const BUILT: bool = {built};").unwrap();
    writeln!(s, "pub fn file(name: &str) -> Option<&'static [u8]> {{ match name {{").unwrap();
    for (n, p) in &files {
        writeln!(s, "{:?} => Some(include_bytes!({:?})),", n, p.canonicalize().unwrap().to_string_lossy()).unwrap();
    }
    if !built {
        println!("cargo:warning=eenkah web UI not built (ui/dist missing): run `make build`");
        writeln!(
            s,
            r#""index.html" => Some(b"<!doctype html><title>eenkah</title><p>The web UI was not built into this binary. Run <code>make build</code>."),"#
        )
        .unwrap();
    }
    writeln!(s, "_ => None }} }}").unwrap();
    std::fs::write(PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("ui_assets.rs"), s).unwrap();
}
