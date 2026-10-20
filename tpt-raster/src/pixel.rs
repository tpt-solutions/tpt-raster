//! Pixel formats. Each format is a pixel value type plus its raw storage element.
//!
//! The canonical working space is `[r, g, b, a]` as `f32` in `0.0..=1.0`. Integer formats
//! convert through [`crate::round::unit_to_u8`]. Float formats store as-is.

use crate::round::unit_to_u8;

/// A pixel value type that can live in a [`crate::Surface`].
pub trait PixelFormat: Copy + Default + core::fmt::Debug {
    /// One pixel's raw storage element (one per pixel, not per channel).
    type Storage: Copy + Default;

    /// Bytes occupied by one pixel in storage.
    const BYTES_PER_PIXEL: usize = core::mem::size_of::<Self::Storage>();

    /// Packs the pixel into its storage element.
    fn to_storage(self) -> Self::Storage;
    /// Unpacks a storage element into a pixel.
    fn from_storage(s: Self::Storage) -> Self;

    /// Widens to the canonical `f32` RGBA form.
    fn to_f32(self) -> [f32; 4];
    /// Narrows from the canonical `f32` RGBA form, rounding as the format requires.
    fn from_f32(c: [f32; 4]) -> Self;
}

/// 8-bit-per-channel RGBA. The standard UI format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba8 {
    /// Red, 0..=255.
    pub r: u8,
    /// Green, 0..=255.
    pub g: u8,
    /// Blue, 0..=255.
    pub b: u8,
    /// Alpha, 0..=255 (straight, not premultiplied).
    pub a: u8,
}

impl PixelFormat for Rgba8 {
    type Storage = [u8; 4];

    #[inline]
    fn to_storage(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    #[inline]
    fn from_storage(s: [u8; 4]) -> Self {
        Self {
            r: s[0],
            g: s[1],
            b: s[2],
            a: s[3],
        }
    }

    #[inline]
    fn to_f32(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a].map(|c| c as f32 / 255.0)
    }

    #[inline]
    fn from_f32(c: [f32; 4]) -> Self {
        Self {
            r: unit_to_u8(c[0]),
            g: unit_to_u8(c[1]),
            b: unit_to_u8(c[2]),
            a: unit_to_u8(c[3]),
        }
    }
}

/// 32-bit float RGBA. Used for scientific and intermediate work.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rgba32F {
    /// Red, normally `0.0..=1.0`.
    pub r: f32,
    /// Green, normally `0.0..=1.0`.
    pub g: f32,
    /// Blue, normally `0.0..=1.0`.
    pub b: f32,
    /// Alpha, normally `0.0..=1.0` (straight, not premultiplied).
    pub a: f32,
}

impl PixelFormat for Rgba32F {
    type Storage = [f32; 4];

    #[inline]
    fn to_storage(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    #[inline]
    fn from_storage(s: [f32; 4]) -> Self {
        Self {
            r: s[0],
            g: s[1],
            b: s[2],
            a: s[3],
        }
    }

    #[inline]
    fn to_f32(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    #[inline]
    fn from_f32(c: [f32; 4]) -> Self {
        Self {
            r: c[0],
            g: c[1],
            b: c[2],
            a: c[3],
        }
    }
}

/// 8-bit grayscale. Read as opaque `[v, v, v, 1]`.
///
/// Mask use (coverage in alpha) will get its own type when masks land in Phase 2.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gray8(pub u8);

impl PixelFormat for Gray8 {
    type Storage = u8;

    #[inline]
    fn to_storage(self) -> u8 {
        self.0
    }

    #[inline]
    fn from_storage(s: u8) -> Self {
        Self(s)
    }

    #[inline]
    fn to_f32(self) -> [f32; 4] {
        let v = self.0 as f32 / 255.0;
        [v, v, v, 1.0]
    }

    #[inline]
    fn from_f32(c: [f32; 4]) -> Self {
        // Rec. 709 luma weights, applied in f32 in a fixed order.
        let y = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        Self(unit_to_u8(y))
    }
}

/// IEEE 754 binary16 stored as its raw bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct F16(pub u16);

impl F16 {
    /// Converts with round-to-nearest-even. Handles subnormals, overflow to infinity,
    /// and NaN (quiet, payload truncated).
    pub fn from_f32(x: f32) -> Self {
        let bits = x.to_bits();
        let sign = ((bits >> 16) & 0x8000) as u16;
        let exp = ((bits >> 23) & 0xff) as i32;
        let man = bits & 0x007f_ffff;

        if exp == 0xff {
            // Infinity, or NaN with the quiet bit forced so it never becomes infinity.
            let payload = (man >> 13) as u16;
            return if man == 0 {
                Self(sign | 0x7c00)
            } else {
                Self(sign | 0x7c00 | 0x0200 | payload)
            };
        }

        // Half exponent: e = exp - 127 + 15.
        let e = exp - 112;
        if e >= 31 {
            return Self(sign | 0x7c00);
        }
        if e <= 0 {
            // Subnormal half (or zero). Result unit is 2^-24.
            if e < -10 {
                return Self(sign);
            }
            let m = man | 0x0080_0000;
            let shift = (14 - e) as u32;
            let mut h = m >> shift;
            let rem = m & ((1 << shift) - 1);
            let halfway = 1 << (shift - 1);
            if rem > halfway || (rem == halfway && (h & 1) == 1) {
                // A carry out of the subnormal range lands exactly on the smallest normal.
                h += 1;
            }
            return Self(sign | h as u16);
        }

        // Normal half.
        let mut h = ((e as u32) << 10) | (man >> 13);
        let rem = man & 0x1fff;
        if rem > 0x1000 || (rem == 0x1000 && (h & 1) == 1) {
            // A carry can roll the exponent up to infinity, which is the correct result.
            h += 1;
        }
        Self(sign | h as u16)
    }

    /// Widens exactly (every binary16 value is representable in binary32).
    pub fn to_f32(self) -> f32 {
        let h = self.0;
        let sign = (h as u32 & 0x8000) << 16;
        let exp = ((h >> 10) & 0x1f) as u32;
        let man = (h & 0x03ff) as u32;

        let bits = if exp == 0 {
            if man == 0 {
                sign
            } else {
                // Normalize the subnormal: shift until the leading bit reaches bit 10.
                let mut m = man;
                let mut e: i32 = -14;
                while m & 0x0400 == 0 {
                    m <<= 1;
                    e -= 1;
                }
                let m = m & 0x03ff;
                sign | (((e + 127) as u32) << 23) | (m << 13)
            }
        } else if exp == 0x1f {
            sign | 0x7f80_0000 | (man << 13)
        } else {
            sign | ((exp + 112) << 23) | (man << 13)
        };
        f32::from_bits(bits)
    }
}

/// RGBA with binary16 channels. HDR and intermediate surfaces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba16F {
    /// Red.
    pub r: F16,
    /// Green.
    pub g: F16,
    /// Blue.
    pub b: F16,
    /// Alpha (straight, not premultiplied).
    pub a: F16,
}

impl PixelFormat for Rgba16F {
    type Storage = [u16; 4];

    #[inline]
    fn to_storage(self) -> [u16; 4] {
        [self.r.0, self.g.0, self.b.0, self.a.0]
    }

    #[inline]
    fn from_storage(s: [u16; 4]) -> Self {
        Self {
            r: F16(s[0]),
            g: F16(s[1]),
            b: F16(s[2]),
            a: F16(s[3]),
        }
    }

    #[inline]
    fn to_f32(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a].map(F16::to_f32)
    }

    #[inline]
    fn from_f32(c: [f32; 4]) -> Self {
        Self {
            r: F16::from_f32(c[0]),
            g: F16::from_f32(c[1]),
            b: F16::from_f32(c[2]),
            a: F16::from_f32(c[3]),
        }
    }
}
