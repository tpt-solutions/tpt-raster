//! SIMD versus scalar equivalence. Only built with the `simd` feature.

#![cfg(feature = "simd")]

use tpt_color::{ColorSpaceId, premultiply, unpremultiply};
use tpt_raster::Surface;

use tpt_raster::composite::{BlendMode, SourceOver, composite_surface};
use tpt_raster::pixel::{Gray8, PixelFormat, Rgba8, Rgba16F, Rgba32F};
use tpt_raster::round::{round_to_u8, unit_to_u8};
use tpt_raster::simd::{round_to_u8_x8, unit_to_u8_x8};
use tpt_testkit::fuzz::Rng;

const SPECIALS: [f32; 16] = [
    0.0,
    -0.0,
    0.5,
    1.5,
    254.5,
    255.0,
    255.4,
    -1.0,
    f32::NAN,
    -f32::NAN,
    f32::INFINITY,
    f32::NEG_INFINITY,
    f32::MIN_POSITIVE,
    1.0e-40,
    1.0e30,
    -1.0e-30,
];

#[test]
fn round_to_u8_simd_matches_scalar_on_specials() {
    for chunk in SPECIALS.chunks(8) {
        let mut lanes = [0.0f32; 8];
        lanes[..chunk.len()].copy_from_slice(chunk);
        let simd = round_to_u8_x8(lanes);
        for i in 0..chunk.len() {
            assert_eq!(simd[i], round_to_u8(lanes[i]), "lane value {}", lanes[i]);
        }
    }
}

#[test]
fn round_to_u8_simd_matches_scalar_on_random_bit_patterns() {
    let mut rng = Rng::new(0xABCDEF);
    for _ in 0..200_000 {
        let lanes: [f32; 8] = core::array::from_fn(|_| rng.next_f32_bits());
        let simd = round_to_u8_x8(lanes);
        for i in 0..8 {
            assert_eq!(
                simd[i],
                round_to_u8(lanes[i]),
                "bits {:#010x}",
                lanes[i].to_bits()
            );
        }
    }
}

#[test]
fn unit_to_u8_simd_matches_scalar() {
    let mut rng = Rng::new(7);
    for _ in 0..50_000 {
        let lanes: [f32; 8] = core::array::from_fn(|_| rng.below(2001) as f32 / 1000.0 - 0.5);
        let simd = unit_to_u8_x8(lanes);
        for i in 0..8 {
            assert_eq!(simd[i], unit_to_u8(lanes[i]));
        }
    }
}

/// Scalar reference for `composite_surface` with SourceOver. Written out per pixel, so it does
/// not share code with the SIMD kernel.
fn reference_composite<D: PixelFormat, S: PixelFormat>(
    dst: &mut Surface<'_, D>,
    src: &Surface<'_, S>,
    opacity: f32,
) {
    let opacity = if opacity.is_nan() {
        0.0
    } else {
        opacity.clamp(0.0, 1.0)
    };
    for y in 0..dst.height() {
        for x in 0..dst.width() {
            let s = premultiply(src.get_pixel(x, y).to_f32()).map(|c| c * opacity);
            let d = premultiply(dst.get_pixel(x, y).to_f32());
            let out = unpremultiply(SourceOver::blend(s, d));
            dst.set_pixel(x, y, D::from_f32(out));
        }
    }
}

fn random_surface<P: PixelFormat + 'static>(
    rng: &mut Rng,
    w: u32,
    h: u32,
    make: impl Fn(f32, f32, f32, f32) -> P,
) -> Surface<'static, P> {
    let mut s = Surface::new(w, h, ColorSpaceId::Srgb).unwrap();
    for y in 0..h {
        for x in 0..w {
            let mut v = || rng.below(1001) as f32 / 1000.0;
            let a = v();
            let c = [v() * a, v() * a, v() * a];
            s.set_pixel(x, y, make(c[0], c[1], c[2], a));
        }
    }
    s
}

fn storage_bits<P: PixelFormat>(s: &Surface<'_, P>) -> Vec<Vec<u32>>
where
    P::Storage: Copy,
{
    // Compare through the pixel's f32 widening, bit for bit, so f32 differences show up.
    (0..s.height())
        .flat_map(|y| (0..s.width()).map(move |x| (x, y)))
        .map(|(x, y)| {
            s.get_pixel(x, y)
                .to_f32()
                .iter()
                .map(|c| c.to_bits())
                .collect()
        })
        .collect()
}

fn check_composite<D: PixelFormat + 'static, S: PixelFormat + 'static>(
    seed: u64,
    make_d: impl Fn(f32, f32, f32, f32) -> D,
    make_s: impl Fn(f32, f32, f32, f32) -> S,
) {
    let mut rng = Rng::new(seed);
    for case in 0..60 {
        // Widths 1..=20 cover the 8-lane body and the scalar tail, and mixed sizes.
        let w = 1 + rng.below(20) as u32;
        let h = 1 + rng.below(4) as u32;
        let dst = random_surface(&mut rng, w, h, &make_d);
        let src = random_surface(&mut rng, w, h, &make_s);
        let opacity = [1.0, 0.5, 0.0, 2.0, f32::NAN, 0.333][case % 6];

        let mut slow = dst.clone_pixels();
        reference_composite(&mut slow, &src, opacity);
        let expected = storage_bits(&slow);

        // The SIMD kernel, called directly so every format is covered, whatever dispatch picks.
        let clamped = if opacity.is_nan() {
            0.0
        } else {
            opacity.clamp(0.0, 1.0)
        };
        let mut kernel = dst.clone_pixels();
        tpt_raster::simd::composite_source_over(&mut kernel, &src, clamped);
        assert_eq!(
            storage_bits(&kernel),
            expected,
            "kernel: case {case} w={w} h={h} opacity={opacity}"
        );

        // The public entry point, which dispatches to the kernel only for lane-path formats.
        let mut dispatched = dst.clone_pixels();
        composite_surface(&mut dispatched, &src, opacity, SourceOver).unwrap();
        assert_eq!(
            storage_bits(&dispatched),
            expected,
            "dispatch: case {case} w={w} h={h} opacity={opacity}"
        );
    }
}

trait ClonePixels<P: PixelFormat> {
    fn clone_pixels(&self) -> Surface<'static, P>;
}

impl<P: PixelFormat + 'static> ClonePixels<P> for Surface<'_, P> {
    fn clone_pixels(&self) -> Surface<'static, P> {
        let mut out = Surface::new(self.width(), self.height(), self.color_space()).unwrap();
        for y in 0..self.height() {
            for x in 0..self.width() {
                out.set_pixel(x, y, self.get_pixel(x, y));
            }
        }
        out
    }
}

#[test]
fn simd_composite_matches_reference_rgba8() {
    check_composite(
        1,
        |r, g, b, a| Rgba8::from_f32([r, g, b, a]),
        |r, g, b, a| Rgba8::from_f32([r, g, b, a]),
    );
}

#[test]
fn simd_composite_matches_reference_rgba32f() {
    check_composite(
        2,
        |r, g, b, a| Rgba32F { r, g, b, a },
        |r, g, b, a| Rgba32F { r, g, b, a },
    );
}

#[test]
fn simd_composite_matches_reference_mixed_formats() {
    check_composite(
        3,
        |r, g, b, a| Rgba8::from_f32([r, g, b, a]),
        |r, g, b, a| Rgba32F { r, g, b, a },
    );
    check_composite(
        4,
        |r, g, b, a| Rgba16F::from_f32([r, g, b, a]),
        |r, g, b, a| Rgba8::from_f32([r, g, b, a]),
    );
    check_composite(
        5,
        |_, _, _, _| Gray8(100),
        |r, g, b, a| Rgba32F { r, g, b, a },
    );
}

/// Best of `rounds` timings, to reduce noise from other load on the machine.
fn best_of(rounds: u32, iters: u32, mut f: impl FnMut()) -> std::time::Duration {
    (0..rounds)
        .map(|_| tpt_testkit::bench::time_iters(iters, &mut f))
        .min()
        .unwrap()
}

/// Timing comparison. Run with:
/// `cargo test -p tpt-raster --release --features simd --test simd -- --ignored --nocapture`
#[test]
#[ignore = "benchmark: run in release with --nocapture"]
fn bench_simd_composite_against_scalar() {
    let mut rng = Rng::new(99);
    let (rounds, iters) = (5, 10);
    let mpix = 1024.0 * 1024.0 * iters as f64 / 1.0e6;

    // Rgba32F: no lane override, so the default per-pixel load and store.
    let dst = random_surface(&mut rng, 1024, 1024, |r, g, b, a| Rgba32F { r, g, b, a });
    let src = random_surface(&mut rng, 1024, 1024, |r, g, b, a| Rgba32F { r, g, b, a });
    report("Rgba32F", mpix, &dst, &src, rounds, iters);

    // Rgba8: has the lane override, so load and store run in lanes.
    let dst8 = random_surface(&mut rng, 1024, 1024, |r, g, b, a| {
        Rgba8::from_f32([r, g, b, a])
    });
    let src8 = random_surface(&mut rng, 1024, 1024, |r, g, b, a| {
        Rgba8::from_f32([r, g, b, a])
    });
    report("Rgba8", mpix, &dst8, &src8, rounds, iters);
}

fn report<P: PixelFormat + 'static>(
    name: &str,
    mpix: f64,
    dst: &Surface<'_, P>,
    src: &Surface<'_, P>,
    rounds: u32,
    iters: u32,
) {
    let mut a = dst.clone_pixels();
    let simd = best_of(rounds, iters, || {
        composite_surface(&mut a, src, 1.0, SourceOver).unwrap()
    });
    let mut b = dst.clone_pixels();
    let scalar = best_of(rounds, iters, || reference_composite(&mut b, src, 1.0));
    println!(
        "{name} 1024x1024 x{iters} (best of {rounds}): dispatch {:.1} Mpx/s, scalar reference {:.1} Mpx/s, ratio {:.2}x",
        mpix / simd.as_secs_f64(),
        mpix / scalar.as_secs_f64(),
        scalar.as_secs_f64() / simd.as_secs_f64(),
    );
}
