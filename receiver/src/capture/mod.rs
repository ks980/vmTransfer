//! Image-source boundaries. PNG and optional live capture share the same decoder.
#[cfg(feature = "live-capture")]
mod dbus;
#[cfg(feature = "live-capture")]
mod live;
pub mod png;
pub mod portal;
#[cfg(feature = "live-capture")]
mod video;
use crate::{codec::bitmap::Bitmap, Result};

/// Source-owned image and optional monotonic capture time (nanoseconds since source start).
pub struct CapturedFrame {
    pub bitmap: Bitmap,
    pub timestamp_ns: Option<u64>,
}

/// Pull interface for future application orchestration. Do not request another
/// frame after ReceiveSession::is_consuming() becomes false. A live implementation
/// must bound its queue (prefer newest frame), release PipeWire buffers promptly,
/// and surface permission/disconnection errors. None means normal source exhaustion.
pub trait FrameSource {
    fn next_frame(&mut self) -> Result<Option<CapturedFrame>>;
    /// Idempotently stop capture and release streams/session resources.
    fn stop(&mut self) -> Result<()>;
}
