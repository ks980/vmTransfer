//! Validated borrowed images. Stride and channel order are explicit so capture
//! adapters never leak PipeWire-specific buffers into protocol code.
use crate::{Error, Result};

pub const MAX_PIXELS: usize = 32 * 1024 * 1024;
#[derive(Debug, Clone, Copy)]
pub enum PixelFormat {
    Rgb8,
    Rgba8,
    Bgra8,
    Bgrx8,
}
impl PixelFormat {
    pub fn channels(self) -> usize {
        match self {
            Self::Rgb8 => 3,
            _ => 4,
        }
    }
}

#[derive(Clone, Copy)]
pub struct BitmapView<'a> {
    pub(crate) width: usize,
    pub(crate) height: usize,
    stride: usize,
    format: PixelFormat,
    bytes: &'a [u8],
}
impl<'a> BitmapView<'a> {
    /// Validate dimensions/stride before sampling. Alpha is ignored: adapters must
    /// supply opaque/composited pixels, as the PNG adapter does.
    pub fn new(
        width: usize,
        height: usize,
        stride: usize,
        format: PixelFormat,
        bytes: &'a [u8],
    ) -> Result<Self> {
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || width.checked_mul(height).is_none_or(|n| n > MAX_PIXELS)
        {
            return Err(Error::Invalid("image dimensions"));
        }
        let row = width * format.channels();
        let required = (height - 1)
            .checked_mul(stride)
            .and_then(|n| n.checked_add(row));
        if stride < row || required.is_none_or(|n| n > bytes.len()) {
            return Err(Error::Invalid("image stride/buffer"));
        }
        Ok(Self {
            width,
            height,
            stride,
            format,
            bytes,
        })
    }
    pub fn width(self) -> usize {
        self.width
    }
    pub fn height(self) -> usize {
        self.height
    }
    pub(crate) fn rgb(self, x: usize, y: usize) -> [f64; 3] {
        let i = y * self.stride + x * self.format.channels();
        match self.format {
            PixelFormat::Rgb8 | PixelFormat::Rgba8 => [
                self.bytes[i] as f64,
                self.bytes[i + 1] as f64,
                self.bytes[i + 2] as f64,
            ],
            _ => [
                self.bytes[i + 2] as f64,
                self.bytes[i + 1] as f64,
                self.bytes[i] as f64,
            ],
        }
    }
    pub(crate) fn sample(self, x: f64, y: f64) -> Result<[f64; 3]> {
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.0
            || y < 0.0
            || x >= self.width as f64
            || y >= self.height as f64
        {
            return Err(Error::Locate);
        }
        Ok(self.rgb(x.floor() as usize, y.floor() as usize))
    }
    pub(crate) fn dark(self, x: usize, y: usize) -> bool {
        self.rgb(x, y).iter().all(|&v| v < 128.0)
    }
}

/// Adapter-owned opaque RGBA bitmap. Future PipeWire capture may borrow instead.
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}
impl Bitmap {
    pub fn view(&self) -> Result<BitmapView<'_>> {
        BitmapView::new(
            self.width,
            self.height,
            self.width.checked_mul(4).ok_or(Error::Invalid("width"))?,
            PixelFormat::Rgba8,
            &self.rgba,
        )
    }
}
