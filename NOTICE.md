# OpenPremier: attribution and provenance

OpenPremier is GPL-3.0-or-later. Third-party code, fonts, models and marks retain their owners.

- Rust dependencies: exact versions in Cargo.lock and tools/fframes-worker/Cargo.lock.
  `cargo xtask license-audit` retains original copyright/license/NOTICE texts and generates
  immutable crate source URLs/checksums for both resolved graphs, including build/platform deps.
- FFmpeg: GPL with version-3 components. Exact DLL sources, patches and enabled external-library
  sources still need to be matched. Generic release URLs do not prove corresponding source.
  The installed DLL hashes match the recovered 2026-09-28 upstream archive; see
  licenses/ffmpeg/prebuilt-windows-x64.json. This is binary identity evidence, not a complete source offer.
- fframes 1.2.0: MIT; original notice in tools/fframes-worker/licenses/fframes-MIT.txt.
  Its OpenPremier wrapper is GPL-3.0-or-later. fframes_skia_renderer 1.2.0 is MIT.
- Skia GPU: Google BSD notice in licenses/skia/LICENSE_SKIA.txt; exact Windows archive identity
  in licenses/skia/prebuilt-windows-x64.json. Embedded native dependency review remains pending.
  Pinned DEPS/native dependency notices are retained in licenses/skia/native-notices/.
- Windows worker MSVC CRT: unmodified Visual Studio Redist DLLs, proprietary Microsoft terms.
  See docs/legal/skia-and-msvc.md; these are not licensed under the project GPL.
- Inter Regular/SemiBold: Copyright (c) 2016 The Inter Project Authors, SIL OFL 1.1;
  crates/op-ui/assets/fonts/Inter-OFL.txt. Embedded egui font notices are also retained.
- OpenPremier OP monogram: generated from geometric shapes by tools/make_icon.py under the
  project's GPL-3.0-or-later license. Historical OS badges and release images still require
  complete provenance review; their presence in Git alone does not establish source rights.
- Whisper models: optional downloads, absent from the portable package; upstream model
  licensing/provenance must accompany any future redistribution.
- Drift / CutWire Studios: GPL-3.0 architecture/workflow reference (docs/drift-comparison.md).
  Existing notes are not forensic proof that no historical adaptation occurred. Any identified
  adapted code requires original authorship, exact revision and retained notices.
- auto-editor, unsilence, tightcut, Recut, TimeBolt, Earshot and OpenJumpCuts: documented workflow
  references. Referencing a workflow is not a code license grant. Runtime dependencies retain
  their individual licenses.

Adobe and Premiere Pro are trademarks of Adobe Inc.; Video Copilot/FX Console and Steinberg/VST
identify their respective products/standards. OpenPremier is independent and unaffiliated.
Quick Actions is a native command/effect palette. It is not FX Console or an OpenFX host.
OpenFX/VST3 hosting and Wasm scripting are planned, not implemented compatibility claims.
The current VST3 SDK is MIT; individual plugin licenses/trademarks are separate. None is bundled.

Distribution review is pending: docs/legal/audit-2026-10-07.md. Tests and metadata cannot certify
all historical provenance, patent rights or legal compliance in every jurisdiction.
