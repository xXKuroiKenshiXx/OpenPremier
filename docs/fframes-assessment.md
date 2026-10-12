# fframes assessment for OpenPremier

Reviewed 2026-10-06. Upstream source revision:
`30b48f3da60040e0fd7c811fdcd5b159e70a5b02`; the main crate declares version 1.2.0.
The initial assessment below was followed by an implemented, separately compiled CPU export
worker. See [render-engines.md](render-engines.md) for the selectable engine, tested scope and
limitations. The editor still uses wgpu for preview and its complete native effect catalog.

## What it provides

[fframes](https://github.com/dmtrKovalenko/fframes) generates videos from Rust functions that
return SVG trees. Scenes, animation and audio are described in code. Its renderer rasterizes
frames and encodes the result with FFmpeg. It offers diagnostics, contact sheets, image
snapshots, audio analysis and preview tools. The Rust library defaults to CPU rendering;
the separate Skia backend provides GPU rendering. See its
[pipeline](https://github.com/dmtrKovalenko/fframes/blob/30b48f3da60040e0fd7c811fdcd5b159e70a5b02/fframes/src/lib.rs)
and [feature flags](https://github.com/dmtrKovalenko/fframes/blob/30b48f3da60040e0fd7c811fdcd5b159e70a5b02/fframes/Cargo.toml).

Its native core uses Rust 2024. The optional browser editor uses React/ReScript and a WebAssembly
bridge; adopting the core would not require replacing OpenPremier's egui interface. See
[editor dependencies](https://github.com/dmtrKovalenko/fframes/blob/30b48f3da60040e0fd7c811fdcd5b159e70a5b02/fframes-editor/package.json).

## Fit with the existing editor

The following mapping is an OpenPremier design assessment, not an upstream compatibility claim.

| Capability | Useful role here | Acceptance evidence still needed |
|---|---|---|
| Animated SVG/text/scenes | Original title and motion graphic generator, exposed in Essential Graphics | Editable parameters, glyph/font coverage, transparent compositing and undo |
| Cached static SVG subtrees | Candidate optimization for repeated vector graphics | Actual measurements against existing renderer with identical output |
| Snapshot/strip/inspect tools | Reference for a frame regression and export review workflow | Fixtures for alpha, edges, fonts, effects and exact timeline positions |
| Audio analysis | Offline loudness/peak report | Calibrated synthetic signals and agreement with project routing |
| Browser editor | Optional separate authoring tool | No present reason to replace egui or docking |

fframes does not establish `.prproj` fidelity, Premiere timeline semantics, Adobe effect parity,
multicam behavior, an Adobe binary-plugin host, adaptive audio routing or HDR/color equivalence.
Those remain separate items in the [audit](audit-2026-10-06.md).

## Direct dependency: confirmed blocker

An isolated `cargo metadata` probe combined `fframes = "=1.2.0"` (default features disabled)
with the same `ffmpeg-next = "9"` codec/format/scaling/resampling features as OpenPremier.
It failed with exit code 101 before any build scripts or target programs were executed:

```text
ffmpeg-sys-fframes links to the native library ffmpeg,
but it conflicts with ffmpeg-sys-next, which links to ffmpeg as well.
```

Both published bindings declare `links = "ffmpeg"`. Cargo permits only one package declaring
the same native link in one dependency graph; see the
[Cargo resolver rule](https://doc.rust-lang.org/cargo/reference/resolver.html#links).
The private probe, log and crate checksum receipt are in
`local-research/fframes-assessment/`. This establishes dependency-resolution failure for this
combination, not compilation or performance of a standalone fframes program.

## Other adaptation work

The upstream [Video trait](https://github.com/dmtrKovalenko/fframes/blob/30b48f3da60040e0fd7c811fdcd5b159e70a5b02/fframes/src/video.rs)
declares `FPS: usize`, and its encoder-facing video information uses integer FPS. OpenPremier
keeps exact rational rates such as 30000/1001. Rounding them to 30 would change timing.
A bridge must preserve exact project time or limit supported rates explicitly.

Skia/Vulkan is a different rendering integration from wgpu. Sharing pixels first is possible;
sharing GPU resources requires explicit device, synchronization and ownership design. The
upstream [frame export interface](https://github.com/dmtrKovalenko/fframes/blob/30b48f3da60040e0fd7c811fdcd5b159e70a5b02/fframes/src/renderer/frame_export.rs)
supports software/hardware frames, but does not by itself supply a wgpu adapter. HDR, straight
versus premultiplied alpha, cancellation and error propagation need tests before use.

The project carries an [MIT license](https://github.com/dmtrKovalenko/fframes/blob/30b48f3da60040e0fd7c811fdcd5b159e70a5b02/LICENSE.txt).
Any incorporated portion must retain its required notices; codec/Skia/FFmpeg notices remain
part of packaging review. The initial assessment added no runtime code; the subsequent worker
depends on the published fframes 1.2.0 crate and ships its required notices.

## Initial recommended implementation order (historical)

1. Keep a standalone Rust generator outside the editor's Cargo dependency graph. Exchange a
   versioned description of parameters/times and generated media through ordinary files.
2. Prove one short title animation with alpha, exact declared duration and reimport, using a
   supported integer rate first. Measure startup/render cost and cancellation behavior.
3. Add exact rational-time support before accepting NTSC projects. Keep generation/reimport one
   project transaction and preserve the editable description next to the generated asset.
4. Consider an in-process integration only after unifying FFmpeg bindings and validating GPU
   resource transfer. A dependency rename alone does not solve native library ownership.

The standalone CPU worker now compiles and renders actual project scenes, preserves rational
time through the editor's sampler, and exports through the editor's codecs/audio mixer. Seven
integration tests cover pixels, alpha, cancellation, media, glyphs, nesting and rejection; this
is export integration rather than the proposed generation/reimport workflow. There is still no
OpenPremier-versus-fframes benchmark or complete differential pixel-parity corpus.

## GPU continuation, 2026-10-07

The isolated worker now also uses published fframes_skia_renderer 1.2.0 and Skia 0.153.3.
A physical Vulkan GPU is required in GPU mode; local pixel/export acceptance ran on an
NVIDIA RTX 3090 Ti. CPU remains selectable. Scope, native-dependency review and remaining
integration work are documented in [render-engines.md](render-engines.md) and
[fframes-completion.md](fframes-completion.md). This does not establish full editor parity
or a performance advantage; there is still no complete differential image corpus.
