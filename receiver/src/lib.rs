//! Transport-independent VMT1 receiver. PNG and future capture adapters share the
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
