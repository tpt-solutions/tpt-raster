//! Filter tests: kernels, blur, convolution, edge detection, colour matrices and morphology.

use tpt_color::ColorSpaceId;
use tpt_raster::Surface;
use tpt_raster::filter::*;
use tpt_raster::pixel::{Gray8, PixelFormat, Rgba8, Rgba16F, Rgba32F};
use tpt_testkit::fuzz::Rng;

const EPS: f32 = 1e-4;

fn grid(w: u32, h: u32, f: impl Fn(u32, u32) -> Rgba32F) -> Surface<'static, Rgba32F> {
    let mut s = Surface::new(w, h, ColorSpaceId::Srgb).unwrap();
    for y in 0..h {
        for x in 0..w {
            s.set_pixel(x, y, f(x, y));
        }
    }
    s
}

fn px(r: f32, g: f32, b: f32, a: f32) -> Rgba32F {
    Rgba32F { r, g, b, a }
}

fn near(p: Rgba32F, want: [f32; 4]) -> bool {
    [p.r, p.g, p.b, p.a]
        .iter()
        .zip(&want)
        .all(|(a, b)| (a - b).abs() < EPS)
}

#[test]
fn gaussian_kernel_is_normalized_symmetric_and_sized() {
    let k = gaussian_kernel(1.0);
    assert_eq!(k.len(), 2 * 3 + 1);
    assert!((k.iter().sum::<f32>() - 1.0).abs() < 1e-5);
    for i in 0..k.len() / 2 {
        assert_eq!(k[i], k[k.len() - 1 - i]);
    }
    // The centre is the largest weight.
    assert!(k[3] > k[2]);
}

#[test]
fn degenerate_sigmas_give_identity_kernel() {
    for s in [0.0, -1.0, f32::NAN, f32::INFINITY, 1e-6] {
        assert_eq!(gaussian_kernel(s), vec![1.0], "sigma {s}");
    }
}

#[test]
fn huge_sigma_is_clamped_to_bound_kernel_length() {
    let k = gaussian_kernel(1.0e9);
    assert_eq!(k.len(), 2 * (3 * 256) + 1);
}

#[test]
fn blur_preserves_total_energy_away_from_edges() {
    let mut s = grid(9, 9, |x, y| {
        if (x, y) == (4, 4) {
            px(1.0, 1.0, 1.0, 1.0)
        } else {
            px(0.0, 0.0, 0.0, 0.0)
        }
    });
    gaussian_blur(&mut s, 1.0);
    // Straight colour is 1 wherever alpha is non-zero, so energy is conserved in alpha.
    let total: f32 = s.as_slice().iter().map(|p| p[3]).sum();
    assert!((total - 1.0).abs() < 1e-4, "total {total}");
    // The centre is the peak, and the blur is symmetric about it.
    assert!(s.get_pixel(4, 4).a > s.get_pixel(3, 4).a);
    assert!((s.get_pixel(3, 4).a - s.get_pixel(5, 4).a).abs() < EPS);
}

#[test]
fn blur_leaves_uniform_images_unchanged() {
    let mut s = grid(5, 4, |_, _| px(0.25, 0.5, 0.75, 1.0));
    gaussian_blur(&mut s, 2.0);
    assert!(
        s.as_slice()
            .iter()
            .all(|&p| near(Rgba32F::from_storage(p), [0.25, 0.5, 0.75, 1.0]))
    );
}

#[test]
fn blur_does_not_bleed_colour_out_of_transparent_pixels() {
    // Opaque red on the left, fully transparent on the right. Straight-alpha blur would tint the
    // transparent side with black. Premultiplied blur keeps its colour red.
    let mut s = grid(6, 1, |x, _| {
        if x < 2 {
            px(1.0, 0.0, 0.0, 1.0)
        } else {
            px(0.0, 0.0, 0.0, 0.0)
        }
    });
    gaussian_blur(&mut s, 1.0);
    let p = s.get_pixel(2, 0);
    assert!(p.a > 0.0 && p.a < 1.0, "alpha {}", p.a);
    assert!((p.r - 1.0).abs() < 1e-3, "red {}", p.r);
    assert!(p.g.abs() < 1e-3 && p.b.abs() < 1e-3);
}

#[test]
fn zero_sigma_blur_is_a_no_op() {
    let original = grid(3, 3, |x, y| px(x as f32 / 2.0, y as f32 / 2.0, 0.5, 1.0));
    let mut s = original.clone_for_test();
    gaussian_blur(&mut s, 0.0);
    assert_eq!(s.as_slice(), original.as_slice());
}

#[test]
fn identity_3x3_convolution_is_a_no_op() {
    let id = [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]];
    let original = grid(4, 3, |x, y| px(x as f32 / 4.0, y as f32 / 3.0, 0.1, 0.9));
    let mut s = original.clone_for_test();
    convolve_3x3_filter(&mut s, &id);
    for (a, b) in s.as_slice().iter().zip(original.as_slice()) {
        for k in 0..4 {
            assert!((a[k] - b[k]).abs() < EPS);
        }
    }
}

#[test]
fn sharpen_keeps_uniform_images_and_boosts_a_spike() {
    let mut flat = grid(3, 3, |_, _| px(0.5, 0.5, 0.5, 1.0));
    sharpen(&mut flat, 1.0);
    assert!(
        flat.as_slice()
            .iter()
            .all(|&p| near(Rgba32F::from_storage(p), [0.5, 0.5, 0.5, 1.0]))
    );

    let mut spike = grid(3, 3, |x, y| {
        if (x, y) == (1, 1) {
            px(0.5, 0.5, 0.5, 1.0)
        } else {
            px(0.25, 0.25, 0.25, 1.0)
        }
    });
    sharpen(&mut spike, 0.5);
    assert!(spike.get_pixel(1, 1).r > 0.5);
}

#[test]
fn sobel_is_zero_on_flat_input_and_fires_on_a_step() {
    let mut flat = grid(6, 3, |_, _| px(0.3, 0.3, 0.3, 1.0));
    sobel_edges(&mut flat);
    assert!(flat.as_slice().iter().all(|&p| p[0].abs() < EPS));

    // Dark left half, bright right half. The step sits between x=2 and x=3.
    let mut step = grid(6, 3, |x, _| {
        if x < 3 {
            px(0.0, 0.0, 0.0, 1.0)
        } else {
            px(1.0, 1.0, 1.0, 1.0)
        }
    });
    sobel_edges(&mut step);
    assert!((step.get_pixel(2, 1).r - 1.0).abs() < EPS);
    assert!((step.get_pixel(3, 1).r - 1.0).abs() < EPS);
    assert!(step.get_pixel(0, 1).r < EPS);
    assert!(step.get_pixel(5, 1).r < EPS);
    // Alpha is kept.
    assert!((step.get_pixel(2, 1).a - 1.0).abs() < EPS);
}

#[test]
fn color_matrix_identity_and_brightness() {
    let original = grid(2, 1, |x, _| {
        if x == 0 {
            px(0.8, 0.4, 0.2, 1.0)
        } else {
            px(0.1, 0.9, 0.5, 0.5)
        }
    });
    let mut s = original.clone_for_test();
    apply_color_matrix(&mut s, &identity_matrix());
    assert_eq!(s.as_slice(), original.as_slice());

    apply_color_matrix(&mut s, &brightness(0.5));
    assert!(near(s.get_pixel(0, 0), [0.4, 0.2, 0.1, 1.0]));
}

#[test]
fn color_matrix_clamps_out_of_range_results() {
    let mut s = grid(1, 1, |_, _| px(0.9, 0.9, 0.9, 1.0));
    apply_color_matrix(&mut s, &brightness(3.0));
    assert!(near(s.get_pixel(0, 0), [1.0, 1.0, 1.0, 1.0]));
}

#[test]
fn saturate_zero_gives_luma_grey_and_one_is_identity() {
    let mut s = grid(1, 1, |_, _| px(1.0, 0.0, 0.0, 1.0));
    apply_color_matrix(&mut s, &saturate(0.0));
    let p = s.get_pixel(0, 0);
    assert!((p.r - 0.213).abs() < EPS && (p.g - 0.213).abs() < EPS && (p.b - 0.213).abs() < EPS);

    let mut s = grid(1, 1, |_, _| px(0.2, 0.6, 0.9, 1.0));
    apply_color_matrix(&mut s, &saturate(1.0));
    assert!(near(s.get_pixel(0, 0), [0.2, 0.6, 0.9, 1.0]));
}

#[test]
fn hue_rotate_zero_and_full_turn_are_identity() {
    for deg in [0.0, 360.0] {
        let mut s = grid(1, 1, |_, _| px(0.2, 0.6, 0.9, 1.0));
        apply_color_matrix(&mut s, &hue_rotate(deg));
        assert!(near(s.get_pixel(0, 0), [0.2, 0.6, 0.9, 1.0]), "deg {deg}");
    }
}

#[test]
fn sepia_zero_is_identity_and_full_sepia_warms_white() {
    let mut s = grid(1, 1, |_, _| px(0.4, 0.5, 0.6, 1.0));
    apply_color_matrix(&mut s, &sepia(0.0));
    assert!(near(s.get_pixel(0, 0), [0.4, 0.5, 0.6, 1.0]));

    let mut s = grid(1, 1, |_, _| px(1.0, 1.0, 1.0, 1.0));
    apply_color_matrix(&mut s, &sepia(1.0));
    let p = s.get_pixel(0, 0);
    // Row sums: red and green clamp to 1, blue is 0.272 + 0.534 + 0.131 = 0.937.
    assert!((p.r - 1.0).abs() < EPS && (p.g - 1.0).abs() < EPS);
    assert!((p.b - 0.937).abs() < EPS, "blue {}", p.b);
}

#[test]
fn dilate_grows_and_erode_shrinks_a_single_pixel() {
    let dot = || {
        grid(5, 5, |x, y| {
            if (x, y) == (2, 2) {
                px(1.0, 1.0, 1.0, 1.0)
            } else {
                px(0.0, 0.0, 0.0, 0.0)
            }
        })
    };
    let count = |s: &Surface<'_, Rgba32F>| s.as_slice().iter().filter(|p| p[3] > 0.5).count();

    let mut s = dot();
    dilate(&mut s, 1);
    assert_eq!(count(&s), 9);

    let mut s = dot();
    erode(&mut s, 1);
    assert_eq!(count(&s), 0);
}

#[test]
fn open_removes_specks_and_close_fills_holes() {
    let mut speck = grid(5, 5, |x, y| {
        if (x, y) == (2, 2) {
            px(1.0, 1.0, 1.0, 1.0)
        } else {
            px(0.0, 0.0, 0.0, 0.0)
        }
    });
    open(&mut speck, 1);
    assert!(speck.as_slice().iter().all(|p| p[3] == 0.0));

    let mut holed = grid(5, 5, |x, y| {
        if (x, y) == (2, 2) {
            px(0.0, 0.0, 0.0, 0.0)
        } else {
            px(1.0, 1.0, 1.0, 1.0)
        }
    });
    close(&mut holed, 1);
    assert!(holed.as_slice().iter().all(|p| p[3] == 1.0));
}

#[test]
fn huge_morphology_radius_is_capped_not_slow() {
    let mut s = grid(3, 3, |_, _| px(1.0, 1.0, 1.0, 1.0));
    dilate(&mut s, u32::MAX);
    assert!(s.as_slice().iter().all(|p| p[3] == 1.0));
}

#[test]
fn filters_work_on_every_pixel_format() {
    let mut g: Surface<'static, Gray8> = Surface::new(4, 4, ColorSpaceId::Srgb).unwrap();
    g.clear(Gray8(128));
    gaussian_blur(&mut g, 1.5);
    assert!(g.as_slice().iter().all(|&v| v == 128));

    let mut a: Surface<'static, Rgba8> = Surface::new(4, 4, ColorSpaceId::Srgb).unwrap();
    a.clear(Rgba8 {
        r: 10,
        g: 20,
        b: 30,
        a: 255,
    });
    gaussian_blur(&mut a, 1.0);
    assert_eq!(
        a.get_pixel(1, 1),
        Rgba8 {
            r: 10,
            g: 20,
            b: 30,
            a: 255
        }
    );

    let mut h: Surface<'static, Rgba16F> = Surface::new(4, 4, ColorSpaceId::Srgb).unwrap();
    h.clear(Rgba16F::from_f32([0.5, 0.5, 0.5, 1.0]));
    sobel_edges(&mut h);
    assert!(
        h.as_slice()
            .iter()
            .all(|&p| Rgba16F::from_storage(p).r.to_f32() < 1e-3)
    );
}

#[test]
fn fuzz_filters_never_panic_and_stay_in_range() {
    let mut rng = Rng::new(2024);
    for _ in 0..400 {
        let (w, h) = (rng.below(9) as u32, rng.below(9) as u32);
        let mut s: Surface<'static, Rgba32F> = Surface::new(w, h, ColorSpaceId::Srgb).unwrap();
        for y in 0..h {
            for x in 0..w {
                let v = |r: &mut Rng| (r.below(1001) as f32) / 1000.0;
                let a = v(&mut rng);
                s.set_pixel(
                    x,
                    y,
                    px(v(&mut rng) * a, v(&mut rng) * a, v(&mut rng) * a, a),
                );
            }
        }
        let sigma = [0.0, 0.5, 2.0, f32::NAN, -3.0, 1.0e6][rng.below(6) as usize];
        gaussian_blur(&mut s, sigma);
        sharpen(&mut s, rng.below(5) as f32);
        sobel_edges(&mut s);
        let r = rng.below(20) as u32;
        dilate(&mut s, r);
        erode(&mut s, r);
        open(&mut s, r);
        close(&mut s, r);
        apply_color_matrix(&mut s, &hue_rotate(rng.below(720) as f32));
        convolve_separable(&mut s, &[0.25, 0.5, 0.25], &[1.0]);
        for p in s.as_slice() {
            assert!(
                p.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)),
                "{p:?}"
            );
        }
    }
}

/// Copies a surface so a test can compare before and after.
trait CloneForTest {
    fn clone_for_test(&self) -> Surface<'static, Rgba32F>;
}

impl CloneForTest for Surface<'_, Rgba32F> {
    fn clone_for_test(&self) -> Surface<'static, Rgba32F> {
        let mut out = Surface::new(self.width(), self.height(), self.color_space()).unwrap();
        for y in 0..self.height() {
            for x in 0..self.width() {
                out.set_pixel(x, y, self.get_pixel(x, y));
            }
        }
        out
    }
}
