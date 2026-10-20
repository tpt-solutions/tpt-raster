//! Module C (scalar baseline): Porter-Duff compositing between surfaces.
//!
//! The math runs on each surface's values as stored, with no linearization. Surface values are
//! straight alpha. Each pixel is premultiplied on read, blended, unpremultiplied, and rounded
//! once on write. When the two surfaces use different color spaces, the source is converted
//! first (premultiplied in, premultiplied out).

use tpt_color::{convert_premultiplied, premultiply, unpremultiply};

use crate::pixel::PixelFormat;
use crate::surface::{Surface, SurfaceError};

/// A premultiplied-alpha blend operator.
///
/// Inputs and output are premultiplied `[r, g, b, a]` in `0.0..=1.0`.
pub trait BlendMode {
    /// True only when `blend` is exactly premultiplied SourceOver. Lets the SIMD path replace it.
    const IS_SOURCE_OVER: bool = false;

    /// Combines one premultiplied source pixel with one premultiplied destination pixel.
    fn blend(src: [f32; 4], dst: [f32; 4]) -> [f32; 4];
}

/// Porter-Duff `SourceOver`: `src + dst * (1 - src.a)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceOver;

impl BlendMode for SourceOver {
    const IS_SOURCE_OVER: bool = true;

    #[inline]
    fn blend(src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
        let k = 1.0 - src[3];
        [
            src[0] + dst[0] * k,
            src[1] + dst[1] * k,
            src[2] + dst[2] * k,
            src[3] + dst[3] * k,
        ]
    }
}

/// Composites `src` onto `dst` in place, scaled by `opacity` (clamped to `0.0..=1.0`, NaN → 0).
///
/// Source and destination formats may differ. Dimensions must match exactly.
pub fn composite_surface<D, S, B>(
    dst: &mut Surface<'_, D>,
    src: &Surface<'_, S>,
    opacity: f32,
    _blend: B,
) -> Result<(), SurfaceError>
where
    D: PixelFormat,
    S: PixelFormat,
    B: BlendMode,
{
    if dst.width() != src.width() || dst.height() != src.height() {
        return Err(SurfaceError::DimensionMismatch);
    }
    let opacity = if opacity.is_nan() {
        0.0
    } else {
        opacity.clamp(0.0, 1.0)
    };
    let (src_cs, dst_cs) = (src.color_space(), dst.color_space());

    #[cfg(all(feature = "simd", not(feature = "scalar-fallback")))]
    if B::IS_SOURCE_OVER && src_cs == dst_cs {
        crate::simd::composite_source_over(dst, src, opacity);
        return Ok(());
    }

    for y in 0..dst.height() {
        for x in 0..dst.width() {
            let mut s = premultiply(src.get_pixel(x, y).to_f32());
            if src_cs != dst_cs {
                s = convert_premultiplied(s, src_cs, dst_cs);
            }
            let s = s.map(|c| c * opacity);
            let d = premultiply(dst.get_pixel(x, y).to_f32());
            let out = unpremultiply(B::blend(s, d));
            dst.set_pixel(x, y, D::from_f32(out));
        }
    }
    Ok(())
}
