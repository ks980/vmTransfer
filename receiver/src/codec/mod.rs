//! Pure bitmap -> validated frame path shared by PNG fixtures and live capture.
pub mod bitmap;
pub mod locator;
pub mod palette;
use crate::{
    protocol::{parse_frame, unpack_symbols, ValidatedFrame},
    Error, Result,
};
use bitmap::BitmapView;

/// Locate, calibrate, sample, unpack and validate one image. No collection state is
/// modified here. Multiple candidates are bounded by the locator; the first fully
/// valid frame wins. CRC failure is reported preferentially over a false candidate.
pub fn decode_bitmap(image: BitmapView<'_>) -> Result<ValidatedFrame> {
    let grids = locator::locate(image)?;
    let mut last = Error::Palette;
    let mut saw_crc = false;
    for grid in grids {
        let result = (|| {
            let palette = palette::CalibratedPalette::from_image(image, grid)?;
            let n = grid.profile.matrix_size;
            let mut symbols = vec![0u8; n * n];
            for y in 0..n {
                for x in 0..n {
                    let rgb = grid.sample(image, 16.5 + x as f64, 16.5 + y as f64)?;
                    symbols[y * n + x] = palette.classify(rgb)?;
                }
            }
            let frame = parse_frame(&unpack_symbols(&symbols)?)?;
            if frame.header().profile != grid.profile {
                return Err(Error::Invalid("image/header profile mismatch"));
            }
            Ok(frame)
        })();
        match result {
            Ok(frame) => return Ok(frame),
            Err(Error::Crc) => saw_crc = true,
            Err(e) => last = e,
        }
    }
    if saw_crc {
        Err(Error::Crc)
    } else {
        Err(last)
    }
}
