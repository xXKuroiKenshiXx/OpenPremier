# Premiere compatibility: evidence and remaining work

Reviewed 2026-10-06, working tree 0.10.0. This matrix records implemented behavior and gaps;
it does not certify Adobe parity. Static inspection of a feature name is not a behavior test.
See [audit](audit-2026-10-06.md) for source changes and [implementation status](implementation-status.md)
for completed checks. Existing Rust/egui/wgpu panels and editing functions remain in place.

| Area | Present in OpenPremier | Still needed for a compatibility claim | Acceptance evidence |
|---|---|---|---|
| Project import | Partial `.prproj`, native `.opproj`, OTIO/FCP XML/EDL, presets/version-copy utility | Unknown-node preservation, caption/data tracks, style items, masks, multicam and lossless writing | Minimal own fixture per feature; compare object identities, timings and content before/after import/round trip |
| Local Premiere corpus | Three templates import; Social has 18 clips in 10 sequences | Social drops seven caption/data tracks and two style items; other two templates have no clips | Obtain controlled projects with media and all supported feature types; reject or report every loss |
| Timeline editing | Insert/overwrite, ripple/roll/slip/slide/stretch, links, locks, targeting, nesting | Complete focus/selection/sync-lock/handles/snapping combinations; multicam switching | Expected-result tables per operation and undo, rational FPS, linked tracks and locked tracks |
| History | Validated transactions, saved-state identity, sealed drag merges, monotonic revision, copy-on-write effect parameters and a clip-copy budget for undo | UI behavior for large histories (History panel with hundreds of steps) | `tests/stress_edits.rs`: 16,000 clips stay near 0.3 GB with 1-4 ms per cut; save/branch/undo fixtures and interactive focus checks |
| Captions | Transcription, SRT/VTT, animated graphics, split/merge/start-end edits, three MCP tools | Audio-aligned words, spurious transcription tail suppression, caption tracks/styles imported without loss | Original voiced/no-speech corpus with marked words and accuracy metrics; UI/MCP/export agreement |
| Titles/graphics | Existing graphics/effects and keyframes | Rich text, reusable parameterized templates, animation import/export | Fonts/glyphs/alpha/anchor/line-wrap fixtures and editable reimport |
| Alternative export engines | Native GPU/software selection; isolated fframes 1.2.0 CPU export for supported SDR scenes; alpha, rational time, media/audio, titles, shapes and nesting tested | fframes rejects unsupported effects/transitions/HDR; preview remains wgpu; no Skia GPU integration | Broader pixel conformance, performance/memory measurements and additional platforms; see [render-engines.md](render-engines.md) |
| Render/color | GPU compositor, nodes, scopes, implemented native effects/transitions | HDR/color management/OCIO, exact alpha/effect behavior, temporal frame algorithms | Pixel ramps/impulses across formats, bit depths and backends with tolerances |
| Media/proxies | FFmpeg software/hardware paths, caches, proxies, reverse, relink | Codec/backend/VFR matrix, zero-copy surfaces, render In/Out cache; debug stress readback 111–121 ms/frame | Original codec fixtures, seek/cancel/error tests and release-profile memory/latency budgets |
| Audio routing | Native mono/stereo/5.1 bus, nested masters, export/playback/scrub downmix | Adaptive routing, speaker maps, exhaustive DSP/channel behavior, device hardware | Channel impulses passed; extend to real devices and reference audio results |
| Audio production | Native EQ/dynamics/effects and meters | Calibrated LUFS/true peak, ducking, advanced cleanup, VST3 host | Known signals, calibrated meters, automation and isolated plugin fault tests |
| Export | FFmpeg codecs, preview, pause and frame-rate choice | Full preset mappings, queue/error/cancel matrix, color/HDR metadata fidelity | Decode exported own fixtures and compare frame/sample counts, timestamps, pixels and audio |
| UI/workspaces | 17 dockable panels, six layouts, monitors, English/Spanish | Multi-monitor, accessibility, IME, mixed DPI and interaction coverage | Runtime observation and scripted user workflows; static dispatch alone is insufficient |
| Extensions | Independent native plugin tools | OpenFX/VST3/Wasm hosting; no Adobe binary plugin host | Isolation/capability specification, ABI/dependency review, hostile plugin fault fixtures |
| Reliability | Acknowledged recovery, retries, cancellation, unique temporary saves and coordinated backup | Crash injection, power loss, multiple processes, naming collisions | Process termination at I/O boundaries; restored valid project and no unrelated file loss |
| Platforms/packages | Windows package/self-test flow; other platform build configuration | New Linux/macOS runs, signing/notarization and hardware coverage | Final-layout tests per platform and recorded checksums |

The local application GUI could not be observed through the environment's window service.
Startup decompilation, PE boundaries and UXP static calls guide further investigation; none
prove that an editing operation produces the same output as Premiere. Each further implementation
must preserve existing commands, panels, persisted data and undo, then add a feature-specific
fixture before moving a gap to verified.
