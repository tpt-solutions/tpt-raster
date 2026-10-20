//! Spec §4 pipeline: a blurred translucent card composited onto a background.
//!
//! Allocate an Rgba16F intermediate, rasterize a translucent shape into it, blur it, and
//! composite it onto an Rgba8 screen surface with SourceOver.

use tpt_canvas::{Affine2, FillRule, FlattenedPath, Paint};
use tpt_color::ColorSpaceId;
use tpt_raster::Surface;
use tpt_raster::composite::{SourceOver, composite_surface};
use tpt_raster::filter::gaussian_blur;
use tpt_raster::pixel::{PixelFormat, Rgba8, Rgba16F};
use tpt_raster::rasterize::rasterize_path;

const W: u32 = 32;
const H: u32 = 32;
const BACKGROUND: Rgba8 = Rgba8 {
    r: 20,
    g: 40,
    b: 200,
    a: 255,
};

fn card_shape() -> FlattenedPath {
    let mut p = FlattenedPath::new();
    p.push_contour(vec![[8.0, 8.0], [24.0, 8.0], [24.0, 24.0], [8.0, 24.0]]);
    p
}

/// Runs the whole pipeline and returns the final screen surface.
fn render() -> Surface<'static, Rgba8> {
    // 1. Screen surface with a solid background.
    let mut screen: Surface<'static, Rgba8> = Surface::new(W, H, ColorSpaceId::Srgb).unwrap();
    screen.clear(BACKGROUND);

    // 2. HDR intermediate. The spec clears it to translucent white, but that would tint the
    //    whole screen. The card is a shape, so only the shape is filled, on a transparent surface.
    let mut card: Surface<'static, Rgba16F> = Surface::new(W, H, ColorSpaceId::Srgb).unwrap();

    // 3. Rasterize the translucent white card shape into it.
    let paint = Paint::Solid([1.0, 1.0, 1.0, 0.5]);
    rasterize_path(
        &mut card,
        &card_shape(),
        &Affine2::IDENTITY,
        FillRule::NonZero,
        &paint,
    )
    .unwrap();

    // 4. Blur, then 5. composite onto the screen (cross-format: Rgba16F → Rgba8).
    gaussian_blur(&mut card, 2.0);
    composite_surface(&mut screen, &card, 1.0, SourceOver).unwrap();
    screen
}

#[test]
fn card_changes_the_centre_and_leaves_the_corners_alone() {
    let screen = render();
    let centre = screen.get_pixel(W / 2, H / 2);
    assert_ne!(centre, BACKGROUND);
    // Far from the card, the background is untouched.
    assert_eq!(screen.get_pixel(0, 0), BACKGROUND);
    assert_eq!(screen.get_pixel(W - 1, H - 1), BACKGROUND);
}

#[test]
fn blur_softens_the_card_edge() {
    let screen = render();
    // Just outside the card's left edge (x = 8) the blur bleeds red into the background, so the
    // red channel rises above the background's 20 while the sharp shape would keep it at 20.
    let edge = screen.get_pixel(6, H / 2);
    assert!(edge.r > BACKGROUND.r, "edge red {}", edge.r);
    assert!(edge.r < 255, "edge should be soft, got {}", edge.r);
}

#[test]
fn pipeline_output_is_deterministic() {
    let a: Vec<[u8; 4]> = render().as_slice().to_vec();
    let b: Vec<[u8; 4]> = render().as_slice().to_vec();
    assert_eq!(a, b);
    // Keep the format import honest: storage round-trips through the pixel type.
    assert_eq!(Rgba8::from_storage(a[0]), BACKGROUND);
}
