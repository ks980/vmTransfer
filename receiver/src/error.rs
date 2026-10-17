//! Typed failures distinguish discarded frames from terminal receive failures.
use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    /// Inconsistent pixels/bytes: discard without changing collection state.
    Crc,
    /// No supported finder arrangement was detected.
    Locate,
    /// Reference swatches or data colors are ambiguous.
    Palette,
    /// Bounds, canonical encoding, or protocol consistency check failed.
    Invalid(&'static str),
    /// Same transfer identity contains contradictory, CRC-valid content.
    Conflict,
    /// All blocks arrived but file length or SHA-256 is wrong; session is terminal.
    HashMismatch,
    /// An operation was attempted on a failed session.
    FailedSession,
    Io(io::Error),
    Image(String),
    /// Permission, Portal, PipeWire or stream-lifecycle failure.
    Capture(crate::capture::portal::CaptureFailure),
}

pub type Result<T> = std::result::Result<T, Error>;
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Crc => write!(f, "frame CRC mismatch"),
            Self::Locate => write!(f, "profile-1 finder markers not found"),
            Self::Palette => write!(f, "insufficient color confidence"),
            Self::Invalid(s) => write!(f, "invalid frame/image: {s}"),
            Self::Conflict => write!(
                f,
                "conflicting content for the active transfer; start a fresh session"
            ),
            Self::HashMismatch => write!(f, "file length/SHA-256 mismatch; start a fresh session"),
            Self::FailedSession => write!(f, "receive session already failed"),
            Self::Io(e) => write!(f, "I/O: {e}"),
            Self::Image(e) => write!(f, "PNG: {e}"),
            Self::Capture(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for Error {}
