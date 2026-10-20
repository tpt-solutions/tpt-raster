//! Porter-Duff operators and CSS/SVG blend mode tests.

use tpt_raster::composite::{BlendMode, SourceOver};
use tpt_raster::modes::*;
use tpt_testkit::fuzz::Rng;

const EPS: f32 = 1e-5;

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < EPS)
}

/// Random premultiplied RGBA with colour ≤ alpha.
fn premul(rng: &mut Rng) -> [f32; 4] {
    let a = rng.below(1001) as f32 / 1000.0;
    let c = |rng: &mut Rng| rng.below(1001) as f32 / 1000.0 * a;
    [c(rng), c(rng), c(rng), a]
}

#[test]
fn source_in_keeps_source_where_destination_is_opaque() {
    let src = [0.5, 0.0, 0.0, 0.5];
    let dst = [0.0, 0.0, 1.0, 1.0];
    assert!(close(SourceIn::blend(src, dst), [0.5, 0.0, 0.0, 0.5]));
    assert!(close(SourceIn::blend(src, [0.0; 4]), [0.0; 4]));
}

#[test]
fn destination_out_removes_destination_under_source() {
    let src = [0.0, 0.0, 0.0, 0.25];
    let dst = [0.0, 0.0, 1.0, 1.0];
    assert!(close(
        DestinationOut::blend(src, dst),
        [0.0, 0.0, 0.75, 0.75]
    ));
}

#[test]
fn xor_of_two_half_alpha_layers() {
    let src = [0.5, 0.0, 0.0, 0.5];
    let dst = [0.0, 0.0, 0.5, 0.5];
    // Each side keeps (1 - other alpha) of itself.
    assert!(close(Xor::blend(src, dst), [0.25, 0.0, 0.25, 0.5]));
}

#[test]
fn plus_clamps_to_opaque() {
    assert_eq!(
        Plus::blend([0.8, 0.0, 0.0, 0.8], [0.8, 0.0, 0.0, 0.8]),
        [1.0, 0.0, 0.0, 1.0]
    );
}

#[test]
fn multiply_and_screen_on_opaque_colors() {
    let src = [0.5, 0.5, 0.5, 1.0];
    let dst = [0.8, 0.8, 0.8, 1.0];
    assert!(close(Multiply::blend(src, dst), [0.4, 0.4, 0.4, 1.0]));
    assert!(close(Screen::blend(src, dst), [0.9, 0.9, 0.9, 1.0]));
}

#[test]
fn difference_and_exclusion_on_opaque_colors() {
    let src = [0.5, 0.5, 0.5, 1.0];
    let dst = [0.8, 0.8, 0.8, 1.0];
    assert!(close(Difference::blend(src, dst), [0.3, 0.3, 0.3, 1.0]));
    // 0.5 + 0.8 - 2 * 0.5 * 0.8 = 0.5
    assert!(close(Exclusion::blend(src, dst), [0.5, 0.5, 0.5, 1.0]));
}

#[test]
fn overlay_is_hard_light_with_roles_swapped() {
    let src = [0.3, 0.3, 0.3, 1.0];
    let dst = [0.8, 0.8, 0.8, 1.0];
    assert!(close(Overlay::blend(src, dst), HardLight::blend(dst, src)));
}

#[test]
fn darken_and_lighten_pick_extremes() {
    let src = [0.2, 0.9, 0.5, 1.0];
    let dst = [0.6, 0.4, 0.5, 1.0];
    assert!(close(Darken::blend(src, dst), [0.2, 0.4, 0.5, 1.0]));
    assert!(close(Lighten::blend(src, dst), [0.6, 0.9, 0.5, 1.0]));
}

#[test]
fn color_dodge_and_burn_handle_their_edge_cases() {
    // Backdrop 0 dodges to 0. Source 1 dodges to 1.
    assert!(close(
        ColorDodge::blend([1.0; 4], [0.5, 0.5, 0.5, 1.0]),
        [1.0, 1.0, 1.0, 1.0]
    ));
    assert!(close(
        ColorDodge::blend([0.5, 0.5, 0.5, 1.0], [0.0, 0.0, 0.0, 1.0]),
        [0.0, 0.0, 0.0, 1.0]
    ));
    // Backdrop 1 burns to 1. Source 0 burns to 0.
    assert!(close(
        ColorBurn::blend([0.0; 4], [1.0, 1.0, 1.0, 1.0]),
        [1.0, 1.0, 1.0, 1.0]
    ));
    assert!(close(
        ColorBurn::blend([0.0, 0.0, 0.0, 1.0], [0.5, 0.5, 0.5, 1.0]),
        [0.0, 0.0, 0.0, 1.0]
    ));
}

#[test]
fn soft_light_matches_both_branches() {
    // Source 0.5 is neutral: the backdrop is unchanged.
    assert!(close(
        SoftLight::blend([0.5, 0.5, 0.5, 1.0], [0.3, 0.3, 0.3, 1.0]),
        [0.3, 0.3, 0.3, 1.0]
    ));
    // Source 0.25 darkens, source 0.75 lightens (backdrop 0.5 sits at the midpoint).
    let dark = SoftLight::blend([0.25, 0.25, 0.25, 1.0], [0.5, 0.5, 0.5, 1.0]);
    let light = SoftLight::blend([0.75, 0.75, 0.75, 1.0], [0.5, 0.5, 0.5, 1.0]);
    assert!(dark[0] < 0.5 && light[0] > 0.5);
}

#[test]
fn color_takes_source_hue_with_backdrop_luminosity() {
    // Grey source has no hue or saturation, so the result is grey at the backdrop's luminosity.
    let dst = [0.8, 0.2, 0.2, 1.0];
    let lum = 0.3 * 0.8 + 0.59 * 0.2 + 0.11 * 0.2;
    let out = Color::blend([0.5, 0.5, 0.5, 1.0], dst);
    assert!(close(out, [lum, lum, lum, 1.0]), "{out:?}");
}

#[test]
fn luminosity_takes_backdrop_hue_with_source_luminosity() {
    let src = [0.8, 0.2, 0.2, 1.0];
    let lum = 0.3 * 0.8 + 0.59 * 0.2 + 0.11 * 0.2;
    let out = Luminosity::blend(src, [0.5, 0.5, 0.5, 1.0]);
    assert!(close(out, [lum, lum, lum, 1.0]), "{out:?}");
}

#[test]
fn hue_saturation_and_color_keep_backdrop_luminosity() {
    let src = [0.9, 0.1, 0.3, 1.0];
    let dst = [0.4, 0.6, 0.2, 1.0];
    let lum = |c: [f32; 4]| 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2];
    for out in [
        Hue::blend(src, dst),
        Saturation::blend(src, dst),
        Color::blend(src, dst),
    ] {
        assert!((lum(out) - lum(dst)).abs() < 1e-4, "{out:?}");
    }
}

#[test]
fn every_mode_is_identity_over_transparent_backdrop_and_transparent_source() {
    let mut rng = Rng::new(7);
    for _ in 0..200 {
        let p = premul(&mut rng);
        let zero = [0.0f32; 4];
        assert!(close(Multiply::blend(p, zero), p));
        assert!(close(Hue::blend(p, zero), p));
        assert!(close(Luminosity::blend(p, zero), p));
        assert!(close(Multiply::blend(zero, p), p));
        assert!(close(Hue::blend(zero, p), p));
        assert!(close(Luminosity::blend(zero, p), p));
        assert!(close(SourceOver::blend(p, zero), p));
    }
}

#[test]
fn fuzz_all_operators_stay_finite_and_in_range() {
    let mut rng = Rng::new(99);
    for _ in 0..20_000 {
        let s = premul(&mut rng);
        let d = premul(&mut rng);
        let outs = [
            Clear::blend(s, d),
            Source::blend(s, d),
            Destination::blend(s, d),
            DestinationOver::blend(s, d),
            SourceIn::blend(s, d),
            DestinationIn::blend(s, d),
            SourceOut::blend(s, d),
            DestinationOut::blend(s, d),
            SourceAtop::blend(s, d),
            DestinationAtop::blend(s, d),
            Xor::blend(s, d),
            Plus::blend(s, d),
            Multiply::blend(s, d),
            Screen::blend(s, d),
            Overlay::blend(s, d),
            Darken::blend(s, d),
            Lighten::blend(s, d),
            ColorDodge::blend(s, d),
            ColorBurn::blend(s, d),
            HardLight::blend(s, d),
            SoftLight::blend(s, d),
            Difference::blend(s, d),
            Exclusion::blend(s, d),
            Hue::blend(s, d),
            Saturation::blend(s, d),
            Color::blend(s, d),
            Luminosity::blend(s, d),
        ];
        for out in outs {
            assert!(
                out.iter()
                    .all(|v| v.is_finite() && (-EPS..=1.0 + EPS).contains(v)),
                "{out:?}"
            );
        }
    }
}
