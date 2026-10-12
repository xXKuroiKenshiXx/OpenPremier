# OpenJumpCuts settings in Open Premiere

[OpenJumpCuts](https://github.com/cameron-astor/jumpcut) by Cameron Astor is a GPL-3.0 CEP extension
for Adobe Premiere. Its audio analysis runs in Python/pydub and its host scripts edit Premiere.
Open Premiere uses its own Rust detector and timeline operations, so the Adobe extension runtime
is not loaded. The four settings have native equivalents (SIL-JUMP-001).

Clip > Remove Silence > OpenJumpCuts Preset, or Choose a preset > OpenJumpCuts, sets:

| Setting | Value | Open Premiere control |
|---|---:|---|
| silenceCutoff | -80 dBFS | Loudness threshold |
| removeOver | 1 second | Shortest Silence |
| keepOver | 0.3 seconds | Shortest Sound |
| padding | 0.5 seconds each side | Keep Before/After Cut |

The same preset and Load OpenJumpCuts Settings option are available in Remove Silence from Files.
The loader accepts a JSON object with these four numeric fields, in seconds and dBFS:

```json
{"silenceCutoff": -45, "removeOver": 0.5, "keepOver": 0.3, "padding": 0.1}
```

Optional `in`, `out` and `start` fields are ignored: choose Selected Clips, Whole Sequence or
In to Out in our dialog. Loading settings enables sound detection and disables transcription;
it does not cut anything. Analyze, review shaded ranges, exclude cuts and Apply as usual.
The fixed -80 dBFS default mostly detects digital silence; adjust it for real room noise.

No bit-for-bit match with the extension is claimed: our analysis uses 10 ms curves, removes
short sounds transitively, applies padding after combining runs, and aligns timeline cuts to
frames with undo and optional audio smoothing. Tests cover no silences, all silence, a short
sound between long silences, edge padding, a pause consumed by its padding and invalid settings.
