//! Per-frame reference swatches compensate for moderate channel gain/offset.
use super::{bitmap::BitmapView, locator::GridTransform};
use crate::{Error, Result};

pub struct CalibratedPalette {
    black: [f64; 3],
    range: [f64; 3],
    colors: [[f64; 3]; 4],
}
impl CalibratedPalette {
    /// Average nine interior samples per swatch, then require distinguishable colors.
    pub fn from_image(image: BitmapView<'_>, grid: GridTransform) -> Result<Self> {
        let mut observed = [[0.0; 3]; 5];
        for (i, color) in observed.iter_mut().enumerate() {
            for dy in [-1.0, 0.0, 1.0] {
                for dx in [-1.0, 0.0, 1.0] {
                    let rgb = grid.sample(
                        image,
                        20.0 + 16.0 * i as f64 + dx,
                        grid.profile.matrix_size as f64 + 24.0 + dy,
                    )?;
                    for c in 0..3 {
                        color[c] += rgb[c] / 9.0;
                    }
                }
            }
        }
        let mut palette = Self {
            black: observed[0],
            range: [0.0; 3],
            colors: [[0.0; 3]; 4],
        };
        for (c, range) in palette.range.iter_mut().enumerate() {
            *range = observed[4][c] - observed[0][c];
            if *range < 80.0 {
                return Err(Error::Palette);
            }
        }
        for (i, color) in observed.iter().take(4).enumerate() {
            palette.colors[i] = palette.normalize(*color);
        }
        for i in 0..4 {
            for j in i + 1..4 {
                if distance(palette.colors[i], palette.colors[j]) < 0.5 {
                    return Err(Error::Palette);
                }
            }
        }
        Ok(palette)
    }
    fn normalize(&self, rgb: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| (rgb[i] - self.black[i]) / self.range[i])
    }
    /// Return a 2-bit symbol only if both absolute distance and separation pass.
    pub fn classify(&self, rgb: [f64; 3]) -> Result<u8> {
        let color = self.normalize(rgb);
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
        if best.0 > 0.35 || second - best.0 < 0.15 {
            Err(Error::Palette)
        } else {
            Ok(best.1 as u8)
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
