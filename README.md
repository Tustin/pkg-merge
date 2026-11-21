# pkg-merge

[![build](https://github.com/rafmarsan/pkg-merge/actions/workflows/build.yml/badge.svg)](https://github.com/rafmarsan/pkg-merge/actions/workflows/build.yml)

Une las piezas de un **PKG de PS4 o PS5** en un único archivo, usando el **manifiesto oficial `.json`** de Sony. Funciona con juegos base, parches y DLC, y no depende de que las piezas se llamen `_0.pkg`, `_1.pkg` o `_sc.pkg`: cada pieza se coloca en su `fileOffset` exacto.

- Verifica el **SHA-256 de cada pieza** antes de dar el merge por bueno.
- Verifica el **`packageDigest` global** del manifiesto.
- Permite escribir la salida en **otro disco** (`--output`), sin duplicar los 100+ GB.
- Modo **in-place** para no necesitar el doble de espacio.
- **Reanudable** tras un corte, apagón o Ctrl+C.
- Puede **descargar las piezas que falten** directamente al offset final.
- Un solo `.exe` para Windows x64, sin instalador, sin runtime y sin DLLs.

> Herramienta no oficial. No está afiliada a Sony. Úsala únicamente con copias que hayas obtenido legalmente y guarda una copia de seguridad de tus piezas.

---

## Índice

1. [Descarga y requisitos](#descarga-y-requisitos)
2. [Uso fácil (arrastrar y soltar)](#uso-fácil-arrastrar-y-soltar)
3. [Uso avanzado](#uso-avanzado)
4. [Opciones](#opciones)
5. [Cómo funciona](#cómo-funciona)
6. [Verificación SHA-256](#verificación-sha-256)
7. [Modo in-place y borrado progresivo](#modo-in-place-y-borrado-progresivo)
8. [Reanudar un merge](#reanudar-un-merge)
9. [Descargar piezas que falten](#descargar-piezas-que-falten)
10. [Nombre y ubicación del archivo final](#nombre-y-ubicación-del-archivo-final)
11. [Códigos de salida](#códigos-de-salida)
12. [Preguntas frecuentes](#preguntas-frecuentes)
13. [Desarrollo y compilación](#desarrollo-y-compilación)
14. [English quick start](#english-quick-start)

---

## Descarga y requisitos

- **Windows 10/11 x64**.
- No hace falta instalar nada.
- Descarga `pkg-merge.exe` desde la página de [Releases](https://github.com/rafmarsan/pkg-merge/releases).
- El `.exe` no está firmado, así que Windows puede mostrar un aviso de "editor desconocido" la primera vez. Es normal en herramientas open source; puedes comprobarlo compilando el código tú mismo o revisando el workflow de GitHub Actions.

---

## Uso fácil (arrastrar y soltar)

1. Consigue la URL del manifiesto. Normalmente será una URL que termina en `.crc`, por ejemplo:

   ```text
   https://.../EP0082-PPSA08668_00-EUROPE0000000000.crc
   ```

2. Abre esa URL en el navegador **cambiando `.crc` por `.json`**:

   ```text
   https://.../EP0082-PPSA08668_00-EUROPE0000000000.json
   ```

3. Guarda ese archivo `.json` **dentro de la carpeta donde están las piezas** (por ejemplo `Z:\pieces\EP0082-PPSA08668_00-EUROPE0000000000.json`).

4. Arrastra la carpeta completa sobre `pkg-merge.exe`.

El programa detecta el `.json`, empareja cada pieza con la URL del manifiesto, verifica los hashes y crea el `.pkg` final **en esa misma carpeta** (o donde indiques con `--output`).

### Si prefieres usar la URL directamente

```bat
pkg-merge.exe https://.../EP0082-PPSA08668_00-EUROPE0000000000.crc --parts-dir Z:\pieces --output C:\out
```

También acepta la URL terminada en `.json`, una ruta local al `.json`, o una carpeta.

---

## Uso avanzado

```bat
pkg-merge <url.crc | url.json | manifiesto.json | carpeta> [opciones]
```

### Unir leyendo de un disco y escribiendo en otro

```bat
pkg-merge Z:\pieces --output C:\out
```

`Z:\` puede ser lento (red, HDD externo) y `C:\` un NVMe rápido. El resultado se escribe directamente en `C:\out`.

### Verificar antes de un merge largo

```bat
pkg-merge Z:\pieces --check
pkg-merge Z:\pieces --check --jobs 1
```

`--check` valida el manifiesto y el SHA-256 de cada pieza sin escribir nada. Usa `--jobs 1` si las piezas están en un HDD (el paralelismo puede ir más lento en discos mecánicos).

### Unir sin duplicar espacio (in-place)

```bat
pkg-merge Z:\pieces --in-place --delete-parts --yes
```

Reutiliza la pieza 0 como archivo final, escribe el resto encima y borra cada pieza **después de verificarla**. El pico de espacio extra es aproximadamente el tamaño de una pieza (4 GB), no el del juego completo.

### Reanudar tras un corte

Vuelve a ejecutar exactamente el mismo comando. El programa lee el journal `.merge.json` y continúa desde la última pieza verificada.

### Descargar lo que falte

```bat
pkg-merge https://.../EP0082-PPSA08668_00-EUROPE0000000000.crc --parts-dir Z:\pieces --output C:\out --download-missing
```

Las piezas ausentes se descargan directamente al offset del archivo final, sin ocupar disco adicional, y se verifican con su SHA-256 al terminar.

---

## Opciones

| Opción | Descripción |
|---|---|
| `ENTRADA` | URL `.crc`/`.json`, ruta a un `.json` o carpeta con las piezas. |
| `--output <dir\|archivo.pkg>` | Carpeta o archivo final. Si es una carpeta, el nombre sale del manifiesto. |
| `--parts-dir <dir>` | Carpeta de las piezas. Se puede repetir. Si no se indica, usa la carpeta del manifiesto y el directorio actual. |
| `--manifest <url\|ruta>` | Manifiesto explícito, en lugar de la entrada posicional. |
| `--in-place` | Usa la pieza 0 como archivo final, sin duplicar el PKG completo. |
| `--delete-parts` | Borra cada pieza después de escribirla, sincronizarla y verificar su SHA-256. Requiere `--in-place`. |
| `--verify <post\|pre\|none>` | `post` (por defecto): copia y hashea en la misma pasada. `pre`: hashea antes de copiar (dos lecturas). `none`: sin hashes. |
| `--check` | Solo verifica manifiesto y piezas; no escribe nada. |
| `--buffer <MB>` | Búfer de E/S en MiB. Por defecto 64; máximo 1024. |
| `--jobs <N>` | Hilos para `--check`. Por defecto 4. |
| `--final-digest <auto\|rehash\|skip>` | Cómo verificar el `packageDigest` global. Por defecto `auto`. |
| `--allow-digest-mismatch` | Si el hash global no coincide, avisa pero deja el archivo final. |
| `--download-missing` | Descarga las piezas que no estén en disco. |
| `--force` | Sobrescribe la salida y reinicia un merge anterior. |
| `--yes` | No pide confirmación para `--delete-parts`. |
| `--no-progress` | Oculta la barra de progreso. |
| `--log <archivo>` | Guarda un log detallado con tiempos. |
| `-v`, `--verbose` | Muestra detalles adicionales (mapeo de piezas, espacio libre, etc.). |
| `--no-save-manifest` | No guarda una copia del manifiesto junto a la salida. |
| `-h`, `--help` | Ayuda. |
| `-V`, `--version` | Versión. |

> `--stop-after N` existe solo para las pruebas automáticas y está oculto en la ayuda.

---

## Cómo funciona

1. **Lee el manifiesto `.json`** (o lo descarga de la URL).
2. **Valida el manifiesto** antes de tocar el disco:
   - `pieces` no está vacío.
   - `fileOffset` empieza en 0 y las piezas son contiguas, sin huecos ni solapes.
   - La suma de `fileSize` coincide con `originalFileSize`.
   - `numberOfSplitFiles` coincide con el número de piezas (si existe).
   - `hashValue` y `packageDigest` son SHA-256 de 64 caracteres hex.
3. **Empareja cada pieza local** con la URL del manifiesto:
   - Primero por nombre de archivo (lo normal).
   - Si el usuario renombró las piezas, por tamaño único.
   - Si hay ambigüedad, por SHA-256. Esto resuelve casos como una pieza `JP0082-...` y la última `EP0082-..._sc.pkg`.
4. **Escribe cada pieza en su `fileOffset` exacto**, no por orden de nombre.
5. **Verifica** el SHA-256 de cada pieza y el `packageDigest` global.
6. Solo entonces renombra `<salida>.pkg.part` al nombre final.

### Datos reales comprobados

En un manifiesto real de PS5 (`PPSA08668`) con 36 piezas:

- `originalFileSize`: `146832031744`
- `numberOfSplitFiles`: `36`
- `pieces`: offsets contiguos y suma exacta.
- Los nombres de las piezas pueden pertenecer a otro título/región; los `fileOffset` son la fuente de verdad.
- Descargamos una pieza real (`_sc.pkg`, 153 MB) y su `hashValue` coincidió **exactamente** con su SHA-256. Por eso la verificación por pieza es fiable.
- `playgoChunkCrcHashValue` es el SHA-256 del archivo `.crc`; el manifiesto está en el `.json`. El programa obtiene el `.json` cambiando únicamente la extensión.

> `packageDigest` se asume calculado sobre la concatenación completa en orden de offset. Las piezas se verifican individualmente, así que el contenido es correcto aunque algún manifiesto use otra convención para el digest global. Si eso ocurre, el programa lo dirá y podrás usar `--final-digest rehash` o `--allow-digest-mismatch`.

---

## Verificación SHA-256

### Por pieza

Cada `hashValue` del manifiesto se compara con el SHA-256 real del archivo. Si una pieza está corrupta:

- El merge se detiene.
- Se indica exactamente qué pieza falla.
- Se muestran el hash esperado y el obtenido.
- El archivo final **no** se genera.

### Global (`packageDigest`)

- `auto` (por defecto): se calcula durante la misma pasada de copia, sin lectura extra.
- `rehash`: relee el archivo final completo y lo verifica (útil al reanudar o si quieres una comprobación extra).
- `skip`: no lo verifica.

### Modos de verificación

- `post` (por defecto): una sola lectura por pieza. Copia y hashea a la vez.
- `pre`: hashea la pieza antes de copiarla. Son dos lecturas, pero no se escribe nada de una pieza corrupta.
- `none`: máxima velocidad, sin hashes. No recomendado.

Las piezas descargadas con `--download-missing` se verifican siempre al terminar de escribirlas: no existen en disco antes, así que no hay un "pre" posible.

---

## Modo in-place y borrado progresivo

`--in-place` está pensado para cuando todas las piezas y la salida están en el **mismo disco** y no tienes espacio para el doble.

Qué hace exactamente:

1. Verifica la pieza 0 **antes de tocarla**. Si está corrupta, no cambia nada.
2. La renombra a `<nombre>.pkg.part`.
3. Escribe cada pieza siguiente en su offset.
4. Tras verificar y sincronizar cada pieza, si usaste `--delete-parts`, la borra.
5. Al terminar, renombra `.pkg.part` al `.pkg` final.

Seguridad:

- Nunca borra la pieza 0.
- Nunca borra una pieza antes de verificar su SHA-256 y hacer `FlushFileBuffers`.
- Si se corta el proceso, queda un `.pkg.part` reconocible y un journal para reanudar.
- `--force` puede recuperar un `.pkg.part` truncándolo a la pieza 0 original si el archivo original ya no existe.
- Aun así, **haz una copia de seguridad**: `--delete-parts` es destructivo por diseño.

Si las piezas están en `Z:\` y la salida en `C:\`, usa el modo normal (`--output C:\out`). El modo in-place necesita la pieza 0 y la salida en el mismo volumen para que el renombrado sea instantáneo.

---

## Reanudar un merge

Durante el merge se escribe un journal junto al archivo temporal:

```text
C:\out\EP0082-...pkg.part.merge.json
```

Contiene:

- Modo (`normal` o `in-place`).
- Tamaño original y `packageDigest`.
- Última pieza completada.
- Bytes escritos.

Al volver a ejecutar el mismo comando:

- Se verifica que el journal corresponde al mismo manifiesto.
- Se re-verifica la última pieza completada desde el `.part` (si es la única, además se reconstruye el estado del SHA-256 global).
- Se continúa desde la siguiente pieza.

Sobre el digest global al reanudar:

- Si se reanudó a mitad y no se pudo reconstruir el estado del SHA-256, `auto` avisa. Usa `--final-digest rehash` si quieres verificar el global leyendo el archivo completo una vez.
- Si el merge ya estaba completo y el digest había fallado, `auto` vuelve a leer y verificar; no se salta el error.

Para empezar de cero: `--force`.

---

## Descargar piezas que falten

Con `--download-missing`, las piezas ausentes se descargan desde su `url` del manifiesto y se escriben **directamente en el offset del archivo final**, hasheando mientras se descargan. No se guarda una copia temporal de 4 GB.

- Soporta `http://` y `https://`.
- Aunque muchas URLs del CDN usan `http`, la integridad está garantizada por el SHA-256 del manifiesto, que se descarga por HTTPS.
- Si una descarga termina con un hash incorrecto, el merge falla indicando la pieza. Vuelve a ejecutar o corrige el manifiesto.
- `--check` no descarga: reporta las piezas que faltan.

---

## Nombre y ubicación del archivo final

Por defecto, el nombre sale del **nombre del manifiesto**, no de la pieza 0:

```text
EP0082-PPSA08668_00-EUROPE0000000000.json
  -> EP0082-PPSA08668_00-EUROPE0000000000.pkg
```

Esto es importante porque en un mismo manifiesto la pieza 0 puede llamarse `JP0082-PPSA08664_...` y la última `EP0082-PPSA08668_..._sc.pkg`.

- `--output C:\out` crea la carpeta si hace falta y escribe `C:\out\<nombre>.pkg`.
- `--output C:\out\mi-juego.pkg` usa exactamente ese archivo.
- También se guarda una copia del manifiesto como `<nombre>.manifest.json` para trazabilidad. Se puede desactivar con `--no-save-manifest`.

---

## Códigos de salida

| Código | Significado |
|---|---|
| `0` | Todo correcto. |
| `1` | Error de uso o cancelado por el usuario. |
| `2` | Manifiesto inválido, no encontrado o error al descargarlo. |
| `3` | Piezas faltantes, corruptas o con tamaño incorrecto. |
| `4` | El `packageDigest` global no coincide. |
| `5` | Error de entrada/salida o espacio insuficiente. |
| `130` | Interrumpido con Ctrl+C o detenido; se puede reanudar. |

---

## Preguntas frecuentes

### Windows dice "editor desconocido" o SmartScreen lo bloquea

El `.exe` no está firmado. Es normal. Puedes compilarlo tú mismo o revisar el workflow de GitHub Actions. Si SmartScreen lo bloquea, usa "Más información" → "Ejecutar de todas formas" solo si confías en el binario.

### Error de certificado o de red al descargar el manifiesto

La descarga usa el almacén de certificados de Windows (Schannel). Si estás detrás de un proxy corporativo que intercepta HTTPS con su propio certificado, puede rechazarlo. Prueba desde otra red, instala el certificado del proxy en Windows o descarga el `.json` con el navegador y pásalo como archivo local (el programa no necesita Internet si ya tienes el manifiesto y las piezas).

### Dice que faltan piezas

- Comprueba que `--parts-dir` apunta a la carpeta correcta.
- Los nombres no tienen que coincidir con los de la URL: el programa busca por nombre, tamaño y hash.
- Usa `-v` para ver qué archivo se asigna a cada pieza.
- Si de verdad falta alguna, usa `--download-missing`.

### El `packageDigest` no coincide, pero todas las piezas están bien

Cada pieza se verifica con su SHA-256, así que el contenido es correcto. Prueba:

```bat
pkg-merge ... --final-digest rehash
pkg-merge ... --allow-digest-mismatch
```

Si el manifiesto usa una convención distinta para el digest global, el archivo sigue siendo válido. `--allow-digest-mismatch` solo debe usarse entendiendo esto.

### Se cortó el proceso o apagué el PC

Vuelve a ejecutar el mismo comando. El journal y el `.pkg.part` permiten continuar. Si quieres verificar el hash global, añade `--final-digest rehash`.

### ¿Puedo borrar las piezas al terminar?

Sí, con `--in-place --delete-parts`. Se borran **después** de verificar y sincronizar cada una. Sin `--delete-parts`, las piezas no se tocan.

### ¿Funciona con parches y DLC?

Sí. Un parche de una sola pieza también funciona: se copia, se verifica y se renombra el resultado.

### ¿Funciona con PS4 y PS5?

Sí. El formato del manifiesto es el mismo. Se ha validado con un manifiesto real de PS5 (`PPSA08668`, 36 piezas).

### ¿Por qué no hay un modo "sin manifiesto"?

El modo antiguo adivinaba las piezas por nombres `_0.pkg`, `_1.pkg`, etc. Falla con `_sc.pkg`, con nombres de distintas regiones y con offsets no uniformes. El manifiesto es la fuente de verdad y permite verificar cada pieza.

### Va lento

- Usa `--buffer 128` en NVMe o SMB rápidos.
- En un HDD, el límite es el disco; no uses `--jobs` alto en `--check`.
- Evita el antivirus escaneando el `.part` de 100+ GB si puedes excluir la carpeta.
- El modo normal leyendo de `Z:\` y escribiendo en `C:\` suele ser mucho más rápido que un in-place en el mismo disco lento.

### ¿Modifica mis piezas?

No, salvo que uses `--delete-parts`. En modo normal solo se leen. En `--in-place` se renombra la pieza 0 y se escribe en el `.part`; las demás se borran solo con `--delete-parts`.

---

## Desarrollo y compilación

El proyecto está en **Rust** y se compila con Cargo. No necesitas instalar Rust en el host: hay un contenedor Docker preparado.

### Con Docker (recomendado)

```bash
make image      # construye la imagen pkg-merge-dev
make test       # cargo test dentro del contenedor
make fmt        # rustfmt
make clippy     # clippy con -D warnings
make check      # cargo check
make release-check
```

O directamente:

```bash
docker run --rm \
  -v "$PWD":/work \
  -v pkg-merge-cargo:/usr/local/cargo/registry \
  -v pkg-merge-target:/target \
  -e CARGO_TARGET_DIR=/target \
  -w /work pkg-merge-dev \
  cargo test --all
```

Los artefactos de compilación van al volumen `pkg-merge-target`, no al repositorio. Para limpiar:

```bash
make clean-volumes
docker rmi pkg-merge-dev
```

No se instala nada en macOS/Windows fuera de Docker.

### Estructura

```text
src/
  main.rs        CLI + orquestación + resolución de rutas
  cli.rs         definición de argumentos (clap)
  manifest.rs    descarga, parseo y validación del .json
  locate.rs      emparejamiento URL -> archivo local
  verify.rs      SHA-256 por rangos
  merge.rs       escritura por offsets, in-place, journal, progreso
  journal.rs     estado reanudable
  progress.rs    barras y ETA
  http.rs        descargas con ureq + native-tls
  log.rs         logs y archivo --log
  error.rs       errores y códigos de salida
  util.rs        rutas, tamaños, nombres, espacio libre
tests/
  integration.rs 22 pruebas con fixtures sintéticos
.devcontainer/   imagen de desarrollo
.github/workflows/build.yml  CI + Releases
Makefile
```

### Pruebas

Las pruebas cubren:

- Merge normal y digest global.
- Pieza corrupta y pieza faltante.
- `--check` en serie y en paralelo.
- In-place con borrado progresivo y protección de la pieza 0.
- Reanudación tras `--stop-after` y rehash del digest.
- Mapeo por tamaño y por hash cuando las piezas están renombradas.
- Manifiesto con huecos.
- `packageDigest` incorrecto y `--allow-digest-mismatch`.
- `--parts-dir`, autodetección de carpeta y nombre de salida.
- PKG de una sola pieza.
- Descarga de una pieza faltante vía HTTP local.

Ejecuta `make test` para todas.

### CI y Releases

- **Linux**: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --locked`.
- **Windows**: `cargo test --locked`, `cargo build --release --locked`, y `pkg-merge.exe` como artifact.
- Al crear un tag `v*`, el `.exe` se adjunta automáticamente a la Release.

Para publicar una versión:

```bash
git tag v0.2.0
git push origin v0.2.0
```

---

## English quick start

`pkg-merge` joins split PS4/PS5 PKG files using Sony's official `.json` manifest.

1. Download `pkg-merge.exe` from [Releases](https://github.com/rafmarsan/pkg-merge/releases).
2. Take the `.crc` manifest URL and replace `.crc` with `.json`.
3. Save that `.json` in the folder with all the `.pkg` pieces.
4. Drag the folder onto `pkg-merge.exe`.

It verifies the SHA-256 of every piece and the global `packageDigest`, and only creates the final `<name>.pkg` if everything matches.

```bat
pkg-merge <folder|manifest.json|crc-url> [--output <dir|file.pkg>]
          [--parts-dir <dir>] [--check] [--in-place --delete-parts --yes]
          [--download-missing] [--verify post|pre|none]
          [--final-digest auto|rehash|skip] [--buffer 64] [--jobs 4]
          [--force] [--allow-digest-mismatch] [--no-progress] [--log <file>]
```

Exit codes: `0` OK, `1` usage, `2` manifest, `3` pieces/hash, `4` global digest, `5` I/O/space, `130` interrupted.

> Unofficial tool, not affiliated with Sony. Keep backups. `--delete-parts` is destructive by design.
