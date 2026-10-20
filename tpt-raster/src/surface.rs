//! Module A: pixel buffers.
//!
//! Bounds policy: `get_pixel` and `set_pixel` panic on out-of-range coordinates, like slice
//! indexing. `try_get_pixel` and `try_set_pixel` return `None` / `false` instead, for code that
//! takes coordinates from untrusted input.
//!
//! Stride is measured in **pixels**, not bytes. Each pixel is one `P::Storage` element, so a
//! byte stride would have to be a multiple of `BYTES_PER_PIXEL` anyway.

use alloc::{vec, vec::Vec};
use core::fmt;

use tpt_color::ColorSpaceId;

use crate::pixel::PixelFormat;

/// Errors from surface construction and composition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceError {
    /// `stride` is smaller than `width`, so rows would overlap.
    StrideTooSmall {
        /// The stride that was requested.
        stride: u32,
        /// The width it must cover.
        width: u32,
    },
    /// The borrowed buffer cannot hold the described pixels.
    BufferTooSmall {
        /// Pixels needed.
        required: usize,
        /// Pixels supplied.
        actual: usize,
    },
    /// `width * height` or `stride * height` does not fit in `usize`.
    SizeOverflow,
    /// Source and destination have different dimensions.
    DimensionMismatch,
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StrideTooSmall { stride, width } => {
                write!(f, "stride {stride} is smaller than width {width}")
            }
            Self::BufferTooSmall { required, actual } => {
                write!(f, "buffer holds {actual} pixels, need {required}")
            }
            Self::SizeOverflow => f.write_str("surface size overflows usize"),
            Self::DimensionMismatch => f.write_str("surface dimensions do not match"),
        }
    }
}

/// Backing store for a surface: owned, or borrowed from the caller.
pub enum SurfaceBuffer<'a, P: PixelFormat> {
    /// Heap storage owned by the surface.
    Owned(Vec<P::Storage>),
    /// Storage borrowed from the caller.
    Borrowed(&'a mut [P::Storage]),
}

impl<P: PixelFormat> SurfaceBuffer<'_, P> {
    #[inline]
    fn as_slice(&self) -> &[P::Storage] {
        match self {
            Self::Owned(v) => v,
            Self::Borrowed(s) => s,
        }
    }

    #[inline]
    fn as_mut_slice(&mut self) -> &mut [P::Storage] {
        match self {
            Self::Owned(v) => v,
            Self::Borrowed(s) => s,
        }
    }
}

/// A 2D grid of pixels in one format and one color space.
pub struct Surface<'a, P: PixelFormat> {
    data: SurfaceBuffer<'a, P>,
    width: u32,
    height: u32,
    stride: u32,
    color_space: ColorSpaceId,
}

/// Number of storage elements needed to hold `height` rows of `width` pixels at `stride`.
fn required_len(width: u32, height: u32, stride: u32) -> Result<usize, SurfaceError> {
    if height == 0 {
        return Ok(0);
    }
    let rows = (height as usize - 1)
        .checked_mul(stride as usize)
        .ok_or(SurfaceError::SizeOverflow)?;
    rows.checked_add(width as usize)
        .ok_or(SurfaceError::SizeOverflow)
}

impl<'a, P: PixelFormat> Surface<'a, P> {
    /// Allocates an owned, zero-filled surface with `stride == width`.
    pub fn new(width: u32, height: u32, color_space: ColorSpaceId) -> Result<Self, SurfaceError> {
        let len = (width as usize)
            .checked_mul(height as usize)
            .ok_or(SurfaceError::SizeOverflow)?;
        Ok(Self {
            data: SurfaceBuffer::Owned(vec![P::Storage::default(); len]),
            width,
            height,
            stride: width,
            color_space,
        })
    }

    /// Wraps a caller-owned buffer. Rows start every `stride` pixels.
    ///
    /// Padding between rows (`stride - width`) is never read or written.
    pub fn from_slice(
        buf: &'a mut [P::Storage],
        width: u32,
        height: u32,
        stride: u32,
        color_space: ColorSpaceId,
    ) -> Result<Self, SurfaceError> {
        if stride < width {
            return Err(SurfaceError::StrideTooSmall { stride, width });
        }
        let required = required_len(width, height, stride)?;
        if buf.len() < required {
            return Err(SurfaceError::BufferTooSmall {
                required,
                actual: buf.len(),
            });
        }
        Ok(Self {
            data: SurfaceBuffer::Borrowed(buf),
            width,
            height,
            stride,
            color_space,
        })
    }

    /// Width in pixels.
    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Row pitch, in pixels.
    #[inline]
    pub fn stride(&self) -> u32 {
        self.stride
    }

    /// The color space the channel values are expressed in.
    #[inline]
    pub fn color_space(&self) -> ColorSpaceId {
        self.color_space
    }

    /// Index of `(x, y)` in the backing slice. Caller ensures the coordinate is in range.
    #[inline]
    fn index(&self, x: u32, y: u32) -> usize {
        y as usize * self.stride as usize + x as usize
    }

    #[inline]
    fn in_bounds(&self, x: u32, y: u32) -> bool {
        x < self.width && y < self.height
    }

    /// Reads a pixel. Panics if `(x, y)` is out of range.
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> P {
        self.try_get_pixel(x, y)
            .unwrap_or_else(|| panic!("pixel ({x}, {y}) outside {}x{}", self.width, self.height))
    }

    /// Writes a pixel. Panics if `(x, y)` is out of range.
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, pixel: P) {
        let (w, h) = (self.width, self.height);
        assert!(self.in_bounds(x, y), "pixel ({x}, {y}) outside {w}x{h}");
        self.set_unchecked(x, y, pixel);
    }

    /// Reads a pixel, or `None` if `(x, y)` is out of range.
    #[inline]
    pub fn try_get_pixel(&self, x: u32, y: u32) -> Option<P> {
        if !self.in_bounds(x, y) {
            return None;
        }
        Some(P::from_storage(self.data.as_slice()[self.index(x, y)]))
    }

    /// Writes a pixel. Returns `false` (and writes nothing) if `(x, y)` is out of range.
    #[inline]
    pub fn try_set_pixel(&mut self, x: u32, y: u32, pixel: P) -> bool {
        if !self.in_bounds(x, y) {
            return false;
        }
        self.set_unchecked(x, y, pixel);
        true
    }

    #[inline]
    fn set_unchecked(&mut self, x: u32, y: u32, pixel: P) {
        let i = self.index(x, y);
        self.data.as_mut_slice()[i] = pixel.to_storage();
    }

    /// The whole backing slice, including any row padding.
    #[inline]
    pub fn as_slice(&self) -> &[P::Storage] {
        self.data.as_slice()
    }

    /// The whole backing slice, including any row padding, mutably.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [P::Storage] {
        self.data.as_mut_slice()
    }

    /// Row `y` (exactly `width` pixels, no padding). Panics if `y >= height`.
    pub fn row(&self, y: u32) -> &[P::Storage] {
        assert!(y < self.height, "row {y} outside height {}", self.height);
        let start = y as usize * self.stride as usize;
        &self.data.as_slice()[start..start + self.width as usize]
    }

    /// Row `y`, mutably. Panics if `y >= height`.
    pub fn row_mut(&mut self, y: u32) -> &mut [P::Storage] {
        assert!(y < self.height, "row {y} outside height {}", self.height);
        let start = y as usize * self.stride as usize;
        let end = start + self.width as usize;
        &mut self.data.as_mut_slice()[start..end]
    }

    /// Iterates over the rows, top to bottom, without padding.
    pub fn rows(&self) -> impl Iterator<Item = &[P::Storage]> + '_ {
        (0..self.height).map(move |y| self.row(y))
    }

    /// Sets every pixel (padding is untouched).
    pub fn clear(&mut self, pixel: P) {
        let storage = pixel.to_storage();
        for y in 0..self.height {
            self.row_mut(y).fill(storage);
        }
    }
}
