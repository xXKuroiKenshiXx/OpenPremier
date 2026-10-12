# Skia GPU worker: native dependencies

Local integration, 2026-10-07. Publication remains subject to the pending distribution review.

The GPL-3.0-or-later worker uses published `fframes_skia_renderer =1.2.0` (MIT),
`skia-safe` / `skia-bindings =0.153.3` (MIT) and Vulkan `ash` (MIT OR Apache-2.0).
Original crate/upstream notices and exact Cargo checksums accompany the source inventory.

The Windows Skia archive is pinned in `licenses/skia/prebuilt-windows-x64.json`.
Its SHA-256 matches the digest published by the upstream GitHub release. The eight static
libraries were compared byte for byte by SHA-256 with the installed build output.
`licenses/skia/LICENSE_SKIA.txt` retains the original Google BSD notice from that archive.
The archive remains in the local native dependency directory, outside the application source ZIP.

This receipt establishes acquired binary identity; it does not establish complete corresponding
source, reproducibility or all embedded third-party notices. Review Skia's exact
`m153-0.101.2` sources/DEPS and embedded ICU, font and codec libraries before publication.
The existing FFmpeg GPL source/provenance review also remains pending.

The published Windows archive uses the dynamic MSVC CRT. `xtask` applies
`-C target-feature=-crt-static` only to the separate worker Cargo invocation.
The editor retains its existing static-runtime build. Local Windows packages copy unmodified
`msvcp140.dll`, `vcruntime140.dll` and `vcruntime140_1.dll` from the installed Visual Studio
`VC/Redist/MSVC/<version>/x64/Microsoft.VC*.CRT` directory, never System32 or DebugCRT.
Native inventory records the selected origin, size and SHA-256 of each DLL.

These Microsoft DLLs are proprietary runtime components, not GPL/MIT code. Their distribution
depends on the applicable Visual Studio license terms and redistributable list:
[Microsoft deployment guidance](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files),
[Visual Studio 2026 redistribution list](https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution).
Confirm the applicable Build Tools terms and GPL system-library treatment before publication;
listing files in REDIST does not by itself establish the distributor's license entitlement.
The release gate remains pending and no external publication is authorized by this local test.

La continuación retuvo el source archive exacto de Skia y comparó su DEPS; los 13 avisos
candidatos de dependencias nativas se conservan en `licenses/skia/native-notices/inventory.json`.
Esto mejora los materiales de revisión, pero no prueba por sí solo alcance de enlace,
correspondencia completa, reproducibilidad ni el derecho de redistribución de MSVC.
