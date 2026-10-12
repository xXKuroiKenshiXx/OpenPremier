# Technical documentation

OpenPremier 0.10.1 is an early alpha with builds for Windows, Linux and macOS. This documentation records the plan, the implemented architecture and which builds and behavior have actually been tested.

## Project documentation

| Document | Purpose |
|---|---|
| [roadmap.md](roadmap.md) | What editors can do today and what comes next, in order |
| [implementation-status.md](implementation-status.md) | Implemented modules, verified checks and known limits |
| [audit-2026-10-06.md](audit-2026-10-06.md) | Source audit, preservation of existing features, reconstruction priorities and evidence limits |
| [caption-editing.md](caption-editing.md) | Caption split/merge, start/end editing, MCP tools and preservation rules |
| [audio-routing.md](audio-routing.md) | Native mono/stereo/5.1 mixing, nested masters, downmix and device limits |
| [premiere-parity-matrix.md](premiere-parity-matrix.md) | Feature gaps, evidence and acceptance requirements for compatibility |
| [fframes-assessment.md](fframes-assessment.md) | Rust SVG video generator: initial assessment, dependency conflict and subsequent CPU worker implementation |
| [render-engines.md](render-engines.md) | Selectable native GPU/software and isolated fframes CPU/Skia GPU export; supported scenes, packaging and limitations |
| [drift-comparison.md](drift-comparison.md) | Review of the Drift editor's rendering and features, and what we adopted |
| [release-process.md](release-process.md) | Package and release procedure |
| [architecture.md](architecture.md) | Rust subsystem boundaries and dependency direction |
| [data-model.md](data-model.md) | Project, timeline, effect and audio data model |
| [keyboard-shortcuts.md](keyboard-shortcuts.md) | Default shortcuts and dispatch rules |
| [timeline-behavior.md](timeline-behavior.md) | Editing, trimming, snapping, links and history |
| [effects.md](effects.md) | Effects, transitions, audio effects and graphics |
| [assistants.md](assistants.md) | Editing with AI assistants (Model Context Protocol) |
| [silence.md](silence.md) | Cutting silences, filler words and repeated takes |
| [color-nodes.md](color-nodes.md) | The node-based color system: correctors, mixers, qualifier, windows and keys |
| [updates.md](updates.md) | Updating from inside the program: check, download, checksum and install |
| [performance.md](performance.md) | Heavy-load benchmark results and what they mean for smaller computers |
| [plugin-api-spec.md](plugin-api-spec.md) | OpenFX and WebAssembly extension design |

## Clean-room policy

OpenPremier is developed independently and contains no code, assets or data taken from Adobe
products; see [legal/clean-room.md](legal/clean-room.md). Notes from studying other products are
kept outside this repository.

- [Entrega HDR y aceptación por plataforma](hdr-and-platform-acceptance.md): conversión PQ/HLG, pruebas y pendientes.
