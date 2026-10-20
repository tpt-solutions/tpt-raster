//! Path rasterization tests: coverage values, fill rules, clipping, robustness and paints.

use tpt_canvas::{Affine2, FillRule, FlattenedPath, Paint};
use tpt_color::ColorSpaceId;
use tpt_raster::Surface;
use tpt_raster::pixel::{Rgba8, Rgba32F};
use tpt_raster::rasterize::{coverage_mask, rasterize_path};
use tpt_testkit::fuzz::Rng;
use tpt_testkit::golden::assert_bytes_eq;

const EPS: f32 = 1e-5;

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<[f32; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn path(contours: Vec<Vec<[f32; 2]>>) -> FlattenedPath {
    let mut p = FlattenedPath::new();
    for c in contours {
        p.push_contour(c);
    }
    p
}

fn mask(w: u32, h: u32, p: &FlattenedPath, rule: FillRule) -> Vec<Vec<f32>> {
    let m = coverage_mask(w, h, p, &Affine2::IDENTITY, rule).unwrap();
    (0..h)
        .map(|y| (0..w).map(|x| m.get(x, y)).collect())
        .collect()
}

fn total(m: &[Vec<f32>]) -> f64 {
    m.iter().flatten().map(|&c| c as f64).sum()
}

#[test]
fn pixel_aligned_square_is_fully_covered() {
    let m = mask(
        4,
        4,
        &path(vec![rect(1.0, 1.0, 3.0, 3.0)]),
        FillRule::NonZero,
    );
    for (y, row) in m.iter().enumerate() {
        for (x, &c) in row.iter().enumerate() {
            let inside = (1..3).contains(&x) && (1..3).contains(&y);
            let want = if inside { 1.0 } else { 0.0 };
            assert!((c - want).abs() < EPS, "({x},{y}) = {c}");
        }
    }
}

#[test]
fn half_pixel_edges_give_half_coverage() {
    // Spans x 0.5..2.5 and y 0..1. Columns 0 and 2 are half covered, column 1 is full.
    let m = mask(
        4,
        1,
        &path(vec![rect(0.5, 0.0, 2.5, 1.0)]),
        FillRule::NonZero,
    );
    let want = [0.5, 1.0, 0.5, 0.0];
    for (x, (&got, &w)) in m[0].iter().zip(&want).enumerate() {
        assert!((got - w).abs() < EPS, "x={x}: {got} != {w}");
    }
}

#[test]
fn diagonal_triangle_has_exact_area() {
    // Triangle (0,0),(4,0),(0,4) has area 8. Its hypotenuse crosses pixels diagonally.
    let m = mask(
        4,
        4,
        &path(vec![vec![[0.0, 0.0], [4.0, 0.0], [0.0, 4.0]]]),
        FillRule::NonZero,
    );
    assert!((total(&m) - 8.0).abs() < 1e-4, "area {}", total(&m));
    // The corner pixel (0,0) is fully inside and the far corner (3,3) is fully outside.
    assert!((m[0][0] - 1.0).abs() < EPS);
    assert!(m[3][3].abs() < EPS);
}

#[test]
fn random_simple_polygons_conserve_area() {
    // Star-shaped polygons around a centre are always simple, so the coverage sum must equal
    // the shoelace area. Coordinates stay inside the surface.
    let mut rng = Rng::new(0xC0FFEE);
    for case in 0..300 {
        let n = 3 + rng.below(12) as usize;
        let mut pts = Vec::with_capacity(n);
        for k in 0..n {
            let angle = std::f32::consts::TAU * (k as f32) / (n as f32);
            let r = 2.0 + (rng.below(800) as f32) / 100.0; // 2..10
            pts.push([12.0 + r * angle.cos(), 12.0 + r * angle.sin()]);
        }
        let mut area2 = 0.0f64;
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            area2 += a[0] as f64 * b[1] as f64 - b[0] as f64 * a[1] as f64;
        }
        let area = (area2 / 2.0).abs();
        let m = mask(24, 24, &path(vec![pts]), FillRule::NonZero);
        let got = total(&m);
        assert!(
            (got - area).abs() < 1e-3 * area.max(1.0),
            "case {case}: {got} vs {area}"
        );
    }
}

#[test]
fn same_direction_nesting_nonzero_fills_evenodd_hollows() {
    let outer = rect(0.0, 0.0, 4.0, 4.0);
    let inner = rect(1.0, 1.0, 3.0, 3.0);
    let p = path(vec![outer, inner]);

    let nz = mask(4, 4, &p, FillRule::NonZero);
    assert!((nz[1][1] - 1.0).abs() < EPS, "nonzero inner = {}", nz[1][1]);

    let eo = mask(4, 4, &p, FillRule::EvenOdd);
    assert!(eo[1][1].abs() < EPS, "evenodd inner = {}", eo[1][1]);
    assert!((eo[0][0] - 1.0).abs() < EPS);
}

#[test]
fn opposite_direction_nesting_hollows_under_both_rules() {
    let outer = rect(0.0, 0.0, 4.0, 4.0);
    let mut inner = rect(1.0, 1.0, 3.0, 3.0);
    inner.reverse();
    let p = path(vec![outer, inner]);
    for rule in [FillRule::NonZero, FillRule::EvenOdd] {
        let m = mask(4, 4, &p, rule);
        assert!(m[1][1].abs() < EPS, "{rule:?} inner = {}", m[1][1]);
        assert!((m[0][0] - 1.0).abs() < EPS, "{rule:?} ring = {}", m[0][0]);
    }
}

#[test]
fn fully_offscreen_path_covers_nothing() {
    let m = mask(
        4,
        4,
        &path(vec![rect(-10.0, -10.0, -5.0, -5.0)]),
        FillRule::NonZero,
    );
    assert!(m.iter().flatten().all(|&c| c == 0.0));
    let m = mask(
        4,
        4,
        &path(vec![rect(10.0, 0.0, 20.0, 4.0)]),
        FillRule::NonZero,
    );
    assert!(m.iter().flatten().all(|&c| c == 0.0));
}

#[test]
fn path_straddling_the_left_edge_is_clipped_not_dropped() {
    // x from -2 to 1.5: columns 0 is full, column 1 is half.
    let m = mask(
        4,
        1,
        &path(vec![rect(-2.0, 0.0, 1.5, 1.0)]),
        FillRule::NonZero,
    );
    assert!((m[0][0] - 1.0).abs() < EPS);
    assert!((m[0][1] - 0.5).abs() < EPS);
    assert!(m[0][2].abs() < EPS);
}

#[test]
fn path_straddling_top_and_bottom_is_clipped() {
    let m = mask(
        2,
        2,
        &path(vec![rect(0.0, -3.0, 2.0, 10.0)]),
        FillRule::NonZero,
    );
    assert!(m.iter().flatten().all(|&c| (c - 1.0).abs() < EPS));
}

#[test]
fn huge_coordinates_do_not_overflow_or_panic() {
    let big = rect(-1.0e20, -1.0e20, 1.0e20, 1.0e20);
    let m = mask(3, 3, &path(vec![big]), FillRule::NonZero);
    assert!(m.iter().flatten().all(|&c| (c - 1.0).abs() < EPS));
}

#[test]
fn non_finite_coordinates_are_dropped() {
    let nan = vec![[f32::NAN, 0.0], [2.0, 0.0], [2.0, 2.0]];
    let inf = vec![[0.0, 0.0], [f32::INFINITY, 0.0], [2.0, 2.0]];
    let m = mask(3, 3, &path(vec![nan, inf]), FillRule::NonZero);
    assert!(
        m.iter()
            .flatten()
            .all(|c| c.is_finite() && (0.0..=1.0).contains(c))
    );
}

#[test]
fn zero_size_surface_and_degenerate_contours_are_safe() {
    let p = path(vec![
        vec![],
        vec![[1.0, 1.0]],
        rect(0.0, 0.0, 0.0, 0.0),
        rect(1.0, 1.0, 1.0, 9.0),
    ]);
    let m = coverage_mask(0, 0, &p, &Affine2::IDENTITY, FillRule::NonZero).unwrap();
    assert!(m.as_slice().is_empty());
    let m = coverage_mask(3, 3, &p, &Affine2::IDENTITY, FillRule::EvenOdd).unwrap();
    assert!(m.as_slice().iter().all(|&c| c == 0.0));
}

#[test]
fn fuzz_random_paths_never_panic_and_stay_in_range() {
    let mut rng = Rng::new(42);
    for _ in 0..5_000 {
        let w = rng.below(12) as u32;
        let h = rng.below(12) as u32;
        let mut contours = Vec::new();
        for _ in 0..(1 + rng.below(4)) {
            let n = rng.below(7) as usize;
            let c: Vec<[f32; 2]> = (0..n)
                .map(|_| {
                    // Mix ordinary values, huge values, NaN and infinity.
                    let pick = |r: &mut Rng| match r.below(4) {
                        0 => r.next_f32_bits(),
                        1 => (r.below(4000) as f32) / 10.0 - 100.0,
                        2 => (r.below(3) as f32) * 1.0e30,
                        _ => (r.below(2000) as f32) / 100.0 - 5.0,
                    };
                    [pick(&mut rng), pick(&mut rng)]
                })
                .collect();
            contours.push(c);
        }
        let t = Affine2 {
            m: [
                rng.next_f32_bits().clamp(-1e3, 1e3),
                0.0,
                0.0,
                1.0,
                rng.below(20) as f32,
                0.0,
            ],
        };
        let rule = if rng.below(2) == 0 {
            FillRule::NonZero
        } else {
            FillRule::EvenOdd
        };
        let m = coverage_mask(w, h, &path(contours), &t, rule).unwrap();
        if let Some(bad) = m
            .as_slice()
            .iter()
            .find(|c| !(c.is_finite() && (0.0..=1.0).contains(*c)))
        {
            panic!("bad {bad} w={w} h={h} t={:?} rule={rule:?}", t.m);
        }
    }
}

#[test]
fn solid_fill_composites_onto_surface_with_coverage() {
    // Half-covered pixel with opaque red paint over opaque blue gives a 50/50 mix.
    let mut s: Surface<'static, Rgba8> = Surface::new(2, 1, ColorSpaceId::Srgb).unwrap();
    s.clear(Rgba8 {
        r: 0,
        g: 0,
        b: 255,
        a: 255,
    });
    let p = path(vec![rect(0.0, 0.0, 0.5, 1.0)]);
    let paint = Paint::Solid([1.0, 0.0, 0.0, 1.0]);
    rasterize_path(&mut s, &p, &Affine2::IDENTITY, FillRule::NonZero, &paint).unwrap();
    assert_eq!(
        s.get_pixel(0, 0),
        Rgba8 {
            r: 128,
            g: 0,
            b: 128,
            a: 255
        }
    );
    assert_eq!(
        s.get_pixel(1, 0),
        Rgba8 {
            r: 0,
            g: 0,
            b: 255,
            a: 255
        }
    );
}

#[test]
fn golden_bytes_for_a_filled_triangle() {
    let mut s: Surface<'static, Rgba8> = Surface::new(3, 3, ColorSpaceId::Srgb).unwrap();
    let p = path(vec![vec![[0.0, 0.0], [3.0, 0.0], [0.0, 3.0]]]);
    let paint = Paint::Solid([0.0, 1.0, 0.0, 1.0]);
    rasterize_path(&mut s, &p, &Affine2::IDENTITY, FillRule::NonZero, &paint).unwrap();
    // Full coverage inside the triangle, half on the hypotenuse, none outside. Surface starts
    // transparent, so a half-covered pixel is green at alpha 128 (unpremultiplied green stays 1).
    let bytes: Vec<u8> = s.as_slice().iter().flatten().copied().collect();
    let full = [0u8, 255, 0, 255];
    let half = [0u8, 255, 0, 128];
    let none = [0u8; 4];
    let want: Vec<u8> = [
        full, full, half, // row 0: x+y <= 3 for x in 0..=1 is full, cell (2,0) is half
        full, half, none, // row 1
        half, none, none, // row 2
    ]
    .concat();
    assert_bytes_eq(&bytes, &want);
}

#[test]
fn linear_gradient_fills_with_interpolated_color() {
    let mut s: Surface<'static, Rgba32F> = Surface::new(3, 1, ColorSpaceId::Srgb).unwrap();
    let p = path(vec![rect(0.0, 0.0, 3.0, 1.0)]);
    let paint = Paint::Linear {
        start: [0.0, 0.0],
        end: [3.0, 0.0],
        from: [0.0, 0.0, 0.0, 1.0],
        to: [1.0, 1.0, 1.0, 1.0],
    };
    rasterize_path(&mut s, &p, &Affine2::IDENTITY, FillRule::NonZero, &paint).unwrap();
    // Pixel centres at x = 0.5, 1.5, 2.5 give t = 1/6, 1/2, 5/6.
    let r: Vec<f32> = (0..3).map(|x| s.get_pixel(x, 0).r).collect();
    assert!((r[0] - 1.0 / 6.0).abs() < EPS);
    assert!((r[1] - 0.5).abs() < EPS);
    assert!((r[2] - 5.0 / 6.0).abs() < EPS);
}

#[test]
fn radial_gradient_is_from_at_centre_and_to_beyond_radius() {
    let paint = Paint::Radial {
        center: [0.0, 0.0],
        radius: 2.0,
        from: [1.0, 0.0, 0.0, 1.0],
        to: [0.0, 0.0, 1.0, 1.0],
    };
    assert_eq!(paint.color_at(0.0, 0.0), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(paint.color_at(10.0, 0.0), [0.0, 0.0, 1.0, 1.0]);
}

/// A regular polygon approximating a circle, centred on `(cx, cy)`.
fn circle(cx: f32, cy: f32, r: f32, n: usize) -> Vec<[f32; 2]> {
    (0..n)
        .map(|k| {
            let a = std::f32::consts::TAU * k as f32 / n as f32;
            [cx + r * a.cos(), cy + r * a.sin()]
        })
        .collect()
}

/// A pentagram: the five points joined in steps of two, so the path crosses itself.
fn pentagram(cx: f32, cy: f32, r: f32) -> Vec<[f32; 2]> {
    (0..5)
        .map(|k| {
            let a = std::f32::consts::TAU * (2 * k) as f32 / 5.0 - std::f32::consts::FRAC_PI_2;
            [cx + r * a.cos(), cy + r * a.sin()]
        })
        .collect()
}

fn shoelace(p: &[[f32; 2]]) -> f64 {
    let n = p.len();
    let twice: f64 = (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            a[0] as f64 * b[1] as f64 - b[0] as f64 * a[1] as f64
        })
        .sum();
    (twice / 2.0).abs()
}

#[test]
fn circle_coverage_matches_its_polygon_area() {
    let c = circle(16.0, 16.0, 12.0, 96);
    let m = mask(32, 32, &path(vec![c.clone()]), FillRule::NonZero);
    let got = total(&m);
    let want = shoelace(&c);
    assert!((got - want).abs() < 1e-3 * want, "{got} vs {want}");
    // The circle's true area is pi r^2; 96 sides come within 0.1% of it.
    let pi_r2 = std::f64::consts::PI * 144.0;
    assert!((got - pi_r2).abs() / pi_r2 < 0.002, "{got} vs {pi_r2}");
}

#[test]
fn pentagram_fill_rules_differ_in_the_self_overlap() {
    // A pentagram's central pentagon is winding 2, so NonZero fills it and EvenOdd leaves a hole.
    let s = pentagram(16.0, 16.0, 14.0);
    let p = path(vec![s]);
    let nz = total(&mask(32, 32, &p, FillRule::NonZero));
    let eo = total(&mask(32, 32, &p, FillRule::EvenOdd));
    assert!(nz > eo, "nonzero {nz} should exceed evenodd {eo}");
    assert!(eo > 0.0);
}

#[test]
#[ignore = "long-running fuzz: cargo test -p tpt-raster --test rasterize -- --ignored"]
fn long_fuzz_random_paths_never_panic() {
    let mut rng = Rng::new(0xDEAD_BEEF);
    for _ in 0..2_000_000 {
        let w = rng.below(24) as u32;
        let h = rng.below(24) as u32;
        let n = rng.below(9) as usize;
        let pts: Vec<[f32; 2]> = (0..n)
            .map(|_| [rng.next_f32_bits(), rng.next_f32_bits()])
            .collect();
        let rule = if rng.below(2) == 0 {
            FillRule::NonZero
        } else {
            FillRule::EvenOdd
        };
        let m = coverage_mask(w, h, &path(vec![pts]), &Affine2::IDENTITY, rule).unwrap();
        assert!(
            m.as_slice()
                .iter()
                .all(|c| c.is_finite() && (0.0..=1.0).contains(c))
        );
    }
}

/// A rectangle with quarter-circle corners of radius `r`, each arc split into `seg` steps.
fn rounded_rect(x0: f32, y0: f32, x1: f32, y1: f32, r: f32, seg: usize) -> Vec<[f32; 2]> {
    use std::f32::consts::{FRAC_PI_2, PI};
    let corners = [
        (x0 + r, y0 + r, PI),
        (x1 - r, y0 + r, PI + FRAC_PI_2),
        (x1 - r, y1 - r, 0.0),
        (x0 + r, y1 - r, FRAC_PI_2),
    ];
    let mut pts = Vec::with_capacity(4 * (seg + 1));
    for (cx, cy, start) in corners {
        for k in 0..=seg {
            let a = start + FRAC_PI_2 * k as f32 / seg as f32;
            pts.push([cx + r * a.cos(), cy + r * a.sin()]);
        }
    }
    pts
}

#[test]
fn rounded_rect_area_matches_the_analytic_shape() {
    // Area is w*h minus the four corner squares' missing quarter discs: (4 - pi) r^2.
    let (w, h, r) = (20.0f64, 12.0f64, 4.0f64);
    let p = rounded_rect(2.0, 2.0, 22.0, 14.0, r as f32, 32);
    let m = mask(24, 16, &path(vec![p]), FillRule::NonZero);
    let want = w * h - (4.0 - std::f64::consts::PI) * r * r;
    let got = total(&m);
    assert!((got - want).abs() / want < 2e-3, "{got} vs {want}");
    // The sharp corner pixel is outside the arc; the pixel just inside the middle of an edge is full.
    assert!(m[2][2] < 0.01, "corner pixel {}", m[2][2]);
    assert!((m[8][12] - 1.0).abs() < EPS, "edge pixel {}", m[8][12]);
}

#[test]
fn thin_horizontal_line_covers_exactly_its_width() {
    // A 0.25-pixel-tall band across row 1 gives 0.25 coverage along the whole row, and nothing else.
    let m = mask(
        4,
        4,
        &path(vec![rect(0.0, 1.2, 4.0, 1.45)]),
        FillRule::NonZero,
    );
    for (y, row) in m.iter().enumerate() {
        for (x, &c) in row.iter().enumerate() {
            let want = if y == 1 { 0.25 } else { 0.0 };
            assert!((c - want).abs() < EPS, "({x},{y}) = {c}");
        }
    }
}

#[test]
fn thin_diagonal_line_has_its_geometric_area() {
    // A 0.5-pixel-thick segment at 45 degrees, length 8*sqrt(2), lies inside the surface.
    // Its area is length * thickness, with the perpendicular offset of 0.25 on each side.
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let d = 0.25f32;
    let (ox, oy) = (d * s, -d * s);
    let (a, b) = ([1.0, 1.0], [9.0, 9.0]);
    let poly = vec![
        [a[0] + ox, a[1] + oy],
        [b[0] + ox, b[1] + oy],
        [b[0] - ox, b[1] - oy],
        [a[0] - ox, a[1] - oy],
    ];
    let m = mask(12, 12, &path(vec![poly.clone()]), FillRule::NonZero);
    let want = shoelace(&poly);
    let got = total(&m);
    assert!((got - want).abs() < 1e-3 * want, "{got} vs {want}");
    assert!((want - 8.0 * 2f64.sqrt() * 0.5).abs() < 1e-3);
}

#[test]
fn five_point_star_coverage_matches_its_polygon_area() {
    // A simple (non-self-intersecting) ten-vertex star: outer radius 14, inner radius 6.
    let mut star = Vec::new();
    for k in 0..10 {
        let r = if k % 2 == 0 { 14.0f32 } else { 6.0 };
        let a = std::f32::consts::TAU * k as f32 / 10.0 - std::f32::consts::FRAC_PI_2;
        star.push([16.0 + r * a.cos(), 16.0 + r * a.sin()]);
    }
    let want = shoelace(&star);
    let got = total(&mask(32, 32, &path(vec![star]), FillRule::NonZero));
    assert!((got - want).abs() < 1e-3 * want, "{got} vs {want}");
}
