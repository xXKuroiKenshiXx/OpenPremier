# Plugins nativos, audio y manga

Edición > Preferencias > Plugins lista los tres módulos incluidos, con interruptores independientes.
Edición > Plugins abre el administrador y las carpetas favoritas. El Explorador de plugins es una
sección sin catálogo: no consulta servidores, busca archivos ni descarga nada.

## Curvas de movimiento

Activa Curvas de movimiento, selecciona un clip y anima un parámetro con dos o más keyframes.
Abre Edición > Curvas de movimiento, o el menú contextual de un keyframe en Controles de efectos.
Elige parámetro, tramo y componente X/Y o R/G/B/A. El gráfico de valores muestra la interpolación
real; el gráfico de velocidad muestra su derivada. Arrastra los controles azules o edita X1/Y1/X2/Y2.
Hay suavizado, entrada/salida, anticipación, sobreimpulso, lineal y rebote amortiguado.
Los presets de curva se guardan por nombre; clic derecho permite borrarlos.

Las curvas cambian velocidad e influencia Bezier de los keyframes nativos. Rebote añade keyframes
ordinarios en el tramo. Se puede deshacer y la animación sigue funcionando al desactivar el módulo,
guardar/reabrir el proyecto o exportar. La vista nativa de Controles de efectos sigue disponible.
Es un editor de curvas temporales; no convierte automáticamente trayectorias espaciales en curvas
Bezier ni cambia el rendimiento de reproducción.

## Portales de carpetas

Añade una carpeta, cambia su nombre, ábrela en el explorador del sistema o importa sus archivos.
Puedes editar la ruta: una ruta relativa se resuelve junto al archivo del proyecto guardado.
«Añadir carpeta de medios del proyecto» usa `media`. Las carpetas inexistentes se muestran como
no disponibles; el módulo no las crea ni busca alternativas automáticamente.

## Acciones rápidas

Edición > Acciones rápidas, o **Ctrl+Alt+F12**, abre una búsqueda local de efectos, transiciones,
presets y comandos. Escribe, navega con arriba/abajo y pulsa Enter, o haz clic en un resultado.
Los efectos y presets se aplican a los clips seleccionados. Las transiciones se añaden en sus
extremos; sin selección se aplican en el cabezal de las pistas objetivo. El botón estrella marca
favoritos; clic derecho oculta/muestra un resultado dentro de esta paleta.

En Crear macros, añade pasos con `+`, reordénalos con la flecha o elimínalos, escribe un nombre
y opcionalmente un atajo libre, y guarda. Macros guardadas permite ejecutarlas, editarlas o borrarlas.
Una macro de un solo efecto sirve como atajo directo. Los pasos usan comandos síncronos del editor
y efectos del catálogo; cada edición se deshace por separado. No ejecutan scripts externos ni
automatizan diálogos de exportación. Los atajos no se disparan al escribir texto y respetan los
atajos normales si aparece un conflicto posterior. Desactivar Acciones rápidas desactiva también
sus macros y atajos, conservando los ajustes.

## Portapapeles nativo

Ctrl+V reconoce imágenes, archivos de medios y enlaces directos de imágenes. Se elimina el doble
pegado por eventos de texto y teclas y se recuerda el gesto cuando Ctrl se suelta antes que V.
Las imágenes se guardan como archivos de medios; el diálogo o las preferencias eligen la carpeta.
El destino conserva la secuencia y el instante de Ctrl+V durante el guardado/importación.
Los clips se colocan en una pista libre sobre el material existente y la inserción se puede deshacer.
Abrir otro proyecto cancela los pegados/importaciones pendientes del anterior.
Edición > Copiar fotograma al portapapeles renderiza el fotograma de la secuencia a resolución
completa, con los efectos, listo para pegar en otro programa o en la timeline.

## Audio

Efectos > Efectos de audio > Especial contiene:

- **Desfasador de tono / Pitch Shifter**: ventana de 20/40/100 ms, interpolación de cuatro puntos,
  suavizado de cambios de tono, desafinación estéreo, mezcla y ganancia de salida. Mantiene la
  duración. Es un algoritmo nativo de líneas de retardo; no incluye corrección de formantes ni
  afinación automática y no pretende igualar la calidad de algoritmos comerciales.
- **Frenado de cinta / Ralentización**: velocidad inicial/final, inicio y duración de la rampa.
  Remuestrea el audio: baja velocidad y tono juntos hasta detenerse, con salida a silencio para
  evitar mantener una muestra DC. Para audio lento constante, usa ambas velocidades al 50%.
  El clip mantiene su duración en la timeline; no alarga la secuencia ni cambia el vídeo. Si necesitas
  que audio y vídeo cambien de duración juntos, usa Velocidad/Duración del clip. La rampa se integra
  desde el inicio del clip de forma determinista, por lo que funciona también al buscar otra posición.
  En audio anidado, el remuestreo aproxima la rampa dentro de cada bloque de mezcla.

## Manga y páginas

Efectos > Efectos de vídeo > Manga y páginas:

- **Cámara 3D para manga**: recorrido izquierda/derecha y arriba/abajo, zoom, inclinación de
  perspectiva, duración y suavizado. El movimiento empieza en el inicio del clip en la timeline.
- **Parallax**: anima Desplazamiento de cámara con keyframes. Profundidad de capa multiplica
  el desplazamiento; Profundidad por luminancia añade una aproximación de relieve en una imagen
  plana. Zoom, inclinación X/Y y bordes transparentes controlan el encuadre.
- **Marco y viñeta para manga**: bordes horizontales/verticales, suavidad, color, opacidad y viñeta.
- **Ocultación de diálogo**: una zona elíptica/rectangular transparente, con centro, tamaño y
  suavidad animables, permite esconder parcialmente texto o una imagen. Para un personaje recortado,
  coloca fondo en V1, texto en V2 y el personaje PNG con alfa en V3; el texto queda detrás del personaje.
  Aplica Parallax a las capas con distintas profundidades. También puedes usar el efecto nativo
  Track Matte Key con alfa e inversión para una máscara más precisa. No hay segmentación automática
  de personajes ni generación de capas a partir de una sola imagen.

Transiciones de vídeo > Manga y páginas incluye **Pasar página**: cuatro direcciones, curvatura,
sombra y reversa. Tanto los efectos como la transición usan el mismo renderizador GPU para
previsualización y exportación, incluido el modo de rasterización por software.

Los módulos son implementaciones nativas independientes. Los archivos originales de extensiones
Adobe o plugins de audio comerciales no se pueden instalar o ejecutar en este administrador.
