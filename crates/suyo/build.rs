//! Embeds the built web UI (ui/dist) into the binary. Without a build (no Node toolchain),
//! a placeholder page is embedded instead so `cargo build` / `cargo test` still work.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "svg" => "image/svg+xml",
        "json" | "map" => "application/json",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, root, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            out.push((rel, p));
        }
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let dist = manifest.join("ui").join("dist");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=ui/dist");
    // a missing file always counts as changed, so a placeholder build picks up the UI once built
    println!("cargo:rerun-if-changed=ui/dist/index.html");

    let mut files = vec![];
    if dist.join("index.html").is_file() {
        walk(&dist, &dist, &mut files);
    } else {
        println!("cargo:warning=ui/dist/index.html not found: embedding a placeholder page (run `make build`)");
    }

    let mut src = String::from("/// (path under dist, content type, bytes)\npub static UI_FILES: &[(&str, &str, &[u8])] = &[\n");
    for (rel, path) in &files {
        writeln!(
            src,
            "    ({rel:?}, {:?}, include_bytes!({:?})),",
            content_type(rel),
            path.to_string_lossy()
        )
        .unwrap();
    }
    if files.is_empty() {
        src.push_str("    (\"index.html\", \"text/html; charset=utf-8\", b\"<!doctype html><html lang=\\\"en\\\"><head><meta charset=\\\"utf-8\\\"><title>suyo</title></head><body style=\\\"font:15px system-ui;padding:40px\\\"><h1>suyo</h1><p>UI not built: run <code>make build</code></p><p>The API (/render, /api/...) works without it.</p></body></html>\"),\n");
    }
    src.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("ui_assets.rs");
    std::fs::write(out, src).unwrap();
}
