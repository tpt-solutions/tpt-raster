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
  - **`round_to_u8` does not exist.** Closest: `tpt-simd-rounding::round_to_nearest_even_i32(F32x8) -> I32x8` (saturating, NaN→0), `round_with_bias_i32`, and `tpt-simd-saturate::pack_i16_to_u8` / `pack_i32_to_i16`. Plan: build `f32→u8` locally in tpt-raster from these (round-ties-even → clamp 0..255 → pack), or contribute `round_to_u8` upstream. Spec's Architect's Note should be updated accordingly. **Done:** `spec.txt` now describes the local `round_to_u8()`, built from tpt-simd's rounding and saturate primitives.
  - Fixed lane widths: `Simd<T, N>` with `f32×8`, `i32×8`, `i8×32`, `i16×16`, `u8×16`; const-generic `N`. Spec's "8 or 16 pixels at a time" = 8 f32 lanes (2 RGBA px) per op, so design loops around F32x8.
  - Has `swizzle([usize; N])`, `sat_add/sat_sub/sat_mul`, `clamp`, `min/max`, `simd_lt/gt..` masks + `select`, `cast`, `from_slice`/`store_partial`, `permute`, `transpose_4x4_f32`, `tpt-simd-blend` (select/blend), `tpt-simd-convolve`, `tpt-simd-interpolate` (inspect before writing own versions).
  - MSRV **1.95**, edition **2024**, toolchain = stable (installed rustc 1.98 OK). tpt-raster uses edition 2024 / rust-version 1.95.
  - Scalar fallback exists (`scalar-only` feature); `std` feature for intrinsics; `nightly` feature exists — don't enable.
  - Uses `libm` for lane math; `lane_mul_add` uses hardware `mul_add` under `std`, libm `fmaf` otherwise (both correctly-rounded fused, should match) — **but** spec forbids relying on contraction ambiguity: avoid `mul_add` in pixel math unless fused-everywhere is guaranteed, and never build with `-C target-cpu=native` in golden/CI runs.
- [x] Decide which `tpt-simd-*` member crates to depend on — **decided** (see Decisions); revisit once Phase 3 needs more.
- [ ] Verify cross-platform bit-identity of rounding/saturate/blend ops on x86_64 vs. ARM64 vs. WASM (tpt-simd is v0.1.0, 14 commits — treat as unproven; our SIMD-vs-scalar equivalence tests double as its validation). **Blocked here:** CI matrix covers Linux x86_64, Linux ARM64, macOS, Windows, but no run has happened yet, and no wasm test runner is set up.
- [ ] Feed gaps found in tpt-simd (e.g. `round_to_u8`, u8↔f32 pack/unpack for RGBA) back upstream as issues/PRs. **Decision (recommended):** drafted in `scratchpad/tpt-simd-issues-draft.md`, **not posted**, because filing is outward-facing. File after review.
  - **Re-checked upstream (master `ed05b33`, tag v0.1.1):** issues 1 and 2 of the draft are now resolved upstream. `tpt-simd-rounding::round_to_u8` (ties-to-even, clamp, NaN→0, returns `[u8; 8]`) and `tpt-simd-pixel::{planar,interleaved}_rgba8` both landed, so the draft is stale on those and needs updating before any filing. Still open: `tpt-simd-pixel::unit_f32_to_u8` rounds half up (`(v + 0.5) as u8`), which differs from ties-to-even at exact .5 values. Question 3 (cross-target bit-identity) is still unanswered; upstream CI runs x86_64 and aarch64 tests only, and wasm32 is build-only.
  - **Bump test (not applied; pin stays `be4f4c4`):** with `ed05b33` in a temporary worktree, `tpt-raster` tests pass with `simd`, and wasm32 and riscv32imc `simd` builds finish. Upstream's `templates/` manifest prints a non-fatal `{{project-name}}` error during resolution. Upstream's aarch64 rounding now uses NEON `vrndn`/`fcvtzs` directly; the upstream comments say these match the portable definitions, but we have not run that path here.

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
- [x] Verify the crate builds for `thumbv7em-none-eabihf` / riscv no_std target — **thumbv7em ✓** (with and without `simd`), **wasm32 ✓**, **riscv32imac / riscv32imc ✓** (with and without `simd`, `cargo check`).

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
- [~] `Paint`: Pattern (surface-backed) — implemented as `sample::draw_image` with `EdgeMode::Repeat` (tested as a repeating tile). **Not** a `Paint` variant: that needs a surface reference in tpt-canvas, which the stub does not have.
- [x] Robustness: NaN/Inf dropped, zero-area and degenerate contours ignored, coordinates clamped to ±1e15, 5k-case fuzz with no panics or out-of-range coverage. Found and fixed: `dxdy` overflow gave NaN on near-horizontal edges.
- [~] Golden tests: squares, half-pixel edges, triangle bytes, nesting under both rules, clipping, huge coords. Circles, rounded rects, a simple star and thin lines now have **analytic** coverage tests in `tests/rasterize.rs`: circle area against its polygon, rounded-rect area against `w*h - (4-π)r²`, a 0.25-px horizontal band giving exactly 0.25 coverage, and a 0.5-px diagonal band against its geometric area. They check values. Byte goldens pin a circle, a rounded rect and thin lines in `tests/golden.rs`, with literal geometry so the pins are platform-independent.
- [~] **Milestone:** complex paths with smooth edges — area-conservation holds on 300 random star-shaped polygons (within 1e-3 relative). Visual inspection not done.

## Phase 3 — SIMD Acceleration (Weeks 6–8)
- [x] Study/confirm `tpt-simd` API; add thin abstraction layer if needed — `src/simd.rs` is the only layer. It uses `Simd<f32, 8>` with `max`/`min`/`zip_with`/`map`, and `round_to_nearest_even_i32` plus a clamp for rounding. tpt-simd has no operator overloads.
- [~] SIMD `SourceOver` (8 px per iteration) with scalar tail — `simd::composite_source_over`. Row-based, with lane load/store hooks on `PixelFormat`. Rgba8 has a lane path (`byte as f32 / 255`, lane rounding via `unit_to_u8_x8`), so only Rgba8 destinations dispatch to it (`HAS_LANE_PATH`). Bit-exact with the scalar reference. Measured against the crate's own scalar composite: Rgba8 about 1.1–1.4×, Rgba32F slower (0.8–0.9×), so Rgba32F stays scalar. The 10× milestone is not met.
- [ ] SIMD coverage accumulation & mask-apply in rasterizer — **Decision (recommended): deferred.** The per-row prefix sum is sequential, so vectorising it changes the f32 operation order and would invalidate the goldens. Revisit only with a new golden baseline.
- [~] SIMD separable blur — `simd::convolve_plane_x8` runs each pass on channel planes, eight pixels per lane group, with the same tap order as the scalar code. Bit-identical: `convolve_plane_x8` is tested against an independent reference over 300 random cases, and the blur goldens hold with and without `simd`. Measured on 1024×1024 Rgba32F, sigma 4: about 82 ms SIMD against 90–94 ms scalar (≈1.1–1.2×). Default x86_64 is 128-bit SSE2, so 8-lane vectors take two ops. Wider lanes would need a non-default target, which the goldens rule out. The bench is an ignored test, `bench_gaussian_blur_simd_vs_scalar`.
*Profiling (release, 1024×1024 Rgba32F, sigma 4):* one plane pass is about 6–7 ms SIMD against 30–47 ms for the naive reference, so the kernel is about 5× per pass. The full blur is 8 passes plus about 15 ms of load and store, which keeps the total at about 1.1–1.4× the crate's scalar blur. Tried and reverted: row-accumulation for the vertical pass (26 ms), and four independent blocks per pass (no gain). Building for AVX2 (`target-cpu=x86-64-v3`) gave about 1.4× on the horizontal pass and nothing on the vertical. The bottleneck is not vector width alone. Reaching 10× overall would need a different arithmetic order (which changes the goldens) or a fused multi-pass design. Neither is done.
*Opt-in wider vectors (done):* `.cargo/config.toml` has a commented-out `target-cpu=x86-64-v3` block, and the README documents the `RUSTFLAGS` route. The default build is unchanged. Under this build the full suite passes, with the goldens (121 tests with `simd`), so the output matches. Measured 1024×1024 Rgba32F: blur about 1.1–1.2× faster than the default SIMD build. Rgba8 composite about 1.2–1.3× against the test's scalar reference, against 1.1× by default.
- [x] SIMD Rgba8 <-> f32 pack/unpack, `round_to_u8` — `round_to_u8_x8` and `unit_to_u8_x8` match the scalar function on special values and 200k random bit patterns. Rgba8 pack/unpack runs through the `load_lanes`/`store_lanes` hooks in `pixel.rs`, which use the same lane rounding. The loads are still scalar gathers, so the speed-up is in the rounding, not the widening.
- [x] Scalar fallback behind feature flag — `scalar-fallback` forces the scalar composite. Tested both ways: 109 tests with `simd`, 103 without.
### Module D: `sample`
- [x] Interpolation: Nearest (scalar)
- [x] Interpolation: Bilinear (scalar; SIMD version pending)
- [x] Interpolation: Bicubic Mitchell-Netravali (scalar; SIMD pending)
- [x] Interpolation: Lanczos3 (scalar; SIMD pending)
- [x] Edge modes: Clamp, Repeat, Mirror, Transparent
- [x] Mipmap generation + level selection for downscaling — 2×2 box chain, level = floor(log2(downscale)); no trilinear blend between levels
- [x] Transformed image draw (scale/rotate/skew via `Affine2`) — `draw_image`, inverse mapping per destination pixel; returns `SingularTransform` for non-invertible matrices
### Verification
- [~] SIMD-vs-scalar byte-for-byte equivalence harness (whole test suite runs twice) — `tests/simd.rs` compares rounding and composite bit-for-bit against a scalar reference, over random and special inputs. The whole suite also runs with `simd` on (109 tests) and forced scalar (103 tests). It is not yet a byte-diff of every filter and sampler output.
- [~] Criterion-style benchmarks baseline — ignored test `bench_simd_composite_against_scalar` (release, `--nocapture`, best of 5). Noisy: the same run varies by about 20%. No Criterion, and no filter or blend benchmarks yet.
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
- [ ] SIMD versions of all blend modes — **Decision (recommended): deferred.** The blend modes are scalar. Lane versions of 15 formulas need bit-exact op ordering, and the measured SIMD gain so far is small (1.1–1.4×). Revisit if a profile shows blending as a hot spot.
- [x] Pipeline demo: blurred translucent card (spec §4) as integration test — `tests/pipeline.rs`. **Deviation:** the card is a shape on a transparent surface. The spec's "clear the surface to translucent white" would tint the whole screen.
- [~] **Milestone:** HTML5 Canvas 2D compositing parity — the operators are checked against hand-computed values and identities, **not against browser reference output**. Parity is unverified.

## Phase 5 — Integration & Polish (Week 12)
- [x] `tpt-canvas::RasterBackend` integration — the trait is in the stub; `impl RasterBackend for Surface` is in `tpt-raster/src/canvas.rs`, tested with static and `dyn` dispatch (`tests/canvas.rs`). Re-check against the real `tpt-canvas` when it exists: the spec does not define the trait, so its shape is a guess.
- [~] Fuzzing: random/malformed paths into `rasterize_path` — 5k cases in the normal suite, plus an `#[ignore]`d 2M-case run that passes (about 28 s). Not yet run in CI.
- [x] Fuzz filters, sampler and composite with random dims/strides — `tests/stride.rs`: 100–200 random geometries per op, with padding filled by a sentinel. Checks that padding is never written and that strided results equal packed results. Covers composite (f32 and Rgba8), blur, sharpen, morphology, colour matrix, rasterize and draw_image.
- [~] Final golden-file suite (cross-platform: x86_64, ARM64, WASM identical) — `tests/golden.rs` pins FNV-1a hashes of six fixed scenes (star, blur, bicubic rotation, mipmapped draw, Hue blend, morphology). Same hashes with and without `simd`. Cross-platform check needs the CI matrix to run, and WASM has no test runner yet.
- [ ] Benchmarks in CI; fail on >5% regression
- [x] API docs, README, examples, `unsafe` audit — `cargo doc -D warnings` clean; README written; `examples/pipeline.rs` prints the §4 pipeline as ASCII; `#![forbid(unsafe_code)]`.
- [x] CHANGELOG and licence — CHANGELOG written. Licence is MIT OR Apache-2.0, copyright TPT Solutions, with LICENSE-MIT and LICENSE-APACHE. Crate metadata is set. `publish = false` until you decide to publish.
- [ ] Publish v1.0.0 — **Decision (recommended): do not publish v1.0.0 yet.** The 10× milestone is unmet, and the cross-platform checks have not run. Ship as 0.x once CI is green. `publish = false` stays.

## Cross-Cutting
- [~] Determinism review: no FMA contraction, fixed op ordering, no f64 pixel math, no `libm`-dependent functions without a deterministic implementation — **no f64, no `mul_add`, and no `powf` in `tpt-raster/src`; Rust does not contract `a*b+c` by default.** `libm` 0.2.16 (default `arch` feature) overrides only correctly-rounded operations with hardware instructions on x86_64 (`sqrt`, `fma`), aarch64 (`rint`, `sqrt`, `fma`) and wasm32 (`ceil`, `floor`, `rint`, `sqrt`, `trunc`). Those give identical results on every target. `expf`, `sinf`, `cosf`, `log2f` and `fmodf` are pure software on those targets. The exception is 32-bit x86 without SSE2 (x87 `expf` and friends), which is not a target we build for. **Still open:** the `libm` transcendentals are not yet checked bit-for-bit across ARM64 and WASM (needs the CI matrix).
- [x] Minimize `unsafe`; document every block — *`tpt-raster` and the stubs use `#![forbid(unsafe_code)]`.*
- [ ] No-allocation hot paths (scratch buffers reused) — **Decision (recommended): deferred.** Composite allocates nothing per pixel. Filters allocate one f32 buffer per pass, not per pixel. Scratch reuse is an API change with little measured benefit yet.
- [x] Error type design (`no_std` compatible) — `SurfaceError` implements `Display` via `core::fmt`
