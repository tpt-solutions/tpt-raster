//! Phase 3: SIMD kernels on tpt-simd, enabled by the `simd` feature.
//!
//! Each kernel does the same f32 operations, in the same order, as its scalar counterpart. Results
//! must match byte for byte. `tests/simd.rs` checks that, and the whole suite runs with the feature
//! on. Setting `scalar-fallback` forces the scalar paths.
//!
//! Lanes are filled with scalar loads (`get_pixel`), so the speed-up is only in the arithmetic
//! and rounding. Packed loads are a later optimisation.

use tpt_simd_core::Simd;

use tpt_color::{premultiply, unpremultiply};

use crate::pixel::PixelFormat;
use crate::round::round_to_u8;
use crate::surface::Surface;

/// Eight `f32` lanes.
pub type F32x8 = Simd<f32, 8>;

/// `round_to_u8` on eight lanes. Lane for lane identical to the scalar function.
///
/// The clamp runs first. NaN lanes become 0 and out-of-range lanes saturate. The rounding is
/// round-half-to-even, the same as the scalar `rintf`.
#[inline]
pub fn round_to_u8_x8(v: [f32; 8]) -> [u8; 8] {
    let clamped = F32x8::from_array(v)
        .max(F32x8::splat(0.0))
        .min(F32x8::splat(255.0));
    // NaN was already removed by the clamp, so the result is a finite value in 0..=255.
    let rounded = tpt_simd_rounding::round_to_nearest_even_i32(clamped).to_array();
    core::array::from_fn(|i| rounded[i] as u8)
}

/// `unit_to_u8` on eight lanes: multiply by 255, then `round_to_u8_x8`.
#[inline]
pub fn unit_to_u8_x8(v: [f32; 8]) -> [u8; 8] {
    round_to_u8_x8(core::array::from_fn(|i| v[i] * 255.0))
}

/// Premultiplied SourceOver on eight lanes. Channels are `[r, g, b, a]`, each an `F32x8`.
///
/// Per lane this is `src + dst * (1 - src.a)`, the same as `composite::SourceOver`.
#[inline]
pub fn source_over_x8(src: [[f32; 8]; 4], dst: [[f32; 8]; 4]) -> [[f32; 8]; 4] {
    let k: [f32; 8] = core::array::from_fn(|i| 1.0 - src[3][i]);
    core::array::from_fn(|c| core::array::from_fn(|i| src[c][i] + dst[c][i] * k[i]))
}

/// Unpremultiplies eight lanes. A zero alpha gives zero, as in the scalar `unpremultiply`.
#[inline]
pub fn unpremultiply_x8(c: [[f32; 8]; 4]) -> [[f32; 8]; 4] {
    let a = c[3];
    // Divide by 1 where alpha is zero, so the division never produces NaN, then select 0 there.
    // Branch-free per lane, so the loop vectorises. Results match the scalar branch exactly.
    let safe: [f32; 8] = core::array::from_fn(|i| if a[i] == 0.0 { 1.0 } else { a[i] });
    let zero: [bool; 8] = core::array::from_fn(|i| a[i] == 0.0);
    core::array::from_fn(|ch| {
        core::array::from_fn(|i| {
            if ch == 3 {
                // Alpha is kept as-is, matching the scalar `unpremultiply`.
                a[i]
            } else {
                let q = c[ch][i] / safe[i];
                if zero[i] { 0.0 } else { q }
            }
        })
    })
}

/// Scalar reference used by the tests. It must agree with [`round_to_u8_x8`].
#[doc(hidden)]
pub fn round_to_u8_scalar(v: [f32; 8]) -> [u8; 8] {
    core::array::from_fn(|i| round_to_u8(v[i]))
}

/// SIMD `composite_surface` for premultiplied SourceOver with matching colour spaces.
///
/// Works row by row. Eight consecutive pixels per row go through the lanes. Each format loads
/// and stores its lanes through `PixelFormat::load_lanes` and `store_lanes`. The tail runs the
/// same arithmetic one pixel at a time. `opacity` must already be clamped. Dimensions must match.
pub fn composite_source_over<D: PixelFormat, S: PixelFormat>(
    dst: &mut Surface<'_, D>,
    src: &Surface<'_, S>,
    opacity: f32,
) {
    use crate::composite::{BlendMode, SourceOver};

    let w = dst.width() as usize;
    for y in 0..dst.height() {
        let srow = src.row(y);
        let drow = dst.row_mut(y);
        let mut x = 0;
        while x + 8 <= w {
            let mut s = [[0.0f32; 8]; 4];
            let mut d = [[0.0f32; 8]; 4];
            S::load_lanes(srow, x, &mut s);
            D::load_lanes(drow, x, &mut d);
            let s = scale_x8(premultiply_x8(s), opacity);
            let d = premultiply_x8(d);
            let out = unpremultiply_x8(source_over_x8(s, d));
            D::store_lanes(&out, drow, x);
            x += 8;
        }
        while x < w {
            let s = premultiply(S::from_storage(srow[x]).to_f32()).map(|c| c * opacity);
            let d = premultiply(D::from_storage(drow[x]).to_f32());
            let out = unpremultiply(SourceOver::blend(s, d));
            drow[x] = D::from_f32(out).to_storage();
            x += 1;
        }
    }
}

/// Premultiplies eight lanes. Per lane this is `[c0*a, c1*a, c2*a, a]`, as in the scalar code.
#[inline]
pub fn premultiply_x8(c: [[f32; 8]; 4]) -> [[f32; 8]; 4] {
    let a = c[3];
    core::array::from_fn(|ch| {
        core::array::from_fn(|i| if ch == 3 { a[i] } else { c[ch][i] * a[i] })
    })
}

/// Multiplies all four channels of eight lanes by `k`.
#[inline]
pub fn scale_x8(c: [[f32; 8]; 4], k: f32) -> [[f32; 8]; 4] {
    c.map(|ch| ch.map(|v| v * k))
}
