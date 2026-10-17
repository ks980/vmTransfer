//! Portal authorization contracts. With `live-capture`, PortalCapture implements
//! them using zbus + the system PipeWire ABI. No desktop capture occurs by default.

#[cfg(feature = "live-capture")]
pub use super::live::PortalCapture;

/// Selection preference, not a promise: the portal/user decides the available source.
#[derive(Debug, Clone, Copy)]
pub enum CaptureTarget {
    Window,
    Monitor,
    WindowOrMonitor,
}

/// Cancellation is an explicit user outcome; it must not trigger repeated dialogs.
#[derive(Debug, Clone)]
pub enum CaptureFailure {
    PermissionDenied,
    Cancelled,
    PortalUnavailable,
    Disconnected,
    UnsupportedFormat,
    /// No initial frame arrived before the configured first-frame deadline.
    FirstFrameTimeout,
    Backend(String),
}

impl std::fmt::Display for CaptureFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PermissionDenied => write!(f, "capture permission denied; authorize a window/screen in the GNOME chooser"),
            Self::Cancelled => write!(f, "capture selection cancelled"),
            Self::PortalUnavailable => write!(f, "ScreenCast portal unavailable; run as your desktop user in a GNOME session"),
            Self::Disconnected => write!(f, "capture was closed or disconnected; start a new receive session"),
            Self::UnsupportedFormat => write!(f, "unsupported capture format; a mappable packed RGB buffer is required"),
            Self::FirstFrameTimeout => write!(f, "no initial video frame within the deadline; check the selected window and PipeWire session"),
            Self::Backend(message) => write!(f, "capture: {message}"),
        }
    }
}
impl std::error::Error for CaptureFailure {}

/// Future concrete adapter implements FrameSource after obtaining user consent.
/// The authorization operation is async because D-Bus requests involve user input.
/// Signature remains independent of a specific async runtime or ashpd version.
#[allow(async_fn_in_trait)]
pub trait AuthorizedCapture: super::FrameSource + Sized {
    async fn authorize(target: CaptureTarget) -> std::result::Result<Self, CaptureFailure>;
}
