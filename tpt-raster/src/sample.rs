//! Module D: image sampling and transformed image draws.
//!
//! Sampling works on premultiplied values, so filtering next to transparent pixels does not
//! darken them. Results are unpremultiplied on write.
//!
//! Coordinates are continuous source pixel coordinates. Pixel `i` covers `[i, i+1)`, and its
//! centre is at `i + 0.5`. Edge modes decide what happens outside `0..width` and `0..height`.
//!
//! The scalar code is the reference. SIMD versions are a Phase 3 item.

use alloc::{vec, vec::Vec};

use tpt_canvas::Affine2;
use tpt_color::{convert_premultiplied, premultiply, unpremultiply};

use crate::composite::{BlendMode, SourceOver};
use crate::pixel::PixelFormat;
use crate::surface::{Surface, SurfaceError};

/// Interpolation between source pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Interp {
    /// The nearest pixel. No blending.
    #[default]
    Nearest,
    /// 2×2 linear.
    Bilinear,
    /// 4×4 Mitchell-Netravali (B = C = 1/3).
    Bicubic,
    /// 6×6 Lanczos, a = 3.
    Lanczos3,
}

/// What a source lookup does outside the image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeMode {
    /// Repeat the nearest edge pixel.
    #[default]
    Clamp,
    /// Tile the image.
    Repeat,
    /// Tile the image, mirrored at each seam.
    Mirror,
    /// Outside pixels are fully transparent.
    Transparent,
}

/// Maps `i` into `0..n`. `None` means the lookup is transparent. Requires `n >= 1`.
fn remap(i: i64, n: i64, edge: EdgeMode) -> Option<i64> {
    match edge {
        EdgeMode::Clamp => Some(i.clamp(0, n - 1)),
        EdgeMode::Repeat => Some(i.rem_euclid(n)),
        EdgeMode::Mirror => {
            let m = i.rem_euclid(2 * n);
            Some(if m >= n { 2 * n - 1 - m } else { m })
        }
        EdgeMode::Transparent => (0..n).contains(&i).then_some(i),
    }
}

/// Mitchell-Netravali cubic with B = C = 1/3.
fn mitchell(t: f32) -> f32 {
    const B: f32 = 1.0 / 3.0;
    const C: f32 = 1.0 / 3.0;
    let t = t.abs();
    if t < 1.0 {
        ((12.0 - 9.0 * B - 6.0 * C) * t * t * t
            + (-18.0 + 12.0 * B + 6.0 * C) * t * t
            + (6.0 - 2.0 * B))
            / 6.0
    } else if t < 2.0 {
        ((-B - 6.0 * C) * t * t * t
            + (6.0 * B + 30.0 * C) * t * t
            + (-12.0 * B - 48.0 * C) * t
            + (8.0 * B + 24.0 * C))
            / 6.0
    } else {
        0.0
    }
}

fn sinc(x: f32) -> f32 {
    if x == 0.0 {
        1.0
    } else {
        let px = core::f32::consts::PI * x;
        libm::sinf(px) / px
    }
}

/// Lanczos kernel with a = 3.
fn lanczos3(t: f32) -> f32 {
    if t.abs() < 3.0 {
        sinc(t) * sinc(t / 3.0)
    } else {
        0.0
    }
}

/// Tap weights for one axis. `start` is the first integer index, and `w` holds `n` weights
/// that sum to 1 (normalized, so flat input stays flat).
struct Taps {
    start: i64,
    n: usize,
    w: [f32; 6],
}

fn taps(interp: Interp, p: f32) -> Taps {
    if interp == Interp::Nearest {
        let mut w = [0.0; 6];
        w[0] = 1.0;
        return Taps {
            start: libm::floorf(p) as i64,
            n: 1,
            w,
        };
    }
    // Pixel centres sit at i + 0.5, so the tap grid is offset by half a pixel.
    let f = p - 0.5;
    let base = libm::floorf(f);
    let t = f - base;
    let x0 = base as i64;
    let (start, n, kernel): (i64, usize, fn(f32) -> f32) = match interp {
        Interp::Bilinear => (x0, 2, |d| (1.0 - d.abs()).max(0.0)),
        Interp::Bicubic => (x0 - 1, 4, mitchell),
        _ => (x0 - 2, 6, lanczos3),
    };
    let mut w = [0.0f32; 6];
    let mut sum = 0.0f32;
    for (k, slot) in w.iter_mut().enumerate().take(n) {
        // Tap k sits at integer index start + k, which is `start + k - x0 - t` from the sample.
        let dist = (start + k as i64 - x0) as f32 - t;
        *slot = kernel(dist);
        sum += *slot;
    }
    if sum != 0.0 {
        for v in w.iter_mut().take(n) {
            *v /= sum;
        }
    }
    Taps { start, n, w }
}

/// Premultiplied source pixel at integer coordinates, with the edge mode applied.
fn fetch<P: PixelFormat>(
    src: &Surface<'_, P>,
    ix: i64,
    iy: i64,
    edge: EdgeMode,
) -> Option<[f32; 4]> {
    let (w, h) = (src.width() as i64, src.height() as i64);
    let x = remap(ix, w, edge)?;
    let y = remap(iy, h, edge)?;
    Some(premultiply(src.get_pixel(x as u32, y as u32).to_f32()))
}

/// Samples `src` at continuous coordinates `(x, y)`. Returns premultiplied RGBA.
///
/// The source must be non-empty. Output is clamped so colour ≤ alpha, which keeps negative
/// lobes from bicubic and Lanczos from producing invalid colours.
pub fn sample_premultiplied<P: PixelFormat>(
    src: &Surface<'_, P>,
    x: f32,
    y: f32,
    interp: Interp,
    edge: EdgeMode,
) -> [f32; 4] {
    let tx = taps(interp, x);
    let ty = taps(interp, y);
    let mut acc = [0.0f32; 4];
    for j in 0..ty.n {
        for i in 0..tx.n {
            let weight = tx.w[i] * ty.w[j];
            if let Some(p) = fetch(src, tx.start + i as i64, ty.start + j as i64, edge) {
                for (a, &v) in acc.iter_mut().zip(&p) {
                    *a += weight * v;
                }
            }
        }
    }
    let a = acc[3].clamp(0.0, 1.0);
    [
        acc[0].clamp(0.0, a),
        acc[1].clamp(0.0, a),
        acc[2].clamp(0.0, a),
        a,
    ]
}

/// Samples `src` at `(x, y)` and returns straight RGBA.
pub fn sample<P: PixelFormat>(
    src: &Surface<'_, P>,
    x: f32,
    y: f32,
    interp: Interp,
    edge: EdgeMode,
) -> [f32; 4] {
    unpremultiply(sample_premultiplied(src, x, y, interp, edge))
}

/// Draws `src` into `dst` through `transform` (source → destination).
///
/// Each destination pixel centre is mapped back into the source and sampled. The result is
/// composited with `SourceOver`, scaled by `opacity`. Colour spaces are converted when they
/// differ. Returns [`SurfaceError::SingularTransform`] if the transform has no inverse.
pub fn draw_image<D: PixelFormat, S: PixelFormat>(
    dst: &mut Surface<'_, D>,
    src: &Surface<'_, S>,
    transform: &Affine2,
    interp: Interp,
    edge: EdgeMode,
    opacity: f32,
) -> Result<(), SurfaceError> {
    let inv = transform.invert().ok_or(SurfaceError::SingularTransform)?;
    if src.width() == 0 || src.height() == 0 {
        return Ok(());
    }
    let opacity = if opacity.is_nan() {
        0.0
    } else {
        opacity.clamp(0.0, 1.0)
    };
    let (src_cs, dst_cs) = (src.color_space(), dst.color_space());
    for y in 0..dst.height() {
        for x in 0..dst.width() {
            let (sx, sy) = inv.apply(x as f32 + 0.5, y as f32 + 0.5);
            let mut s = sample_premultiplied(src, sx, sy, interp, edge);
            if src_cs != dst_cs {
                s = convert_premultiplied(s, src_cs, dst_cs);
            }
            if s[3] <= 0.0 {
                continue;
            }
            let s = s.map(|c| c * opacity);
            let d = premultiply(dst.get_pixel(x, y).to_f32());
            let out = unpremultiply(SourceOver::blend(s, d));
            dst.set_pixel(x, y, D::from_f32(out));
        }
    }
    Ok(())
}

/// Builds a mip chain from `src`. Level 0 is a copy, and each level halves both dimensions
/// (rounded up, minimum 1) with a 2×2 box filter in premultiplied space.
///
/// Level 0 is always present, even for an empty source.
pub fn mipmap_chain<P: PixelFormat>(
    src: &Surface<'_, P>,
) -> Result<Vec<Surface<'static, P>>, SurfaceError> {
    let mut level0: Surface<'static, P> =
        Surface::new(src.width(), src.height(), src.color_space())?;
    for y in 0..src.height() {
        for x in 0..src.width() {
            level0.set_pixel(x, y, src.get_pixel(x, y));
        }
    }
    let mut chain = vec![level0];
    loop {
        let prev = chain.last().expect("chain is never empty");
        let (w, h) = (prev.width(), prev.height());
        if w <= 1 && h <= 1 {
            break;
        }
        let (nw, nh) = (w.div_ceil(2).max(1), h.div_ceil(2).max(1));
        let mut next: Surface<'static, P> = Surface::new(nw, nh, prev.color_space())?;
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0.0f32; 4];
                let mut count = 0.0f32;
                for dy in 0..2 {
                    for dx in 0..2 {
                        let (sx, sy) = (2 * x + dx, 2 * y + dy);
                        if sx < w && sy < h {
                            let p = premultiply(prev.get_pixel(sx, sy).to_f32());
                            for (a, &v) in acc.iter_mut().zip(&p) {
                                *a += v;
                            }
                            count += 1.0;
                        }
                    }
                }
                if count == 0.0 {
                    // Only possible when the previous level has zero width or height.
                    continue;
                }
                let avg = acc.map(|v| v / count);
                next.set_pixel(x, y, P::from_f32(unpremultiply(avg)));
            }
        }
        chain.push(next);
    }
    Ok(chain)
}

/// Draws with mip level selection. The level is `floor(log2(downscale))`, where `downscale` is
/// the number of source pixels per destination pixel, clamped to the chain.
///
/// This is `draw_image` on the chosen level. The transform is scaled to match, so callers
/// pass the same transform they would for full resolution.
pub fn draw_image_mipmapped<D: PixelFormat, S: PixelFormat>(
    dst: &mut Surface<'_, D>,
    chain: &[Surface<'_, S>],
    transform: &Affine2,
    interp: Interp,
    edge: EdgeMode,
    opacity: f32,
) -> Result<(), SurfaceError> {
    if chain.is_empty() {
        return Err(SurfaceError::SizeOverflow);
    }
    let m = transform.m;
    // Linear part determinant: area scale from source to destination.
    let det = (m[0] * m[3] - m[1] * m[2]).abs();
    let per_dst_px = if det > 0.0 && det.is_finite() {
        libm::sqrtf(1.0 / det)
    } else {
        1.0
    };
    let level = if per_dst_px > 1.0 {
        libm::floorf(libm::log2f(per_dst_px)) as usize
    } else {
        0
    };
    let level = level.min(chain.len() - 1);
    // Level L has pixels 2^L times larger, so source coordinates scale by 2^L.
    let k = (1u32 << level.min(31)) as f32;
    let scaled = Affine2 {
        m: [m[0] * k, m[1] * k, m[2] * k, m[3] * k, m[4], m[5]],
    };
    draw_image(dst, &chain[level], &scaled, interp, edge, opacity)
}
