//! eenkah-connect: download the athlete's own data from Garmin Connect.
//!
//! This is the only part of eenkah that talks to the network, and it does so only when the
//! athlete runs `eenkah login` or `eenkah import` without a path. Everything it downloads lands
//! in an inbox folder, which the normal file import then reads; nothing here touches the
//! database.

pub mod auth;
pub mod http;
pub mod sync;

pub use auth::{login, Prompt, Tokens};
pub use http::{Counters, Net, Retry, Transport};
pub use sync::{sync, Account, Client, Known, Options, Progress, Report};
