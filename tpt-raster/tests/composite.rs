//! Porter-Duff compositing tests.
use tpt_color::ColorSpaceId;
use tpt_raster::composite::{SourceOver, composite_surface};
use tpt_raster::pixel::{Gray8, PixelFormat, Rgba8, Rgba32F};
use tpt_raster::{Surface, SurfaceError};
use tpt_testkit::golden::assert_bytes_eq;

const WHITE: Rgba8 = Rgba8 {
    r: 255,
    g: 255,
    b: 255,
    a: 255,
};
const BLACK: Rgba8 = Rgba8 {
    r: 0,
    g: 0,
    b: 0,
    a: 255,
};

fn solid<P: PixelFormat>(w: u32, h: u32, p: P) -> Surface<'static, P> {
    let mut s = Surface::new(w, h, ColorSpaceId::Srgb).unwrap();
    s.clear(p);
    s
}

fn rgba32(r: f32, g: f32, b: f32, a: f32) -> Rgba32F {
    Rgba32F { r, g, b, a }
}

#[test]
fn opaque_source_replaces_destination() {
    let mut dst = solid(2, 2, BLACK);
    let src = solid(
        2,
        2,
        Rgba8 {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
    );
    composite_surface(&mut dst, &src, 1.0, SourceOver).unwrap();
    assert!(dst.rows().all(|r| r.iter().all(|p| *p == [255, 0, 0, 255])));
}

#[test]
fn transparent_source_leaves_destination_unchanged() {
    let mut dst = solid(
        2,
        2,
        Rgba8 {
            r: 10,
            g: 20,
            b: 30,
            a: 255,
        },
    );
    let src = solid(
        2,
        2,
        Rgba8 {
            r: 255,
            g: 255,
            b: 255,
            a: 0,
        },
    );
    composite_surface(&mut dst, &src, 1.0, SourceOver).unwrap();
    assert!(dst.as_slice().iter().all(|p| *p == [10, 20, 30, 255]));
}

#[test]
fn half_alpha_red_over_white() {
    // Premultiplied: src (0.5, 0, 0, 0.5), dst (1, 1, 1, 1).
    // out = src + dst * 0.5 = (1, 0.5, 0.5, 1). Green rounds 127.5 to 128 (ties to even).
    let mut dst = solid(1, 1, WHITE);
    let src = solid(1, 1, rgba32(1.0, 0.0, 0.0, 0.5));
    composite_surface(&mut dst, &src, 1.0, SourceOver).unwrap();
    assert_eq!(
        dst.get_pixel(0, 0),
        Rgba8 {
            r: 255,
            g: 128,
            b: 128,
            a: 255
        }
    );
}

#[test]
fn opacity_scales_the_source() {
    // Opaque black at 50% over white is mid gray.
    let mut dst = solid(1, 1, WHITE);
    let src = solid(1, 1, BLACK);
    composite_surface(&mut dst, &src, 0.5, SourceOver).unwrap();
    assert_eq!(
        dst.get_pixel(0, 0),
        Rgba8 {
            r: 128,
            g: 128,
            b: 128,
            a: 255
        }
    );
}

#[test]
fn opacity_is_clamped_and_nan_is_zero() {
    let src = solid(1, 1, BLACK);

    let mut dst = solid(1, 1, WHITE);
    composite_surface(&mut dst, &src, 2.0, SourceOver).unwrap();
    assert_eq!(dst.get_pixel(0, 0), BLACK);

    let mut dst = solid(1, 1, WHITE);
    composite_surface(&mut dst, &src, f32::NAN, SourceOver).unwrap();
    assert_eq!(dst.get_pixel(0, 0), WHITE);
}

#[test]
fn dimension_mismatch_is_an_error() {
    let mut dst = solid(2, 2, BLACK);
    let src = solid(2, 3, WHITE);
    let err = composite_surface(&mut dst, &src, 1.0, SourceOver).unwrap_err();
    assert_eq!(err, SurfaceError::DimensionMismatch);
}

#[test]
fn cross_format_gray_over_black_in_float() {
    let mut dst = solid(1, 1, rgba32(0.0, 0.0, 0.0, 1.0));
    let src = solid(1, 1, Gray8(255));
    composite_surface(&mut dst, &src, 1.0, SourceOver).unwrap();
    assert_eq!(dst.get_pixel(0, 0), rgba32(1.0, 1.0, 1.0, 1.0));
}

#[test]
fn float_source_into_u8_destination_rounds_once() {
    // Half-transparent white over opaque black: 0.5 * 255 = 127.5, ties to even -> 128.
    let mut dst = solid(1, 1, BLACK);
    let src = solid(1, 1, rgba32(1.0, 1.0, 1.0, 0.5));
    composite_surface(&mut dst, &src, 1.0, SourceOver).unwrap();
    assert_eq!(
        dst.get_pixel(0, 0),
        Rgba8 {
            r: 128,
            g: 128,
            b: 128,
            a: 255
        }
    );
}

#[test]
fn color_space_conversion_on_composite() {
    // Linear 0.5 gray, converted to sRGB, is about 0.7354, which is about 188 as u8.
    let mut dst = solid(1, 1, BLACK);
    let mut src: Surface<'static, Rgba32F> = Surface::new(1, 1, ColorSpaceId::LinearSrgb).unwrap();
    src.set_pixel(0, 0, rgba32(0.5, 0.5, 0.5, 1.0));
    composite_surface(&mut dst, &src, 1.0, SourceOver).unwrap();
    let out = dst.get_pixel(0, 0);
    assert!((187..=188).contains(&out.r), "r = {}", out.r);
    assert_eq!(out.r, out.g);
    assert_eq!(out.g, out.b);
}

#[test]
fn golden_bytes_for_a_small_scene() {
    // 2x1: left pixel is half-transparent red over opaque blue, right pixel is opaque.
    let mut dst: Surface<'static, Rgba8> = Surface::new(2, 1, ColorSpaceId::Srgb).unwrap();
    dst.set_pixel(
        0,
        0,
        Rgba8 {
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        },
    );
    dst.set_pixel(
        1,
        0,
        Rgba8 {
            r: 0,
            g: 255,
            b: 0,
            a: 255,
        },
    );
    let mut src: Surface<'static, Rgba8> = Surface::new(2, 1, ColorSpaceId::Srgb).unwrap();
    src.set_pixel(
        0,
        0,
        Rgba8 {
            r: 255,
            g: 0,
            b: 0,
            a: 128,
        },
    );
    src.set_pixel(
        1,
        0,
        Rgba8 {
            r: 255,
            g: 255,
            b: 255,
            a: 0,
        },
    );

    composite_surface(&mut dst, &src, 1.0, SourceOver).unwrap();

    // Left: with a = 128/255, src premult is (a, 0, 0, a) and dst premult is (0, 0, 1, 1).
    // Result: r = a, b = 1 - a, alpha 1. r * 255 = 128.0 and b * 255 = 127.0.
    // Right: fully transparent source leaves green untouched.
    let bytes: Vec<u8> = dst.as_slice().iter().flatten().copied().collect();
    assert_bytes_eq(&bytes, &[128, 0, 127, 255, 0, 255, 0, 255]);
}
