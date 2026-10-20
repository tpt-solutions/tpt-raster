//! Stride fuzzing. Padding between rows must never be read into results or written to, and a
//! strided surface must give the same pixels as a tightly packed copy of it.

use tpt_canvas::{Affine2, FillRule, FlattenedPath, Paint};
use tpt_color::ColorSpaceId;
use tpt_raster::Surface;
use tpt_raster::composite::{SourceOver, composite_surface};
use tpt_raster::filter::{apply_color_matrix, gaussian_blur, hue_rotate, open, sharpen};
use tpt_raster::pixel::{Rgba8, Rgba32F};
use tpt_raster::rasterize::rasterize_path;
use tpt_raster::sample::{EdgeMode, Interp, draw_image};
use tpt_testkit::fuzz::Rng;

/// Fills padding cells before a test, so any stray write shows up as a changed value.
const PAD_F32: [f32; 4] = [-7.0; 4];
const PAD_U8: [u8; 4] = [9, 8, 7, 6];

/// Row-major indices of the padding cells: for each row, columns `w..stride`.
fn pad_indices(w: u32, h: u32, stride: u32) -> Vec<usize> {
    // The buffer ends after the last row's pixels, so the last row has no trailing padding.
    let len = if h == 0 {
        0
    } else {
        ((h - 1) * stride + w) as usize
    };
    (0..h)
        .flat_map(|y| (w..stride).map(move |x| (y * stride + x) as usize))
        .filter(|&i| i < len)
        .collect()
}

/// The `w × h` pixels of a strided buffer, row by row, without padding.
fn unpadded<T: Copy>(buf: &[T], w: u32, h: u32, stride: u32) -> Vec<T> {
    (0..h)
        .flat_map(|y| (0..w).map(move |x| buf[(y * stride + x) as usize]))
        .collect()
}

/// A strided buffer of random pixels. Padding is set to `pad`.
fn random_f32(rng: &mut Rng, w: u32, h: u32, stride: u32) -> Vec<[f32; 4]> {
    let len = if h == 0 {
        0
    } else {
        (stride * (h - 1) + w) as usize
    };
    let mut buf = vec![PAD_F32; len];
    for y in 0..h {
        for x in 0..w {
            let a = rng.below(1001) as f32 / 1000.0;
            let mut c = || rng.below(1001) as f32 / 1000.0 * a;
            buf[(y * stride + x) as usize] = [c(), c(), c(), a];
        }
    }
    buf
}

fn random_u8(rng: &mut Rng, w: u32, h: u32, stride: u32) -> Vec<[u8; 4]> {
    let len = if h == 0 {
        0
    } else {
        (stride * (h - 1) + w) as usize
    };
    let mut buf = vec![PAD_U8; len];
    for y in 0..h {
        for x in 0..w {
            buf[(y * stride + x) as usize] = [0, 1, 2, 3].map(|_| rng.below(256) as u8);
        }
    }
    buf
}

/// Random width, height and stride, with stride ≥ width and often more padding.
fn geometry(rng: &mut Rng) -> (u32, u32, u32) {
    let w = 1 + rng.below(12) as u32;
    let h = 1 + rng.below(8) as u32;
    (w, h, w + rng.below(9) as u32)
}

#[test]
fn composite_respects_stride_and_leaves_padding_alone() {
    let mut rng = Rng::new(11);
    for case in 0..200 {
        let (w, h, sd) = geometry(&mut rng);
        let ss = w + rng.below(5) as u32;
        let mut dst = random_f32(&mut rng, w, h, sd);
        let mut src = random_f32(&mut rng, w, h, ss);
        let dst_pad = pad_indices(w, h, sd);
        let src_pad = pad_indices(w, h, ss);
        let dst_pad_before: Vec<_> = dst_pad.iter().map(|&i| dst[i]).collect();
        let src_pad_before: Vec<_> = src_pad.iter().map(|&i| src[i]).collect();

        // The same operation on packed copies gives the reference pixels.
        let mut dst_packed = unpadded(&dst, w, h, sd);
        let src_packed = unpadded(&src, w, h, ss);
        {
            let mut d =
                Surface::<Rgba32F>::from_slice(&mut dst, w, h, sd, ColorSpaceId::Srgb).unwrap();
            let s = Surface::<Rgba32F>::from_slice(&mut src, w, h, ss, ColorSpaceId::Srgb).unwrap();
            composite_surface(&mut d, &s, 0.75, SourceOver).unwrap();
        }
        let mut dp =
            Surface::<Rgba32F>::from_slice(&mut dst_packed, w, h, w, ColorSpaceId::Srgb).unwrap();
        let mut src_packed = src_packed;
        let sp =
            Surface::<Rgba32F>::from_slice(&mut src_packed, w, h, w, ColorSpaceId::Srgb).unwrap();
        composite_surface(&mut dp, &sp, 0.75, SourceOver).unwrap();

        assert_eq!(
            dst_pad.iter().map(|&i| dst[i]).collect::<Vec<_>>(),
            dst_pad_before,
            "dst padding, case {case}"
        );
        assert_eq!(
            src_pad.iter().map(|&i| src[i]).collect::<Vec<_>>(),
            src_pad_before,
            "src padding, case {case}"
        );
        assert_eq!(
            unpadded(&dst, w, h, sd),
            dst_packed,
            "pixels differ from packed, case {case}"
        );
    }
}

#[test]
fn rgba8_strided_composite_matches_packed_and_keeps_padding() {
    let mut rng = Rng::new(44);
    for case in 0..100 {
        let (w, h, sd) = geometry(&mut rng);
        let mut dst = random_u8(&mut rng, w, h, sd);
        let mut src = random_u8(&mut rng, w, h, sd);
        let pad = pad_indices(w, h, sd);
        let pad_before: Vec<_> = pad.iter().map(|&i| dst[i]).collect();

        let mut dst_packed = unpadded(&dst, w, h, sd);
        let mut src_packed = unpadded(&src, w, h, sd);
        {
            let mut d =
                Surface::<Rgba8>::from_slice(&mut dst, w, h, sd, ColorSpaceId::Srgb).unwrap();
            let s = Surface::<Rgba8>::from_slice(&mut src, w, h, sd, ColorSpaceId::Srgb).unwrap();
            composite_surface(&mut d, &s, 1.0, SourceOver).unwrap();
        }
        {
            let mut d =
                Surface::<Rgba8>::from_slice(&mut dst_packed, w, h, w, ColorSpaceId::Srgb).unwrap();
            let s =
                Surface::<Rgba8>::from_slice(&mut src_packed, w, h, w, ColorSpaceId::Srgb).unwrap();
            composite_surface(&mut d, &s, 1.0, SourceOver).unwrap();
        }
        assert_eq!(
            pad.iter().map(|&i| dst[i]).collect::<Vec<_>>(),
            pad_before,
            "rgba8 padding, case {case}"
        );
        assert_eq!(
            unpadded(&dst, w, h, sd),
            dst_packed,
            "rgba8 strided vs packed, case {case}"
        );
    }
}

#[test]
fn filters_respect_stride_and_leaves_padding_alone() {
    let mut rng = Rng::new(22);
    for case in 0..120 {
        let (w, h, sd) = geometry(&mut rng);
        let op = rng.below(4);
        let mut buf = random_f32(&mut rng, w, h, sd);
        let mut packed = unpadded(&buf, w, h, sd);
        let pad = pad_indices(w, h, sd);
        let pad_before: Vec<_> = pad.iter().map(|&i| buf[i]).collect();
        {
            let mut s =
                Surface::<Rgba32F>::from_slice(&mut buf, w, h, sd, ColorSpaceId::Srgb).unwrap();
            let mut p =
                Surface::<Rgba32F>::from_slice(&mut packed, w, h, w, ColorSpaceId::Srgb).unwrap();
            run_filter(op, &mut s);
            run_filter(op, &mut p);
        }
        assert_eq!(
            pad.iter().map(|&i| buf[i]).collect::<Vec<_>>(),
            pad_before,
            "op {op} padding, case {case}"
        );
        assert_eq!(
            unpadded(&buf, w, h, sd),
            packed,
            "op {op} strided vs packed, case {case}"
        );
    }
}

fn run_filter(op: u64, s: &mut Surface<'_, Rgba32F>) {
    match op {
        0 => gaussian_blur(s, 1.3),
        1 => sharpen(s, 0.6),
        2 => open(s, 1),
        _ => apply_color_matrix(s, &hue_rotate(40.0)),
    }
}

#[test]
fn rasterize_and_draw_respect_stride() {
    let mut rng = Rng::new(33);
    for case in 0..100 {
        let (w, h, sd) = geometry(&mut rng);
        let pad = pad_indices(w, h, sd);

        // Rasterize a triangle into a strided surface.
        let mut buf = random_f32(&mut rng, w, h, sd);
        let pad_before: Vec<_> = pad.iter().map(|&i| buf[i]).collect();
        let (x0, y0) = (rng.below(w as u64) as f32, rng.below(h as u64) as f32);
        let mut path = FlattenedPath::new();
        path.push_contour(vec![[x0, y0], [w as f32, y0], [x0, h as f32]]);
        {
            let mut s =
                Surface::<Rgba32F>::from_slice(&mut buf, w, h, sd, ColorSpaceId::Srgb).unwrap();
            let paint = Paint::Solid([0.2, 0.9, 0.4, 0.8]);
            rasterize_path(&mut s, &path, &Affine2::IDENTITY, FillRule::NonZero, &paint).unwrap();
        }
        assert_eq!(
            pad.iter().map(|&i| buf[i]).collect::<Vec<_>>(),
            pad_before,
            "rasterize padding, case {case}"
        );

        // Draw a strided source into a strided destination, shifted by one pixel.
        let mut src = random_f32(&mut rng, w, h, sd);
        let mut dst = random_f32(&mut rng, w, h, sd);
        let dst_pad_before: Vec<_> = pad.iter().map(|&i| dst[i]).collect();
        let t = Affine2 {
            m: [1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
        };
        {
            let s = Surface::<Rgba32F>::from_slice(&mut src, w, h, sd, ColorSpaceId::Srgb).unwrap();
            let mut d =
                Surface::<Rgba32F>::from_slice(&mut dst, w, h, sd, ColorSpaceId::Srgb).unwrap();
            draw_image(&mut d, &s, &t, Interp::Bilinear, EdgeMode::Transparent, 1.0).unwrap();
        }
        assert_eq!(
            pad.iter().map(|&i| dst[i]).collect::<Vec<_>>(),
            dst_pad_before,
            "draw padding, case {case}"
        );
    }
}
