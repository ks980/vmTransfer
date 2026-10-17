//! Transport-independent VMT1/VMT2 receiver. PNG and live capture share the
//! same bitmap decoder and receive session; no sockets or desktop APIs live here.
pub mod capture;
pub mod codec;
pub mod error;
pub mod metrics;
pub mod protocol;
pub mod session;
pub mod storage;
pub mod ui;

pub use error::{Error, Result};
