//! Sampling, edge modes, transformed draws, mipmaps and pattern fills.

use tpt_canvas::Affine2;
use tpt_color::ColorSpaceId;
use tpt_raster::pixel::{Rgba8, Rgba32F};
use tpt_raster::sample::{
    EdgeMode, Interp, draw_image, draw_image_mipmapped, mipmap_chain, sample,
};
use tpt_raster::{Surface, SurfaceError};
use tpt_testkit::fuzz::Rng;

const EPS: f32 = 1e-4;

fn grad(w: u32, h: u32) -> Surface<'static, Rgba32F> {
    let mut s = Surface::new(w, h, ColorSpaceId::Srgb).unwrap();
    for y in 0..h {
        for x in 0..w {
            let v = x as f32 / (w.max(2) - 1) as f32;
            s.set_pixel(
                x,
                y,
                Rgba32F {
                    r: v,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            );
        }
    }
    s
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < EPS
}

#[test]
fn nearest_at_pixel_centres_returns_the_pixel() {
    let s = grad(4, 1);
    for x in 0..4 {
        let c = sample(&s, x as f32 + 0.5, 0.5, Interp::Nearest, EdgeMode::Clamp);
        assert!(near(c[0], s.get_pixel(x, 0).r), "x={x}");
    }
}

#[test]
fn bilinear_midpoint_is_the_average_of_neighbours() {
    // Values 0 and 1 at pixel centres 0.5 and 1.5. The point x = 1.0 is halfway between them.
    let mut s: Surface<'static, Rgba32F> = Surface::new(2, 1, ColorSpaceId::Srgb).unwrap();
    s.set_pixel(
        0,
        0,
        Rgba32F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
    );
    s.set_pixel(
        1,
        0,
        Rgba32F {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
    );
    let c = sample(&s, 1.0, 0.5, Interp::Bilinear, EdgeMode::Clamp);
    assert!(near(c[0], 0.5), "{}", c[0]);
}

#[test]
fn every_interpolator_preserves_flat_input() {
    let mut s: Surface<'static, Rgba32F> = Surface::new(5, 5, ColorSpaceId::Srgb).unwrap();
    s.clear(Rgba32F {
        r: 0.3,
        g: 0.6,
        b: 0.9,
        a: 1.0,
    });
    for interp in [
        Interp::Nearest,
        Interp::Bilinear,
        Interp::Bicubic,
        Interp::Lanczos3,
    ] {
        for (x, y) in [(0.1, 0.2), (2.5, 2.5), (4.9, 0.0), (-3.0, 7.0)] {
            let c = sample(&s, x, y, interp, EdgeMode::Clamp);
            assert!(
                near(c[0], 0.3) && near(c[1], 0.6) && near(c[2], 0.9),
                "{interp:?} at ({x},{y}) = {c:?}"
            );
        }
    }
}

#[test]
fn clamp_repeats_the_edge_pixel() {
    let s = grad(3, 1);
    let far_left = sample(&s, -50.0, 0.5, Interp::Nearest, EdgeMode::Clamp);
    assert!(near(far_left[0], s.get_pixel(0, 0).r));
    let far_right = sample(&s, 50.0, 0.5, Interp::Nearest, EdgeMode::Clamp);
    assert!(near(far_right[0], s.get_pixel(2, 0).r));
}

#[test]
fn repeat_tiles_the_image() {
    let s = grad(4, 1);
    // Pixel -1 is the centre at x = -0.5, which tiles to the last pixel (3).
    let c = sample(&s, -0.5, 0.5, Interp::Nearest, EdgeMode::Repeat);
    assert!(near(c[0], s.get_pixel(3, 0).r));
    for x in [0.5, 4.5, 8.5] {
        let c = sample(&s, x, 0.5, Interp::Nearest, EdgeMode::Repeat);
        assert!(near(c[0], s.get_pixel(0, 0).r), "x={x}");
    }
}

#[test]
fn mirror_reflects_at_each_seam() {
    let s = grad(4, 1);
    // x = -0.5 is the centre of pixel -1, which mirrors to pixel 0.
    assert!(near(
        sample(&s, -0.5, 0.5, Interp::Nearest, EdgeMode::Mirror)[0],
        s.get_pixel(0, 0).r
    ));
    // x = 4.5 is pixel 4, which mirrors back to pixel 3.
    assert!(near(
        sample(&s, 4.5, 0.5, Interp::Nearest, EdgeMode::Mirror)[0],
        s.get_pixel(3, 0).r
    ));
}

#[test]
fn transparent_outside_the_image() {
    let s = grad(4, 1);
    let outside = sample(&s, -0.5, 0.5, Interp::Nearest, EdgeMode::Transparent);
    assert_eq!(outside[3], 0.0);
    let inside = sample(&s, 1.5, 0.5, Interp::Nearest, EdgeMode::Transparent);
    assert!(near(inside[3], 1.0));
}

#[test]
fn integer_translation_moves_the_image() {
    let src = grad(4, 1);
    let mut dst: Surface<'static, Rgba32F> = Surface::new(6, 1, ColorSpaceId::Srgb).unwrap();
    let t = Affine2 {
        m: [1.0, 0.0, 0.0, 1.0, 2.0, 0.0],
    };
    draw_image(
        &mut dst,
        &src,
        &t,
        Interp::Nearest,
        EdgeMode::Transparent,
        1.0,
    )
    .unwrap();
    assert_eq!(dst.get_pixel(0, 0).a, 0.0);
    assert_eq!(dst.get_pixel(1, 0).a, 0.0);
    for x in 2..6 {
        assert!(
            near(dst.get_pixel(x, 0).r, src.get_pixel(x - 2, 0).r),
            "x={x}"
        );
    }
}

#[test]
fn draw_with_opacity_scales_the_result() {
    let mut src: Surface<'static, Rgba32F> = Surface::new(1, 1, ColorSpaceId::Srgb).unwrap();
    src.set_pixel(
        0,
        0,
        Rgba32F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        },
    );
    let mut dst: Surface<'static, Rgba32F> = Surface::new(1, 1, ColorSpaceId::Srgb).unwrap();
    draw_image(
        &mut dst,
        &src,
        &Affine2::IDENTITY,
        Interp::Nearest,
        EdgeMode::Clamp,
        0.25,
    )
    .unwrap();
    assert!(near(dst.get_pixel(0, 0).a, 0.25));
}

#[test]
fn singular_transform_is_an_error() {
    let src = grad(2, 2);
    let mut dst: Surface<'static, Rgba32F> = Surface::new(2, 2, ColorSpaceId::Srgb).unwrap();
    let flat = Affine2 {
        m: [1.0, 2.0, 2.0, 4.0, 0.0, 0.0],
    };
    let err = draw_image(&mut dst, &src, &flat, Interp::Nearest, EdgeMode::Clamp, 1.0).unwrap_err();
    assert_eq!(err, SurfaceError::SingularTransform);
}

#[test]
fn pattern_fill_repeats_a_tile_across_the_destination() {
    // 2×2 tile: black, white on the top row; white, black on the bottom row.
    let mut tile: Surface<'static, Rgba8> = Surface::new(2, 2, ColorSpaceId::Srgb).unwrap();
    let (black, white) = (
        Rgba8 {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        },
        Rgba8 {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
    );
    tile.set_pixel(0, 0, black);
    tile.set_pixel(1, 0, white);
    tile.set_pixel(0, 1, white);
    tile.set_pixel(1, 1, black);
    let mut dst: Surface<'static, Rgba8> = Surface::new(8, 8, ColorSpaceId::Srgb).unwrap();
    draw_image(
        &mut dst,
        &tile,
        &Affine2::IDENTITY,
        Interp::Nearest,
        EdgeMode::Repeat,
        1.0,
    )
    .unwrap();
    // Dst (x, y) maps to tile (x mod 2, y mod 2), so the whole 8×8 grid is the tile repeated.
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(
                dst.get_pixel(x, y),
                tile.get_pixel(x % 2, y % 2),
                "({x},{y})"
            );
        }
    }
}

#[test]
fn mip_chain_halves_down_to_one_pixel() {
    let s = grad(9, 5);
    let chain = mipmap_chain(&s).unwrap();
    let sizes: Vec<(u32, u32)> = chain.iter().map(|l| (l.width(), l.height())).collect();
    assert_eq!(sizes, [(9, 5), (5, 3), (3, 2), (2, 1), (1, 1)]);
}

#[test]
fn mip_chain_of_empty_source_is_just_level_zero() {
    let s: Surface<'static, Rgba8> = Surface::new(0, 0, ColorSpaceId::Srgb).unwrap();
    let chain = mipmap_chain(&s).unwrap();
    assert_eq!(chain.len(), 1);
}

#[test]
fn mipmapped_downscale_averages_a_checkerboard_to_grey() {
    // An 8×8 black and white checkerboard is averaged to mid-grey when drawn at 1/4 size. Full
    // resolution sampling would pick single black or white pixels instead.
    let mut src: Surface<'static, Rgba32F> = Surface::new(8, 8, ColorSpaceId::Srgb).unwrap();
    for y in 0..8 {
        for x in 0..8 {
            let v = if (x + y) % 2 == 0 { 0.0 } else { 1.0 };
            src.set_pixel(
                x,
                y,
                Rgba32F {
                    r: v,
                    g: v,
                    b: v,
                    a: 1.0,
                },
            );
        }
    }
    let chain = mipmap_chain(&src).unwrap();
    let mut dst: Surface<'static, Rgba32F> = Surface::new(2, 2, ColorSpaceId::Srgb).unwrap();
    let t = Affine2 {
        m: [0.25, 0.0, 0.0, 0.25, 0.0, 0.0],
    };
    draw_image_mipmapped(&mut dst, &chain, &t, Interp::Bilinear, EdgeMode::Clamp, 1.0).unwrap();
    for p in dst.as_slice() {
        assert!((p[0] - 0.5).abs() < 0.2, "{p:?}");
    }
}

#[test]
fn fuzz_sampling_and_drawing_stay_finite_and_in_range() {
    let mut rng = Rng::new(31337);
    let modes = [
        EdgeMode::Clamp,
        EdgeMode::Repeat,
        EdgeMode::Mirror,
        EdgeMode::Transparent,
    ];
    let interps = [
        Interp::Nearest,
        Interp::Bilinear,
        Interp::Bicubic,
        Interp::Lanczos3,
    ];
    for _ in 0..300 {
        let (w, h) = (1 + rng.below(6) as u32, 1 + rng.below(6) as u32);
        let mut src: Surface<'static, Rgba32F> = Surface::new(w, h, ColorSpaceId::Srgb).unwrap();
        for y in 0..h {
            for x in 0..w {
                let a = rng.below(1001) as f32 / 1000.0;
                let mut c = || rng.below(1001) as f32 / 1000.0 * a;
                src.set_pixel(
                    x,
                    y,
                    Rgba32F {
                        r: c(),
                        g: c(),
                        b: c(),
                        a,
                    },
                );
            }
        }
        let t = Affine2 {
            m: [
                rng.below(40) as f32 / 10.0 - 2.0,
                rng.below(20) as f32 / 10.0 - 1.0,
                rng.below(20) as f32 / 10.0 - 1.0,
                rng.below(40) as f32 / 10.0 - 2.0,
                rng.below(100) as f32 - 20.0,
                rng.below(100) as f32 - 20.0,
            ],
        };
        let mode = modes[rng.below(4) as usize];
        let interp = interps[rng.below(4) as usize];
        let mut dst: Surface<'static, Rgba32F> = Surface::new(7, 7, ColorSpaceId::Srgb).unwrap();
        // A singular transform is allowed to fail, but nothing may panic.
        let _ = draw_image(&mut dst, &src, &t, interp, mode, 1.0);
        for p in dst.as_slice() {
            assert!(
                p.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)),
                "{p:?}"
            );
        }
        let chain = mipmap_chain(&src).unwrap();
        let _ = draw_image_mipmapped(&mut dst, &chain, &t, interp, mode, 0.5);
        for p in dst.as_slice() {
            assert!(
                p.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)),
                "{p:?}"
            );
        }
    }
}
