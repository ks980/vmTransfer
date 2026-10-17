//! Pure bounds-checked conversion of mapped packed video. C only describes the
//! buffer; Rust validates chunk bounds, stride and crop before reading pixels.
use crate::{
    codec::bitmap::{Bitmap, MAX_PIXELS},
    Error, Result,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum PackedFormat {
    Bgra,
    Bgrx,
    Rgba,
    Rgbx,
    Rgb,
    Bgr,
}
impl TryFrom<u32> for PackedFormat {
    type Error = Error;
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(Self::Bgra),
            1 => Ok(Self::Bgrx),
            2 => Ok(Self::Rgba),
            3 => Ok(Self::Rgbx),
            4 => Ok(Self::Rgb),
            5 => Ok(Self::Bgr),
            _ => Err(Error::Invalid("packed video format")),
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct VideoLayout {
    pub width: usize,
    pub height: usize,
    pub stride: i32,
    pub format: PackedFormat,
    pub offset: usize,
    pub size: usize,
    pub crop: [usize; 4],
}
pub(crate) fn copy_video(bytes: &[u8], layout: VideoLayout) -> Result<Bitmap> {
    let VideoLayout {
        width,
        height,
        stride,
        format,
        offset,
        size,
        crop: [x, y, w, h],
    } = layout;
    let channels = match format {
        PackedFormat::Rgb | PackedFormat::Bgr => 3,
        _ => 4,
    };
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || width.checked_mul(height).is_none_or(|n| n > MAX_PIXELS)
        || stride <= 0
    {
        return Err(Error::Invalid("video dimensions/negative stride"));
    }
    let stride = stride as usize;
    let extent = (height - 1)
        .checked_mul(stride)
        .and_then(|v| v.checked_add(width * channels));
    if stride < width * channels
        || extent.is_none_or(|n| n > size)
        || offset.checked_add(size).is_none_or(|n| n > bytes.len())
    {
        return Err(Error::Invalid("video chunk/stride bounds"));
    }
    if w == 0
        || h == 0
        || x.checked_add(w).is_none_or(|n| n > width)
        || y.checked_add(h).is_none_or(|n| n > height)
    {
        return Err(Error::Invalid("video crop bounds"));
    }
    let mut rgba = vec![255; w * h * 4];
    for row in 0..h {
        for column in 0..w {
            let source = offset + (y + row) * stride + (x + column) * channels;
            let target = (row * w + column) * 4;
            let p = &bytes[source..source + channels];
            match format {
                PackedFormat::Bgra | PackedFormat::Bgrx | PackedFormat::Bgr => {
                    rgba[target..target + 3].copy_from_slice(&[p[2], p[1], p[0]])
                }
                _ => rgba[target..target + 3].copy_from_slice(&p[..3]),
            }
        }
    }
    Ok(Bitmap {
        width: w,
        height: h,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offset_padding_crop_and_all_formats() {
        for code in 0..6 {
            let format = PackedFormat::try_from(code).unwrap();
            let channels = if code < 4 { 4 } else { 3 };
            let stride = 4 * channels + 7;
            let mut bytes = vec![222; 5 + stride * 3];
            for row in 0..3 {
                for column in 0..4 {
                    let index = 5 + row * stride + column * channels;
                    let rgb = if matches!(
                        format,
                        PackedFormat::Bgra | PackedFormat::Bgrx | PackedFormat::Bgr
                    ) {
                        [30, 20, 10]
                    } else {
                        [10, 20, 30]
                    };
                    bytes[index..index + 3].copy_from_slice(&rgb);
                }
            }
            let image = copy_video(
                &bytes,
                VideoLayout {
                    width: 4,
                    height: 3,
                    stride: stride as i32,
                    format,
                    offset: 5,
                    size: stride * 3,
                    crop: [1, 1, 2, 2],
                },
            )
            .unwrap();
            assert_eq!((image.width, image.height), (2, 2));
            assert_eq!(image.rgba, [10, 20, 30, 255].repeat(4));
        }
    }
    #[test]
    fn malformed_buffers_are_rejected_without_reads() {
        let valid = VideoLayout {
            width: 4,
            height: 3,
            stride: 16,
            format: PackedFormat::Bgrx,
            offset: 0,
            size: 48,
            crop: [0, 0, 4, 3],
        };
        for layout in [
            VideoLayout {
                stride: -16,
                ..valid
            },
            VideoLayout { stride: 1, ..valid },
            VideoLayout { size: 47, ..valid },
            VideoLayout {
                offset: usize::MAX,
                ..valid
            },
            VideoLayout {
                width: 8193,
                ..valid
            },
            VideoLayout {
                crop: [4, 0, 1, 1],
                ..valid
            },
            VideoLayout {
                crop: [0, 0, 0, 1],
                ..valid
            },
            VideoLayout {
                crop: [usize::MAX, 0, 2, 2],
                ..valid
            },
        ] {
            assert!(copy_video(&[0; 48], layout).is_err());
        }
        assert!(PackedFormat::try_from(100).is_err());
    }

    #[test]
    fn mapped_bgrx_crop_enters_the_same_decoder_as_png() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../protocol/vectors/one");
        let original = crate::capture::png::load_png(&root.join("frame-000.png")).unwrap();
        let width = original.width + 11;
        let height = original.height + 13;
        let stride = width * 4 + 32;
        let mut mapped = vec![120; 17 + stride * height];
        for y in 0..original.height {
            for x in 0..original.width {
                let p = (y * original.width + x) * 4;
                let q = 17 + (y + 7) * stride + (x + 3) * 4;
                mapped[q..q + 4].copy_from_slice(&[
                    original.rgba[p + 2],
                    original.rgba[p + 1],
                    original.rgba[p],
                    0,
                ]);
            }
        }
        let copied = copy_video(
            &mapped,
            VideoLayout {
                width,
                height,
                stride: stride as i32,
                format: PackedFormat::Bgrx,
                offset: 17,
                size: stride * height,
                crop: [3, 7, original.width, original.height],
            },
        )
        .unwrap();
        assert_eq!(copied.rgba, original.rgba);
        let decoded = crate::codec::decode_bitmap(copied.view().unwrap()).unwrap();
        let expected =
            crate::protocol::parse_frame(&std::fs::read(root.join("frame-000.bin")).unwrap())
                .unwrap();
        assert_eq!(decoded.header(), expected.header());
        assert_eq!(decoded.payload(), expected.payload());
    }
}
