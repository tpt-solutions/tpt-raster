//! `tpt-canvas::RasterBackend` on `Surface`: a backend-generic caller produces the same pixels
//! as calling `rasterize_path` directly, through both static and `dyn` dispatch.

use tpt_canvas::{Affine2, FillRule, FlattenedPath, Paint, RasterBackend};
use tpt_color::ColorSpaceId;
use tpt_raster::Surface;
use tpt_raster::pixel::{Rgba8, Rgba32F};
use tpt_raster::rasterize::rasterize_path;

/// Draws two overlapping shapes through the trait alone, as tpt-canvas would.
fn draw_scene<B: RasterBackend + ?Sized>(backend: &mut B) -> Result<(), B::Error> {
    let mut square = FlattenedPath::new();
    square.push_contour(vec![[1.0, 1.0], [7.0, 1.0], [7.0, 7.0], [1.0, 7.0]]);
    let mut tri = FlattenedPath::new();
    tri.push_contour(vec![[4.0, 0.5], [9.5, 9.5], [0.5, 9.5]]);
    backend.fill_path(
        &square,
        &Affine2::IDENTITY,
        FillRule::NonZero,
        &Paint::Solid([1.0, 0.0, 0.0, 0.6]),
    )?;
    backend.fill_path(
        &tri,
        &Affine2::IDENTITY,
        FillRule::EvenOdd,
        &Paint::Linear {
            start: [0.0, 0.0],
            end: [10.0, 0.0],
            from: [0.0, 0.0, 1.0, 1.0],
            to: [0.0, 1.0, 0.0, 0.5],
        },
    )
}

fn direct<P: tpt_raster::pixel::PixelFormat>() -> Surface<'static, P> {
    let mut s = Surface::<P>::new(10, 10, ColorSpaceId::Srgb).unwrap();
    let mut square = FlattenedPath::new();
    square.push_contour(vec![[1.0, 1.0], [7.0, 1.0], [7.0, 7.0], [1.0, 7.0]]);
    let mut tri = FlattenedPath::new();
    tri.push_contour(vec![[4.0, 0.5], [9.5, 9.5], [0.5, 9.5]]);
    rasterize_path(
        &mut s,
        &square,
        &Affine2::IDENTITY,
        FillRule::NonZero,
        &Paint::Solid([1.0, 0.0, 0.0, 0.6]),
    )
    .unwrap();
    rasterize_path(
        &mut s,
        &tri,
        &Affine2::IDENTITY,
        FillRule::EvenOdd,
        &Paint::Linear {
            start: [0.0, 0.0],
            end: [10.0, 0.0],
            from: [0.0, 0.0, 1.0, 1.0],
            to: [0.0, 1.0, 0.0, 0.5],
        },
    )
    .unwrap();
    s
}

#[test]
fn backend_generic_draw_matches_direct_rasterize_rgba8() {
    let mut s = Surface::<Rgba8>::new(10, 10, ColorSpaceId::Srgb).unwrap();
    draw_scene(&mut s).unwrap();
    assert_eq!(s.as_slice(), direct::<Rgba8>().as_slice());
}

#[test]
fn backend_generic_draw_matches_direct_rasterize_rgba32f() {
    let mut s = Surface::<Rgba32F>::new(10, 10, ColorSpaceId::Srgb).unwrap();
    draw_scene(&mut s).unwrap();
    assert_eq!(s.as_slice(), direct::<Rgba32F>().as_slice());
}

#[test]
fn dyn_dispatch_draws_the_same_pixels() {
    let mut s = Surface::<Rgba8>::new(10, 10, ColorSpaceId::Srgb).unwrap();
    let backend: &mut dyn RasterBackend<Error = tpt_raster::SurfaceError> = &mut s;
    draw_scene(backend).unwrap();
    assert_eq!(s.as_slice(), direct::<Rgba8>().as_slice());
}

#[test]
fn non_finite_path_is_ignored_not_an_error() {
    let mut s = Surface::<Rgba8>::new(4, 4, ColorSpaceId::Srgb).unwrap();
    let mut bad = FlattenedPath::new();
    bad.push_contour(vec![[f32::NAN, 0.0], [2.0, f32::INFINITY], [1.0, 3.0]]);
    s.fill_path(
        &bad,
        &Affine2::IDENTITY,
        FillRule::NonZero,
        &Paint::Solid([1.0; 4]),
    )
    .unwrap();
    assert!(s.as_slice().iter().all(|p| *p == [0u8; 4]));
}
