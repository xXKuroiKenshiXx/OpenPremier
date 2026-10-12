# Entrega HDR, plataformas y aceptación

2026-10-07. Implementación local sin publicar. La entrega PQ/HLG ya convierte los píxeles;
no se limita a etiquetar una imagen SDR como HDR. Esto no certifica un flujo completo de
masterización, un monitor HDR ni paridad con Premiere.

## Uso

En Exportar, elegir HEVC, ProRes 422 HQ o ProRes 4444 y seleccionar **Color de exportación**:
SDR Rec.709, Rec.2100 PQ o Rec.2100 HLG. La ventana permite cambiar su tamaño y desplazarse
para acceder a todos los controles. El motor nativo/software/fframes y el codificador por
hardware son selecciones independientes. HDR fuerza la ruta flotante de fframes.

El API MCP `export` acepta `color: "bt709" | "pq" | "hlg"` y un objeto opcional `hdr10`:
`mastering_max_nits`, `mastering_min_nits`, `max_cll`, `max_fall`. Los valores predeterminados
son 1000 cd/m², 0.0001 cd/m², 1000 y 400. Las primarias de masterización son BT.2020/D65.
Son metadatos definidos por el usuario: no se midieron automáticamente del contenido.
HLG no incluye metadatos estáticos HDR10. Valores inválidos, H.264, DNxHR HQ o PNG con HDR
se rechazan antes de abrir el archivo de destino. SDR sigue siendo la opción predeterminada.

## Cadena de color

- Entrada HDR: decodificación YUV/RGB con profundidad, rango y matriz del archivo; EOTF PQ
  absoluta, HLG con OOTF de una pantalla de referencia de 1000 cd/m² y gamma de sistema 1.2,
  sRGB o RGB lineal. La conversión deshace y restaura la premultiplicación de alpha.
- Trabajo HDR: RGBA16F, primarias Rec.2020, codificación gamma 2.4 extendida, con 1.0 lineal
  igual a 203 cd/m². Los gráficos SDR usan ese blanco de referencia. Mantener Rec.2020 evita
  cancelación numérica de canales oscuros al transportar colores muy saturados en half-float.
  Skia GPU conserva RGBA16F; Skia CPU compone en RGBAF32 y convierte half-float en Rust para
  preservar los subnormales que la conversión raster F16 de Skia descartaba. El protocolo
  permanece RGBA16F premultiplicado; no cambia el formato de proyectos ni las opciones de motor.
- Los efectos y transiciones creativos existentes conservan su contrato Rec.709 mediante
  conversiones de compatibilidad; disoluciones básicas y composición permanecen en el espacio
  de trabajo HDR. La composición lineal HDR maneja alpha antes y después de la transferencia.
  Los nodos con clamps/LUTs/rangos SDR conservan esos límites: falta un corpus HDR por nodo,
  parámetros extremos, gráficos de referencia y una política de gamut/tone mapping de grading.
- Entrega: PQ/HLG y matriz BT.2020 no constante, YUV limitado de 10 bits. HEVC usa Main10;
  NV12/P010 sólo reorganizan las muestras cuando el codificador lo requiere. ProRes conserva
  su profundidad de salida y ProRes 4444 también alpha. PQ lleva mastering y CLL/FALL en los
  datos del flujo/contenedor y, en HEVC, SEI. No existe fallback a HEVC de 8 bits para HDR.
- La miniatura de exportación aplica una curva SDR Reinhard por luminancia y conversión a
  Rec.709. No es una pantalla HDR de referencia. Source/Program y scopes necesitan todavía
  gestión HDR del proyecto/pantalla; no se anuncian como monitores de masterización.

Las transferencias/primarias desconocidas usan el supuesto Rec.709 existente. La etiqueta
Bt601 genérica se interpreta como SMPTE-C y P3 como P3-D65; el modelo debe distinguir PAL,
DCI-P3 y otras variantes antes de prometer cobertura colorimétrica de todos los archivos.

Las ecuaciones proceden de [ITU-R BT.2100](https://www.itu.int/rec/R-REC-BT.2100/en) y el blanco
de gráficos de [ITU-R BT.2408](https://www.itu.int/hub/publication/r-rep-bt-2408-4-2021/).

## Validación ejecutada

320 pruebas del workspace pasaron, cero fallos y cinco opcionales ignoradas. Se exigieron
GPU nativa, software, fframes CPU/GPU y NVENC para las pruebas HEVC HDR. Los tests de editores
que modifican la recuperación global se aislaron para evitar que invalide una prueba paralela.
No se modificó el mecanismo de recuperación en producción.

Además pasaron dos regresiones del worker aislado en Windows y Linux: todos los subnormales
half-float positivos, frontera normal, highlights y source-over con alpha pequeño. La prueba
del puente IPC verifica también los 1023 subnormales con alpha pequeño en CPU y GPU.

La aceptación del paquete Windows añade 24 exportaciones de una escena con título y desenfoque:
dos transferencias, tres códecs y cuatro motores (GPU nativo, WARP, fframes CPU/GPU), con PATH
limitado a System32. Al decodificar los tres cuadros, cada worker coincide exactamente con su
preparador nativo en esta escena (máxima diferencia: cero códigos YUV de 10 bits). Los motores
nativos Vulkan/WARP sí difieren hasta 23 códigos; esa medición se conserva por separado.
Se comprueban además metadatos HDR10 personalizados, destinos existentes y proyecto intacto.
Esto no garantiza igualdad entre todos los drivers ni en cualquier escena/códec.

`crates/op-render/tests/hdr.rs` compara códigos YUV con ecuaciones escalares independientes:
PQ a 0–10000 cd/m², HLG a 0–1000 cd/m², colores Rec.2020 saturados, alpha, negro transparente
y compatibilidad SDR. Tolerancia: hasta dos códigos de luma para grises y tres en roundtrips
de gama amplia, después de cuantización RGBA16F/YUV de 10 bits.

`crates/op-application/tests/hdr_export.rs` exporta y decodifica tres cuadros por archivo:
24 combinaciones Windows de dos transferencias, tres códecs, tres motores y HEVC CPU/NVENC.
Comprueba primarias/matriz/transferencia, profundidad y códigos de luma; ffprobe comprueba
mastering/CLL/FALL en PQ y su ausencia en HLG. También reexporta fuentes HDR reales de 1000
nits y protege destinos existentes frente a configuraciones HDR inválidas.

| Entorno | Evidencia ejecutada | Límite |
|---|---|---|
| Windows, RTX 3090 Ti / Vulkan | Render nativo, fframes GPU, 24 exportaciones y fuentes HDR | Una GPU NVIDIA/driver |
| Windows, RTX 3090 Ti / DX12 | Tres pruebas colorimétricas HDR | Otra API, misma GPU |
| Windows WARP / DX12 | Tres pruebas HDR y preparación de exportaciones fframes CPU | Software |
| Linux Ubuntu 24.04 bajo WSL | Vulkan llvmpipe, fframes CPU, 16 exportaciones HDR; HEVC libx265/NVENC; pruebas del puente/SVG | El compositor Vulkan es software; Skia GPU rechaza el dispositivo CPU |
| macOS / Metal | Worker añadido a CI y empaquetador DMG | Sin ejecución en Mac en esta revisión |
| AMD / Intel / ARM | Rutas y dependencias configuradas | Sin hardware disponible para aceptación |

Linux y macOS incorporan ahora el worker al empaquetado, comprueban sus dependencias y ejecutan
self-tests CPU. Esas rutas de paquete necesitan validación completa AppImage/DMG; la ejecución
WSL anterior prueba binarios de desarrollo. No se atribuye aceptación a CI sólo por configurarla.

## QA interactivo pendiente

La ventana de la aplicación todavía no se probó de forma interactiva con estas funciones. Los
tests de render/preview/layout no sustituyen esa prueba; el QA interactivo continúa pendiente.

Al recuperar acceso a la ventana, validar exportación HDR y errores antes de escribir; acceso
a los controles con las tres densidades y alturas reducidas; cambio de motor en Source/Program,
seek/reproducción, scopes/alpha, edición durante exportación, pausa/cancelación y reapertura
de proyectos/layouts. Registrar capturas y resultados por motor/driver, incluyendo sesiones largas.

## Distribución

Se recuperaron y verificaron la identidad del archivo FFmpeg y todas sus DLL instaladas,
fuentes upstream fijadas de FFmpeg/Skia/recetas, y DEPS/avisos de dependencias Skia. Son evidencia
parcial. Fuentes nativas externas completas, correspondencia de build, derechos MSVC y la
investigación histórica de implementaciones propietarias siguen pendientes. La puerta de
publicación exige ausencia de bloqueos, digest actual y archivos de evidencia retenidos;
no basta cambiar una cadena a `approved`. Véase [la auditoría legal](legal/audit-2026-10-07.md).
