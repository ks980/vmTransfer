//! Native PipeWire ownership, bounded latest-frame slot, and Portal session guard.
//! No frame crosses the C ABI until a portal-authorized FD has been obtained.
use super::{
    dbus::{self, PortalSession},
    portal::{AuthorizedCapture, CaptureFailure, CaptureTarget},
    video::{copy_video, PackedFormat, VideoLayout},
    CapturedFrame, FrameSource,
};
use crate::{codec::bitmap::Bitmap, Error, Result};
use futures_lite::StreamExt;
use serde::Serialize;
use std::{
    ffi::{c_char, c_void, CStr, CString},
    os::fd::IntoRawFd,
    ptr::NonNull,
    sync::{Arc, Condvar, Mutex, Once},
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[repr(C)]
struct RawFrame {
    data: *const u8,
    maxsize: u32,
    offset: u32,
    size: u32,
    stride: i32,
    width: u32,
    height: u32,
    format: u32,
    crop_x: u32,
    crop_y: u32,
    crop_width: u32,
    crop_height: u32,
}
unsafe extern "C" {
    fn vmt_pw_init();
    fn vmt_pw_open(
        fd: i32,
        node: u32,
        serial: *const c_char,
        user: *mut c_void,
        frame: unsafe extern "C" fn(*mut c_void, *const RawFrame),
        error: unsafe extern "C" fn(*mut c_void, i32, *const c_char),
    ) -> *mut c_void;
    fn vmt_pw_close(capture: *mut c_void);
}
static INIT: Once = Once::new();

/// These count native video buffers, independently of decoded valid file blocks.
#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct CaptureStats {
    pub buffers: u64,
    pub overwritten: u64,
}
#[derive(Default)]
struct Slot {
    frame: Option<CapturedFrame>,
    error: Option<CaptureFailure>,
    stopped: bool,
    stats: CaptureStats,
}
struct Shared {
    slot: Mutex<Slot>,
    ready: Condvar,
    started: Instant,
}
impl Shared {
    fn new() -> Self {
        Self {
            slot: Mutex::new(Slot::default()),
            ready: Condvar::new(),
            started: Instant::now(),
        }
    }
    fn fail(&self, error: CaptureFailure) {
        if let Ok(mut slot) = self.slot.lock() {
            if !slot.stopped && slot.error.is_none() {
                slot.error = Some(error);
                slot.frame = None;
            }
            self.ready.notify_all();
        }
    }
    fn push(&self, bitmap: Bitmap) {
        if let Ok(mut slot) = self.slot.lock() {
            if slot.stopped || slot.error.is_some() {
                return;
            }
            slot.stats.buffers += 1;
            if slot.frame.is_some() {
                slot.stats.overwritten += 1;
            }
            slot.frame = Some(CapturedFrame {
                bitmap,
                timestamp_ns: Some(self.started.elapsed().as_nanos().min(u64::MAX as u128) as u64),
            });
            self.ready.notify_one();
        }
    }
    fn next(&self, timeout: Option<Duration>) -> Result<Option<CapturedFrame>> {
        let mut slot = self.slot.lock().map_err(|_| {
            Error::Capture(CaptureFailure::Backend("capture queue poisoned".into()))
        })?;
        let deadline = timeout.map(|duration| Instant::now() + duration);
        loop {
            if let Some(error) = slot.error.clone() {
                return Err(Error::Capture(error));
            }
            if slot.stopped {
                return Ok(None);
            }
            if let Some(frame) = slot.frame.take() {
                return Ok(Some(frame));
            }
            if let Some(deadline) = deadline {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(Error::Capture(CaptureFailure::FirstFrameTimeout));
                }
                slot = self
                    .ready
                    .wait_timeout(slot, remaining)
                    .map_err(|_| Error::Capture(CaptureFailure::Disconnected))?
                    .0;
            } else {
                slot = self
                    .ready
                    .wait(slot)
                    .map_err(|_| Error::Capture(CaptureFailure::Disconnected))?;
            }
        }
    }
    fn stop(&self) {
        if let Ok(mut slot) = self.slot.lock() {
            slot.stopped = true;
            slot.frame = None;
            self.ready.notify_all();
        }
    }
}

/// An authorized capture source. The PipeWire thread owns mapped buffers; callbacks
/// synchronously copy only bounded pixels, then return them to PipeWire. A capacity-1
/// slot prevents decoder lag from creating unbounded queues. Drop joins both workers
/// before releasing callback state, then closes the Portal session.
pub struct PortalCapture {
    native: Option<NonNull<c_void>>,
    shared: Arc<Shared>,
    portal: Option<PortalSession>,
    cancel: async_channel::Sender<()>,
    watcher: Option<JoinHandle<()>>,
    first: bool,
    first_frame_timeout: Duration,
}
impl AuthorizedCapture for PortalCapture {
    async fn authorize(target: CaptureTarget) -> std::result::Result<Self, CaptureFailure> {
        let remote = dbus::authorize(target).await?;
        let shared = Arc::new(Shared::new());
        INIT.call_once(|| unsafe { vmt_pw_init() });
        let serial = remote
            .serial
            .map(|serial| CString::new(serial.to_string()).unwrap());
        // SAFETY: C consumes the FD on every path. Arc keeps user state stable;
        // callbacks finish before close returns and before shared can be dropped.
        let native = unsafe {
            vmt_pw_open(
                remote.fd.into_raw_fd(),
                remote.node,
                serial.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
                Arc::as_ptr(&shared) as *mut c_void,
                on_frame,
                on_error,
            )
        };
        let Some(native) = NonNull::new(native) else {
            return Err(CaptureFailure::Backend(format!(
                "PipeWire connection: {}",
                std::io::Error::last_os_error()
            )));
        };
        let (cancel, receive) = async_channel::bounded(1);
        let mut source = Self {
            native: Some(native),
            shared: shared.clone(),
            portal: Some(remote.session),
            cancel,
            watcher: None,
            first: true,
            first_frame_timeout: Duration::from_secs(15),
        };
        let watch = std::thread::Builder::new()
            .name("vmt-portal-watch".into())
            .spawn(move || {
                futures_lite::future::block_on(async move {
                    let mut closed = remote.closed;
                    let ended = futures_lite::future::race(
                        async {
                            let _ = closed.next().await;
                            true
                        },
                        async {
                            let _ = receive.recv().await;
                            false
                        },
                    )
                    .await;
                    if ended {
                        shared.fail(CaptureFailure::Disconnected);
                    }
                });
            })
            .map_err(|e| CaptureFailure::Backend(e.to_string()))?;
        source.watcher = Some(watch);
        Ok(source)
    }
}
impl PortalCapture {
    /// Only the initial frame has a deadline. Paused senders/static desktop streams
    /// may legitimately emit no updates indefinitely; Portal closure still wakes us.
    pub fn set_first_frame_timeout(&mut self, timeout: Duration) {
        self.first_frame_timeout = timeout;
    }
    pub fn stats(&self) -> CaptureStats {
        self.shared.slot.lock().map(|s| s.stats).unwrap_or_default()
    }
    fn close(&mut self) -> std::result::Result<(), CaptureFailure> {
        self.shared.stop();
        let _ = self.cancel.try_send(());
        if let Some(native) = self.native.take() {
            unsafe {
                vmt_pw_close(native.as_ptr());
            }
        }
        if let Some(watcher) = self.watcher.take() {
            let _ = watcher.join();
        }
        if let Some(mut session) = self.portal.take() {
            futures_lite::future::block_on(session.close())?;
        }
        Ok(())
    }
}
impl FrameSource for PortalCapture {
    fn next_frame(&mut self) -> Result<Option<CapturedFrame>> {
        let frame = self.shared.next(if self.first {
            Some(self.first_frame_timeout)
        } else {
            None
        })?;
        if frame.is_some() {
            self.first = false;
        }
        Ok(frame)
    }
    fn stop(&mut self) -> Result<()> {
        self.close().map_err(Error::Capture)
    }
}
impl Drop for PortalCapture {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

unsafe extern "C" fn on_frame(user: *mut c_void, raw: *const RawFrame) {
    // SAFETY: pw_bridge.c calls while the Arc allocation and mapped buffer live.
    let shared = unsafe { &*(user as *const Shared) };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
        let raw = unsafe { &*raw };
        if raw.data.is_null() || raw.maxsize == 0 || raw.maxsize > 256 * 1024 * 1024 {
            return Err(Error::Invalid("mapped video size"));
        }
        let layout = VideoLayout {
            width: raw.width as usize,
            height: raw.height as usize,
            stride: raw.stride,
            format: PackedFormat::try_from(raw.format)?,
            offset: raw.offset as usize,
            size: raw.size as usize,
            crop: [
                raw.crop_x as usize,
                raw.crop_y as usize,
                raw.crop_width as usize,
                raw.crop_height as usize,
            ],
        };
        // SAFETY: MAP_BUFFERS supplies a readable mapping of maxsize bytes for this
        // callback only. copy_video does not retain a slice and checks every extent.
        let bytes = unsafe { std::slice::from_raw_parts(raw.data, raw.maxsize as usize) };
        shared.push(copy_video(bytes, layout)?);
        Ok(())
    }));
    match result {
        Ok(Ok(())) => (),
        Ok(Err(e)) => shared.fail(CaptureFailure::Backend(e.to_string())),
        Err(_) => shared.fail(CaptureFailure::Backend("pixel callback panicked".into())),
    }
}
unsafe extern "C" fn on_error(user: *mut c_void, code: i32, message: *const c_char) {
    let shared = unsafe { &*(user as *const Shared) };
    let result = std::panic::catch_unwind(|| {
        let error = if code == 2 {
            CaptureFailure::UnsupportedFormat
        } else {
            let message = if message.is_null() {
                "PipeWire disconnected".into()
            } else {
                unsafe { CStr::from_ptr(message) }
                    .to_string_lossy()
                    .into_owned()
            };
            CaptureFailure::Backend(message)
        };
        shared.fail(error);
    });
    if result.is_err() {
        shared.fail(CaptureFailure::Disconnected);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bitmap(value: u8) -> Bitmap {
        Bitmap {
            width: 1,
            height: 1,
            rgba: vec![value, 0, 0, 255],
        }
    }
    #[test]
    fn queue_keeps_only_latest_and_counts_overwrites() {
        let shared = Shared::new();
        for value in 0..100 {
            shared.push(bitmap(value));
        }
        assert_eq!(
            shared
                .next(Some(Duration::ZERO))
                .unwrap()
                .unwrap()
                .bitmap
                .rgba[0],
            99
        );
        let stats = shared.slot.lock().unwrap().stats;
        assert_eq!(stats.buffers, 100);
        assert_eq!(stats.overwritten, 99);
    }
    #[test]
    fn stop_and_failure_wake_blocked_consumer() {
        for fail in [false, true] {
            let shared = Arc::new(Shared::new());
            let other = shared.clone();
            let thread = std::thread::spawn(move || other.next(None));
            if fail {
                shared.fail(CaptureFailure::Disconnected);
            } else {
                shared.stop();
            }
            let result = thread.join().unwrap();
            if fail {
                assert!(matches!(
                    result,
                    Err(Error::Capture(CaptureFailure::Disconnected))
                ));
            } else {
                assert!(result.unwrap().is_none());
            }
        }
    }
    #[test]
    fn initial_timeout_is_explicit_and_errors_discard_pending_frame() {
        let shared = Shared::new();
        assert!(matches!(
            shared.next(Some(Duration::ZERO)),
            Err(Error::Capture(CaptureFailure::FirstFrameTimeout))
        ));
        shared.push(bitmap(1));
        shared.fail(CaptureFailure::Disconnected);
        assert!(shared.next(Some(Duration::ZERO)).is_err());
    }
}
