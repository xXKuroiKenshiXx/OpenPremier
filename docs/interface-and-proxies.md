# Interfaz y proxies — 2026-10-07

La interfaz usa densidad **Cómoda** por defecto. Preferencias > Densidad de la interfaz permite
Compacta, Cómoda o Amplia, independientemente del porcentaje de escala/DPI y de la fuente.
El cambio se aplica sin reiniciar y se guarda en preferences.json. Las preferencias antiguas
reciben la densidad Cómoda y conservan idioma, fuente, escala y el resto de sus ajustes.

Se amplían texto base, separación, botones, listas de proyecto, parámetros de efectos, pestañas
y controles del monitor. Las nuevas pistas tienen 58 puntos; las alturas personalizadas guardadas
se conservan. Se mantienen los diecisiete paneles y seis espacios de trabajo, con separadores
más fáciles de arrastrar. Los diseños guardados no se reinician; los diseños de fábrica dan más
ancho a la tira de herramientas y esta conserva controles accesibles en diseños antiguos estrechos.

Acciones rápidas sigue siendo la paleta nativa de búsqueda de efectos, comandos y macros.
OpenFX es otro estándar: la paleta no carga plugins OpenFX ni FX Console.

## Proxies

Preferencias > Proxies automáticos > Crear proxies al importar video es opcional y está apagado
por defecto. Resolución de los proxies permite 540p, 720p o 1080p (lado corto, sin ampliar fuentes
más pequeñas). La elección también se usa al crear proxies manualmente desde Proyecto > Proxy.
No se recrean automáticamente proxies ya adjuntos al cambiar la resolución.

La importación sigue disponible mientras una única cola convierte videos en segundo plano.
El archivo original y el audio permanecen como fuentes del proyecto/exportación; el proxy es
una copia H.264 para reproducción. Imágenes fijas, audio sin video y transparencia conservada
se excluyen. El convertidor actual acepta fuentes YUV; un formato no soportado reporta un error
sin borrar el original ni bloquear la importación. No se promete compatibilidad universal con
HDR, interpretaciones complejas o todos los formatos de cámara.

La identidad del caché incluye tamaño, fecha de modificación, resolución, tasa interpretada e
índice/cantidad de cuadros. La cola evita duplicar trabajos de un mismo asset. Al abrir otro
proyecto se cancela/separa la cola anterior; undo, relink o cambios de interpretación no pueden
adjuntar un resultado a una fuente diferente aunque se reutilice un ID. Los proxies se adjuntan
mediante una transacción reversible y pueden retirarse con el comando existente.

Zero-copy de superficies de hardware y render-cache de rangos siguen pendientes del primer
bloque del roadmap: necesitan interoperabilidad por backend y validación propia. No se declaran
implementados por haber agregado proxies.

La validación de estilo genera imágenes sin ventana usando los controles, fuentes y theme reales;
son muestras de estilo, no capturas de toda la aplicación. La revisión interactiva completa de
todos los paneles/DPI/monitores queda pendiente en un entorno con automatización nativa habilitada.
