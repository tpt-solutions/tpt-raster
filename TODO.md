# tpt-raster — v1.0 Task Checklist

Source of truth: [spec.txt](spec.txt)

## Decisions (from clarifying Q&A)
- Cargo **workspace**, stable Rust (latest), `no_std + alloc` core
- `tpt-simd` is real (on GitHub) → git dependency; `tpt-color`, `tpt-canvas`, `tpt-testkit` → **local stub crates** with the APIs named in the spec
- Hand-rolled f16 <-> f32 (no `half` crate)
- Blend math runs on the surface's values as-is (Canvas 2D parity); conversion only when two surfaces' color spaces differ
- All pixel math in f32, never f64; final rounding via `round_to_u8()`
- Scope of this session: checklist only, no code

## Open items (blocking)
- [x] tpt-simd repo: https://github.com/tpt-solutions/tpt-simd (branch `master`, v0.1.0 pre-release, no tags/releases, Apache-2.0 OR MIT)
- [ ] Pin a git `rev` for tpt-simd (inspected `be4f4c4e9dd8fa60460dcf6aae2afadab5ccd077`; no tags exist)
- [x] Inspected tpt-simd source (cloned to scratchpad). Findings:
  - **`round_to_u8` does not exist.** Closest: `tpt-simd-rounding::round_to_nearest_even_i32(F32x8) -> I32x8` (saturating, NaN→0), `round_with_bias_i32`, and `tpt-simd-saturate::pack_i16_to_u8` / `pack_i32_to_i16`. Plan: build `f32→u8` locally in tpt-raster from these (round-ties-even → clamp 0..255 → pack), or contribute `round_to_u8` upstream. Spec's Architect's Note should be updated accordingly.
  - Fixed lane widths: `Simd<T, N>` with `f32×8`, `i32×8`, `i8×32`, `i16×16`, `u8×16`; const-generic `N`. Spec's "8 or 16 pixels at a time" = 8 f32 lanes (2 RGBA px) per op, so design loops around F32x8.
  - Has `swizzle([usize; N])`, `sat_add/sat_sub/sat_mul`, `clamp`, `min/max`, `simd_lt/gt..` masks + `select`, `cast`, `from_slice`/`store_partial`, `permute`, `transpose_4x4_f32`, `tpt-simd-blend` (select/blend), `tpt-simd-convolve`, `tpt-simd-interpolate` (inspect before writing own versions).
  - MSRV **1.95**, edition **2024**, toolchain = stable (installed rustc 1.98 OK). tpt-raster should use edition 2024 / rust-version ≥1.95.
  - Scalar fallback exists (`scalar-only` feature); `std` feature for intrinsics; `nightly` feature exists — don't enable.
  - Uses `libm` for lane math; `lane_mul_add` uses hardware `mul_add` under `std`, libm `fmaf` otherwise (both correctly-rounded fused, should match) — **but** spec forbids relying on contraction ambiguity: avoid `mul_add` in pixel math unless fused-everywhere is guaranteed, and never build with `-C target-cpu=native` in golden/CI runs.
- [ ] Decide which `tpt-simd-*` member crates to depend on (`-core`, `-vector`, `-rounding`, `-saturate`, `-blend`, `-convolve`, `-interpolate`, `-permute`) vs. umbrella `tpt-simd`
- [ ] Verify cross-platform bit-identity of rounding/saturate/blend ops on x86_64 vs. ARM64 vs. WASM (tpt-simd is v0.1.0, 14 commits — treat as unproven; our SIMD-vs-scalar equivalence tests double as its validation)
- [ ] Feed gaps found in tpt-simd (e.g. `round_to_u8`, u8↔f32 pack/unpack for RGBA) back upstream as issues/PRs

---

## Phase 0 — Workspace Setup
- [ ] `git init`, `.gitignore`, `rust-toolchain.toml` (stable)
- [ ] Workspace `Cargo.toml` (members: `tpt-raster`, `stubs/tpt-color`, `stubs/tpt-canvas`, `stubs/tpt-testkit`)
- [ ] Add `tpt-simd` as git dependency
- [ ] `tpt-raster` crate: `#![no_std]`, `extern crate alloc`, feature flags (`std`, `simd`, `scalar-fallback`)
- [ ] Stub `tpt-color`: `ColorSpaceId`, premultiply/unpremultiply, conversion fn (sRGB <-> linear at minimum)
- [ ] Stub `tpt-canvas`: `FlattenedPath`, `Affine2`, `FillRule`, `Paint` types
- [ ] Stub `tpt-testkit`: `golden`, `fuzz`, `bench` helpers
- [ ] CI (GitHub Actions): build, test, clippy, fmt, no_std target check (`wasm32-unknown-unknown`), x86_64 + aarch64 + wasm test matrix
- [ ] Verify the crate builds for `thumbv7em-none-eabihf` / riscv no_std target

## Phase 1 — Surfaces & Basic Compositing (Weeks 1–2)
### Module A: `surface`
- [ ] `PixelFormat` trait (`Storage` type, channel count, bytes per pixel)
- [ ] Pixel formats: `Rgba8`, `Rgba32F`, `Gray8`
- [ ] `Rgba16F` with hand-rolled f16<->f32 conversion (round-to-nearest-even, subnormals, inf/NaN) + exhaustive 65536-value test
- [ ] `SurfaceBuffer<P>`: owned (`Vec`) and borrowed (`&mut [..]`) backing
- [ ] Configurable stride (row pitch) with validation
- [ ] `Surface<P>`: `new`, `from_slice`, `get_pixel`, `set_pixel`, `as_slice`, `as_mut_slice`, row iterators
- [ ] Bounds checking policy (panic vs. `Option`) decided and documented
- [ ] `ColorSpaceId` carried on every surface
- [ ] Unit tests: stride, borrowed buffers, edge pixels, zero-size surfaces
### Module C (scalar baseline)
- [ ] `BlendMode` trait (`blend(src, dst) -> [f32; 4]`)
- [ ] Porter-Duff `SourceOver` (premultiplied) scalar
- [ ] `composite_surface` with `opacity`, scalar
- [ ] Cross-format composite (Rgba8 <-> f32 conversion paths)
- [ ] Integrate `tpt-color` premultiplied alpha
- [ ] Different-color-space auto-conversion on composite
- [ ] Deterministic rounding: `f32 -> u8` through one shared function
- [ ] Phase 1 golden tests

## Phase 2 — Path Rasterization (Weeks 3–5)
### Module B: `rasterize`
- [ ] Edge setup from `FlattenedPath` + `Affine2` transform
- [ ] Clipping to surface bounds (incl. off-screen/huge coordinates)
- [ ] Active Edge Table scanline engine
- [ ] Analytic area/coverage accumulation (fixed-point or strictly-ordered f32)
- [ ] `NonZero` winding rule
- [ ] `EvenOdd` winding rule
- [ ] Coverage mask -> pixels (mask × paint -> composite)
- [ ] `Paint`: Solid
- [ ] `Paint`: Linear gradient
- [ ] `Paint`: Radial gradient
- [ ] `Paint`: Pattern (surface-backed)
- [ ] Robustness: NaN/Inf, zero-area, self-intersecting, degenerate edges, enormous coords
- [ ] Golden tests: circles, rounded rects, stars (both fill rules), thin lines, overlapping translucent shapes
- [ ] **Milestone:** complex paths with smooth edges

## Phase 3 — SIMD Acceleration (Weeks 6–8)
- [ ] Study/confirm `tpt-simd` API; add thin abstraction layer if needed
- [ ] SIMD `SourceOver` (8/16 px per iteration) with scalar tail handling
- [ ] SIMD coverage accumulation & mask-apply in rasterizer
- [ ] SIMD Rgba8 <-> f32 pack/unpack, `round_to_u8`
- [ ] Scalar fallback behind feature flag
### Module D: `sample`
- [ ] Interpolation: Nearest
- [ ] Interpolation: Bilinear (SIMD)
- [ ] Interpolation: Bicubic Mitchell-Netravali (SIMD)
- [ ] Interpolation: Lanczos3
- [ ] Edge modes: Clamp, Repeat, Mirror, Transparent
- [ ] Mipmap generation + level selection for downscaling
- [ ] Transformed image draw (scale/rotate/skew via `Affine2`)
### Verification
- [ ] SIMD-vs-scalar byte-for-byte equivalence test harness (whole test suite runs twice)
- [ ] Criterion-style benchmarks baseline (scalar vs. SIMD)
- [ ] **Milestone:** ≥10× over scalar baseline

## Phase 4 — Filters & Advanced Blending (Weeks 9–11)
### Module E: `filter`
- [ ] Separable convolution framework (horizontal + vertical passes)
- [ ] Gaussian blur (kernel generation deterministic; edge handling)
- [ ] Sharpen
- [ ] Edge detect (Sobel or similar)
- [ ] 5×4 color matrix engine
- [ ] Presets: brightness, contrast, saturation, hue rotate, sepia
- [ ] Morphology: dilate, erode, open, close
- [ ] Filters on Rgba8, Rgba16F, Rgba32F, Gray8 (as applicable)
### Module C (complete)
- [ ] Remaining Porter-Duff: SourceIn, SourceOut, SourceAtop, Dest*, DestinationOut, Xor, Clear, Plus, etc.
- [ ] CSS/SVG blend modes (15+): Multiply, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight, SoftLight, Difference, Exclusion, Hue, Saturation, Color, Luminosity
- [ ] SIMD versions of all blend modes
- [ ] Pipeline demo: blurred translucent card (spec §4) as integration test
- [ ] **Milestone:** HTML5 Canvas 2D compositing parity (compare against reference values)

## Phase 5 — Integration & Polish (Week 12)
- [ ] `tpt-canvas::RasterBackend` integration (against stub, then real when available)
- [ ] Fuzzing: random/malformed paths into `rasterize_path`; never panic/OOB (run millions of cases)
- [ ] Fuzz filters, sampler, and composite with random dims/strides
- [ ] Final golden-file suite (cross-platform: x86_64, ARM64, WASM identical)
- [ ] Benchmarks in CI; fail on >5% regression
- [ ] API docs, README, examples, `unsafe` audit
- [ ] CHANGELOG, license, crate metadata
- [ ] Publish v1.0.0

## Cross-Cutting
- [ ] Determinism review: no FMA contraction, fixed op ordering, no f64 pixel math, no `libm`-dependent functions without a deterministic implementation
- [ ] Minimize `unsafe`; document every block
- [ ] No-allocation hot paths (scratch buffers reused)
- [ ] Error type design (`no_std` compatible)
