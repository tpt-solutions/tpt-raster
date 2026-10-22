//! `tpt-raster`: 2D rasterization and compositing on pixel surfaces.
//!
//! Core rules (see `spec.txt`):
//! - `no_std` + `alloc`. No `unsafe` in this crate.
//! - All pixel math is `f32`. Conversion back to `u8` goes through [`round::round_to_u8`] only.
//! - Surface values are straight (unpremultiplied) alpha. Compositing premultiplies on read
//!   and unpremultiplies on write.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub mod canvas;
pub mod composite;
pub mod filter;
pub mod modes;
pub mod pixel;
pub mod rasterize;
pub mod round;
pub mod sample;
#[cfg(feature = "simd")]
pub mod simd;
pub mod surface;

pub use surface::{Surface, SurfaceError};
