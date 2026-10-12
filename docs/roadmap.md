# Roadmap

**Updated:** 2026-10-07 (version 0.10.0)<br>
**Companion documents:** [implementation-status.md](implementation-status.md) (technical status and
validation gaps), [CHANGELOG.md](../CHANGELOG.md) (what each release changed),
[drift-comparison.md](drift-comparison.md) (lessons from another open-source editor).

This file is the user-facing plan: what an editor can already do, and what comes next, in order of
priority. Items move from "Next" to "Done" in the release that ships them.

## Implemented in the current working tree: unreleased

- HDR delivery selector: Rec.2100 PQ/HLG, BT.2020 10-bit HEVC/ProRes, real source/transfer
  conversion, HDR10 mastering/CLL/FALL metadata, and fframes floating composition. Windows
  Vulkan/DX12/WARP and Linux WSL software/Vulkan/fframes CPU exercised; Linux/macOS worker
  packaging and CI added, Mac/AMD/Intel and interactive QA still unverified.
  See [hdr-and-platform-acceptance.md](hdr-and-platform-acceptance.md).

- Comfortable interface density by default, with Compact/Spacious choices, roomier dock headers,
  controls, project/parameter rows and transport areas. Saved layouts and seventeen panels remain.
- Automatic editing proxies after media import (opt-in), selectable 540p/720p/1080p short side,
  background creation and source-identity guards. Exports retain original media; no enlargement.
- Dependency copyright/license/NOTICE retention, pinned upstream attribution supplements,
  application source snapshots and explicit distribution review. Native corresponding-source and
  historical provenance review remain pending. See [legal/audit-2026-10-07.md](legal/audit-2026-10-07.md).

- Export renderer selector: native GPU/software and isolated fframes CPU/Skia GPU workers,
  preserving native audio/codecs and rational frame rates. An explicit RGBA16F hybrid bridge
  shares all 92 native effects, 43 transitions, 27 blends, captions, mattes/adjustments and
  source interpretation. Preferences now select asynchronous Source/Program fframes previews.
  The independent SVG subset remains available for compatible 8-bit exports. Hardware/platform
  QA, interactive monitor QA, HDR grading/creative-node acceptance and texture sharing remain. Full integration criteria:
  [fframes-completion.md](fframes-completion.md). See [render-engines.md](render-engines.md).

- Caption start/end editing and three MCP tools, preserving word timings in one undo step.
- Six-channel audio mixing and explicit downmix, including nested masters, playback and scrub.
- Recovery write acknowledgments/cancellation, concurrent native saves, bounded model validation
  and saved-state history identity. See [audit-2026-10-06.md](audit-2026-10-06.md).

- Split at Playhead and Merge Selected in the existing Captions panel: frame-aware, word timing
  preserved, one undo step, unsupported combinations rejected. See [caption-editing.md](caption-editing.md).
- Invalid speeds/missing required source identities reported during project validation.
- Autosave retention matches one project's generated names; XML budgets include CDATA/entities.
- GPU readback failures propagate to export, MCP and clipboard; failed/abandoned buffers discarded.
- Source audit and reconstruction priorities in [audit-2026-10-06.md](audit-2026-10-06.md).

## Done

### Native extensions and image animation (0.10)

- Bundled Motion Curves, Folder Portals and Quick Actions, independently enabled in Preferences.
- Offline Plugin Browser without a catalog; no external binary plugin host yet.
- Bezier/value/speed editing, saved easing presets and native bounce keyframes.
- Local effect/transition/preset/command palette, favorites, hiding, macros and optional shortcuts.
- Bitmap Ctrl+V event fixes, frozen asynchronous paste destinations and Copy Frame to Clipboard.
- Pitch Shifter window/interpolation/stereo controls and deterministic native Tape Stop / Slowdown.
- GPU Manga Camera 3D, Parallax, frames/vignettes, dialogue occlusion and Page Turn. Layered PNG/text
  compositing supports partially hidden dialogue; automatic segmentation is not implemented.

### Compatibility additions (0.10)

- Partial Premiere import from gzip or plain UTF-8 XML, independent of serialization-version numbers.
- File > Downgrade Adobe Premiere Project and `--downgrade-prproj`: preserve original XML and patch a new copy's Project version, with no overwrite. This does not translate newer features.
- Load Premiere Sequence Preset: partial `.sqpreset` settings import for progressive sequences.
- Native OpenJumpCuts preset and JSON settings loading in timeline and batch silence dialogs.

### Editing

- Multitrack timeline: insert, overwrite, lift, extract, razor, ripple, roll, slip, slide, rate
  stretch, nesting, snapping, linked selection, sync and track locks, markers, keyframes.
- Premiere-style panels and workspaces: Project, Source, Program, Timeline, Effect Controls,
  Effects, History, Audio Mixer, Meters, Markers, Info, Lumetri, Scopes, Graphics, Tools.
- Copy and paste images from the browser, other programs or the file manager (Ctrl+V) (0.4).
- Reversed clips (Speed/Duration > Reverse Speed), also read from Premiere projects (0.5.1).

### Media and performance

- FFmpeg decoding and encoding; export to H.264, HEVC, ProRes 422 HQ, ProRes 4444, DNxHR HQ and
  PNG sequences in QuickTime, with pause, live preview and a chosen output frame rate.
- Hardware decoding (Direct3D 11, VideoToolbox, VAAPI) in Automatic, Always or Never mode; the
  automatic mode moves a file to the graphics card when the processor falls behind (0.5).
- Hardware encoders (NVENC, AMF, Quick Sync, Media Foundation, VideoToolbox) with an automatic
  retry on the software encoder when one fails (0.5.1).
- Decoder read-ahead with in-order gap filling; exports of camera footage 20 times faster (0.5).
- Backwards reading in blocks: reversed clips and reverse playback no longer seek once per
  frame (0.6).
- **Proxies** (0.6): Create Proxies / Remove Proxies in the Project panel and the Clip menu,
  Toggle Proxies in the Program Monitor and View menu. Proxies are 540p H.264 with a keyframe every
  half second, keep the original's frame numbering and color, and are never used for export.
- Playback and paused resolution (Full, 1/2, 1/4, 1/8) and choice of graphics API.
- Performance profiles (0.7): Ultra Performance, Performance, Balanced, Quality and Maximum
  Quality, recommended from the processor, memory and graphics card at first start; Software Only
  rendering and automatic fallback to the processor's rasterizer, so the program runs on
  computers without a graphics card; effect shaders compiled in idle time; custom settings
  changed one by one.
- **Link Media** (0.7): missing files located by hand (with the others in the same folder) or
  found by searching the computer by name, duration, size and audio streams.

- Exports encode on every processor core (0.8): about 3.5 times faster with ProRes, DNxHR, PNG
  and software H.264/HEVC; frames are read back from the GPU while the next one renders.

### Color nodes, icon view and batches (0.9)

- Color Nodes: a node graph for grading (serial, parallel and layer nodes, primaries, curves
  including hue and saturation curves, qualifier, windows, keys, LUTs). See
  [color-nodes.md](color-nodes.md).
- The Effects panel has an icon view (off by default; the list of names stays the normal view)
  with an animated picture for every effect.
- Fourteen new effects: a Noise & Grain set (Film Stock Grain with 8, 16, 35 and 65 mm and pushed
  stocks, Sensor Noise, Salt & Pepper, TV Static, Fractal Noise, Dust & Scratches) and a Cinema
  category (Film Emulation with ten looks, Bleach Bypass, Cinematic Split Tone, Gate Weave,
  Pro-Mist Diffusion, Anamorphic Streaks, Chromatic Aberration, Super 8 Film).
- Remove Silence from Files: batch cutting, and a separate section for filler words, repeats and
  retakes in the dialog.

### Silence and speech cutting (0.9)

- Clip > Remove Silence: five ways to find silence (loudness, relative, adaptive noise floor,
  speech band, neural voice detector), words found in a transcript (fillers in six languages,
  repeated words and phrases, repeated takes, dead air between words), review over the waveform,
  five presets, and cut / leave gaps / speed up / markers. Also `detect_silence` and
  `remove_silence` for assistants and `--remove-silence` on the command line. See
  [silence.md](silence.md).

### AI assistants (0.8)

- `OpenPremier --mcp`: the Model Context Protocol for Claude Code, Codex and other clients, with
  28 tools for media, editing, effects, titles, captions, frames and export; live in the open
  window when the user allows it. See [assistants.md](assistants.md).

### Captions (0.6)

- Graphics > Captions > Transcribe and Create Captions: local speech recognition (Whisper Tiny,
  Base or Small through candle, downloaded once on request) with language detection and word
  times; captions on their own track, one clip per caption.
- Twelve animated styles driven by the word times: Classic, Boxed, Karaoke, Highlight Box, Pop,
  One Word, Typewriter, Bounce In, Neon, Creator, Fade In, Underline. Everything is editable in
  Effect Controls; Apply Caption Style to All restyles the sequence.
- SRT and WebVTT import and export; New Caption for typed captions.
- Captions panel (0.7): every caption with its time and editable text, and style controls for all
  captions or the selected ones; spoken language and English translation choices; transcription
  about a third faster and language detection over the most voiced parts.

### Effects

- 73 video effects, 42 transitions and 27 audio effects, including Lumetri Color, keyers, blurs,
  distortions, light and film looks, and (0.6) Halftone, Duotone, Oil Paint, Pencil Sketch, Neon
  Edges, Halation, Lens Flare, Ripple, Zoom Pulse, CRT Screen, the Hexagons, Shatter, Ink,
  Pixelate and Kaleidoscope transitions, and Tremolo, Auto-Pan, Phaser, Bitcrusher, Distortion,
  Noise Gate, Pitch Shifter and Telephone and Radio audio effects.
- Drag-and-drop animation presets; foreign effects in imported projects replaced by the closest
  equivalent.

### Projects and platforms

- Every common picture format (0.8): SVG, AVIF, HEIC, JPEG XL, JPEG 2000, ICO, DDS, QOI and more
  besides the usual ones; pasting puts media at the pointer's place on the timeline.

- Native `.opproj`, OTIO, FCP XML and EDL interchange; partial read-only `.prproj` import.
- Windows ZIP, Linux AppImage and macOS disk images (Apple silicon and Intel); updating from
  inside the program is planned.
- Spanish and English interface, `.kys` keyboard shortcut import, crash recovery, log viewer.

## Next

Ordered by how much they improve everyday editing. Items marked (Drift) are features the Drift
editor shows are valuable for the same audience; see [drift-comparison.md](drift-comparison.md).

### 1. Smoother playback on modest hardware

- **Zero-copy hardware frames:** hand Direct3D 11 / VAAPI / VideoToolbox surfaces to the renderer
  without the round trip through system memory (Drift does this for D3D11 and VAAPI). Needs
  wgpu-hal interop per backend.
- **Render cache for heavy sections** (Premiere's "Render In to Out"): render effect-heavy ranges
  once to an intermediate file and play that.
- Automatic proxy preferences are implemented in the unreleased tree above; validate broader
  camera-media/color/interpretation cases before presenting universal proxy compatibility.
- **Effect thumbnails** in the Effects panel rendered on the current frame (Drift).

### 2. Titles, captions and graphics

- **Caption workflow:** extend the implemented split/merge/timing controls to measured
  alignment and richer caption track/style interoperability.
- **Exact word timing:** align each word to the audio (Whisper cross-attention or forced
  alignment) instead of spreading a timed phrase over its words; faster transcription on the
  GPU (candle with CUDA or Metal).
- **Caption style presets** with saved custom looks, emoji and keyword highlighting.
- **Text animators:** per-character, per-word and per-line presets (fade, rise, pop, typewriter,
  karaoke highlight) (Drift has about 40).
- **Stickers and emoji** as graphics, and Lottie/SVG animated graphics (Drift).

### 3. Effects

- **Temporal effects** that look at neighbouring frames: echo/motion trail, frame blending,
  optical-flow slow motion.
- **Look templates:** one-click stacks of effects, transitions and audio that follow the music's
  beats (needs beat detection) (Drift).
- **Effect packages:** a data format (JSON parameters plus WGSL passes) so new effects and
  transitions can be added without rebuilding the program, with an add-on browser (Drift ships a
  similar JSON + GLSL format and a signed add-on market).
- More transitions: page curl, morph cut, film roll, light rays.
- Masks with tracking (Premiere's mask tracking) and planar tracking.

### 4. Audio

- Loudness normalization to a target (LUFS) and a true-peak meter.
- Auto-ducking of music under speech.
- Voice clean-up: noise reduction, de-esser, de-reverb; a local AI denoiser as an option (Drift
  uses DeepFilterNet).
- VST3 plug-in hosting (current SDK is MIT; retain its notices and review individual plugin terms,
  ABI isolation, latency and conformance before integration).

### 5. Assisted editing (local, optional models)

- Scene detection on import to split long recordings.
- Beat detection for cutting to music.
- Subject cut-out (segmentation) and background removal; depth-based effects (Drift uses SAM 2,
  RVM and Video Depth Anything).
- Auto-reframe for vertical formats.

### 6. Silence and speech editing

- **Exact word times** (a CTC aligner) so word cuts are exact and repeats Whisper writes once are
  found; a larger neural voice detector (Silero) for hard recordings.
- Edit the transcript like text: delete words to cut them (Descript, Drift).
- Multi-track and multicam: cut silences in one microphone only, switching cameras on who speaks.
- Save the dialog's settings in the project and add a keyboard shortcut.

### 6b. AI assistants

- Tools for keyframes, color (Lumetri) presets, audio levels and ducking, and the caption list.
- Progress of exports and transcriptions started by an assistant, reported back to it.
- SVG rasterized at the size it is shown, so logos stay sharp when scaled up.

### 7. Compatibility and platform

- `.prproj` writing once lossless round trips are demonstrated with original fixtures.
- OpenFX plug-in hosting and a WebAssembly scripting API.
- macOS notarization and signed Windows builds; Flatpak.
- Accessibility, multi-monitor docking, HiDPI edge cases.

## Not planned

- Accounts, subscriptions, watermarks or telemetry.
- Copying another product's code, assets or exact effect output (see the clean-room policy in
  [legal/clean-room.md](legal/clean-room.md)).
