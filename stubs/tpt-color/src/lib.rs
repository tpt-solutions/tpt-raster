//! Local stub of `tpt-color`.
//!
//! Only the API that `tpt-raster` needs is provided. The real crate will replace
//! this one; keep the signatures stable so that swap is a path-to-version change.
//!
//! All channel values are `f32` in the `0.0..=1.0` range. Alpha is always linear.
#![no_std]
#![forbid(unsafe_code)]

/// Identifies the color space that a surface's channel values are expressed in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColorSpaceId {
    /// Gamma-encoded sRGB (the usual UI space).
    #[default]
    Srgb,
    /// Linear-light sRGB primaries.
    LinearSrgb,
}

/// Multiplies the color channels by alpha. Input and output are `[r, g, b, a]`.
#[inline]
pub fn premultiply(c: [f32; 4]) -> [f32; 4] {
    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
}

/// Divides the color channels by alpha. A fully transparent input maps to `[0; 4]`.
#[inline]
pub fn unpremultiply(c: [f32; 4]) -> [f32; 4] {
    if c[3] == 0.0 {
        [0.0; 4]
    } else {
        [c[0] / c[3], c[1] / c[3], c[2] / c[3], c[3]]
    }
}

/// sRGB transfer function (encoded to linear).
#[inline]
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        libm::powf((v + 0.055) / 1.055, 2.4)
    }
}

/// Inverse sRGB transfer function (linear to encoded).
#[inline]
pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * libm::powf(v, 1.0 / 2.4) - 0.055
    }
}

/// Converts premultiplied `[r, g, b, a]` from one color space to another.
///
/// The color is unpremultiplied, converted, and premultiplied again so the
/// conversion never sees alpha-scaled values.
#[inline]
pub fn convert_premultiplied(c: [f32; 4], from: ColorSpaceId, to: ColorSpaceId) -> [f32; 4] {
    if from == to {
        return c;
    }
    let u = unpremultiply(c);
    let f: fn(f32) -> f32 = match (from, to) {
        (ColorSpaceId::Srgb, ColorSpaceId::LinearSrgb) => srgb_to_linear,
        _ => linear_to_srgb,
    };
    premultiply([f(u[0]), f(u[1]), f(u[2]), u[3]])
}
