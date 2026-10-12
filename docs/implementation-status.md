# Implementation status

The 2026-10-07 HDR continuation adds real PQ/HLG 10-bit delivery, source conversion, HDR10
metadata and Windows Vulkan/DX12/WARP plus Linux WSL acceptance. See
[hdr-and-platform-acceptance.md](hdr-and-platform-acceptance.md) for the tested scope, runtime
block on interactive QA and remaining platform/legal requirements.

**Snapshot:** 2026-10-07<br>
**Application version:** 0.10.0 technical alpha<br>
**Primary language/UI:** Rust 2024 / egui 0.36<br>
**Compatibility validation:** in progress; full parity has not been established

This file records repository reality. It does not replace the normative Premiere-compatibility
specifications or upgrade static observations into validated behavior.

The latest fframes continuation adds explicit hybrid compatibility using shared native source
and effect preparation with real Skia floating-point composition. Ordinary tracks remain separate;
adjustments, destination-dependent blends and linear composition use named native checkpoints.
All 92 effects, 43 transitions and 27 blend modes are available through this bridge, including
captions, feather, source interpretation and nesting. They are not claimed as independent SkSL ports.
Export progress identifies both processors. CPU compatibility uses Skia raster and a software
native effects adapter; GPU requires physical Skia and native adapters without a CPU fallback.

RGBA16F transport preserves premultiplied alpha, superwhite and sub-eight-bit values.
CPU Skia composition uses RGBAF32 with explicit half conversions to preserve tiny shadows/alpha;
the GPU surface and wire protocol remain RGBA16F. Regression tests cover every positive half
subnormal and small-alpha source-over. High-bit
delivery codecs use native GPU delivery conversion after Skia composition, avoiding the old
RGBA8 bottleneck. PQ/HLG sources follow the same conversion as the native editor; export is
now selectable as SDR BT.709 or real Rec.2100 PQ/HLG / BT.2020 / 10-bit HEVC/ProRes.
HDR grading monitors and full creative-node mastering acceptance remain pending.

Source/Program monitors now select native/fframes CPU/fframes GPU in Preferences. Coalesced
background requests, stale-snapshot guards, persistent contexts and nonblocking cancellation
preserve the UI thread. Interactive monitor QA remains unverified.

Six additional differential/runtime tests exercise both floating backends, the entire effect
catalog, three samples per transition, all blends, caption styles/feather/adjustments, interpreted
RGBA16 PQ/HLG/rotation/PAR, preview coalescing and all six exported codecs against native output.
The independent SVG subset retains its ten earlier integration tests. The last prior acceptance
had 308 passing tests; see the current acceptance in [fframes-completion.md](fframes-completion.md).
Optional model/stress/image tests and the proprietary project corpus retain earlier evidence.

Independent effect ports, shared GPU resources, HDR grading/creative-node acceptance, performance benchmarks and
cross-hardware/platform acceptance still remain. No 100% integration claim is made. See
[render-engines.md](render-engines.md), [fframes-completion.md](fframes-completion.md) and
[fframes-coverage.json](fframes-coverage.json). Skia archive/static-library identity and MSVC
runtime origins are recorded; their native-source/embedded-notice/license review is pending.

The 2026-10-07 continuation adds persistent interface density (Comfortable by default), roomier
shared widgets/dock/list/parameter/transport areas, and opt-in automatic proxies with 540p/720p/
1080p choices. Seventeen panels and saved layouts remain. Background proxy results carry source
identity and old-project queues are isolated. See [interface-and-proxies.md](interface-and-proxies.md).

The licensing review now retains original crate copyright/license/NOTICE files, including pinned
upstream supplements omitted by published crates, and creates exact application source snapshots.
Public distribution review remains pending for native corresponding-source completeness and
historical proprietary-research provenance. OpenFX/Wasm/VST3 remain planned standards, not the
native Quick Actions palette. See [legal/audit-2026-10-07.md](legal/audit-2026-10-07.md).

The preceding UI/proxy continuation passed **305 workspace tests, 0 failures, 5 optional tests ignored**, requiring
GPU, native software and fframes. The optional model/stress/image tests retain their earlier
302-test acceptance evidence; they were not rerun in this continuation. UI control/layout tests
exercise all three densities, with headless egui style artifacts inspected. Interactive all-panel
and multi-monitor/DPI acceptance remains pending. Prior renderer acceptance below is historical;
its package checksum identifies the preserved previous package, not the new source tree.

Version 0.10.0 also adds plain UTF-8 `.prproj` input without a version-number gate, a standalone
copy-only Project-version patch, partial progressive `.sqpreset` loading and the OpenJumpCuts
native preset/settings adapter. See [premiere-compatibility.md](premiere-compatibility.md) and
[openjumpcuts.md](openjumpcuts.md) for scope and limitations. General `.prproj` export and
universal cross-release compatibility remain unvalidated.

Version 0.10.0 adds bundled native plugin preferences, temporal curve editing, folder portals,
action search/macros, clipboard event and destination fixes, native audio speed ramps and GPU
manga/parallax/page tools. See [native-plugins.md](native-plugins.md). External Adobe/audio plugin
hosting is not implemented; these are independent native tools.

The current unreleased work adds caption split/merge/start-end editing and three MCP tools,
native 5.1 routing, acknowledged recovery, coordinated native saves, bounded graph validation
and saved-state history identity. It also includes validation of clip speeds/source identities,
exact autosave retention, complete XML text budgets and explicit GPU readback errors.
See [audit-2026-10-06.md](audit-2026-10-06.md) and
[caption-editing.md](caption-editing.md). Existing panels and local work were preserved.

Earlier CPU-only renderer acceptance (historical): native wgpu GPU/software and an
isolated fframes 1.2.0 CPU worker. The worker handles a conservative SDR subset, preserves
rational output times and native audio/codecs, and requires no GPU in a headless export.
Unsupported scenes fail before opening the destination. Preview retains wgpu; the Skia GPU
backend is not integrated. See [render-engines.md](render-engines.md).

This renderer working tree passed workspace rustfmt/Clippy with warnings denied and separate
worker rustfmt/Clippy. The sequential workspace run required GPU, fframes and native software,
with the same three local `.prproj` templates: **297 passed, 0 failed, 5 optional ignored**.
All five optional tests were then executed and passed: **302 executed, 0 failures**.
The seven new renderer tests include all six codecs, animated alpha, exact 30000/1001 timing,
decoded media with audible native mixing, nested text/shapes, cancellation, destination
preservation and Microsoft Basic Render Driver (Dx12) with a WGSL effect. This is functional
acceptance of the documented subset, not complete renderer or Premiere parity.

The final renderer Windows ZIP passed both final-layout self-tests, including fframes inline
PNG/straight-alpha pixels, with the development FFmpeg removed from PATH. Packaged MCP exposed
33 tools and the three renderer values, exported a three-frame title with each engine, and
all outputs decoded to visible glyph pixels. GPU/software/fframes respectively reported RTX
3090 Ti (Vulkan), Microsoft Basic Render Driver (Dx12), and fframes 1.2.0 / CPU. The tested
runtime files are byte-identical to the final package; the last packaging pass added the exact
upstream MIT notice omitted by the published crates. Windows packaging now refuses existing
stage/ZIP destinations, verified without altering the generated package.

Final local ZIP: `dist/OpenPremier-0.10.0-windows-x64.zip`, **75,192,235 bytes**, SHA-256
`d1bccd635e9110a1a4a79745539a4cadcc4d2e43cb90ca48ca43d44208a9f7c2`.
The prior 295-test package remains in `local-research/build-baseline/295-tests/` with its
original checksum. No release was published; Linux/macOS were not rebuilt for this worker.

## Earlier continuation validation (before alternative renderers)

On 2026-10-06 this working tree passed `cargo xtask lint` and
`OPENPREMIER_REQUIRE_GPU=1 cargo xtask test -- --test-threads=1`: **290 passed, 0 failed,
5 optional tests ignored**. `OPENPREMIER_PRPROJ_DIR` pointed to three local Premiere templates;
the separate corpus run reported seven caption/data tracks and two style items skipped in Social.
The corpus test now rejects an explicitly supplied directory with no projects. This run does
not establish Adobe parity.

The five optional tests were then executed separately and all passed: sequence auto-captions,
speech-word detection, transcription with times, eight original image-format fixtures, and
100-frame-per-case 1080p GPU stress. **295 tests executed in total, 0 failures; all five optional
tests executed.** The voice fixture uses synthetic English speech and a pinned official Whisper
Tiny model; it is not a multilingual/noisy-speech accuracy corpus. Plain transcription and
auto-captions contained the expected text; the silence-word analysis with aggressive filler/retake
options also produced spurious trailing text. Assertions on language, selected words and time bounds
do not establish word accuracy or audio alignment.

The stress run used the development profile on RTX 3090 Ti, not a release benchmark: overall
throughput was 8.4/8.1/6.1/5.8 fps for plain/10 effects/100 effects/10 layers × 10 effects.
Measured readback was 111–121 ms per frame; this is a performance investigation target and does
not support a real-time playback claim. Logs and fixture/model receipts are kept privately.

The final continuation Windows release build and `cargo xtask dist --windows-only` passed.
Its final-layout self-test found FFmpeg 62.12.103, libx264/AAC/ProRes/PNG encoders and NVIDIA
GeForce RTX 3090 Ti / Vulkan, returning compositor pixel `[255, 0, 0, 255]`. A separate stdio
smoke check of the packaged executable completed `initialize` and `tools/list`: version 0.10.0,
33 tools, including all three new caption tools. It checks protocol startup/listing, not live
window edits. The harness explicitly redirects streams and waits for the GUI-subsystem process.

Earlier local ZIP, preserved in `local-research/build-baseline/295-tests/`:
`OpenPremier-0.10.0-windows-x64.zip`, **71,617,630 bytes**, SHA-256
`b980e753d5e1f566c2a7b460d6e06c42241972acb60ec6adf67393ed3ddc8185`.
The earlier 244-test package and its folder were preserved under `local-research/build-baseline/244-tests/`.
No release was published; Linux/macOS were not rebuilt in this continuation.

## Earlier 0.10.0 build baseline

The following source checks completed for 0.10.0 on the Windows development host using the
checked-in `xtask` flow. Package results are recorded separately below.

| Check | Result |
|---|---|
| `cargo xtask lint` | rustfmt and Clippy passed with warnings denied |
| `cargo xtask test -- --test-threads=1` | 226 tests passed on Windows; 5 optional tests ignored |
| Windows release package | 0.10.0 ZIP built; final-layout `--self-test` passed with FFmpeg 62.12.103 and NVIDIA GeForce RTX 3090 Ti / Vulkan |
| Linux AppImage | Historical baseline: WSL `--self-test` and startup in Ubuntu 22.04 and Fedora 42 containers passed; 0.10.0 has not been rebuilt or tested on Linux |

The multimedia suite was run sequentially after a parallel run stalled in background media jobs.
CI is configured to run the suite on Linux (software Vulkan) and macOS (Metal), with GPU tests
required; that configuration is not evidence of a completed 0.10.0 run on those platforms.

The self-test exercises packaged FFmpeg discovery, required encoders, GPU adapter creation, and a
known compositor pixel. It is a startup/integration check, not a performance or parity benchmark.

## Implemented subsystem map

| Subsystem | Present behavior | Important limit |
|---|---|---|
| Core model | Exact rational time, items/bins, tracks, clips, transitions, links, parameters, consistent identities, bounded traversal/cycle validation, saved-state undo/redo | Foreign concepts not represented by the canonical model remain incomplete |
| Timeline | Selection, insert/overwrite, razor, lift/extract, ripple delete/trim, roll, slip, slide, rate stretch, nesting, snapping, track controls | Premiere cross-product behavior is not fully measured |
| Project I/O | Atomic native `.opproj` read/write; OTIO/FCP XML/EDL interchange; gzip/XML `.prproj` import; foreign effects and transitions replaced by the closest implemented equivalent | `.prproj` is read-only and mappings are intentionally partial |
| Media | FFmpeg probe, software and hardware decode (automatic switch when the processor falls behind), NV12/P010 GPU conversion, audio conform/peaks, thumbnails; proxies (540p H.264, short GOP, original frame numbering, created in the background, preview only); backwards reading in decoded blocks for reversed clips; pausable export with live preview and a selectable output frame rate; optional hardware encoders | Zero-copy decoder/GPU interop is pending; hardware decode and codec coverage are not yet validated as a matrix; SVG is rasterized at its own size (small logos look soft when scaled up) |
| Rendering | wgpu compositor, color conversion, blend modes, implemented effects/transitions (including original looks such as multi-octave glow, glitch, shake, grain and light leaks, and mirrored-edge motion transitions; halftone, duotone, Kuwahara oil paint, pencil sketch, neon edges, halation, lens flare, ripple, zoom pulse and CRT looks; hexagon, shatter, ink, pixelate and kaleidoscope transitions), animation presets, graphics, scopes | No differential pixel-parity corpus; OpenColorIO integration remains pending |
| Captions | Local speech recognition (Whisper Tiny/Base/Small through candle, CPU, downloaded on request), proportional word timings, SRT/WebVTT, graphics with 26 styles, split/merge/start-end editing | Not audio-aligned; Tiny silence-word analysis produced spurious trailing text; timing edits reject word truncation/overlap/unsupported animation |
| Audio | Six-channel floating-point bus, nested masters, explicit mono/stereo/5.1 conversion, playback/scrub meters before device conversion, EQ/dynamics/delay/reverb and native effects | Adaptive routing, physical speaker maps, VST3 and DSP oracle tests remain pending; some effects/front pan process only L/R; see [audio-routing.md](audio-routing.md) |
| Performance | Five profiles (animations, redraw rate, preview resolution, frame cache, read-ahead, late-frame wait, thumbnails and waveforms) plus custom settings, with a hardware check (processor, memory, graphics card type) that recommends one, offered at first start; software-only rendering and adapter fallback down to WARP/llvmpipe; core shaders at start, effect shaders in idle frames or on first use | Zero-copy decoder/GPU interop and a render cache for heavy sections remain on the roadmap |
| Silence | `op-silence`: loudness, relative, adaptive-floor, speech-band (FFT features) and neural (Earshot) detection with minimum silence and sound lengths and padding; transcript cuts (fillers in 6 languages, repeated words and phrases, repeated takes, word gaps) snapped to the quietest moment; dialog with waveform preview and per-cut review; ripple cuts across all tracks on frame boundaries with audio crossfades, speed-up and markers as one undo step; assistant tools and a command line | Word times are Whisper's approximate ones; Whisper often drops fillers and writes repeated words once; one microphone cannot be cut separately in a multitrack mix yet |
| Assistants | MCP server (`--mcp`) with 33 tools including caption split/merge/bounds on a headless editor or, when allowed, the open window through a local key-protected port; each editing tool call is one undo step; rendered frames returned as PNG | No keyframe, Lumetri-preset or audio-mix tools yet; long tools in the open window report that they started, not when they end |
| UI | Seventeen dockable panels, saved workspace layouts, monitors, timeline, effect controls, color picker, centered dialogs, in-app log viewer, English/Spanish catalogs | Multi-monitor, accessibility, IME, HiDPI edge cases, and automated interaction coverage remain incomplete |
| Reliability | Panic boundaries; recovery write acknowledgement/retry/generation cancellation; unique native temporaries and coordinated commit/backup; nonblocking emergency save; logs/crash reports | Recovery covers project state, not unfinished exports; multi-process/power-loss/crash-injection testing remains pending |
| Packaging | Portable Windows ZIP, Linux AppImage, macOS disk images (arm64 and x86_64, built in CI), checksums, licenses, final-layout self-test | The macOS app is signed ad hoc and not notarized |

## Highest-priority pending work

User-facing priorities (performance, captions, text animation, temporal effects, effect packages,
assisted editing) are ordered in [roadmap.md](roadmap.md). The list below covers validation and
platform work.

1. Complete controlled E4/E5 fixtures for `.prproj` identity scopes, masks, retiming, automation,
   transitions, malformed input, and unknown-node preservation. Do not add `.prproj` writing until
   lossless round-trip evidence exists.
2. Extend the automated timeline behavior matrix. A randomized edit test (op-timeline
   `stress.rs`) now checks validity and lock safety over thousands of edits across linked selection,
   track targeting, sync locks, locked tracks and trims; expected-result tables per operation, focus
   contexts and snapping at multiple DPI/zoom values remain.
3. Create pixel/audio oracle corpora across bit depths, YUV/RGB formats, color spaces, alpha,
   effects, transitions, audio layouts, and supported GPUs; publish tolerances and performance
   budgets.
4. Implement and conformance-test the missing extensibility layer: isolated OpenFX hosting and a
   capability-based WebAssembly scripting API. VST3 SDK is now MIT; ABI hosting, isolation and
   individual plugin terms still require separate validation.
5. Finish production platform work: zero-copy decoder/GPU interop, cache pressure
   tests, crash-injection tests for the recovery path, accessibility, multi-monitor docking, and
   macOS notarization.
6. Run the clean-room observations against a licensed official Premiere Pro 2024 installation and
   obtain legal/dependency-license review before any public compatibility release.

## Recently fixed interaction contract

Timeline Ctrl/Cmd+wheel zoom now consumes raw wheel events. Each detent applies exactly one 1.20×
step (or its inverse), preserves the time under the pointer, and stops when the event stream stops.
It does not use egui's smooth/inertial zoom accumulator. See `TL-VIEW-001` through `TL-VIEW-004`
in [timeline-behavior.md](timeline-behavior.md).
