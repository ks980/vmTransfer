//! Future native progress presentation boundary. Phase 1's CLI prints events and a
//! JSON metrics snapshot directly. No GUI toolkit or portal behavior is stubbed.
use crate::{capture::portal::CaptureFailure, metrics::MetricsSnapshot, session::ReceiveState};

/// Implement on the UI thread; capture/decoding should send bounded progress
/// updates rather than blocking the PipeWire callback on rendering or file I/O.
pub trait ProgressView {
    /// Show received/total, duplicates, CRC failures, unique valid blocks/s,
    /// effective goodput and elapsed time. Complete/Failed are terminal states.
    fn render(&mut self, state: &ReceiveState, metrics: &MetricsSnapshot<'_>);
    /// Explain permission denied/unavailable/cancelled distinctly. Do not silently
    /// retry a portal dialog or switch to an unauthorized capture API.
    fn capture_failure(&mut self, error: &CaptureFailure);
}
