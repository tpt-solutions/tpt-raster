//! Module E: separable convolution, 3×3 kernels, colour matrices and morphology.
//!
//! Neighbourhood filters read the surface into an f32 buffer, filter it, and write it back.
//! Blur, convolution and morphology work on premultiplied values, so transparent pixels do not
//! darken their neighbours. Results are unpremultiplied and clamped on write.
//!
//! Edge rules: convolution clamps to the nearest edge pixel. Morphology ignores out-of-bounds
//! pixels. Edge detection reads straight-alpha luma and ignores alpha for the edge strength.
//! Colour matrices act on straight colour.
//!
//! All arithmetic is f32 in a fixed order. Each filter allocates one f32 buffer per pass.

use alloc::{vec, vec::Vec};
use core::f32::consts::PI;

use tpt_color::{premultiply, unpremultiply};

use crate::pixel::PixelFormat;
use crate::surface::Surface;

/// Sigma values above this are clamped, which bounds the kernel length.
pub const MAX_SIGMA: f32 = 256.0;

/// Sigma below this is treated as identity. Smaller values make the kernel exponent overflow.
pub const MIN_SIGMA: f32 = 1.0e-3;

/// One pixel in the f32 working buffer: `[r, g, b, a]`.
type Px = [f32; 4];

/// A 5×4 colour matrix. Row `i` is `[m0, m1, m2, m3, offset]` and produces output channel `i`
/// from input `[r, g, b, a]`: `out[i] = m0*r + m1*g + m2*b + m3*a + offset`.
pub type ColorMatrix = [[f32; 5]; 4];

const SOBEL_X: [[f32; 3]; 3] = [[-1.0, 0.0, 1.0], [-2.0, 0.0, 2.0], [-1.0, 0.0, 1.0]];
const SOBEL_Y: [[f32; 3]; 3] = [[-1.0, -2.0, -1.0], [0.0, 0.0, 0.0], [1.0, 2.0, 1.0]];

fn dims<P: PixelFormat>(s: &Surface<'_, P>) -> (usize, usize) {
    (s.width() as usize, s.height() as usize)
}

/// Reads the whole surface into a buffer, optionally premultiplied.
fn load<P: PixelFormat>(s: &Surface<'_, P>, premul: bool) -> Vec<Px> {
    let (w, h) = dims(s);
    let mut buf = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let c = s.get_pixel(x as u32, y as u32).to_f32();
            buf.push(if premul { premultiply(c) } else { c });
        }
    }
    buf
}

/// Writes a buffer back to the surface. Premultiplied input is clamped so that colour ≤ alpha,
/// then unpremultiplied. Straight input is clamped per channel.
fn store<P: PixelFormat>(s: &mut Surface<'_, P>, buf: &[Px], premul: bool) {
    let w = s.width() as usize;
    for (i, c) in buf.iter().enumerate() {
        let c = if premul {
            let a = c[3].clamp(0.0, 1.0);
            let p = [
                c[0].clamp(0.0, a),
                c[1].clamp(0.0, a),
                c[2].clamp(0.0, a),
                a,
            ];
            unpremultiply(p)
        } else {
            c.map(|v| v.clamp(0.0, 1.0))
        };
        s.set_pixel((i % w) as u32, (i / w) as u32, P::from_f32(c));
    }
}

/// Index `i + off` clamped to `0..n`. Requires `n >= 1`.
#[inline]
fn clamp_idx(i: usize, off: isize, n: usize) -> usize {
    (i as isize + off).clamp(0, n as isize - 1) as usize
}

/// Gaussian kernel of radius `ceil(3 * sigma)`, normalized to sum to 1.
///
/// `sigma` at or below [`MIN_SIGMA`], NaN or infinite gives the identity kernel `[1.0]`.
/// Values above [`MAX_SIGMA`] are clamped.
pub fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    if sigma.is_nan() || !sigma.is_finite() || sigma < MIN_SIGMA {
        return vec![1.0];
    }
    let sigma = sigma.min(MAX_SIGMA);
    let r = libm::ceilf(3.0 * sigma) as i32;
    let inv = 1.0 / (2.0 * sigma * sigma);
    let mut k: Vec<f32> = (-r..=r)
        .map(|i| libm::expf(-((i * i) as f32) * inv))
        .collect();
    let sum: f32 = k.iter().sum();
    for v in &mut k {
        *v /= sum;
    }
    k
}

/// One 1D pass with a centred odd-length kernel and clamp-to-edge.
fn convolve_1d(src: &[Px], w: usize, h: usize, k: &[f32], horizontal: bool) -> Vec<Px> {
    let r = (k.len() / 2) as isize;
    let mut out = vec![[0.0f32; 4]; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for (j, &kv) in k.iter().enumerate() {
                let off = j as isize - r;
                let (sx, sy) = if horizontal {
                    (clamp_idx(x, off, w), y)
                } else {
                    (x, clamp_idx(y, off, h))
                };
                for (a, &v) in acc.iter_mut().zip(&src[sy * w + sx]) {
                    *a += kv * v;
                }
            }
            out[y * w + x] = acc;
        }
    }
    out
}

/// 3×3 convolution with clamp-to-edge. `k[dy][dx]` multiplies the pixel at offset `(dx-1, dy-1)`.
fn convolve_3x3(src: &[Px], w: usize, h: usize, k: &[[f32; 3]; 3]) -> Vec<Px> {
    let mut out = vec![[0.0f32; 4]; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for (dy, row) in k.iter().enumerate() {
                for (dx, &kv) in row.iter().enumerate() {
                    let sx = clamp_idx(x, dx as isize - 1, w);
                    let sy = clamp_idx(y, dy as isize - 1, h);
                    for (a, &v) in acc.iter_mut().zip(&src[sy * w + sx]) {
                        *a += kv * v;
                    }
                }
            }
            out[y * w + x] = acc;
        }
    }
    out
}

/// Gaussian blur with standard deviation `sigma`, in premultiplied space.
///
/// Sigma is clamped to [`MAX_SIGMA`]. Sigma below [`MIN_SIGMA`] is a no-op.
pub fn gaussian_blur<P: PixelFormat>(s: &mut Surface<'_, P>, sigma: f32) {
    let k = gaussian_kernel(sigma);
    let (w, h) = dims(s);
    if k.len() == 1 || w * h == 0 {
        return;
    }
    let buf = load(s, true);
    let buf = convolve_1d(&buf, w, h, &k, true);
    let buf = convolve_1d(&buf, w, h, &k, false);
    store(s, &buf, true);
}

/// Separable convolution: `kh` horizontally, then `kv` vertically, in premultiplied space.
///
/// Both kernels must have odd length (panics otherwise). The kernels are used as given, so
/// callers choose whether they sum to 1.
pub fn convolve_separable<P: PixelFormat>(s: &mut Surface<'_, P>, kh: &[f32], kv: &[f32]) {
    assert!(
        kh.len() % 2 == 1 && kv.len() % 2 == 1,
        "kernels must have odd length"
    );
    let (w, h) = dims(s);
    if w * h == 0 {
        return;
    }
    let buf = load(s, true);
    let buf = convolve_1d(&buf, w, h, kh, true);
    let buf = convolve_1d(&buf, w, h, kv, false);
    store(s, &buf, true);
}

/// Arbitrary 3×3 convolution, in premultiplied space.
pub fn convolve_3x3_filter<P: PixelFormat>(s: &mut Surface<'_, P>, k: &[[f32; 3]; 3]) {
    let (w, h) = dims(s);
    if w * h == 0 {
        return;
    }
    let buf = load(s, true);
    let buf = convolve_3x3(&buf, w, h, k);
    store(s, &buf, true);
}

/// Unsharp-style sharpen: the centre gets `1 + 4a` and the four neighbours get `-a`.
///
/// Brightness is preserved (the kernel sums to 1). `amount` is clamped to `0.0..`. Amount 0 is
/// a no-op.
pub fn sharpen<P: PixelFormat>(s: &mut Surface<'_, P>, amount: f32) {
    let a = if amount.is_nan() {
        0.0
    } else {
        amount.max(0.0)
    };
    let k = [[0.0, -a, 0.0], [-a, 1.0 + 4.0 * a, -a], [0.0, -a, 0.0]];
    convolve_3x3_filter(s, &k);
}

/// Sobel edge strength. Writes gray `[m, m, m]` with `m` in `0.0..=1.0`, keeping each pixel's
/// alpha. Works on straight-alpha luma.
pub fn sobel_edges<P: PixelFormat>(s: &mut Surface<'_, P>) {
    let (w, h) = dims(s);
    if w * h == 0 {
        return;
    }
    let straight = load(s, false);
    let luma: Vec<Px> = straight
        .iter()
        .map(|c| {
            let y = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            [y, y, y, c[3]]
        })
        .collect();
    let gx = convolve_3x3(&luma, w, h, &SOBEL_X);
    let gy = convolve_3x3(&luma, w, h, &SOBEL_Y);
    let out: Vec<Px> = gx
        .iter()
        .zip(&gy)
        .zip(&straight)
        .map(|((x, y), c)| {
            let m = libm::sqrtf(x[0] * x[0] + y[0] * y[0]).min(1.0);
            [m, m, m, c[3]]
        })
        .collect();
    store(s, &out, false);
}

/// Applies a 5×4 colour matrix to straight colour, then clamps each channel to `0.0..=1.0`.
pub fn apply_color_matrix<P: PixelFormat>(s: &mut Surface<'_, P>, m: &ColorMatrix) {
    let (w, h) = dims(s);
    for y in 0..h {
        for x in 0..w {
            let c = s.get_pixel(x as u32, y as u32).to_f32();
            let mut out = [0.0f32; 4];
            for (o, row) in out.iter_mut().zip(m) {
                *o = (row[0] * c[0] + row[1] * c[1] + row[2] * c[2] + row[3] * c[3] + row[4])
                    .clamp(0.0, 1.0);
            }
            s.set_pixel(x as u32, y as u32, P::from_f32(out));
        }
    }
}

/// The identity colour matrix.
pub fn identity_matrix() -> ColorMatrix {
    [
        [1.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0, 0.0],
    ]
}

/// Brightness: scales the colour channels by `b`.
pub fn brightness(b: f32) -> ColorMatrix {
    [
        [b, 0.0, 0.0, 0.0, 0.0],
        [0.0, b, 0.0, 0.0, 0.0],
        [0.0, 0.0, b, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0, 0.0],
    ]
}

/// Contrast: scales around mid-grey 0.5 by `c`.
pub fn contrast(c: f32) -> ColorMatrix {
    let o = 0.5 * (1.0 - c);
    [
        [c, 0.0, 0.0, 0.0, o],
        [0.0, c, 0.0, 0.0, o],
        [0.0, 0.0, c, 0.0, o],
        [0.0, 0.0, 0.0, 1.0, 0.0],
    ]
}

/// Saturation: `s = 0` gives grey, `s = 1` is the identity. Uses the W3C luma weights.
pub fn saturate(s: f32) -> ColorMatrix {
    [
        [
            0.213 + 0.787 * s,
            0.715 - 0.715 * s,
            0.072 - 0.072 * s,
            0.0,
            0.0,
        ],
        [
            0.213 - 0.213 * s,
            0.715 + 0.285 * s,
            0.072 - 0.072 * s,
            0.0,
            0.0,
        ],
        [
            0.213 - 0.213 * s,
            0.715 - 0.715 * s,
            0.072 + 0.928 * s,
            0.0,
            0.0,
        ],
        [0.0, 0.0, 0.0, 1.0, 0.0],
    ]
}

/// Hue rotation by `degrees` (W3C `feColorMatrix` hueRotate).
pub fn hue_rotate(degrees: f32) -> ColorMatrix {
    let rad = degrees * (PI / 180.0);
    let (c, s) = (libm::cosf(rad), libm::sinf(rad));
    [
        [
            0.213 + c * 0.787 - s * 0.213,
            0.715 - c * 0.715 - s * 0.715,
            0.072 - c * 0.072 + s * 0.928,
            0.0,
            0.0,
        ],
        [
            0.213 - c * 0.213 + s * 0.143,
            0.715 + c * 0.285 + s * 0.140,
            0.072 - c * 0.072 - s * 0.283,
            0.0,
            0.0,
        ],
        [
            0.213 - c * 0.213 - s * 0.787,
            0.715 - c * 0.715 + s * 0.715,
            0.072 + c * 0.928 + s * 0.072,
            0.0,
            0.0,
        ],
        [0.0, 0.0, 0.0, 1.0, 0.0],
    ]
}

/// Sepia by `amount` (CSS `sepia()`): `0` is the identity, `1` is full sepia.
pub fn sepia(amount: f32) -> ColorMatrix {
    let a = if amount.is_nan() {
        0.0
    } else {
        amount.clamp(0.0, 1.0)
    };
    let full = [
        [0.393, 0.769, 0.189],
        [0.349, 0.686, 0.168],
        [0.272, 0.534, 0.131],
    ];
    let mut m = identity_matrix();
    for (i, row) in full.iter().enumerate() {
        for (j, &t) in row.iter().enumerate() {
            m[i][j] = (1.0 - a) * m[i][j] + a * t;
        }
    }
    m
}

/// One morphology pass over a buffer. `max` selects dilate (max) or erode (min). Out-of-bounds
/// neighbours are skipped.
fn morph_1d(src: &[Px], w: usize, h: usize, r: usize, horizontal: bool, max: bool) -> Vec<Px> {
    let mut out = vec![[0.0f32; 4]; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = src[y * w + x];
            for off in 1..=r {
                for sign in [-1isize, 1] {
                    let d = sign * off as isize;
                    let (nx, ny) = if horizontal {
                        (x as isize + d, y as isize)
                    } else {
                        (x as isize, y as isize + d)
                    };
                    if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                        continue;
                    }
                    let p = src[ny as usize * w + nx as usize];
                    for (a, &v) in acc.iter_mut().zip(&p) {
                        *a = if max { a.max(v) } else { a.min(v) };
                    }
                }
            }
            out[y * w + x] = acc;
        }
    }
    out
}

/// Two separable passes (horizontal, then vertical) over a buffer.
fn morph_buf(buf: &[Px], w: usize, h: usize, r: usize, max: bool) -> Vec<Px> {
    let buf = morph_1d(buf, w, h, r, true, max);
    morph_1d(&buf, w, h, r, false, max)
}

/// Dilate: each pixel takes the maximum over a `(2r+1)²` square (premultiplied space).
pub fn dilate<P: PixelFormat>(s: &mut Surface<'_, P>, r: u32) {
    morph(s, r, true, false);
}

/// Erode: each pixel takes the minimum over a `(2r+1)²` square (premultiplied space).
pub fn erode<P: PixelFormat>(s: &mut Surface<'_, P>, r: u32) {
    morph(s, r, false, false);
}

/// Opening: erode, then dilate. Removes features smaller than the square.
pub fn open<P: PixelFormat>(s: &mut Surface<'_, P>, r: u32) {
    morph(s, r, false, true);
}

/// Closing: dilate, then erode. Fills gaps smaller than the square.
pub fn close<P: PixelFormat>(s: &mut Surface<'_, P>, r: u32) {
    morph(s, r, true, true);
}

/// Shared driver. With `second` set, the pair (erode-then-dilate, or dilate-then-erode) runs
/// in one load/store.
fn morph<P: PixelFormat>(s: &mut Surface<'_, P>, r: u32, first_max: bool, second: bool) {
    let (w, h) = dims(s);
    if w * h == 0 || r == 0 {
        return;
    }
    // A radius beyond the surface size cannot change the result, so cap it to bound the loops.
    let r = (r as usize).min(w.max(h));
    let buf = load(s, true);
    let buf = morph_buf(&buf, w, h, r, first_max);
    let buf = if second {
        morph_buf(&buf, w, h, r, !first_max)
    } else {
        buf
    };
    store(s, &buf, true);
}
