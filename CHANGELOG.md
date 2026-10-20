# Changelog

## Unreleased

Pre-1.0. The API may change.

### Added
- Workspace with local stubs for `tpt-color`, `tpt-canvas` and `tpt-testkit`. `tpt-simd` is an
  optional git dependency pinned to `be4f4c4`.
- `Surface` with owned and borrowed buffers, pixel-unit stride, and bounds policy
  (`get_pixel`/`set_pixel` panic, `try_*` report failure).
- Pixel formats `Rgba8`, `Rgba32F`, `Gray8` and `Rgba16F`, with an exhaustive-tested f16.
- `composite_surface` with `SourceOver`, opacity, cross-format and colour-space conversion.
- Remaining Porter-Duff operators and the 15 CSS/SVG blend modes.
- Signed-area rasterizer with NonZero and EvenOdd fill rules, clipping, and solid, linear and
  radial paints.
- Sampling: nearest, bilinear, bicubic (Mitchell) and Lanczos3, with clamp, repeat, mirror and
  transparent edge modes. Transformed image draws, mipmap chains, and mipmapped draws.
- Filters: Gaussian blur, separable and 3×3 convolution, sharpen, Sobel edges, 5×4 colour
  matrices with brightness, contrast, saturate, hue-rotate and sepia presets, and morphology.
- Optional `simd` feature: `round_to_u8_x8`, and a SIMD SourceOver composite for Rgba8 destinations (about 1.1–1.4× the scalar path). Bit-exact with scalar.
- Stride fuzzing, regression goldens (FNV-1a, six scenes), and a pipeline example.
- CI workflow: fmt, clippy, tests on Linux x86_64 and ARM64, macOS and Windows, plus no_std
  builds for wasm32 and thumbv7em.

### Not yet
- SIMD backend beyond rounding and the Rgba8 SourceOver composite. The 10× milestone is not met.
- Surface-backed pattern paint type in `tpt-canvas`.
- Real `tpt-color`, `tpt-canvas` and `tpt-testkit`.
- Licence and crate publishing metadata.
- WASM test runner, Criterion benchmarks, and a cross-platform run of the golden suite.
