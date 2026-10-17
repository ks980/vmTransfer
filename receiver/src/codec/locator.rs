//! QR-like 1:1:3:1:1 finder detection. No image dimensions, fixture hints or
//! decoded configuration are used to locate one of the predefined profiles.
use super::bitmap::BitmapView;
use crate::{
    protocol::{Profile, PROFILES},
    Error, Result,
};

#[derive(Debug, Clone, Copy)]
struct Finder {
    x: f64,
    y: f64,
    sx: f64,
    sy: f64,
}

/// Affine logical-symbol -> physical-pixel transform. Supports quarter turns and
/// independent axis scales. Coordinates describe pixel edges, samples use centers.
#[derive(Debug, Clone, Copy)]
pub struct GridTransform {
    pub profile: Profile,
    pub origin: [f64; 2],
    pub x_axis: [f64; 2],
    pub y_axis: [f64; 2],
}
impl GridTransform {
    pub fn point(self, x: f64, y: f64) -> [f64; 2] {
        [
            self.origin[0] + x * self.x_axis[0] + y * self.y_axis[0],
            self.origin[1] + x * self.x_axis[1] + y * self.y_axis[1],
        ]
    }
    pub(crate) fn sample(self, image: BitmapView<'_>, x: f64, y: f64) -> Result<[f64; 3]> {
        let p = self.point(x, y);
        image.sample(p[0], p[1])
    }
}

fn ratio(r: [usize; 5]) -> Option<f64> {
    let unit = r.iter().sum::<usize>() as f64 / 7.0;
    if !(1.0..=64.0).contains(&unit) {
        return None;
    }
    for (i, n) in r.into_iter().enumerate() {
        let expected = if i == 2 { 3.0 * unit } else { unit };
        if (n as f64 - expected).abs() > unit * 0.6 + 0.5 {
            return None;
        }
    }
    Some(unit)
}

fn vertical(image: BitmapView<'_>, x: usize, y: usize) -> Option<(f64, f64)> {
    if !image.dark(x, y) {
        return None;
    }
    let mut top = y;
    while top > 0 && image.dark(x, top - 1) {
        top -= 1;
    }
    let mut bottom = y + 1;
    while bottom < image.height && image.dark(x, bottom) {
        bottom += 1;
    }
    let mut a = top;
    while a > 0 && !image.dark(x, a - 1) {
        a -= 1;
    }
    let mut b = a;
    while b > 0 && image.dark(x, b - 1) {
        b -= 1;
    }
    let mut c = bottom;
    while c < image.height && !image.dark(x, c) {
        c += 1;
    }
    let mut d = c;
    while d < image.height && image.dark(x, d) {
        d += 1;
    }
    let unit = ratio([a - b, top - a, bottom - top, c - bottom, d - c])?;
    Some(((top + bottom) as f64 / 2.0, unit))
}

fn check_finder(image: BitmapView<'_>, f: Finder) -> bool {
    for y in 0..7 {
        for x in 0..7 {
            let Ok(rgb) =
                image.sample(f.x + (x as f64 - 3.0) * f.sx, f.y + (y as f64 - 3.0) * f.sy)
            else {
                return false;
            };
            let black = x == 0
                || y == 0
                || x == 6
                || y == 6
                || ((2..=4).contains(&x) && (2..=4).contains(&y));
            if black {
                if !rgb.iter().all(|&v| v < 128.0) {
                    return false;
                }
            } else if !rgb.iter().all(|&v| v > 128.0) {
                return false;
            }
        }
    }
    true
}

/// Return bounded candidate grids. Color calibration and CRC resolve any remaining
/// ambiguity; no frame is accepted merely because three squares looked plausible.
pub fn locate(image: BitmapView<'_>) -> Result<Vec<GridTransform>> {
    let mut finders: Vec<Finder> = Vec::new();
    let mut cross_checks = 0;
    for y in 0..image.height {
        let mut runs: Vec<(bool, usize, usize)> = Vec::with_capacity(image.width.min(1024));
        let mut start = 0;
        let mut dark = image.dark(0, y);
        for x in 1..=image.width {
            if x == image.width || image.dark(x, y) != dark {
                runs.push((dark, start, x - start));
                start = x;
                dark = !dark;
            }
        }
        for window in runs.windows(5) {
            if !window[0].0 {
                continue;
            }
            let Some(sx) = ratio([
                window[0].2,
                window[1].2,
                window[2].2,
                window[3].2,
                window[4].2,
            ]) else {
                continue;
            };
            let cx = window[2].1 as f64 + window[2].2 as f64 / 2.0;
            if finders
                .iter()
                .any(|f| (f.x - cx).abs() < sx && (f.y - y as f64).abs() < f.sy * 1.6)
            {
                continue;
            }
            // Bound pathological false-pattern workloads in unrelated desktop content.
            cross_checks += 1;
            if cross_checks > 16384 {
                return Err(Error::Locate);
            }
            let Some((cy, sy)) = vertical(image, cx.floor() as usize, y) else {
                continue;
            };
            let f = Finder {
                x: cx,
                y: cy,
                sx,
                sy,
            };
            if check_finder(image, f)
                && !finders
                    .iter()
                    .any(|p| (p.x - cx).abs() < sx && (p.y - cy).abs() < sy)
            {
                if finders.len() == 64 {
                    return Err(Error::Locate);
                }
                finders.push(f);
            }
        }
    }
    let mut grids = Vec::new();
    for (i, a) in finders.iter().enumerate() {
        for (j, b) in finders.iter().enumerate() {
            if i == j {
                continue;
            }
            let bx = b.x - a.x;
            let by = b.y - a.y;
            for (k, c) in finders.iter().enumerate() {
                if k == i || k == j {
                    continue;
                }
                let cx = c.x - a.x;
                let cy = c.y - a.y;
                // VNC images are axis aligned (possibly quarter-turned), not perspective photographs.
                if (bx.abs() > 2.0 && by.abs() > 2.0) || (cx.abs() > 2.0 && cy.abs() > 2.0) {
                    continue;
                }
                let bl = bx.hypot(by);
                let cl = cx.hypot(cy);
                if bl < 145.0
                    || cl < 145.0
                    || bx * cy - by * cx <= 0.0
                    || (bx * cx + by * cy).abs() > bl * cl * 0.015
                {
                    continue;
                }
                for profile in PROFILES {
                    let distance = (profile.matrix_size + 17) as f64;
                    // Finder module width is independent of the matrix dimensions.
                    let sx = if bx.abs() > by.abs() { a.sx } else { a.sy };
                    let sy = if cx.abs() > cy.abs() { a.sx } else { a.sy };
                    if (bl / distance - sx).abs() > sx * 0.2 + 0.1
                        || (cl / distance - sy).abs() > sy * 0.2 + 0.1
                    {
                        continue;
                    }
                    let grid = GridTransform {
                        profile,
                        origin: [
                            a.x - (bx + cx) * 7.5 / distance,
                            a.y - (by + cy) * 7.5 / distance,
                        ],
                        x_axis: [bx / distance, by / distance],
                        y_axis: [cx / distance, cy / distance],
                    };
                    if !check_timing(image, grid) {
                        continue;
                    }
                    grids.push(grid);
                    if grids.len() == 16 {
                        return Ok(grids);
                    }
                }
            }
        }
    }
    if grids.is_empty() {
        Err(Error::Locate)
    } else {
        Ok(grids)
    }
}

fn check_timing(image: BitmapView<'_>, grid: GridTransform) -> bool {
    for i in (16..grid.profile.matrix_size + 16).step_by(8) {
        for d in 0..2 {
            for (x, y) in [
                (i as f64 + d as f64 + 0.5, 12.5),
                (12.5, i as f64 + d as f64 + 0.5),
            ] {
                let Ok(rgb) = grid.sample(image, x, y) else {
                    return false;
                };
                if d == 0 {
                    if !rgb.iter().all(|&v| v < 128.0) {
                        return false;
                    }
                } else if !rgb.iter().all(|&v| v > 128.0) {
                    return false;
                }
            }
        }
    }
    true
}
