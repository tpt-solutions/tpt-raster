//! Local stub of `tpt-canvas`: flattened paths, transforms, fill rules and paints.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;

/// A 2D affine transform `[a, b, c, d, e, f]`, mapping `(x, y)` to
/// `(a*x + c*y + e, b*x + d*y + f)` (the SVG/Canvas convention).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine2 {
    pub m: [f32; 6],
}

impl Affine2 {
    pub const IDENTITY: Self = Self {
        m: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    };

    /// Applies the transform to a point.
    #[inline]
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let m = &self.m;
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    }
}

impl Affine2 {
    /// The inverse transform, or `None` if the matrix is singular or non-finite.
    pub fn invert(&self) -> Option<Self> {
        let [a, b, c, d, e, f] = self.m;
        let det = a * d - b * c;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let inv = [
            d / det,
            -b / det,
            -c / det,
            a / det,
            (c * f - d * e) / det,
            (b * e - a * f) / det,
        ];
        if inv.iter().all(|v| v.is_finite()) {
            Some(Self { m: inv })
        } else {
            None
        }
    }
}

impl Default for Affine2 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Winding rule used to decide which areas of a path are inside.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillRule {
    #[default]
    NonZero,
    EvenOdd,
}

/// A path reduced to closed polylines. Curves are assumed to be flattened already.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlattenedPath {
    /// Each inner vector is one closed contour.
    pub contours: Vec<Vec<[f32; 2]>>,
}

impl FlattenedPath {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one closed contour. Contours with fewer than 2 points are kept
    /// as-is; the rasterizer is responsible for ignoring degenerate ones.
    pub fn push_contour(&mut self, points: Vec<[f32; 2]>) {
        self.contours.push(points);
    }
}

/// Paint used to fill a path. Colors are straight `[r, g, b, a]` in `0.0..=1.0`.
/// Patterns (surface-backed) arrive with the real `tpt-canvas`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    /// One color everywhere.
    Solid([f32; 4]),
    /// Blends `from` at `start` to `to` at `end`. Positions outside the segment clamp to the ends.
    Linear {
        start: [f32; 2],
        end: [f32; 2],
        from: [f32; 4],
        to: [f32; 4],
    },
    /// Blends `from` at `center` to `to` at distance `radius`. Beyond the radius clamps to `to`.
    Radial {
        center: [f32; 2],
        radius: f32,
        from: [f32; 4],
        to: [f32; 4],
    },
}

impl Paint {
    /// Straight color at a point. Gradient evaluation is plain f32 with a fixed operation order.
    pub fn color_at(&self, x: f32, y: f32) -> [f32; 4] {
        match *self {
            Paint::Solid(c) => c,
            Paint::Linear {
                start,
                end,
                from,
                to,
            } => {
                let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
                let len2 = dx * dx + dy * dy;
                let t = if len2 == 0.0 {
                    0.0
                } else {
                    ((x - start[0]) * dx + (y - start[1]) * dy) / len2
                };
                lerp(from, to, t)
            }
            Paint::Radial {
                center,
                radius,
                from,
                to,
            } => {
                let (dx, dy) = (x - center[0], y - center[1]);
                let t = if radius <= 0.0 {
                    1.0
                } else {
                    libm::sqrtf(dx * dx + dy * dy) / radius
                };
                lerp(from, to, t)
            }
        }
    }
}

/// Linear interpolation between two colors, with `t` clamped to `0.0..=1.0`.
/// A NaN `t` maps to the `from` color.
fn lerp(from: [f32; 4], to: [f32; 4], t: f32) -> [f32; 4] {
    let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    [0, 1, 2, 3].map(|i| from[i] + (to[i] - from[i]) * t)
}

/// The seam between vector math and pixels.
///
/// `tpt-canvas` builds paths and resolves styles. A backend turns them into pixels. The
/// spec puts this trait in `tpt-canvas` and the pixel work in `tpt-raster`. This stub keeps
/// that shape, so backends can be written and tested before the real crate exists.
pub trait RasterBackend {
    /// Error reported by the backend.
    type Error;

    /// Fills `path` (transformed by `transform`) with `paint` under `rule`.
    fn fill_path(
        &mut self,
        path: &FlattenedPath,
        transform: &Affine2,
        rule: FillRule,
        paint: &Paint,
    ) -> Result<(), Self::Error>;
}
