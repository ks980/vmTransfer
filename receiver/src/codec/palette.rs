//! Fixed monochrome mode bootstrap and per-frame color calibration.
use super::{bitmap::BitmapView, locator::GridTransform};
use crate::{Error, Result};

const PALETTE_2: [[u8; 3]; 2] = [[0, 0, 0], [255, 255, 255]];
const PALETTE_4: [[u8; 3]; 4] = [[0, 0, 0], [255, 0, 0], [0, 255, 0], [0, 0, 255]];
const PALETTE_8: [[u8; 3]; 8] = [
    [0, 0, 0],
    [255, 0, 0],
    [0, 255, 0],
    [0, 0, 255],
    [255, 255, 0],
    [255, 0, 255],
    [0, 255, 255],
    [255, 255, 255],
];
const PALETTE_16: [[u8; 3]; 16] = [
    [0, 0, 0],
    [255, 0, 0],
    [0, 255, 0],
    [0, 0, 255],
    [255, 255, 0],
    [255, 0, 255],
    [0, 255, 255],
    [255, 255, 255],
    [128, 0, 0],
    [0, 128, 0],
    [0, 0, 128],
    [128, 128, 0],
    [128, 0, 128],
    [0, 128, 128],
    [128, 128, 128],
    [255, 128, 0],
];

/// y=13 lies outside both the finder/timing marks and the encoded data area.
/// Legacy VMT1 leaves all twelve bootstrap cells white.
pub fn read_mode(image: BitmapView<'_>, grid: GridTransform) -> Result<u8> {
    let mut codes = [0u8; 4];
    for (repeat, code) in codes.iter_mut().enumerate() {
        for bit in 0..3 {
            let rgb = grid.sample(image, 16.5 + (repeat * 3 + bit) as f64, 13.5)?;
            if rgb.iter().all(|&v| v < 128.0) {
                *code |= 1 << (2 - bit);
            } else if !rgb.iter().all(|&v| v > 128.0) {
                return Err(Error::Palette);
            }
        }
    }
    if codes.iter().any(|&code| code != codes[0]) {
        return Err(Error::Palette);
    }
    match codes[0] {
        0 => Ok(4),
        1 => Ok(2),
        2 => Ok(8),
        3 => Ok(16),
        4 => Ok(24),
        _ => Err(Error::Palette),
    }
}

pub struct CalibratedPalette {
    black: [f64; 3],
    range: [f64; 3],
    colors: Vec<[f64; 3]>,
    mode: u8,
}
impl CalibratedPalette {
    /// Average nine interior samples per swatch. The two fixed anchors calibrate RGB channels.
    pub fn from_image(image: BitmapView<'_>, grid: GridTransform, mode: u8) -> Result<Self> {
        let canonical: &[[u8; 3]] = match mode {
            2 => &PALETTE_2,
            4 => &PALETTE_4,
            8 => &PALETTE_8,
            16 => &PALETTE_16,
            24 => &PALETTE_4,
            _ => return Err(Error::Palette),
        };
        let count = if mode == 4 || mode == 24 {
            5
        } else {
            canonical.len() + 2
        };
        let mut observed = vec![[0.0; 3]; count];
        for (i, color) in observed.iter_mut().enumerate() {
            let (x, y) = if mode == 4 {
                (
                    20.0 + 16.0 * i as f64,
                    grid.profile.matrix_height as f64 + 24.0,
                )
            } else {
                (
                    19.0 + 16.0 * (i % 9) as f64,
                    grid.profile.matrix_height as f64 + 21.0 + 7.0 * (i / 9) as f64,
                )
            };
            for dy in [-1.0, 0.0, 1.0] {
                for dx in [-1.0, 0.0, 1.0] {
                    let rgb = grid.sample(image, x + dx, y + dy)?;
                    for c in 0..3 {
                        color[c] += rgb[c] / 9.0;
                    }
                }
            }
        }
        let black = if mode == 4 || mode == 24 {
            observed[0]
        } else {
            observed[count - 2]
        };
        let white = observed[count - 1];
        let mut palette = Self {
            black,
            range: [0.0; 3],
            colors: Vec::new(),
            mode,
        };
        for (c, range) in palette.range.iter_mut().enumerate() {
            *range = white[c] - black[c];
            if *range < 80.0 {
                return Err(Error::Palette);
            }
        }
        if mode != 24 {
            palette.colors = observed
                .iter()
                .take(canonical.len())
                .map(|&rgb| palette.normalize(rgb))
                .collect();
            for i in 0..palette.colors.len() {
                for j in i + 1..palette.colors.len() {
                    if distance(palette.colors[i], palette.colors[j]) < 0.35 {
                        return Err(Error::Palette);
                    }
                }
            }
        }
        Ok(palette)
    }
    fn normalize(&self, rgb: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| (rgb[i] - self.black[i]) / self.range[i])
    }
    /// RGB888 is exact-byte transport. Even slight lossy changes may fail CRC.
    pub fn classify(&self, rgb: [f64; 3]) -> Result<u32> {
        let color = self.normalize(rgb);
        if self.mode == 24 {
            if color.iter().any(|&v| !(-0.1..=1.1).contains(&v)) {
                return Err(Error::Palette);
            }
            let channel = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
            return Ok((channel(color[0]) << 16) | (channel(color[1]) << 8) | channel(color[2]));
        }
        let mut best = (f64::INFINITY, 0);
        let mut second = f64::INFINITY;
        for (i, &reference) in self.colors.iter().enumerate() {
            let d = distance(color, reference);
            if d < best.0 {
                second = best.0;
                best = (d, i);
            } else if d < second {
                second = d;
            }
        }
        let margin = if self.mode == 16 { 0.1 } else { 0.15 };
        if best.0 > 0.35 || second - best.0 < margin {
            Err(Error::Palette)
        } else {
            Ok(best.1 as u32)
        }
    }
}
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f64>()
        .sqrt()
}
