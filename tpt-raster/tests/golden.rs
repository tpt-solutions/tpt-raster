//! Regression goldens. Each scene is hashed (FNV-1a, 64-bit) and compared with a pinned value.
//!
//! The inputs are deterministic and use only literal constants, not platform trig. The
//! kernels use `libm`, so the output should be identical on every target. The CI matrix
//! (x86_64, ARM64, macOS, Windows) is what checks that. A changed hash is either an intended
//! change to the maths, which needs the pin updated, or a bug.

use tpt_canvas::{Affine2, FillRule, FlattenedPath, Paint};
use tpt_color::ColorSpaceId;
use tpt_raster::Surface;
use tpt_raster::composite::composite_surface;
use tpt_raster::filter::{close, gaussian_blur, open};
use tpt_raster::modes::Hue;
use tpt_raster::pixel::{Rgba8, Rgba32F};
use tpt_raster::rasterize::rasterize_path;
use tpt_raster::sample::{EdgeMode, Interp, draw_image, draw_image_mipmapped, mipmap_chain};
use tpt_testkit::fuzz::Rng;
use tpt_testkit::golden::assert_bytes_eq;

/// FNV-1a, 64-bit. Small, dependency-free and stable.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn rgba8_bytes<'a>(s: &Surface<'a, Rgba8>) -> Vec<u8> {
    s.as_slice().iter().flatten().copied().collect()
}

/// Checks a scene's hash against its pin. The first run prints the actual hash.
fn check(name: &str, bytes: &[u8], pinned: u64) {
    let got = fnv1a(bytes);
    assert_eq!(
        got, pinned,
        "{name}: hash {got:#018x} does not match pin {pinned:#018x}"
    );
}

fn star() -> FlattenedPath {
    let mut p = FlattenedPath::new();
    let pts: Vec<[f32; 2]> = (0..5)
        .map(|k| {
            // Pentagram vertices from literal unit vectors, so no platform trig is involved.
            const COS: [f32; 5] = [0.0, 0.951_056_5, -0.587_785_25, -0.587_785_25, 0.951_056_5];
            const SIN: [f32; 5] = [-1.0, -0.309_017, 0.809_017, -0.809_017, 0.309_017];
            let i = (2 * k) % 5;
            [16.0 + 13.0 * COS[i], 16.0 + 13.0 * SIN[i]]
        })
        .collect();
    p.push_contour(pts);
    p
}

#[test]
fn golden_star_even_odd_over_background() {
    let mut s: Surface<'static, Rgba8> = Surface::new(32, 32, ColorSpaceId::Srgb).unwrap();
    s.clear(Rgba8 {
        r: 20,
        g: 40,
        b: 200,
        a: 255,
    });
    let paint = Paint::Solid([1.0, 0.5, 0.0, 0.9]);
    rasterize_path(
        &mut s,
        &star(),
        &Affine2::IDENTITY,
        FillRule::EvenOdd,
        &paint,
    )
    .unwrap();
    // Sanity check that the pin is not over an empty scene: the star must change many pixels.
    let background = Rgba8 {
        r: 20,
        g: 40,
        b: 200,
        a: 255,
    };
    let changed = s
        .as_slice()
        .iter()
        .filter(|p| p[0] != background.r || p[1] != background.g)
        .count();
    assert!(changed > 100, "star changed only {changed} pixels");
    check("star evenodd", &rgba8_bytes(&s), GOLDEN_STAR);
}

#[test]
fn golden_blur_of_checker() {
    let mut s: Surface<'static, Rgba8> = Surface::new(16, 16, ColorSpaceId::Srgb).unwrap();
    for y in 0..16 {
        for x in 0..16 {
            let v = if (x / 2 + y / 2) % 2 == 0 { 255 } else { 0 };
            s.set_pixel(
                x,
                y,
                Rgba8 {
                    r: v,
                    g: 128,
                    b: 255 - v,
                    a: 255,
                },
            );
        }
    }
    gaussian_blur(&mut s, 1.5);
    check("blur checker", &rgba8_bytes(&s), GOLDEN_BLUR);
}

#[test]
fn golden_bicubic_rotated_draw() {
    let mut src: Surface<'static, Rgba32F> = Surface::new(12, 12, ColorSpaceId::Srgb).unwrap();
    for y in 0..12 {
        for x in 0..12 {
            src.set_pixel(
                x,
                y,
                Rgba32F {
                    r: x as f32 / 11.0,
                    g: y as f32 / 11.0,
                    b: 0.5,
                    a: 1.0,
                },
            );
        }
    }
    let mut dst: Surface<'static, Rgba8> = Surface::new(24, 24, ColorSpaceId::Srgb).unwrap();
    // Rotation by 30 degrees about the centre, from literal cos and sin values.
    let (c, s) = (0.866_025_4f32, 0.5f32);
    let t = Affine2 {
        m: [c, s, -s, c, 6.0, -1.0],
    };
    draw_image(
        &mut dst,
        &src,
        &t,
        Interp::Bicubic,
        EdgeMode::Transparent,
        1.0,
    )
    .unwrap();
    check("bicubic rotated", &rgba8_bytes(&dst), GOLDEN_BICUBIC);
}

#[test]
fn golden_mipmapped_downscale() {
    let mut src: Surface<'static, Rgba8> = Surface::new(32, 32, ColorSpaceId::Srgb).unwrap();
    for y in 0..32 {
        for x in 0..32 {
            let v = ((x * 7 + y * 3) % 256) as u8;
            src.set_pixel(
                x,
                y,
                Rgba8 {
                    r: v,
                    g: 255 - v,
                    b: x as u8,
                    a: 255,
                },
            );
        }
    }
    let chain = mipmap_chain(&src).unwrap();
    let mut dst: Surface<'static, Rgba8> = Surface::new(8, 8, ColorSpaceId::Srgb).unwrap();
    let t = Affine2 {
        m: [0.25, 0.0, 0.0, 0.25, 0.0, 0.0],
    };
    draw_image_mipmapped(&mut dst, &chain, &t, Interp::Bilinear, EdgeMode::Clamp, 1.0).unwrap();
    check("mipmapped", &rgba8_bytes(&dst), GOLDEN_MIPMAP);
}

#[test]
fn golden_hue_blend_over_gradient() {
    let mut dst: Surface<'static, Rgba32F> = Surface::new(16, 4, ColorSpaceId::Srgb).unwrap();
    let mut src: Surface<'static, Rgba32F> = Surface::new(16, 4, ColorSpaceId::Srgb).unwrap();
    for y in 0..4 {
        for x in 0..16 {
            let v = x as f32 / 15.0;
            dst.set_pixel(
                x,
                y,
                Rgba32F {
                    r: v,
                    g: 1.0 - v,
                    b: 0.25,
                    a: 1.0,
                },
            );
            src.set_pixel(
                x,
                y,
                Rgba32F {
                    r: 0.9,
                    g: 0.1 + 0.05 * y as f32,
                    b: 0.3,
                    a: 0.7,
                },
            );
        }
    }
    // Hue blend is a non-separable W3C mode. Composite it with the generic path.
    let mut out: Surface<'static, Rgba32F> = Surface::new(16, 4, ColorSpaceId::Srgb).unwrap();
    for y in 0..4 {
        for x in 0..16 {
            out.set_pixel(x, y, dst.get_pixel(x, y));
        }
    }
    composite_surface(&mut out, &src, 1.0, Hue).unwrap();
    let bytes: Vec<u8> = out
        .as_slice()
        .iter()
        .flatten()
        .flat_map(|c| c.to_le_bytes())
        .collect();
    check("hue blend", &bytes, GOLDEN_HUE);
}

#[test]
fn golden_morphology_on_random_mask() {
    let mut rng = Rng::new(0x5EED);
    let mut s: Surface<'static, Rgba8> = Surface::new(20, 20, ColorSpaceId::Srgb).unwrap();
    for y in 0..20 {
        for x in 0..20 {
            let on = rng.below(3) != 0;
            s.set_pixel(
                x,
                y,
                if on {
                    Rgba8 {
                        r: 255,
                        g: 255,
                        b: 255,
                        a: 255,
                    }
                } else {
                    Rgba8::default()
                },
            );
        }
    }
    open(&mut s, 1);
    close(&mut s, 1);
    check("morphology", &rgba8_bytes(&s), GOLDEN_MORPH);
}

#[test]
fn golden_fnv_matches_known_vector() {
    // Published FNV-1a 64 test vectors.
    assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    assert_bytes_eq(b"ok", b"ok");
}

// Pins. Filled in from the first run; see the comment at the top of this file.
const GOLDEN_STAR: u64 = 0x424e_09b0_458a_c934;
const GOLDEN_BLUR: u64 = 0x8dbb_6d8f_e832_8b65;
const GOLDEN_BICUBIC: u64 = 0x203f_19fb_1438_f28b;
const GOLDEN_MIPMAP: u64 = 0x9776_3c21_4cb9_49a5;
const GOLDEN_HUE: u64 = 0x50be_eae4_e08c_d126;
const GOLDEN_MORPH: u64 = 0x38ab_b494_d601_5735;

/// Literal quarter-arc unit vectors, `(cos, sin)` from 0 to 90 degrees. No platform trig.
const QUARTER: [(f32, f32); 3] = [(1.0, 0.0), (0.707_106_77, 0.707_106_77), (0.0, 1.0)];

/// A 12-gon approximating a circle, from literal unit vectors every 30 degrees.
fn golden_circle() -> FlattenedPath {
    const COS: [f32; 12] = [
        1.0,
        0.866_025_4,
        0.5,
        0.0,
        -0.5,
        -0.866_025_4,
        -1.0,
        -0.866_025_4,
        -0.5,
        0.0,
        0.5,
        0.866_025_4,
    ];
    const SIN: [f32; 12] = [
        0.0,
        0.5,
        0.866_025_4,
        1.0,
        0.866_025_4,
        0.5,
        0.0,
        -0.5,
        -0.866_025_4,
        -1.0,
        -0.866_025_4,
        -0.5,
    ];
    let pts: Vec<[f32; 2]> = (0..12)
        .map(|i| [20.0 + 11.0 * COS[i], 20.0 + 11.0 * SIN[i]])
        .collect();
    let mut p = FlattenedPath::new();
    p.push_contour(pts);
    p
}

/// A rectangle with quarter-circle corners of radius 4, using the literal table above.
fn golden_rounded_rect() -> FlattenedPath {
    let (x0, y0, x1, y1, r) = (3.0f32, 5.0f32, 28.0f32, 24.0f32, 4.0f32);
    let mut pts = Vec::new();
    for &(u, v) in &QUARTER {
        pts.push([x0 + r - r * u, y0 + r - r * v]); // top-left, left -> top
    }
    for &(u, v) in &QUARTER {
        pts.push([x1 - r + r * v, y0 + r - r * u]); // top-right, top -> right
    }
    for &(u, v) in &QUARTER {
        pts.push([x1 - r + r * u, y1 - r + r * v]); // bottom-right, right -> bottom
    }
    for &(u, v) in &QUARTER {
        pts.push([x0 + r - r * v, y1 - r + r * u]); // bottom-left, bottom -> left
    }
    let mut p = FlattenedPath::new();
    p.push_contour(pts);
    p
}

fn golden_scene(path: &FlattenedPath, rule: FillRule) -> Surface<'static, Rgba8> {
    let mut s: Surface<'static, Rgba8> = Surface::new(32, 32, ColorSpaceId::Srgb).unwrap();
    rasterize_path(
        &mut s,
        path,
        &Affine2::IDENTITY,
        rule,
        &Paint::Solid([0.1, 0.8, 0.3, 1.0]),
    )
    .unwrap();
    s
}

#[test]
fn golden_circle_coverage() {
    let s = golden_scene(&golden_circle(), FillRule::NonZero);
    check("circle", &rgba8_bytes(&s), GOLDEN_CIRCLE);
}

#[test]
fn golden_rounded_rect_coverage() {
    let s = golden_scene(&golden_rounded_rect(), FillRule::NonZero);
    check("rounded rect", &rgba8_bytes(&s), GOLDEN_ROUNDED);
}

#[test]
fn golden_thin_line_coverage() {
    // A 0.3-pixel band at y = 10.4..10.7 and a 0.6-pixel band at x = 4.2..4.8. Both are subpixel.
    let mut p = FlattenedPath::new();
    p.push_contour(vec![[1.0, 10.4], [30.0, 10.4], [30.0, 10.7], [1.0, 10.7]]);
    p.push_contour(vec![[4.2, 1.0], [4.8, 1.0], [4.8, 30.0], [4.2, 30.0]]);
    let s = golden_scene(&p, FillRule::NonZero);
    check("thin lines", &rgba8_bytes(&s), GOLDEN_THIN);
}

const GOLDEN_CIRCLE: u64 = 0x7b6e_af7c_1d81_6e2d;
const GOLDEN_ROUNDED: u64 = 0x0354_f981_a0ee_cbf8;
const GOLDEN_THIN: u64 = 0x6955_1579_19de_7cc1;
