//! suyo as a library: the series store, the Graphite expression engine, and
//! the HTTP server with its embedded UI. The `suyo` binary is a thin CLI over
//! this crate; other programs (e.g. eenkah) link it to get the same charts.

pub mod catalog;
pub mod collect;
pub mod eval;
pub mod ingest;
pub mod parser;
pub mod presets;
pub mod render;
pub mod series;
pub mod server;
pub mod sources;
pub mod store;
pub mod svg;
pub mod timeutil;
