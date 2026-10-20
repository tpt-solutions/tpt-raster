//! Pixel format and f16 conversion tests.
use tpt_raster::pixel::{F16, Gray8, PixelFormat, Rgba8};
use tpt_raster::round::round_to_u8;

#[test]
fn round_to_u8_ties_to_even() {
    assert_eq!(round_to_u8(0.5), 0);
    assert_eq!(round_to_u8(1.5), 2);
    assert_eq!(round_to_u8(2.5), 2);
    assert_eq!(round_to_u8(254.5), 254);
    assert_eq!(round_to_u8(255.4), 255);
}

#[test]
fn round_to_u8_clamps_and_maps_nan_to_zero() {
    assert_eq!(round_to_u8(-1.0), 0);
    assert_eq!(round_to_u8(-0.0), 0);
    assert_eq!(round_to_u8(f32::NEG_INFINITY), 0);
    assert_eq!(round_to_u8(f32::NAN), 0);
    assert_eq!(round_to_u8(256.0), 255);
    assert_eq!(round_to_u8(f32::INFINITY), 255);
}

#[test]
fn rgba8_round_trips_every_value() {
    for v in 0..=255u8 {
        let p = Rgba8 {
            r: v,
            g: v,
            b: v,
            a: v,
        };
        assert_eq!(Rgba8::from_f32(p.to_f32()), p, "value {v}");
    }
}

#[test]
fn gray8_reads_opaque() {
    assert_eq!(Gray8(255).to_f32(), [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(Gray8::from_f32([1.0, 1.0, 1.0, 0.0]), Gray8(255));
}

#[test]
fn f16_known_values() {
    assert_eq!(F16::from_f32(1.0), F16(0x3c00));
    assert_eq!(F16::from_f32(-2.0), F16(0xc000));
    assert_eq!(F16::from_f32(65504.0), F16(0x7bff));
    // Halfway between 65504 and 65536. 65504 has an odd mantissa, so ties round up to infinity.
    assert_eq!(F16::from_f32(65520.0), F16(0x7c00));
    assert_eq!(F16::from_f32(f32::INFINITY), F16(0x7c00));
    assert_eq!(F16::from_f32(f32::NEG_INFINITY), F16(0xfc00));
    // Smallest subnormal is 2^-24. Half of it ties to zero (even).
    assert_eq!(F16::from_f32(2f32.powi(-24)), F16(0x0001));
    assert_eq!(F16::from_f32(2f32.powi(-25)), F16(0x0000));
    assert_eq!(F16(0x0001).to_f32(), 2f32.powi(-24));
    assert_eq!(F16(0x03ff).to_f32(), 1023.0 * 2f32.powi(-24));
    assert_eq!(F16(0x0400).to_f32(), 2f32.powi(-14));
}

#[test]
fn f16_nan_stays_nan() {
    assert!(F16::from_f32(f32::NAN).to_f32().is_nan());
    // Signaling-style payload with a zero top mantissa still produces a NaN, not infinity.
    let nan = f32::from_bits(0x7f80_0001);
    assert!(F16::from_f32(nan).to_f32().is_nan());
}

#[test]
fn f16_exhaustive_round_trip() {
    for bits in 0..=u16::MAX {
        let h = F16(bits);
        let f = h.to_f32();
        if f.is_nan() {
            assert!(F16::from_f32(f).to_f32().is_nan(), "bits {bits:#06x}");
            continue;
        }
        assert_eq!(F16::from_f32(f), h, "bits {bits:#06x} widened to {f:e}");
    }
}

#[test]
fn f16_rounding_matches_nearest_representable() {
    // For values in the normal range, the result must be the nearest f16 (ties to even).
    // Checked against the neighbours on each side of a sample of binary32 values.
    let mut state: u32 = 0x1234_5678;
    for _ in 0..200_000 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let x = f32::from_bits(state & 0x7fff_ffff);
        if !x.is_finite() || x >= 65504.0 || x < 2f32.powi(-14) {
            continue;
        }
        let h = F16::from_f32(x);
        let v = h.to_f32();
        let below = F16(h.0.wrapping_sub(1)).to_f32();
        let above = F16(h.0.wrapping_add(1)).to_f32();
        assert!((v - x).abs() <= (below - x).abs(), "x={x:e}");
        assert!((v - x).abs() <= (above - x).abs(), "x={x:e}");
    }
}
