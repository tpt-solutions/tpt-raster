# tpt-raster

A pure-Rust 2D rasterization and compositing engine for the TPT ecosystem. It turns flattened
paths into anti-aliased coverage, composites pixels with Porter-Duff and CSS/SVG blend modes,
samples images, and runs convolution, colour and morphology filters.

- `no_std` + `alloc` for the core. No `unsafe` code.
- All pixel math is `f32`. Conversion to `u8` goes through one function, `round_to_u8`.
- Output is meant to be deterministic: the same inputs give the same bytes on every platform.

**Status: pre-1.0.** Phases 0–2 and 4 of [TODO.md](TODO.md) are implemented on the scalar path.
The SIMD backend (Phase 3) is partial. Rounding and SourceOver composite are implemented on tpt-simd and match the scalar code bit for bit. The composite only helps Rgba8 destinations, by about 1.1–1.4×, and the 10× milestone is not met. Cross-platform identity has not been verified.

## Layout

| Module | Purpose |
| --- | --- |
| `surface` | Pixel buffers, owned or borrowed, with stride |
| `pixel` | `Rgba8`, `Rgba32F`, `Gray8`, `Rgba16F` (hand-rolled f16) |
| `rasterize` | Coverage masks from paths, fills with solid, linear or radial paints |
| `composite` | `SourceOver` and `composite_surface`, with colour-space conversion |
| `modes` | Remaining Porter-Duff operators and the 15 CSS/SVG blend modes |
| `sample` | Nearest, bilinear, bicubic and Lanczos3 sampling, edge modes, mipmaps, image draws |
| `filter` | Gaussian blur, convolution, sharpen, Sobel, colour matrices, morphology |
| `round` | The single f32 → u8 rounding function |

Surface values are straight alpha. Compositing and filtering work in premultiplied space.

## Build and test

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p tpt-raster --no-default-features --target thumbv7em-none-eabihf
cargo build -p tpt-raster --no-default-features --target wasm32-unknown-unknown

# Example: the spec §4 pipeline as ASCII.
cargo run -p tpt-raster --example pipeline

# Slow fuzz run (about 30 s):
cargo test -p tpt-raster --test rasterize -- --ignored
```

The `tpt-color`, `tpt-canvas` and `tpt-testkit` crates in `stubs/` are local stand-ins with the
APIs the spec names. They will be replaced by the real crates.

## Licence

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Copyright (c) 2026 TPT Solutions.
