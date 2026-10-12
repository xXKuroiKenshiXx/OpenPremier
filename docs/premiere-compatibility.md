# Premiere project compatibility

Open Premiere reads recognizable `.prproj` object graphs without blocking on their serialization
version. Gzip/XML and uncompressed UTF-8 XML (including BOM) are accepted. This is partial import,
not guaranteed support for every Premiere release: unfamiliar effects and graph shapes still
appear in the import report. The importer never downgrades or writes the source.

## Observed local corpus, 2026-10-06

Three local Premiere 2024 templates were opened by the importer. Broadcaster contained nine
sequences and no clips; Standard contained two sequences and no clips. Social contained ten
sequences and 18 clips, but the import report recorded **seven caption/data tracks and two
StyleProjectItem objects skipped**. None of these results proves lossless compatibility.
Captions imported as foreign Premiere tracks/styles remain a distinct gap from OpenPremier's
native caption graphics/editor. See [premiere-parity-matrix.md](premiere-parity-matrix.md).

The corpus test remains opt-in through `OPENPREMIER_PRPROJ_DIR`. If supplied, a directory with
zero projects fails the test. The template files remain private and are not shipped.

## Save a copy for an older Premiere

File > Downgrade Adobe Premiere Project opens an independent file utility. Choose the original
`.prproj`, inspect its container and Project versions, select a target Project serialization
number, then Save Downgraded Copy. The default is 1, following
[downgrade-premiere-prproj](https://github.com/ocelotsloth/downgrade-premiere-prproj)
by Mark Stenglein (MIT). It is not a mapping of Premiere release years.

The utility verifies the gzip and bounded XML, identifies exactly one root Project object of
class `62ad66dd-0dcd-42da-a660-6d8fbde94876`, and replaces only its Version value. Other XML bytes,
unknown objects, nested versions, comments, CDATA and the BOM stay intact. The result is gzip
written atomically to a new file; existing files cannot be overwritten. Target 0, increases,
ambiguous Project definitions and malformed containers are rejected (PR-VERSION-001).

This does not convert newer features. An older Premiere may discard them, fail to open the copy,
or render it differently. Verify the copy in the target Premiere before using it. No compatibility
claim is made from synthetic tests alone. The utility operates on original file bytes, not on
the Open Premiere project model; general `.prproj` export remains unavailable (PR-SAVE-002).

```text
OpenPremier.exe --downgrade-prproj input.prproj input-downgraded.prproj
OpenPremier.exe --downgrade-prproj input.prproj input-v41.prproj --target-version 41
```

## Sequence presets

New Sequence / Sequence Settings > Load Premiere Sequence Preset reads `.sqpreset` files
(PR-PRESET-001). Supported: progressive frame size, exact rational FPS from Premiere ticks,
pixel aspect ratio, audio sample rate, initial video track count and audio track count (both
the direct array and SerializerWrappedObject shapes). Changing existing sequence settings keeps
its track counts. The loaded master defaults to stereo. Preview codecs, proprietary editing modes,
channel routing, submix semantics, timecode display and interlacing are not imported.

Malformed numeric fields (including the observed `84SQ667200` token), unsupported field order,
unreasonable dimensions/rates and excessive tracks are rejected. Tests use original synthetic
XML. Adobe presets and other resources are not shipped with Open Premiere.
