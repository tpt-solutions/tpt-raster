//! Renders the spec §4 pipeline (a blurred translucent card over a background) and prints it as
//! ASCII luminance. Run with `cargo run -p tpt-raster --example pipeline`.

use tpt_canvas::{Affine2, FillRule, FlattenedPath, Paint};
use tpt_color::ColorSpaceId;
use tpt_raster::Surface;
use tpt_raster::composite::{SourceOver, composite_surface};
use tpt_raster::filter::gaussian_blur;
use tpt_raster::pixel::{Rgba8, Rgba16F};
use tpt_raster::rasterize::rasterize_path;

const RAMP: &[u8] = b" .:-=+*#%@";

fn main() {
    let (w, h) = (48u32, 24u32);

    let mut screen: Surface<'static, Rgba8> = Surface::new(w, h, ColorSpaceId::Srgb).unwrap();
    screen.clear(Rgba8 {
        r: 20,
        g: 40,
        b: 200,
        a: 255,
    });

    // A translucent white card on a transparent intermediate, then blurred and composited.
    let mut card: Surface<'static, Rgba16F> = Surface::new(w, h, ColorSpaceId::Srgb).unwrap();
    let mut shape = FlattenedPath::new();
    shape.push_contour(vec![[12.0, 6.0], [36.0, 6.0], [36.0, 18.0], [12.0, 18.0]]);
    let paint = Paint::Solid([1.0, 1.0, 1.0, 0.6]);
    rasterize_path(
        &mut card,
        &shape,
        &Affine2::IDENTITY,
        FillRule::NonZero,
        &paint,
    )
    .unwrap();
    gaussian_blur(&mut card, 1.5);
    composite_surface(&mut screen, &card, 1.0, SourceOver).unwrap();

    for y in 0..h {
        let line: String = (0..w)
            .map(|x| {
                let p = screen.get_pixel(x, y);
                // Rec. 709 luma of the straight colour, picked from the ramp.
                let luma =
                    (0.2126 * p.r as f32 + 0.7152 * p.g as f32 + 0.0722 * p.b as f32) / 255.0;
                let i = (luma * (RAMP.len() - 1) as f32).round() as usize;
                RAMP[i.min(RAMP.len() - 1)] as char
            })
            .collect();
        println!("{line}");
    }
}
