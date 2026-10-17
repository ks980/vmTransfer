//! Bounded PNG adapter, used by both CLI and tests. PNG compression is only the
//! fixture image container; original transferred file bytes are never compressed.
use crate::{
    codec::bitmap::{Bitmap, MAX_PIXELS},
    Error, Result,
};
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

/// Diagnostic screenshot export, never an alternate channel for original file
/// bytes. Refuses to overwrite and removes a partially written PNG on failure.
pub fn save_png(path: &Path, bitmap: &Bitmap) -> Result<()> {
    use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};
    bitmap.view()?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    let result = (|| {
        let mut encoder = png::Encoder::new(&file, bitmap.width as u32, bitmap.height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| Error::Image(e.to_string()))?;
        writer
            .write_image_data(&bitmap.rgba)
            .map_err(|e| Error::Image(e.to_string()))?;
        writer.finish().map_err(|e| Error::Image(e.to_string()))?;
        file.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

pub fn load_png(path: &Path) -> Result<Bitmap> {
    let file = File::open(path)?;
    if file.metadata()?.len() > 128 * 1024 * 1024 {
        return Err(Error::Invalid("PNG file size"));
    }
    let mut decoder = png::Decoder::new(BufReader::new(file));
    decoder.set_limits(png::Limits {
        bytes: 128 * 1024 * 1024,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| Error::Image(e.to_string()))?;
    let (width, height) = (reader.info().width as usize, reader.info().height as usize);
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || width.checked_mul(height).is_none_or(|n| n > MAX_PIXELS)
    {
        return Err(Error::Invalid("PNG dimensions"));
    }
    let size = reader
        .output_buffer_size()
        .ok_or(Error::Invalid("PNG buffer size"))?;
    if size > MAX_PIXELS * 4 {
        return Err(Error::Invalid("PNG decoded size"));
    }
    let mut raw = vec![0; size];
    let info = reader
        .next_frame(&mut raw)
        .map_err(|e| Error::Image(e.to_string()))?;
    if info.bit_depth != png::BitDepth::Eight {
        return Err(Error::Invalid("PNG bit depth"));
    }
    let channels = info.color_type.samples();
    let mut rgba = Vec::with_capacity(width * height * 4);
    for p in raw[..info.buffer_size()].chunks_exact(channels) {
        let (rgb, alpha) = match info.color_type {
            png::ColorType::Rgb => ([p[0], p[1], p[2]], 255),
            png::ColorType::Rgba => ([p[0], p[1], p[2]], p[3]),
            png::ColorType::Grayscale => ([p[0]; 3], 255),
            png::ColorType::GrayscaleAlpha => ([p[0]; 3], p[1]),
            _ => return Err(Error::Invalid("unexpanded PNG palette")),
        };
        for c in rgb {
            rgba.push(((c as u32 * alpha as u32 + 255 * (255 - alpha as u32) + 127) / 255) as u8);
        }
        rgba.push(255);
    }
    if rgba.len() != width * height * 4 {
        return Err(Error::Invalid("PNG pixel count"));
    }
    Ok(Bitmap {
        width,
        height,
        rgba,
    })
}

/// Ordered PNG paths only. Metadata and location must be recovered from pixels.
pub struct PngSequence {
    paths: std::vec::IntoIter<PathBuf>,
    stopped: bool,
}
impl PngSequence {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self {
            paths: paths.into_iter(),
            stopped: false,
        }
    }
}
impl super::FrameSource for PngSequence {
    fn next_frame(&mut self) -> Result<Option<super::CapturedFrame>> {
        if self.stopped {
            return Ok(None);
        }
        self.paths
            .next()
            .map(|path| {
                load_png(&path).map(|bitmap| super::CapturedFrame {
                    bitmap,
                    timestamp_ns: None,
                })
            })
            .transpose()
    }
    fn stop(&mut self) -> Result<()> {
        self.stopped = true;
        Ok(())
    }
}
