//! Module C (complete): the remaining Porter-Duff operators and the CSS/SVG blend modes.
//!
//! Every operator takes and returns premultiplied RGBA, like [`crate::composite::SourceOver`].
//! Blend modes follow the W3C compositing formula. Color channels are unpremultiplied before
//! `B(cb, cs)` is evaluated, and a zero-alpha input contributes a zero color. The math is plain
//! f32 in a fixed operation order.

use crate::composite::BlendMode;

macro_rules! porter_duff {
    ($(#[$doc:meta])* $name:ident, |$sa:ident, $da:ident| ($fs:expr, $fd:expr)) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name;

        impl BlendMode for $name {
            #[inline]
            #[allow(unused_variables)]
            fn blend(src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
                let ($sa, $da) = (src[3], dst[3]);
                let (fs, fd): (f32, f32) = ($fs, $fd);
                [0, 1, 2, 3].map(|i| src[i] * fs + dst[i] * fd)
            }
        }
    };
}

porter_duff!(
    /// Porter-Duff `Clear`: the result is transparent.
    Clear, |sa, da| (0.0, 0.0)
);
porter_duff!(
    /// Porter-Duff `Source`: the source replaces the destination.
    Source, |sa, da| (1.0, 0.0)
);
porter_duff!(
    /// Porter-Duff `Destination`: the destination is kept, the source is ignored.
    Destination, |sa, da| (0.0, 1.0)
);
porter_duff!(
    /// Porter-Duff `DestinationOver`: the destination over the source.
    DestinationOver, |sa, da| (1.0 - da, 1.0)
);
porter_duff!(
    /// Porter-Duff `SourceIn`: the source, only where the destination is opaque.
    SourceIn, |sa, da| (da, 0.0)
);
porter_duff!(
    /// Porter-Duff `DestinationIn`: the destination, only where the source is opaque.
    DestinationIn, |sa, da| (0.0, sa)
);
porter_duff!(
    /// Porter-Duff `SourceOut`: the source, only where the destination is transparent.
    SourceOut, |sa, da| (1.0 - da, 0.0)
);
porter_duff!(
    /// Porter-Duff `DestinationOut`: the destination, only where the source is transparent.
    DestinationOut, |sa, da| (0.0, 1.0 - sa)
);
porter_duff!(
    /// Porter-Duff `SourceAtop`: the source over the destination, kept inside the destination.
    SourceAtop, |sa, da| (da, 1.0 - sa)
);
porter_duff!(
    /// Porter-Duff `DestinationAtop`: the destination over the source, kept inside the source.
    DestinationAtop, |sa, da| (1.0 - da, sa)
);
porter_duff!(
    /// Porter-Duff `Xor`: the parts of source and destination that do not overlap.
    Xor, |sa, da| (1.0 - da, 1.0 - sa)
);

/// Porter-Duff `Plus`: additive, clamped to fully opaque.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plus;

impl BlendMode for Plus {
    #[inline]
    fn blend(src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
        [0, 1, 2, 3].map(|i| (src[i] + dst[i]).min(1.0))
    }
}

/// Unpremultiplies one channel. A zero alpha gives zero, so the caller never divides by zero.
#[inline]
fn unpremul(v: f32, a: f32) -> f32 {
    if a == 0.0 { 0.0 } else { v / a }
}

/// Applies a separable blend function per color channel, then the W3C compositing formula.
fn separable_blend(src: [f32; 4], dst: [f32; 4], f: impl Fn(f32, f32) -> f32) -> [f32; 4] {
    let (as_, ab) = (src[3], dst[3]);
    let mut out = [0.0f32; 4];
    for i in 0..3 {
        let cs = unpremul(src[i], as_);
        let cb = unpremul(dst[i], ab);
        out[i] = (1.0 - ab) * src[i] + (1.0 - as_) * dst[i] + as_ * ab * f(cb, cs);
    }
    out[3] = as_ + ab - as_ * ab;
    out
}

/// Non-separable version: `f` sees the whole unpremultiplied color triple.
fn nonseparable_blend(
    src: [f32; 4],
    dst: [f32; 4],
    f: fn([f32; 3], [f32; 3]) -> [f32; 3],
) -> [f32; 4] {
    let (as_, ab) = (src[3], dst[3]);
    let cs = [0, 1, 2].map(|i| unpremul(src[i], as_));
    let cb = [0, 1, 2].map(|i| unpremul(dst[i], ab));
    let b = f(cb, cs);
    let mut out = [0.0f32; 4];
    for i in 0..3 {
        out[i] = (1.0 - ab) * src[i] + (1.0 - as_) * dst[i] + as_ * ab * b[i];
    }
    out[3] = as_ + ab - as_ * ab;
    out
}

macro_rules! separable_mode {
    ($(#[$doc:meta])* $name:ident, |$cb:ident, $cs:ident| $f:expr) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name;

        impl BlendMode for $name {
            #[inline]
            fn blend(src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
                separable_blend(src, dst, |$cb, $cs| $f)
            }
        }
    };
}

fn multiply(cb: f32, cs: f32) -> f32 {
    cb * cs
}

fn screen(cb: f32, cs: f32) -> f32 {
    cb + cs - cb * cs
}

fn hard_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        multiply(cb, 2.0 * cs)
    } else {
        screen(cb, 2.0 * cs - 1.0)
    }
}

fn soft_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
    } else {
        let d = if cb <= 0.25 {
            ((16.0 * cb - 12.0) * cb + 4.0) * cb
        } else {
            libm::sqrtf(cb)
        };
        cb + (2.0 * cs - 1.0) * (d - cb)
    }
}

separable_mode!(
    /// Multiply: darkens by multiplying the channels.
    Multiply, |cb, cs| multiply(cb, cs)
);
separable_mode!(
    /// Screen: lightens, the inverse of Multiply.
    Screen, |cb, cs| screen(cb, cs)
);
separable_mode!(
    /// Overlay: HardLight with the roles of the two layers swapped.
    Overlay, |cb, cs| hard_light(cs, cb)
);
separable_mode!(
    /// Darken: the darker of the two channels.
    Darken, |cb, cs| cb.min(cs)
);
separable_mode!(
    /// Lighten: the lighter of the two channels.
    Lighten, |cb, cs| cb.max(cs)
);
separable_mode!(
    /// ColorDodge: brightens the backdrop to reflect the source.
    ColorDodge, |cb, cs| if cb == 0.0 {
        0.0
    } else if cs >= 1.0 {
        1.0
    } else {
        (cb / (1.0 - cs)).min(1.0)
    }
);
separable_mode!(
    /// ColorBurn: darkens the backdrop to reflect the source.
    ColorBurn, |cb, cs| if cb >= 1.0 {
        1.0
    } else if cs <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - cb) / cs).min(1.0)
    }
);
separable_mode!(
    /// HardLight: Multiply or Screen depending on the source.
    HardLight, |cb, cs| hard_light(cb, cs)
);
separable_mode!(
    /// SoftLight: a gentle version of HardLight.
    SoftLight, |cb, cs| soft_light(cb, cs)
);
separable_mode!(
    /// Difference: the absolute difference of the channels.
    Difference, |cb, cs| (cb - cs).abs()
);
separable_mode!(
    /// Exclusion: like Difference, with lower contrast.
    Exclusion, |cb, cs| cb + cs - 2.0 * cb * cs
);

// Non-separable modes, from the W3C compositing spec.

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 && l - n > 0.0 {
        for v in &mut out {
            *v = l + (*v - l) * l / (l - n);
        }
    }
    if x > 1.0 && x - l > 0.0 {
        for v in &mut out {
            *v = l + (*v - l) * (1.0 - l) / (x - l);
        }
    }
    out
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut imax = 0;
    let mut imin = 0;
    for i in 1..3 {
        if c[i] > c[imax] {
            imax = i;
        }
        if c[i] < c[imin] {
            imin = i;
        }
    }
    let (mx, mn) = (c[imax], c[imin]);
    let mut out = [0.0f32; 3];
    if mx > mn {
        let imid = 3 - imax - imin;
        out[imid] = (c[imid] - mn) * s / (mx - mn);
        out[imax] = s;
    }
    out
}

/// Hue: the source hue with the backdrop's saturation and luminosity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hue;

impl BlendMode for Hue {
    fn blend(src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
        nonseparable_blend(src, dst, |cb, cs| set_lum(set_sat(cs, sat(cb)), lum(cb)))
    }
}

/// Saturation: the source saturation with the backdrop's hue and luminosity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Saturation;

impl BlendMode for Saturation {
    fn blend(src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
        nonseparable_blend(src, dst, |cb, cs| set_lum(set_sat(cb, sat(cs)), lum(cb)))
    }
}

/// Color: the source hue and saturation with the backdrop's luminosity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color;

impl BlendMode for Color {
    fn blend(src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
        nonseparable_blend(src, dst, |cb, cs| set_lum(cs, lum(cb)))
    }
}

/// Luminosity: the source luminosity with the backdrop's hue and saturation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Luminosity;

impl BlendMode for Luminosity {
    fn blend(src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
        nonseparable_blend(src, dst, |cb, cs| set_lum(cb, lum(cs)))
    }
}
