//! The single shared f32 → u8 rounding function.

/// Converts a channel value on the `0.0..=255.0` scale to `u8`.
///
/// Rounds half to even, clamps to `0..=255`, and maps NaN to `0`.
/// Every f32 → u8 conversion in the crate must go through this function.
#[inline]
pub fn round_to_u8(v: f32) -> u8 {
    if v.is_nan() || v <= 0.0 {
        return 0;
    }
    if v >= 255.0 {
        return 255;
    }
    // `rintf` rounds to nearest, ties to even, and is exact in software (no FMA involved).
    libm::rintf(v) as u8
}

/// Converts a normalized channel value (`0.0..=1.0`) to `u8`.
#[inline]
pub fn unit_to_u8(v: f32) -> u8 {
    round_to_u8(v * 255.0)
}
