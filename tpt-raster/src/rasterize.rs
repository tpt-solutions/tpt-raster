//! Module B: path-to-coverage rasterization with analytic anti-aliasing.
//!
//! Coverage comes from signed-area accumulation. Each edge adds its winding-weighted area to
//! the cells it crosses, and a per-row prefix sum gives each pixel's coverage. The result is
//! exact when edges do not overlap inside a single pixel. Where they do, it is an approximation
//! (the same approach as font-rs).
//!
//! Robustness rules:
//! - A segment with a non-finite endpoint after transform is dropped.
//! - Finite coordinates are clamped to ±[`COORD_LIMIT`]. This keeps all arithmetic finite. It
//!   changes geometry only for points further than 1e15 from the origin.
//! - Edges are split where they cross `x = 0` and `x = width`, and edges outside the vertical
//!   range are clipped, so off-screen geometry never writes out of bounds.
//!
//! All accumulation runs in f32 in a fixed order, so results are deterministic.

use alloc::{vec, vec::Vec};

use tpt_canvas::{Affine2, FillRule, FlattenedPath, Paint};
use tpt_color::{premultiply, unpremultiply};

use crate::composite::{BlendMode, SourceOver};
use crate::pixel::PixelFormat;
use crate::surface::{Surface, SurfaceError};

/// Post-transform coordinates are clamped to `±COORD_LIMIT`.
pub const COORD_LIMIT: f32 = 1.0e15;

/// A coverage value per pixel, `0.0..=1.0`, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    width: u32,
    height: u32,
    data: Vec<f32>,
}

impl Mask {
    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Coverage at `(x, y)`. Returns `0.0` outside the mask.
    pub fn get(&self, x: u32, y: u32) -> f32 {
        if x < self.width && y < self.height {
            self.data[y as usize * self.width as usize + x as usize]
        } else {
            0.0
        }
    }

    /// All coverage values, row-major.
    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }
}

/// Rasterizes `path` into a coverage mask of `width` × `height` pixels.
pub fn coverage_mask(
    width: u32,
    height: u32,
    path: &FlattenedPath,
    transform: &Affine2,
    fill_rule: FillRule,
) -> Result<Mask, SurfaceError> {
    let (w, h) = (width as usize, height as usize);
    // Two extra cells per row absorb the writes at `x1i + 1` for edges touching the right side.
    let stride = w.checked_add(2).ok_or(SurfaceError::SizeOverflow)?;
    let acc_len = stride.checked_mul(h).ok_or(SurfaceError::SizeOverflow)?;
    let data_len = w.checked_mul(h).ok_or(SurfaceError::SizeOverflow)?;
    let mut acc = vec![0.0f32; acc_len];
    let fw = width as f32;

    for contour in &path.contours {
        let n = contour.len();
        if n < 2 {
            continue;
        }
        // Contours are implicitly closed: the last point connects back to the first.
        for i in 0..n {
            let a = transform_point(transform, contour[i]);
            let b = transform_point(transform, contour[(i + 1) % n]);
            if let (Some(a), Some(b)) = (a, b) {
                for_each_x_piece(a, b, fw, |p0, p1| {
                    add_line(&mut acc, stride, w, h, p0, p1);
                });
            }
        }
    }

    let mut data = vec![0.0f32; data_len];
    for y in 0..h {
        let mut sum = 0.0f32;
        for x in 0..w {
            sum += acc[y * stride + x];
            data[y * w + x] = apply_rule(sum, fill_rule);
        }
    }
    Ok(Mask {
        width,
        height,
        data,
    })
}

/// Composites `paint`, weighted by `mask`, onto `surface` with `SourceOver`.
///
/// Paint colors are taken as already being in the surface's color space.
pub fn fill_mask<P: PixelFormat>(
    surface: &mut Surface<'_, P>,
    mask: &Mask,
    paint: &Paint,
) -> Result<(), SurfaceError> {
    if surface.width() != mask.width() || surface.height() != mask.height() {
        return Err(SurfaceError::DimensionMismatch);
    }
    for y in 0..surface.height() {
        for x in 0..surface.width() {
            let cov = mask.get(x, y);
            if cov <= 0.0 {
                continue;
            }
            let color = paint.color_at(x as f32 + 0.5, y as f32 + 0.5);
            let src = premultiply(color).map(|c| c * cov);
            let dst = premultiply(surface.get_pixel(x, y).to_f32());
            let out = unpremultiply(SourceOver::blend(src, dst));
            surface.set_pixel(x, y, P::from_f32(out));
        }
    }
    Ok(())
}

/// Rasterizes `path` and composites it onto `surface`.
pub fn rasterize_path<P: PixelFormat>(
    surface: &mut Surface<'_, P>,
    path: &FlattenedPath,
    transform: &Affine2,
    fill_rule: FillRule,
    paint: &Paint,
) -> Result<(), SurfaceError> {
    let mask = coverage_mask(
        surface.width(),
        surface.height(),
        path,
        transform,
        fill_rule,
    )?;
    fill_mask(surface, &mask, paint)
}

/// Applies `transform`, returning `None` for non-finite results and clamping the rest.
fn transform_point(t: &Affine2, p: [f32; 2]) -> Option<[f32; 2]> {
    let (x, y) = t.apply(p[0], p[1]);
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    Some([
        x.clamp(-COORD_LIMIT, COORD_LIMIT),
        y.clamp(-COORD_LIMIT, COORD_LIMIT),
    ])
}

/// Coverage for a winding sum under the given fill rule.
fn apply_rule(sum: f32, rule: FillRule) -> f32 {
    let a = sum.abs();
    match rule {
        FillRule::NonZero => a.min(1.0),
        FillRule::EvenOdd => {
            // Triangle wave with period 2: 0→0, 1→1, 2→0.
            let m = libm::fmodf(a, 2.0);
            if m > 1.0 { 2.0 - m } else { m }
        }
    }
}

/// Splits a segment where it crosses `x = 0` or `x = w`, and clamps x of each piece into
/// `0..=w`. Pieces outside the surface become vertical edges on the boundary, which gives
/// the same winding contribution to every visible pixel.
fn for_each_x_piece(p0: [f32; 2], p1: [f32; 2], w: f32, mut f: impl FnMut([f32; 2], [f32; 2])) {
    let clamp_x = |p: [f32; 2]| [p[0].clamp(0.0, w), p[1]];

    // (t, boundary x) for each crossing, in no particular order yet.
    let mut cuts = [(0.0f32, 0.0f32); 2];
    let mut n = 0;
    for xb in [0.0f32, w] {
        let (a, b) = (p0[0] - xb, p1[0] - xb);
        if a * b < 0.0 {
            cuts[n] = (a / (a - b), xb);
            n += 1;
        }
    }
    if n == 2 && cuts[0].0 > cuts[1].0 {
        cuts.swap(0, 1);
    }

    let mut prev = p0;
    for &(t, xb) in &cuts[..n] {
        let cut = [xb, p0[1] + t * (p1[1] - p0[1])];
        f(clamp_x(prev), clamp_x(cut));
        prev = cut;
    }
    f(clamp_x(prev), clamp_x(p1));
}

/// Adds one segment (already inside `0..=w` horizontally) to the accumulation buffer.
fn add_line(acc: &mut [f32], stride: usize, w: usize, h: usize, p0: [f32; 2], p1: [f32; 2]) {
    if p0[1] == p1[1] {
        return;
    }
    // Orient the segment top to bottom and remember its winding direction.
    let (dir, p0, p1) = if p0[1] < p1[1] {
        (1.0f32, p0, p1)
    } else {
        (-1.0f32, p1, p0)
    };
    // Position along the segment is a fraction in 0..=1, so it stays finite even when the
    // slope itself would overflow (a nearly horizontal segment with a huge x span).
    let dy_total = p1[1] - p0[1];
    let dx_total = p1[0] - p0[0];

    let ystart = p0[1].max(0.0);
    let yend = p1[1].min(h as f32);
    if ystart >= yend {
        return;
    }
    let y_first = libm::floorf(ystart) as usize;
    let y_last = (libm::ceilf(yend) as usize).min(h);

    for y in y_first..y_last {
        let yf = y as f32;
        let ya = yf.max(p0[1]);
        let yb = (yf + 1.0).min(p1[1]);
        if ya >= yb {
            continue;
        }
        let xa = p0[0] + (ya - p0[1]) / dy_total * dx_total;
        let xb = p0[0] + (yb - p0[1]) / dy_total * dx_total;
        let d = (yb - ya) * dir;
        let row = &mut acc[y * stride..(y + 1) * stride];
        accumulate_row(row, w as f32, xa, xb, d);
    }
}

/// Distributes `d` (signed height) of a row segment from `xa` to `xb` into the cells it covers.
/// Standard exact-area split, matching font-rs.
fn accumulate_row(row: &mut [f32], w: f32, xa: f32, xb: f32, d: f32) {
    let xa = xa.clamp(0.0, w);
    let xb = xb.clamp(0.0, w);
    let (x0, x1) = if xa < xb { (xa, xb) } else { (xb, xa) };
    let x0floor = libm::floorf(x0);
    let x0i = x0floor as usize;
    let x1ceil = libm::ceilf(x1);
    let x1i = x1ceil as usize;

    if x1i <= x0i + 1 {
        // Entirely within one or two cells.
        let xmf = 0.5 * (xa + xb) - x0floor;
        row[x0i] += d - d * xmf;
        row[x0i + 1] += d * xmf;
    } else {
        let s = 1.0 / (x1 - x0);
        let x0f = x0 - x0floor;
        let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
        let x1f = x1 - x1ceil + 1.0;
        let am = 0.5 * s * x1f * x1f;
        row[x0i] += d * a0;
        if x1i == x0i + 2 {
            row[x0i + 1] += d * (1.0 - a0 - am);
        } else {
            let a1 = s * (1.5 - x0f);
            row[x0i + 1] += d * (a1 - a0);
            for cell in &mut row[x0i + 2..x1i - 1] {
                *cell += d * s;
            }
            let a2 = a1 + (x1i - x0i - 3) as f32 * s;
            row[x1i - 1] += d * (1.0 - a2 - am);
        }
        row[x1i] += d * am;
    }
}
