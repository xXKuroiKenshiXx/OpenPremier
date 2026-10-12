# Changelog

OpenPremier follows semantic versioning. User-visible changes are grouped by release.

## 0.10.1 - 2026-10-12

### Added

- Updates from inside the program. On start (Preferences > Updates, on by default) and with Help > Check for Updates..., OpenPremier asks GitHub for the newest release, shows what is new and offers Download and Install, What's New, Later or Skip This Version. The package for the system is downloaded in the background and checked against the release's SHA256SUMS.txt; it is installed when the program closes (after the usual offer to save): on Windows the new version replaces the program folder once the old process has ended and starts itself, the Linux AppImage is replaced in place and restarted, and on macOS the downloaded disk image is opened. Copies not installed from a release package only link to the releases page. See docs/updates.md.

## 0.10.0 - 2026-10-12

- Timelines with thousands of cuts no longer run out of memory or slow down. Clips cut from one
  share their effect parameters until one of them changes (copy on write), undo keeps at most
  about a million clip copies (older steps are dropped beyond that, never fewer than 30), and
  selections of thousands of clips are expanded with their links and groups in one pass. With
  3,000 cuts the program held 10.9 GB and each cut took 25 ms; it now holds about 0.3 GB with
  16,000 clips and a cut takes 1 to 4 ms (`tests/stress_edits.rs`).
- The timeline draws only the clips in view, and clips a few pixels wide only as their color;
  thumbnails are asked for only where they fit. Thumbnail memory is bounded (400 pictures) and
  the interface keeps its most recently drawn thumbnails instead of dropping all of them past
  600, which re-uploaded hundreds of textures every frame on long cut timelines. A thumbnail
  request dropped from a full queue is asked for again instead of never showing.
- The Comfortable density (the default) is dense again, close to the earlier layout: 22 px
  controls, 13 px text and tighter spacing; Spacious is a little roomier.

- Preserve tiny HDR shadows and alpha in fframes CPU by using Skia RGBAF32 raster composition
  and explicit half-float wire conversions. Cover every positive half subnormal, small-alpha
  source-over and packaged HDR exports; keep GPU surfaces and IPC in RGBA16F.
- Real HDR delivery: Rec.2100 PQ/HLG with BT.2020 10-bit HEVC/ProRes, source color/luminance
  conversion, authored HDR10 mastering/CLL/FALL metadata, SDR export thumbnails and fframes
  floating composition. Preserve default SDR exports; reject unsupported HDR codecs before
  opening the destination. Add encoded/decoded metadata and independent color-reference tests.
- Validate HDR on Windows Vulkan/DX12/WARP and Linux WSL; add isolated-worker CI/package
  integration for Linux/macOS. Interactive QA, Mac/other physical GPUs and HDR grading remain pending.
- Pin FFmpeg acquisition to the verified 2026-09-28 release/digests, verify DLL identity before
  packaging, retain exact native source candidates and Skia DEPS notices. Require retained
  review evidence and no blockers before publication; legal review remains pending.

- Roomier interface with persistent Compact/Comfortable/Spacious density, enlarged controls,
  dock headers, project/effect rows and monitor transport. Existing layouts/panels are preserved.
- Optional automatic proxies on video import with 540p/720p/1080p selection; stale project/source
  results are discarded and duplicate queued jobs are suppressed. Exports use original media.
- Retain dependency copyright/license/NOTICE texts and pinned upstream supplements, generate
  exact application-source snapshots and native-binary inventories. Record unresolved source/
  provenance issues and check distribution review before publishing.

### Added

- Export renderer selection: native wgpu GPU/software or an isolated fframes 1.2.0 CPU compositor for supported SDR scenes. Persists the preference, reports the effective renderer, preserves rational frame rates and native audio/codecs, and adds optional MCP `export.renderer`. Windows bundles the worker and license notices; see docs/render-engines.md for experimental limits.

- Captions panel: Split at Playhead and Merge Selected, implemented in Rust with one undo step. Splits preserve word timing and graphics animation; merging requires adjacent captions on the same track with identical static styles. Unsupported edits fail without changing the project.
- Caption start/end editing through Times... and three MCP tools for split, merge and bounds; word timings and undo are preserved within the documented contract.
- Native six-channel audio bus, nested-sequence masters, explicit mono/stereo/5.1 conversion for playback, scrub and export, and meters before device conversion.

### Fixed

- Unsupported alternative-renderer scenes are rejected before opening or deleting an existing destination. CPU export handles straight alpha and composites non-alpha codecs onto black; ProRes 4444 decode preserves 12-bit YUV/alpha planes and source color metadata.

- Invalid clip speeds and missing required media, nested-sequence or generator references now produce project errors instead of panics or silently empty frames.
- Autosave retention no longer includes another project's files when its name shares a prefix.
- XML text limits now include CDATA and decoded character references, with checked byte accounting.
- GPU readback failures now propagate to video/frame export, MCP and clipboard instead of returning black pixels as success. Failed or abandoned buffers are discarded.
- Recovery acknowledges successful writes, retries failed revisions and prevents queued writers recreating discarded files. Emergency recovery avoids waiting on held locks.
- Native saves use unique temporaries and serialize commit/backup within the process.
- Corrupt bin trees, inconsistent IDs, nested-sequence cycles and exhausted IDs are rejected before unsafe traversals/counter refresh.
- Undo/redo to the saved state clears dirty without rolling back the monotonic revision; saving seals merged drag edits.
- Explicit corpus/image test runs fail when their fixture directories contain no supported files.

### Documentation

- Added a source audit and reconstruction plan, caption editing contract and explicit validation limits. Corrected the current panel count to seventeen.
- Added a feature parity matrix, audio routing contract and fframes assessment with an isolated FFmpeg dependency-resolution probe.

### Added (plugins and animation)

- Bundled native plugin manager in Preferences and Edit: Motion Curves, Folder Portals and Quick Actions, with independent persisted enable switches. Plugin Browser is offline without a provider.
- Alternate native keyframe value/speed graphs, Bezier handles, named presets and damped bounce. Folder favorites support project-relative paths. Local action search includes effects, presets, transitions, favorites, hiding, ordered macros and optional non-conflicting shortcuts.
- Native clipboard paste handles bitmap key events, text-event duplicates and Ctrl released before V. Deferred pastes retain their original sequence/time; project changes cancel pending imports. Copy Frame to Clipboard renders the sequence at full resolution.
- Native Tape Stop / Slowdown retimes audio and pitch with a deterministic speed ramp. Pitch Shifter adds window sizes, four-point interpolation, pitch smoothing, stereo detune and output gain.
- GPU Manga Camera 3D, Parallax, Manga Frame and Vignette, Dialogue Occlusion and four-direction Page Turn. See docs/native-plugins.md for controls, compositing recipes and limits.


### Changed (Premiere projects)

- Rename the standalone file utility to File > Downgrade Adobe Premiere Project and describe only its copy workflow. Direct `.prproj` opening/importing remains separate and never requires this utility.

### Added (Premiere projects)

- File > Premiere Project Compatibility: inspect and save a new `.prproj` copy with a lower Project serialization version, also `--downgrade-prproj`. Preserves all other XML bytes and refuses overwrites; this does not convert newer Premiere features. See docs/premiere-compatibility.md.
- OpenJumpCuts preset and JSON settings loader in both silence dialogs, using the native Rust detector. See docs/openjumpcuts.md.
- Load Premiere Sequence Preset in New Sequence and Sequence Settings: partial `.sqpreset` import of progressive dimensions, exact FPS, pixel aspect, sample rate and track counts.

### Changed

- Premiere import also accepts uncompressed UTF-8 XML; serialization version numbers do not block recognizable graphs. Support remains partial.

## 0.9.0 - 2026-10-04

### Added

- Remove Silence (Clip menu and the timeline's clip menu): finds silences, filler words, repeated words and phrases and repeated takes in the audio, shows them shaded over the waveform to review one by one, and cuts, speeds up or marks them in one undo step. Five ways to tell sound from silence (Loudness, Relative to Loudest, Adaptive Noise Floor, Speech Band and a neural Voice Detector, with a Voices Only filter), shortest silence and sound, air kept around the words, and five presets (Gentle, Podcast, Tutorial, Jump Cuts, Noisy Room). With Find Words to Cut the audio is transcribed on this computer and fillers (English, Spanish, Portuguese, French, German, Italian, crutch phrases and your own list), repeated words and phrases, repeated takes (the last one stays) and dead air between words are found. Cuts land on frame boundaries across every track, with a short audio crossfade at each join. See docs/silence.md.
- Color Nodes: a node-based color system. The effect (Effects > Color Correction) and the panel in the Color workspace edit a graph of correctors, parallel mixers and layer mixers between a Source and an Output. Each corrector has primaries (lift, gamma, gain and offset wheels, contrast and pivot, saturation, hue, temperature, tint, shadows, highlights, color boost), custom curves (master, red, green, blue, hue vs hue, hue vs saturation, luminance vs saturation, saturation vs saturation), a qualifier by hue, saturation and luminance, windows (rectangle, ellipse, linear, polygon), a key with gain, offset, blur and invert, a key input (also inverted, to grade what is outside), a 3D LUT, blur and sharpen. The viewer can show a node's matte or dim what it does not select. See docs/color-nodes.md.
- Icon view in the Effects panel: the panel stays a list of names, as usual; a button next to the search box switches to cards with a picture of every effect and transition, rendered by the real renderer and animated while the pointer is over it or always (Preferences > Performance > Effect Previews; the profiles choose a default).
- Clip > Remove Silence from Files...: removes silences, filler words, repeats and retakes from many files at once with the same settings, writing the results to a folder; also several files on the command line (`--out-dir`, `--suffix`).
- The Remove Silence dialog has a separate section for filler words, repeated words and retakes, apart from the silence methods.
- Fourteen new effects. Noise & Grain: Film Stock Grain (8 mm, 16 mm, 35 mm, 65 mm and pushed high-ISO stocks), Sensor Noise (luminance and color noise, more in the shadows), Salt & Pepper Noise, TV Static, Fractal Noise (octaves, evolution, six blend modes) and Dust & Scratches (specks, hairs and scratches that change over time). A new Cinema category: Film Emulation (warm negative, cool stock, bleach bypass, cross process, two-strip Technicolor, silver noir, day for night, faded matte, teal and orange, seventies), Bleach Bypass, Cinematic Split Tone, Gate Weave, Pro-Mist Diffusion, Anamorphic Streaks, Chromatic Aberration and Super 8 Film.
- Fourteen new caption styles with new animations: Slide Up, Slide In, Zoom In, Drop In (falls and bounces), Elastic, Rainbow, Wave, Shake, Glitch, Letter by Letter, Spotlight, Hop, Highlighter and Glow Pulse (26 styles in all), each driven by the time of every word and scaled by Animation Strength.
- Ctrl+Y redoes, as well as Ctrl+Shift+Z.
- The `op-silence` crate: the detection in pure Rust, with the neural detector [Earshot](https://github.com/pykeio/earshot) (MIT or Apache-2.0, 40 KB).
- Assistants: `detect_silence` and `remove_silence` tools (30 tools in all).
- Command line: `OpenPremier --remove-silence input output [options]` cuts a file's silences and writes the result without opening a window.

### Changed

- The Color workspace layout changed (it holds the Color Nodes panel); saved layouts of older versions are replaced by the new factory layout.

### Fixed

- Remove Silence no longer asks to analyze again after changing settings: the neural voice detector is measured with every analysis, so choosing it or Voices Only later keeps the analysis; and once analyzed, Apply stays available.
- Remove Silence no longer asks to analyze again when nothing it needs changed: turning off the word search, or changing silence settings, keeps the analysis; only a different scope, a missing neural curve or a different speech model or language needs a new one.

## 0.8.1 - 2026-10-04

### Added

- Any font in the program: Preferences > Interface Font lists every installed font and Add Font... takes any .ttf or .otf file, which then works in titles, captions and the interface without being installed in the system (the files are kept in the program's data folder, Fonts).

### Changed

- The interface typeface is now Inter by default, included in the program (open source, SIL Open Font License; the free typeface closest to Apple's San Francisco), so the interface looks the same on Windows, macOS and Linux. The system font, egui's classic font and any other font remain choices.

## 0.8.0 - 2026-10-03

### Added

- AI assistants can edit with OpenPremier through the Model Context Protocol: `OpenPremier --mcp` serves Claude Code, Codex, Cursor and other MCP clients with 28 tools (project and media, sequences, adding, moving, cutting and speeding up clips, effects and their parameters, transitions, titles, automatic captions, markers, looking at a rendered frame, exporting, any menu command, undo). With Preferences > AI Assistants on, the tools edit the open project live (only from this computer, with a random key; off by default), each change one undo step; otherwise they work on a project of their own. See docs/assistants.md.
- Every common picture format imports and pastes: besides PNG, JPEG, TIFF, WebP, BMP, GIF, TGA, EXR, DPX and PSD, now SVG, AVIF, HEIC/HEIF, JPEG XL, JPEG 2000, ICO, DDS, QOI, PCX, PNM, SGI, XBM/XPM, Sun Raster and Radiance HDR. The import dialog has an Images filter.
- Pasted media (Ctrl+V) goes on the timeline as well as into the project, wherever the focus is: at the time and on the track under the pointer when it is over the tracks (the first free track above when that one is busy), else at the playhead; video files bring their sound to free audio tracks. Without an open sequence, the pasted media starts one.

### Changed

- Exports are about three and a half times faster when the encoder runs on the processor (ProRes, DNxHR, PNG, and H.264/HEVC without a hardware encoder): FFmpeg encoded on a single core, now it uses all of them. A one-second 1080p clip with 100 effects exports at 40 frames per second instead of 11 on the test computer, and ten stacked layers of ten effects at 21 instead of 6.
- Exports read each frame back from the graphics card while the next one renders, instead of waiting for it.
- A heavy-load benchmark (`cargo test --release --test stress_render -- --ignored`) measures one clip with 100 effects and ten stacked layers of ten effects, on the graphics card or on the processor's rasterizer like a computer without one; see docs/performance.md.
- The Hand tool shows a hand (open, and closed while dragging) instead of the system's four-arrow cursor. It is a real system cursor (on Windows made by the program, which Windows lacks), so it follows the mouse at the monitor's refresh rate.
- Transcription choices read Caption Language: Same as spoken (transcribe) or Translate to English. Tests showed that choosing a spoken language other than the real one does not translate.
- The interface code is split into smaller modules (dialogs and the main window each in several files).

### Fixed

- The log could grow to gigabytes: at the Trace detail level the windowing and graphics libraries were recorded too, thousands of lines a second, which also slowed the program. Libraries now only record warnings (OPENPREMIER_LOG_LIBRARIES=1 records them all), the file stops at 100 MB, and a larger log from an earlier session is deleted at start.
- Pasted images of a recovered project were saved inside the autosave folder.
- TGA and ICO pictures did not decode ("the image has no picture").
- A one-frame GIF became a one-frame clip instead of a still.

## 0.7.0 - 2026-10-02

### Added

- Performance profiles in Preferences > Performance: a five-step slider from Ultra Performance to Maximum Quality replaces the Performance Mode checkbox. A profile sets interface animations and smooth scrolling, how often the interface redraws while playing and while background work runs, playback and paused resolution, the decoded frame cache (never more than a quarter of the memory), how far playback decodes ahead, how long playback waits for a late frame, and timeline thumbnails and waveforms. Ultra Performance is meant for old computers and ones without a graphics card; Maximum Quality redraws at the display's refresh rate with richer animations. Exports are never affected.
- Automatic hardware check: the processor, its cores and threads, the memory and the graphics card (dedicated, integrated or none) choose a recommended profile. On the first start a Performance Setup window shows what was found and offers Use Recommended Settings or Choose Manually; Preferences shows the recommendation and Use Recommended at any time.
- Software Only (processor) in Preferences > Graphics API, like Premiere Pro's Mercury Playback Engine Software Only: the interface and preview are drawn by the processor's rasterizer (WARP on Windows, llvmpipe on Linux), for computers without a usable graphics card or with broken drivers.
- The Razor tool shows translucent scissors at the pointer over the timeline, so the clip underneath stays visible, with the red cut line between the blades.
- Custom performance settings: Preferences > Performance > Custom Settings changes each thing a profile sets (interface animations, smooth scrolling, redraws during playback, frames decoded ahead, timeline thumbnails, audio waveforms and preparing effects in the background) one by one; changing one, or a preview resolution or the frame cache, switches to Custom, and the slider goes back to a profile.
- Link Media (File > Link Media, and offered when a project opens with files that moved or were renamed), like Premiere Pro's: each missing file can be located by hand, and the other missing files in the same folder are linked with it; Search Automatically looks in the project folder, the user's media folders and every drive by file name, and when several files share a name it takes the one whose duration, size and audio streams match. Linking is one undo step and keeps each clip's interpretation and proxy; Offline All keeps working without them.
- Captions panel (Window > Captions; in the Editing and Graphics workspaces): every caption of the sequence with its start time and editable text, and one set of style controls (caption style, font, font style, size, text and highlight colors, stroke, box, vertical position, maximum width, animation strength, all caps and shadow) that changes all captions at once, or only the selected ones. Clicking a caption's time selects it and moves the playhead to it.
- Transcription choices: the spoken language (offered in the program's language) and Captions In: the language spoken or an English translation.
- Audio Gain (G) has Premiere Pro's four choices: Set Gain to, Adjust Gain by, Normalize Max Peak to (one gain for the selection, so its loudest peak reaches the level) and Normalize All Peaks to (each clip its own gain), and shows the selection's peak amplitude.
- Edit > Remove Attributes: puts Motion, Opacity, Volume, Channel Volume and Panner back to their defaults and removes effects from the selected clips, choosing which.

### Changed

- Faster transcription: about a third less time for the same result. The speech model's text decoder keeps what it has already read instead of reading the whole sentence again for every word, the audio of each 30-second window is analyzed once however many tries it takes, and silence is skipped without analysis.
- Better language detection: the spoken language is judged on up to three 30-second stretches with the most sound instead of the first 30 seconds, which were often music or silence and made Spanish speech come out as English. The status bar says which language was heard.
- Lighter: about a fifth less memory at rest (155 MB against 199 MB in 0.6.0 on the test computer) and half the processor time at start; the program file is 8 % smaller (whole-program optimization).
- A cleaner look: the interface uses the system's own font (Segoe UI on Windows, San Francisco on macOS, the desktop's sans-serif on Linux) with semibold panel names and window titles, controls have a faint rim that brightens under the pointer, windows and menus have rounder corners and softer shadows, and sliders fill up to their value. Preferences > Interface Font goes back to the classic font.

- The program opens on computers without a graphics card: the window takes the best adapter that can draw it (dedicated, then integrated, then the software rasterizer).
- Faster start: only the shaders every preview needs compile at start; effect shaders compile two per idle frame from the Balanced profile up, and on first use in the lighter profiles (with software rendering, compiling them all at start took several seconds of every core).
- The Preferences window scrolls when it is taller than the screen.

### Fixed

- Paste Attributes with Effects copied a graphic's text or caption onto the target clips as if it were an effect.
- The underline of the active tab of a narrow panel (such as Audio Meters) ran into the neighbouring panel.
- The Essential Graphics placeholder text wraps in a narrow panel instead of being cut off.

## 0.6.0 - 2026-10-02

### Added

- Animated captions, in the style of short-form video apps. Graphics > Captions > Transcribe and Create Captions turns the speech in the sequence into captions automatically: OpenAI's Whisper speech model (Tiny, Base or Small, downloaded once on request) runs on this computer, detects the language or uses the chosen one, and times every word. Captions go on their own Captions track, one clip per caption, with the chosen number of words per caption.
- Twelve caption styles, animated word by word as each word is said: Classic, Boxed, Karaoke (words fill with color), Highlight Box, Pop, One Word, Typewriter, Bounce In, Neon, Creator (bold capitals), Fade In and Underline. Style, font, size, colors, outline, box, shadow, position and animation strength are in Effect Controls; Apply Caption Style to All copies one caption's look to every caption in the sequence.
- Captions can also be imported from SRT and WebVTT files (word times are spread over each caption), exported to SRT or WebVTT, or added by hand (New Caption).

- Proxies, as in Premiere Pro: Proxy > Create Proxies in the Project panel and the Clip menu makes 540p H.264 copies of the selected clips in the background (progress and Cancel in the status bar), and Toggle Proxies in the Program Monitor (also View > Enable Proxies) switches playback between proxies and originals. Proxies keep the original's frame numbering and colors, a keyframe every half second and no reordered frames, so scrubbing and reverse playback are fast; export always uses the original media. Remove Proxies detaches them and deletes the files OpenPremier made.
- Ten video effects: Halftone, Duotone, Oil Paint, Pencil Sketch, Neon Edges, Halation, Lens Flare, Ripple, Zoom Pulse and CRT Screen.
- Five video transitions: Hexagons, Shatter, Ink, Pixelate and Kaleidoscope.
- Eight audio effects: Tremolo, Auto-Pan, Phaser, Bitcrusher, Distortion, Noise Gate, Pitch Shifter (up to two octaves up or down) and Telephone and Radio (telephone, radio, megaphone, walkie-talkie and underwater voices).
- docs/roadmap.md lists what the editor does today and what comes next; docs/drift-comparison.md reviews the Drift editor's rendering and features.

### Fixed

- WAV files that do not name their channel layout (written by many recorders and speech synthesizers) played silent: preparing their audio failed with "Input changed" on every frame.

### Changed

- Reversed clips and reverse playback read the source backwards in blocks of frames: each block costs one seek to a keyframe instead of one per frame (75 frames read backwards went from 76 seeks to under 20 in the test).
- The decoder follows the direction frames are actually requested in, so a reversed clip in a sequence that plays forward reads ahead the right way.

## 0.5.1 - 2026-10-01

### Fixed

- Exporting H.264 or HEVC on a Mac failed with "Invalid argument" from the VideoToolbox encoder. It now runs without B-frames (the likely cause) and may use its software path; if it still fails, the software encoder takes over (below).
- When a hardware encoder fails during an export on any platform, the export starts again with the software encoder instead of stopping.
- Export errors name the step that failed (for example the video encoder in use) instead of only FFmpeg's short message, and the log records which encoder an export uses.
- The default export folder is the user's videos folder on every system (Movies on macOS, the localized XDG folder on Linux) instead of the home folder when no folder is named "Videos"; pasted images default to the pictures folder in the same way.
- The empty Project panel shows Cmd+I instead of Ctrl+I on macOS.
- On Linux systems without the VAAPI libraries (libva), hardware decoding could close the program; it is now only tried when those libraries are installed, and the processor decodes otherwise.
- Premiere Pro project import reads reversed clips (Reverse Speed).

### Changed

- A new end-to-end test exports a clip with sound and effects in every format (H.264, HEVC, ProRes 422 HQ, ProRes 4444, DNxHR HQ and PNG) and reads each file back. Continuous integration runs it, and the effect and transition pixel tests, on a real graphics adapter on Linux and macOS instead of skipping them.

## 0.5.0 - 2026-09-30

### Added

- macOS package: a disk image (`.dmg`) with `OpenPremier.app` for Apple silicon (arm64) and Intel (x86_64) Macs, with FFmpeg bundled inside the app. Keyboard shortcuts are shown with Cmd and Option, and Show in Folder opens Finder.
- Performance Mode in Preferences (Performance): turns off interface animations and smooth scrolling and redraws the interface less often while waiting for background work. Editing and rendering are not affected.
- Hardware decoding in Preferences: Automatic (the default) decodes on the processor and moves a video to the graphics card's decoder when the processor cannot keep up in real time; Always and Never force one or the other. Hardware decoding uses Direct3D 11/DXVA2 on Windows, VideoToolbox on macOS and VAAPI on Linux, and falls back to the processor when the card cannot decode a file.
- Graphics API choice in Preferences: Automatic, Direct3D 12 (Windows), Metal (macOS), Vulkan or OpenGL. If the chosen API cannot start, the next one is tried.

### Changed

- Much faster playback and export of camera files: the decoder keeps a longer read-ahead and decodes every frame in order instead of skipping ahead, which made it seek back to the previous keyframe again and again. Exporting a 4K HEVC 10-bit clip to 1080p went from about 4 to about 80 frames per second on the test machine.
- Export encodes on its own thread while the next frame renders, and reads the rendered planes back from the graphics card in one batch with reused buffers.
- 8-bit and 10-bit semi-planar frames (NV12/P010, what hardware decoders produce) are converted to RGB on the graphics card.
- During playback the interface is redrawn when the next video frame is due (at most 60 times a second) instead of at the screen's refresh rate, which lowers processor and graphics card use on high refresh rate screens.



### Added

- Paste media from the system clipboard with Ctrl+V: images copied in a web browser or an image editor, media files copied in the file manager, and links or paths to images (links are downloaded in the background). The image is saved to a folder, imported into the project and, while a sequence is edited, placed at the playhead on the first free video track above the material there and selected.
- The paste dialog shows a preview, the file name and the destination folder, with an "Always save here" option; the folder and the option can be changed later in Preferences (Pasted Images).
- A randomized test applies thousands of timeline edits (overwrite, insert, razor, delete, ripple delete, lift, extract, trims, moves, transitions and track locks) and checks that the project stays valid and that locked tracks never change.

### Changed

- The log viewer opens as a regular pop-up of a fixed size instead of growing to the full height of the window, and has a Close button.
- Slimmer OP logo: the bowl of the P is drawn lighter than the stem.

### Fixed

- A ripple trim of a clip whose linked partner had been moved out of sync could cut through the partner and make it overlap its neighbor; each linked clip now gets the added or removed time at its own edge.

## 0.3.0 - 2026-09-30

### Added

- Sixteen new video effects of our own: Glow, Radiant Glow (a wide, soft multi-octave glow), RGB Split, Camera Shake, Digital Glitch, Film Grain, Vignette, Cinematic Bars, Lens Distortion, Radial Blur, Motion Tile, Light Leaks, Old Film, VHS Tape, Strobe Light and Kaleidoscope.
- Twelve new video transitions: Zoom In, Zoom Out, Spin, Stretch, Smooth Slide, Blur Dissolve, Luma Fade, Flash, Light Leak, Film Burn, Glitch and Chromatic Split. The motion transitions mirror the image at its edges, so zooms and spins never show empty borders.
- Animation presets in the Effects panel (Presets): slow zooms, punch and pop entrances and exits, fades, slides from each side, spins, blur in/out, impact shake, handheld camera, glitch burst and ready-made looks. They are dragged onto a clip or applied with a double-click like any effect, and their keyframes adapt to the clip length.
- Effect Controls has a divider between the parameters and the keyframe timeline that can be dragged and is remembered, plus a button to show or hide the timeline.

### Changed

- Importing Premiere Pro projects (`.prproj`) and XML now recognizes effects and transitions it does not implement under their own identity and uses our closest equivalent (for example popular glow, blur, shake, glitch and zoom plug-ins), instead of leaving them unrendered. The import report counts these replacements and lists each one.
- `.prproj` import reads Opacity, maps transition items to transitions at the clip edges they cover, and reads Fast Blur settings into Gaussian Blur.
- XML import maps filters by name with their parameter values and keyframes, including Basic Motion scale and rotation and Opacity fades.

### Fixed

- Long effect and parameter names in Effect Controls are shortened (full name on hover) instead of running over the values and buttons, in effect headers and group titles too.

## 0.2.0 - 2026-09-30

### Added

- Exports can be paused and resumed; paused time is not counted in the elapsed and remaining estimates.
- While exporting, the Program monitor shows the frames being rendered with the export progress, and the bottom right corner of the window shows the progress with Pause and Cancel (details such as frames, elapsed and remaining time, speed and encoder on hover). When the export ends, an Open Folder button appears there.
- The output frame rate can be chosen in Export Settings (Ctrl+M); the sequence rate stays the default.
- Crash recovery: shortly after each change the project is written to a recovery file in the background, and an emergency copy plus a crash report are written if the program fails. The recovered project is offered at the next start.
- Program log with an in-app viewer (Help > Show Log...) offering level filter, search, copy and clear. The log file can be switched on or off and its detail level chosen in Preferences; the previous session's log is kept and crash reports are always written.
- Full color picker for color parameters and mattes: saturation and brightness square, hue and opacity strips, RGB fields and hexadecimal code.
- Closing the program during an export asks whether to stop it or keep exporting.

### Changed

- Interface Scale in Preferences is applied with an Apply button instead of resizing the whole interface while the slider moves.
- Projects open on a background thread with a progress indicator; autosave and recovery files are written in the background.
- Panel tabs have gray panel icons, more spacing and a flat look with an underline on the active tab.
- The Tools panel starts as a compact strip without a tab bar. Workspace layouts saved by 0.1.0 are replaced once by the new default layout.
- Dialogs, pop-ups and undocked panels open centered on the program window.
- The panel tab context menu and more status messages follow the interface language.
- Effect Controls and the monitor transport controls adapt to narrow panels: long names are shortened (full name on hover) and less important buttons move to the settings menu instead of overlapping.
- Thinner OP logo.

### Fixed

- A failing panel, dialog, decoder, thumbnail or audio mix no longer closes the program; the affected part is restored or skipped and the problem is logged.
- Graphics validation errors are logged instead of ending the program.
- An export to a folder that cannot be written, for example one protected by Windows Controlled folder access, reports a clear error instead of "No such file or directory".
- Minimizing the window no longer squeezes the panels; their sizes are kept for when it is restored.

## 0.1.0 - 2026-09-29

### Added

- Rust editor application with dockable panels, timeline tools, FFmpeg media I/O, GPU rendering, audio mixing and project interchange.
- Portable Windows x64 ZIP and Linux x86-64 AppImage packages.
- Spanish and English interface catalogs.
- Package self-test and SHA-256 checksums.
- Original violet-and-white OP identity.

### Changed

- Timeline Ctrl/Cmd+wheel zoom uses discrete, cursor-anchored steps without inertial continuation.
- Product name, executable, package names and application identifiers standardized as OpenPremier.

### Known limitations

- `.prproj` import is read-only and covers a subset of project structures.
- OpenFX, VST3 and WebAssembly scripting are not included in this release.
- A macOS package is not available yet.

### Local fframes GPU continuation (2026-10-07, unreleased)

- Select fframes GPU (Skia/Vulkan) independently from CPU/native rendering and video encoding.
- Verify physical GPU/context/output allocation before opening the destination; no CPU fallback.
- Add 16 SVG blend modes, text shadows and Cross Dissolve/Dip to Black/Dip to White, with
  premultiplied transition mixing and content-dependent cache identities.
- Extend CPU/GPU export acceptance, native pixel comparisons, MCP schema and Windows worker CI.
- Record exact catalog gaps, pinned Skia binary/license provenance and worker MSVC runtime.
  Full effect/HDR/preview parity and native redistribution review remain pending.

### Local fframes compatibility continuation (2026-10-07, unreleased)

- Add an explicitly identified hybrid compositor sharing native preparation for all 92 effects,
  43 transitions and 27 blends, captions, feather, mattes/adjustments and interpreted media.
  Preserve separate normal tracks; destination-dependent operations use native checkpoints.
- Transport premultiplied RGBA16F through bounded binary IPC and Skia floating surfaces on
  CPU/GPU. Deliver high-bit codecs through native conversion, with no intermediate RGBA8.
- Add Preferences selection for asynchronous Source/Program fframes previews, coalesced
  requests, stale-project/result guards, persistent workers and nonblocking cancellation.
- Add six CPU/GPU differential/runtime tests, including the complete catalog, caption styles,
  PQ/HLG source conversion, floating precision, preview and all six exported codecs.
- Keep independent engine coverage distinct from hybrid compatibility. Native HDR mastering,
  zero-copy GPU sharing, hardware/platform/UI QA and redistribution review remain pending.
