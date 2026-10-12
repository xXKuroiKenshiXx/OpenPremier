# fframes: adaptación y aceptación

2026-10-07. Compatibilidad híbrida del catálogo actual implementada en Windows x64.
No equivale a una implementación independiente al 100% ni a paridad completa con Premiere.

| Área | Adaptación ejecutada | Pendiente real |
|---|---|---|
| Efectos | Los 92 efectos comparten preparación nativa | Puertos independientes SkSL; corpus completo de parámetros/extremos |
| Transiciones | Las 43 comparten preparación nativa | Referencias más amplias; tres muestras por transición probadas |
| Mezclas | Las 27 disponibles; checkpoints para operaciones dependientes del destino | Puertos independientes y corpus de imágenes |
| Gráficos/fuentes | Captions/feather/barras/ajustes/mattes/anidamiento e interpretación nativos compartidos | QA de proyectos grandes y medios adicionales |
| Precisión | RGBA16F binario; Skia CPU F32/GPU F16 conserva subnormales | Entrega PQ/HLG 10 bits y HDR10 implementada; monitor/corpus de grading HDR pendientes |
| Preview | Selector en Preferencias, Source/Program asíncronos, solicitudes coalescidas | QA visual/interactivo, sesiones largas y rendimiento |
| Entrega | Seis códecs/audio/tiempo racional; entrega 10 bits sin pasar por RGBA8 | Hardware/plataformas adicionales |
| GPU | Skia/Vulkan físico RTX 3090 Ti; efectos GPU físicos | Intel/AMD, Metal/GPU físico Linux, drivers y presión de memoria |
| CPU | Skia raster flotante + preparación nativa software WARP/DX12 | Linux WSL/llvmpipe también probado; Mac/ARM pendientes |
| Transferencia | Bloques flotantes sin PNG/base64, presupuesto 512 MiB | Texturas compartidas, readback/encoder sin copias y benchmarks |
| Licencias | fframes fijado; `half` antes transitivo ahora explícito; procedencia/notices conservados | Fuentes/avisos nativos exactos, términos MSVC y revisión pendiente |

El [inventario generado](fframes-coverage.json) distingue `supported` (subconjunto SVG propio)
y `hybrid_supported` (preparación compartida). Los 140 componentes de vídeo y 27 mezclas tienen
compatibilidad híbrida; 7 componentes y 16 mezclas tienen implementación independiente SVG.
No se contabilizan los efectos compartidos como trabajo implementado por fframes.

```powershell
cargo xtask cargo run --quiet --package op-application --example fframes_capabilities
```

## Evidencia actual

- 320 pruebas de workspace y dos regresiones del worker pasaron, cero fallos, cinco opcionales ignoradas. Se exigieron
  GPU nativa, software nativo, fframes CPU y GPU. Las pruebas opcionales de modelos/stress/
  imagen y el corpus privado de proyectos propietarios conservaron evidencia anterior.
- Seis nuevas pruebas diferenciales/runtime ejecutaron ambas superficies flotantes: los 92
  efectos y 3 gráficos, las 43 transiciones en tres tiempos, las 27 mezclas y composición lineal;
  captions con todos los estilos, feather, ajustes; fuente RGBA16/PAR/rotación/PQ/HLG/alpha;
  preview coalescido y cierre no bloqueante; exportación de los seis códecs a 30000/1001.
- Comparación con salida nativa: tolerancia máxima de dos niveles RGBA8 en los casos de
  composición/efectos; hasta doce después de decodificar códecs con pérdida. Los valores
  flotantes 2.0 y 0.50048828125 y source-over premultiplicado se conservaron exactamente.
- Todos los subnormales half-float positivos se conservan en el puente CPU/GPU. La aceptación
  HDR agrega 24 exportaciones Windows en cuatro motores: en la escena de texto/desenfoque,
  cada worker coincide exactamente con su preparador nativo después de decodificar.
- Las diez pruebas anteriores del subconjunto SVG siguieron pasando. Worker/workspace
  rustfmt y Clippy con warnings denegados pasaron.

Las pistas normales se componen realmente en Skia. Una capa de ajuste o mezcla dependiente
del destino puede consolidar toda la imagen en un checkpoint nativo; si es la última operación,
el trabajo visual queda resuelto por el motor nativo. Exportación y documentación identifican
la ruta híbrida. Ningún efecto extranjero/desconocido se omite para aparentar compatibilidad.

## Criterios pendientes de cierre completo

La compatibilidad funcional del catálogo actual ya no exige retirar efectos ni elegir manualmente
el motor nativo. Para declarar independencia completa, paridad amplia o mejoras de velocidad
faltan puertos propios, corpus de referencias/parámetros, QA visual/plataformas y benchmarks.
La entrega HDR ya convierte píxeles y metadatos; faltan monitor/scopes HDR y aceptación amplia
de nodos creativos. La continuación añade pruebas Windows DX12/WARP y Linux WSL/fframes CPU.
Véase [hdr-and-platform-acceptance.md](hdr-and-platform-acceptance.md).
La distribución pública conserva la revisión legal pendiente; el paquete local no resuelve sus
fuentes nativas ni la procedencia histórica. Véase [render-engines.md](render-engines.md).
