# tpt-raster — v1.0 Task Checklist

Source of truth: [spec.txt](spec.txt)

## Decisions (from clarifying Q&A)
- Cargo **workspace**, stable Rust (latest), `no_std + alloc` core
- `tpt-simd` is real (on GitHub) → git dependency; `tpt-color`, `tpt-canvas`, `tpt-testkit` → **local stub crates** with the APIs named in the spec
- Hand-rolled f16 <-> f32 (no `half` crate)
- Blend math runs on the surface's values as-is (Canvas 2D parity); conversion only when two surfaces' color spaces differ
- All pixel math in f32, never f64; final rounding via `round_to_u8()`
- Scope of this session: checklist only, no code *(superseded: session 2 implements Phase 0 + the Phase 1 scalar baseline)*
- **Surface values are straight (unpremultiplied) alpha.** Compositing premultiplies on read and unpremultiplies on write.
- **Canonical f32 domain is `0.0..=1.0`.** `Rgba8`/`Gray8` convert via `unit_to_u8(v) = round_to_u8(v * 255)`.
- **Stride is in pixels**, not bytes (each pixel is one `Storage` element).
- **`round_to_u8` is local** (`tpt-raster::round`). tpt-simd has no equivalent at the pinned rev.
- **tpt-simd member crates used:** `tpt-simd-core`, `tpt-simd-rounding`, `tpt-simd-saturate`, pinned to rev `be4f4c4e9dd8fa60460dcf6aae2afadab5ccd077`. They are optional behind the `simd` feature, off by default until Phase 3.
- `Surface<'a, P>` carries a lifetime because the buffer can be borrowed. Owned surfaces are `Surface<'static, P>`.
- Bounds policy: `get_pixel` / `set_pixel` panic, `try_get_pixel` / `try_set_pixel` report failure.

## Open items (blocking)
- [x] tpt-simd repo: https://github.com/tpt-solutions/tpt-simd (branch `master`, v0.1.0 pre-release, no tags/releases, Apache-2.0 OR MIT)
- [x] Pin a git `rev` for tpt-simd (`be4f4c4e9dd8fa60460dcf6aae2afadab5ccd077`, set in workspace `Cargo.toml`). Upstream `master` has moved on (0.1.1, phases 11–13); the pin is intentionally not bumped.
- [x] Inspected tpt-simd source (cloned to scratchpad). Findings:
  - **`round_to_u8` does not exist.** Closest: `tpt-simd-rounding::round_to_nearest_even_i32(F32x8) -> I32x8` (saturating, NaN→0), `round_with_bias_i32`, and `tpt-simd-saturate::pack_i16_to_u8` / `pack_i32_to_i16`. Plan: build `f32→u8` locally in tpt-raster from these (round-ties-even → clamp 0..255 → pack), or contribute `round_to_u8` upstream. Spec's Architect's Note should be updated accordingly. **Not yet done: `spec.txt` still names `tpt-simd`'s `round_to_u8()`.**
  - Fixed lane widths: `Simd<T, N>` with `f32×8`, `i32×8`, `i8×32`, `i16×16`, `u8×16`; const-generic `N`. Spec's "8 or 16 pixels at a time" = 8 f32 lanes (2 RGBA px) per op, so design loops around F32x8.
  - Has `swizzle([usize; N])`, `sat_add/sat_sub/sat_mul`, `clamp`, `min/max`, `simd_lt/gt..` masks + `select`, `cast`, `from_slice`/`store_partial`, `permute`, `transpose_4x4_f32`, `tpt-simd-blend` (select/blend), `tpt-simd-convolve`, `tpt-simd-interpolate` (inspect before writing own versions).
  - MSRV **1.95**, edition **2024**, toolchain = stable (installed rustc 1.98 OK). tpt-raster uses edition 2024 / rust-version 1.95.
  - Scalar fallback exists (`scalar-only` feature); `std` feature for intrinsics; `nightly` feature exists — don't enable.
  - Uses `libm` for lane math; `lane_mul_add` uses hardware `mul_add` under `std`, libm `fmaf` otherwise (both correctly-rounded fused, should match) — **but** spec forbids relying on contraction ambiguity: avoid `mul_add` in pixel math unless fused-everywhere is guaranteed, and never build with `-C target-cpu=native` in golden/CI runs.
- [ ] Decide which `tpt-simd-*` member crates to depend on — **decided** (see Decisions); revisit once Phase 3 needs more.
- [ ] Verify cross-platform bit-identity of rounding/saturate/blend ops on x86_64 vs. ARM64 vs. WASM (tpt-simd is v0.1.0, 14 commits — treat as unproven; our SIMD-vs-scalar equivalence tests double as its validation). **Blocked here:** CI matrix covers Linux x86_64, Linux ARM64, macOS, Windows, but no run has happened yet, and no wasm test runner is set up.
- [ ] Feed gaps found in tpt-simd (e.g. `round_to_u8`, u8↔f32 pack/unpack for RGBA) back upstream as issues/PRs — **needs your go-ahead** (outward-facing).

---

## Phase 0 — Workspace Setup
- [x] `git init`, `.gitignore`, `rust-toolchain.toml` (stable) *(repo already initialised; toolchain file added)*
- [x] Workspace `Cargo.toml` (members: `tpt-raster`, `stubs/tpt-color`, `stubs/tpt-canvas`, `stubs/tpt-testkit`)
- [x] Add `tpt-simd` as git dependency *(member crates, optional behind `simd`; see Decisions)*
- [x] `tpt-raster` crate: `#![no_std]`, `extern crate alloc`, feature flags (`std`, `simd`, `scalar-fallback`) *(`scalar-fallback` is a no-op until Phase 3)*
- [x] Stub `tpt-color`: `ColorSpaceId`, premultiply/unpremultiply, conversion fn (sRGB <-> linear at minimum) *(also `convert_premultiplied`)*
- [x] Stub `tpt-canvas`: `FlattenedPath`, `Affine2`, `FillRule`, `Paint` types *(`Paint` has `Solid` only)*
- [x] Stub `tpt-testkit`: `golden`, `fuzz`, `bench` helpers
- [x] CI (GitHub Actions): build, test, clippy, fmt, no_std target check (`wasm32-unknown-unknown`), x86_64 + aarch64 + wasm test matrix *(`.github/workflows/ci.yml`. Written, **not yet run on GitHub**. No wasm test runner yet, so the wasm entry is build-only.)*
- [~] Verify the crate builds for `thumbv7em-none-eabihf` / riscv no_std target — **thumbv7em ✓** (with and without `simd`), **wasm32 ✓**. riscv not checked (target is installed; not run yet).

## Phase 1 — Surfaces & Basic Compositing (Weeks 1–2)
### Module A: `surface`
- [x] `PixelFormat` trait (`Storage` type, channel count, bytes per pixel) *(`Storage` is one element per pixel, so there is no separate channel count)*
- [x] Pixel formats: `Rgba8`, `Rgba32F`, `Gray8`
- [x] `Rgba16F` with hand-rolled f16<->f32 conversion (round-to-nearest-even, subnormals, inf/NaN) + exhaustive 65536-value test
- [x] `SurfaceBuffer<P>`: owned (`Vec`) and borrowed (`&mut [..]`) backing
- [x] Configurable stride (row pitch) with validation
- [x] `Surface<P>`: `new`, `from_slice`, `get_pixel`, `set_pixel`, `as_slice`, `as_mut_slice`, row iterators
- [x] Bounds checking policy (panic vs. `Option`) decided and documented
- [x] `ColorSpaceId` carried on every surface
- [x] Unit tests: stride, borrowed buffers, edge pixels, zero-size surfaces
### Module C (scalar baseline)
- [x] `BlendMode` trait (`blend(src, dst) -> [f32; 4]`) *(static fn per spec; `composite_surface` takes the mode by value as a generic `B`, since `impl Trait` args cannot name `B::blend`)*
- [x] Porter-Duff `SourceOver` (premultiplied) scalar
- [x] `composite_surface` with `opacity`, scalar
- [x] Cross-format composite (Rgba8 <-> f32 conversion paths)
- [x] Integrate `tpt-color` premultiplied alpha
- [x] Different-color-space auto-conversion on composite
- [x] Deterministic rounding: `f32 -> u8` through one shared function
- [~] Phase 1 golden tests — **known-value byte tests** are in `tpt-raster/tests/composite.rs` and `tests/pixel.rs`. File-based golden PNGs are deferred until the real `tpt-testkit::golden`.

## Phase 2 — Path Rasterization (Weeks 3–5)
### Module B: `rasterize`
- [x] Edge setup from `FlattenedPath` + `Affine2` transform
- [x] Clipping to surface bounds (incl. off-screen/huge coordinates) — edges split at x=0 and x=w, y clipped per row
- [~] Active Edge Table scanline engine — **replaced** by signed-area accumulation (row-by-row, no edge table). Same analytic result, different structure; the AET is not built.
- [x] Analytic area/coverage accumulation (strictly-ordered f32, prefix sum per row)
- [x] `NonZero` winding rule
- [x] `EvenOdd` winding rule — exact for non-overlapping edges; approximate where edges overlap inside one pixel (same limitation as font-rs)
- [x] Coverage mask -> pixels (mask × paint -> composite) — `coverage_mask`, `fill_mask`, `rasterize_path`
- [x] `Paint`: Solid
- [x] `Paint`: Linear gradient (2-stop, clamped; stub `tpt-canvas::Paint`)
- [x] `Paint`: Radial gradient (2-stop, clamped)
- [ ] `Paint`: Pattern (surface-backed) — needs a surface-backed `Paint` variant in tpt-canvas; not started
- [x] Robustness: NaN/Inf dropped, zero-area and degenerate contours ignored, coordinates clamped to ±1e15, 5k-case fuzz with no panics or out-of-range coverage. Found and fixed: `dxdy` overflow gave NaN on near-horizontal edges.
- [~] Golden tests: squares, half-pixel edges, triangle bytes, nesting under both rules, clipping, huge coords. **Circles, rounded rects, stars and thin lines are not yet covered by explicit goldens** (star polygons are covered by the area-conservation test).
- [~] **Milestone:** complex paths with smooth edges — area-conservation holds on 300 random star-shaped polygons (within 1e-3 relative). Visual inspection not done.

## Phase 3 — SIMD Acceleration (Weeks 6–8)
- [ ] Study/confirm `tpt-simd` API; add thin abstraction layer if needed
- [ ] SIMD `SourceOver` (8/16 px per iteration) with scalar tail handling
- [ ] SIMD coverage accumulation & mask-apply in rasterizer
- [ ] SIMD Rgba8 <-> f32 pack/unpack, `round_to_u8`
- [~] Scalar fallback behind feature flag — `scalar-fallback` feature declared, no SIMD code yet to switch off
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
- [x] Separable convolution framework (horizontal + vertical passes) — `convolve_separable`, clamp-to-edge
- [x] Gaussian blur (kernel generation deterministic; edge handling) — radius `ceil(3σ)`, σ clamped to 256, σ<1e-3 is identity, clamp-to-edge
- [x] Sharpen — unsharp 3×3, brightness-preserving
- [x] Edge detect (Sobel) — straight-alpha luma, alpha kept
- [x] 5×4 color matrix engine — `apply_color_matrix`, straight colour, clamped
- [x] Presets: brightness, contrast, saturation, hue rotate, sepia — W3C / CSS formulas
- [x] Morphology: dilate, erode, open, close — separable square, out-of-bounds ignored, radius capped to the surface size
- [x] Filters on Rgba8, Rgba16F, Rgba32F, Gray8 — generic over `PixelFormat`, tested on all four
  *(Blur, convolution and morphology run on premultiplied values. Edge detect uses straight luma. Colour matrices use straight colour.)*
  *(Each filter allocates one f32 buffer per pass. No per-pixel allocation.)*
### Module C (complete)
- [x] Remaining Porter-Duff: Clear, Source, Destination, DestinationOver, SourceIn, DestinationIn, SourceOut, DestinationOut, SourceAtop, DestinationAtop, Xor, Plus (`src/modes.rs`)
- [x] CSS/SVG blend modes (15): Multiply, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight, SoftLight, Difference, Exclusion, Hue, Saturation, Color, Luminosity — W3C formula, scalar f32
- [ ] SIMD versions of all blend modes — Phase 3 blocked on tpt-simd gaps (see Open items)
- [x] Pipeline demo: blurred translucent card (spec §4) as integration test — `tests/pipeline.rs`. **Deviation:** the card is a shape on a transparent surface. The spec's "clear the surface to translucent white" would tint the whole screen.
- [~] **Milestone:** HTML5 Canvas 2D compositing parity — the operators are checked against hand-computed values and identities, **not against browser reference output**. Parity is unverified.

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
- [~] Determinism review: no FMA contraction, fixed op ordering, no f64 pixel math, no `libm`-dependent functions without a deterministic implementation — **no f64 and no `unsafe` in use; Rust does not contract `a*b+c` by default.** `libm` is used for `powf`, `rintf`. Those are software implementations, but they have not been reviewed for determinism.
- [x] Minimize `unsafe`; document every block — *`tpt-raster` and the stubs use `#![forbid(unsafe_code)]`.*
- [ ] No-allocation hot paths (scratch buffers reused) — composite allocates nothing per pixel, but no scratch-buffer design exists yet
- [x] Error type design (`no_std` compatible) — `SurfaceError` implements `Display` via `core::fmt`
