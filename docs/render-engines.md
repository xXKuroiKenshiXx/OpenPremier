# Motores de renderizado seleccionables

Actualizado 2026-10-07; OpenPremier 0.10.0, cambios locales sin publicar.

Exportación ofrece GPU nativa, software nativo, fframes CPU y fframes GPU. Preferencias
→ Motor de previsualización selecciona GPU nativa, fframes CPU o fframes GPU para ambos
monitores. Las preferencias antiguas mantienen el motor nativo. La codificación por hardware
se selecciona por separado; el progreso y MCP identifican el compositor y el encoder efectivos.

## Dos rutas explícitas

| Ruta | Preparación y composición | Alcance |
|---|---|---|
| SVG independiente | CpuFrameRenderer o SkiaFrameRenderer de fframes 1.2.0 | Subconjunto SDR: Motion/Opacity, texto/sombra/formas, 16 mezclas y 3 transiciones |
| Compatibilidad híbrida CPU | Preparación nativa en adaptador software; Skia CPU RGBAF32, transporte RGBA16F | Catálogo actual completo del editor |
| Compatibilidad híbrida GPU | Preparación nativa en GPU física; backend Skia/Vulkan RGBA16F | Mismo catálogo, sin fallback CPU |

La exportación elige la ruta híbrida cuando la escena excede el subconjunto SVG o el códec
recibe más de ocho bits. La previsualización fframes siempre utiliza la ruta flotante. El
progreso informa `hybrid RGBA16F / shared effects` y el adaptador empleado. Los efectos
compartidos no se cuentan como puertos propios a SkSL ni como independencia completa de wgpu.

El puente reutiliza los 92 efectos, 43 transiciones, 27 modos de mezcla, captions animados,
feather, barras, mattes, ajustes, anidamiento, keyframes e interpretación de medios existentes.
Las pistas normales se envían separadas y Skia realiza su composición source-over. Las capas
de ajuste, mezclas que dependen del destino y composición lineal generan checkpoints nativos
que incorporan las pistas anteriores. Una escena que termina en un checkpoint puede quedar
resuelta por el motor nativo: no se atribuye ese trabajo a Skia.

## Precisión, color y entrega

Las capas y el resultado híbrido son RGBA16F premultiplicado, transportado como bytes binarios.
Las superficies CPU/GPU usan el backend público de fframes Skia y conservan valores superiores
a uno y diferencias menores que un paso de ocho bits. La superficie CPU y sus imágenes usan
RGBAF32, con conversiones half-float explícitas en Rust: la ruta raster F16 de Skia descartaba
valores subnormales y afectaba sombras/alpha pequeños. GPU conserva su superficie RGBA16F.
La entrega vuelve a la conversión nativa
a YUV de 8/10 bits o RGBA8, según el códec. No se convierte primero a PNG/RGBA8 en esta ruta.

Se conservan rotación, PAR y alpha premultiplicado/ignorado/invertido según la implementación
nativa. Medios PQ/HLG y de alta profundidad siguen su conversión al espacio de trabajo nativo.
La exportación permite Rec.709 SDR o Rec.2100 PQ/HLG de 10 bits con HEVC/ProRes. HDR convierte
entradas/primarias/luminancia y entrega BT.2020, con metadatos HDR10 en PQ. El puente transporta
trabajo Rec.2020 / 203 cd/m² en HDR. El monitor de grading y el corpus HDR de nodos creativos
siguen pendientes. Véase [hdr-and-platform-acceptance.md](hdr-and-platform-acceptance.md).

Los seis códecs, mezcla de audio nativa, tiempos racionales, pausa y cancelación mantienen su
flujo existente. ProRes 422 HQ/4444 usan entrega de 10 bits. PNG/ProRes 4444 conservan alpha.

## Previsualización y límites

Un hilo por monitor mantiene contexto/worker y procesa una solicitud, con una solicitud pendiente
que se sustituye por la más reciente. La UI recibe imágenes flotantes y conserva los monitores,
scopes, checkerboard, controles y navegación. Resultados de otro proyecto/revisión/secuencia/
motor se descartan. Durante reproducción puede mostrarse el último cuadro del mismo proyecto.
Cerrar/resetear un monitor cancela IPC sin esperar en la UI. Las pruebas ejercitan el servicio;
la interacción visual de los monitores todavía necesita QA.

Se comprueban worker, superficie, lectura flotante y adaptador antes de abrir el archivo de salida.
GPU exige Vulkan físico en Windows; su ausencia devuelve error y no cambia a CPU. La aceptación
local corresponde a NVIDIA RTX 3090 Ti y WARP/DX12; Linux WSL tiene aceptación CPU/llvmpipe.
Metal y GPU física Linux/AMD/Intel siguen sin aceptación.

El cuadro máximo tiene 33.177.600 píxeles y 16.384 por eje. Las capas retenidas/transmitidas están
acotadas a 512 MiB por cuadro; al superar ese presupuesto se consolidan en un checkpoint nativo.
Este límite no describe toda la memoria del proceso: texturas, buffers de lectura y Skia añaden
memoria; la superficie y los buffers temporales F32 CPU duplican el tamaño por muestra.
SVG está acotado a 64 MiB. Inicio: 30 s; cuadro: 60 s; cancelación IPC: comprobación cada
25 ms. No existe todavía intercambio directo de texturas ni entrega sin readback. No se promete
mejora de velocidad: el puente añade preparación, composición y transferencias.

## Construcción y protocolo

```powershell
cargo xtask cargo build --release --locked --manifest-path tools/fframes-worker/Cargo.toml
$env:OPENPREMIER_FFRAMES_WORKER = "$PWD/tools/fframes-worker/target/release/OpenPremier-FFrames.exe"
```

El worker conserva un Cargo graph separado para evitar el conflicto `links = ffmpeg` entre
los bindings del editor y fframes. Protocolo 1 confirma backend/formato en el handshake. SVG
devuelve `width*height*4` bytes RGBA8 rectos. `--float-layers` recibe cabecera JSON + N bloques
`width*height*8`, devuelve RGBA16F premultiplicado y rechaza dimensiones/presupuestos incorrectos.
La transmisión escribe bloques separados, sin concatenar una copia adicional del cuadro.

El ZIP Windows incluye ambos EXE, FFmpeg, runtime MSVC y avisos. Revisiones/fuentes nativas
pendientes constan en [legal/skia-and-msvc.md](legal/skia-and-msvc.md); no se autoriza publicación.
MCP `export.renderer` conserva `wgpu`, `wgpu_software`, `fframes_cpu`, `fframes_gpu`.

La arquitectura utiliza el [backend Skia upstream](https://github.com/dmtrKovalenko/fframes/tree/main/fframes-skia-renderer)
fijado a 1.2.0, sin modificar ni incorporar código propietario. Cobertura y aceptación:
[fframes-completion.md](fframes-completion.md), [fframes-coverage.json](fframes-coverage.json).
