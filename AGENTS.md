# AGENTS.md — DAW Heretic MCP

> Memoria viva del proyecto. Una sesión futura lee esto primero.
> Reescrito en la Fase 0 (2026-09-26): la versión anterior mezclaba hallazgos
> verificados con suposiciones, y daba por cierto algo que se demostró falso.

---

## 0. Qué es esto

Servidor MCP en Rust que controla **Reaper** desde un agente de IA. La gracia no
es que se pueda automatizar un DAW —eso lo hace cualquiera— sino que el agente
**no pueda mentirle sobre el estado del DAW**: cada respuesta viene de la API
real de Reaper, y todo lo que se afirma aquí está medido contra el DAW, no
inferido.

- **Licencia:** GPL-3.0-or-later
- **Stack:** Cargo workspace, Rust 2024, `flojo-mcp` (path dep), `tokio`, `rusqlite`, `clap`
- **Puente:** un ReaScript en Lua (`reaper_mcp_server.lua`, 226 KB) talks con el
  MCP por file-RPC.
- **Filosofía:** el agente **no** es de confianza → validar, medir, auditar. Pero
  sin teatro: HMAC entre procesos del mismo usuario no aporta nada, así que **no
  hay daemon**. El agente ES el proceso.

### Estado en cifras (verificadas 2026-09-26)

| | |
|---|---|
| Tools MCP | **10** |
| Acciones del puente | **162**, en 24 grupos |
| Tests | **81**, 0 fallos |
| API de Reaper catalogada | **730** funciones, 268 claves con nombre |
| Funciones `reaper.*` que llama el puente | **165**, las 165 existen |
| Acciones sin parámetros obligatorios | 66 de 162 |

---

## 1. Por qué Reaper y no FL Studio

- 900+ funciones de API y ReaScript con **file I/O** (el sandbox Python de los
  controllers de FL no lo tiene: ni `open`, ni `os.open`, ni `os.makedirs`).
- El catálogo se **genera del HTML oficial** de REAPER v7.80, no se escribe a
  mano. `docs/REAPER_API.md` (1.526 líneas) sale de ahí.
- FL Studio 2025 **sí** está instalado en `D:\Program Files\Image-Line\...`, pero
  su carpeta `System\Plugin\VSTi\x64` —que Reaper ya tenía en su lista de
  rutas— está **vacía**. No hay VSTi de FL que importar.

---

## 2. Topología

```
OpenCode (MCP stdio NDJSON)
      |  10 tools, sin daemon
      v
fl-heretic / daw-heretic-mcp  (Rust)
      |  file-RPC: command.json / response.json en %TEMP%\reaper_mcp
      v
reaper_mcp_server.lua  (ReaScript, hilo principal de Reaper)
      |  API real de Reaper
      v
REAPER 7.80
```

Dos piezas de Lua, versionadas en `crates/heretic-mcp/lua/` e instaladas por
`daw_debug op=install`:

- **`daw_guard.lua`**: envuelve el payload en `pcall`. Sin él, un error de Lua
  abre un diálogo modal y **Reaper deja de leer `command.json`**: el agente solo
  ve "no respondió en 10 s".
- **`daw_supervisor.lua`**: arranca el bridge al abrir Reaper, y escribe un log
  en `%TEMP%\reaper_mcp\supervisor.log`.

---

## 3. Las 10 tools

| tool | para qué |
|---|---|
| `daw_health` | ¿responde el DAW? ¿con qué frecuencia y a qué frecuencia de muestreo? |
| `daw_catalog` | Las 162 acciones con sus parámetros y obligatorios, en 24 grupos |
| `daw_do` | Cualquier acción por nombre. La vía larga |
| `daw_track` | Pistas: crear, renombrar, volumen, pan, mute, solo, color |
| `daw_fx` | Plugins: add, chain, params con nombre, presets, enable/disable |
| `daw_midi` | Notas MIDI: items, insertar en lote, leer, quantizar, humanizar |
| `daw_project` | Proyecto: info, new, save, open, **render a un path** |
| `daw_setup` | Instalación, re-scan de plugins, cadenas de FX, master, buses |
| `daw_master` | Medir y normalizar: LUFS-I, RMS-I, pico, true pico, crest factor |
| `daw_debug` | Diagnóstico: diálogos, `eval` de Lua, logs, relanzar el bridge |

8,3 KB de schemas frente a los 22.400 del MCP de referencia que se estudió. La
razón del corte no es el gusto: son tokens que el agente paga en cada llamada.

---

## 4. Decisiones, con su razón

| Decisión | Por qué |
|---|---|
| **Reaper, no FL Studio** | File I/O en ReaScript y API real. FL tiene un sandbox que no permite ni abrir un fichero |
| **Sin daemon** | El agente es un proceso del mismo usuario. Un HMAC entre los dos sería seguridad de teatro. Serialización → `Mutex`; liveness → `daw_health` |
| **file-RPC, no Named Pipes** | Se puede depurar leyendo dos ficheros JSON. Un Named Pipe que no responde no te dice nada |
| **10 tools, no 162** | Los 8,3 KB de schemas son tokens en cada llamada. El catálogo se consulta bajo demanda |
| **Catálogo generado del puente** | Los parámetros salen de leer el Lua, no de documentarlo. Adivinar un parámetro es el bug que más caro salió en la etapa de FL |
| **`daw_debug op=eval`** | 165 de las 730 funciones de la API no están envueltas. `eval` es la puerta, y va con `pcall` para que un error vuelva como texto |
| **Fase 0 antes que innovar** | El repo afirmaba cosas falsas. Invertir en una capa de notación sobre un catálogo que miente es construir sobre arena |
| **Expandir a la misma lista de notas** | La futura capa de notación compilationará a `midi_insert_notes_batch`, que ya funciona. Si se rompe, se cae sin arrastrar nada |

---

## 5. Trampas medidas

Cada una con **cómo se midió**. Una trampa sin método no es una trampa, es un
rumor.

### Del protocolo

- **Los parámetros son `snake_case`.** El puente lee `p.track_index`. Medido:
  `track_index` funciona y `trackIndex` da `Missing parameter: track_index`. No
  hay conversión en ninguna capa. Seis claves de las tools estaban en camelCase
  y **`daw_track`, `daw_fx` y `daw_midi` fallaban en cuanto necesitaban un
  índice** — y no lo veíamos porque `track_create` no necesita ninguno.
- **Reaper no avisa de un parámetro desconocido.** Usa el valor por defecto y
  devuelve éxito. Es el peor modo de fallo posible: un `trackIndex` mal escrito
  parece que funcionó. Medido con `start_position` en `track_get_info`.
- **Las estructuras anidadas van como cadena JSON.** `midi_insert_notes_batch`
  hace `json_decode(p.notes)`: `notes` tiene que ser una **cadena** con el JSON
  dentro, no un array. Un array da `Invalid notes JSON`, que no explica el
  problema.
- **`daw_project op=render` estaba muerto.** No mandaba `render_dir`,
  `render_pattern` ni `format_code`, que son obligatorios. Arreglado: ahora
  parte el `path` en directorio + nombre y usa `evaw` (WAV).
- **`op=new` no puede poner nombre ni plantilla.** `PROJECT_NAME` es *read-only*
  en la API de Reaper ("is_set will be ignored", en el catálogo oficial). La
  descripción de la tool prometía las dos cosas; se quitó.

### De Lua

- **`p.end` no es Lua válido.** `end` es palabra reservada; ese campo solo se lee
  como `p["end"]`. Un solo carácter así y el bridge **no carga**: Reaper abre
  "ReaScript Error" en cada arranque y el diálogo **sobrevive a `WM_CLOSE`**.
  Hay que matar el proceso. learned a la mala.
- **No existe `TrackFX_GetParameterStepCount`.** No está en el catálogo oficial.
  La real es `TrackFX_GetParameterStepSizes` y devuelve
  `(retval, step, smallstep, largestep, istoggle)`. El comentario del código
  decía "REAPER's own documented convention", que es exactamente cómo se cuela
  una API inventada. Ahora `tools/gen_actions.py` **no genera el catálogo** si
  el puente llama a alguna función que no exista.

### De Reaper

- **50124 no reescanea plugins.** Es *refresh all plug-ins*: refresca la lista
  cargada, no busca ficheros en disco. Medido: con un plugin instalado y
  50124 ejecutado, `daw_setup op=rescan` devolvió `encontrado: false`. Hace
  falta **reiniciar Reaper**.
- **`Main_OnCommand` sobre un ReaScript en marcha lo destruye.** La sustituta no
  arranca. Por eso el supervisor solo relanza al boot, donde se sabe que está
  parado, y para relanzar en caliente está `daw_debug op=restart_bridge`, que
  **pregunta al DAW antes** y se niega si contesta.
- **`GetResourcePath()` no lleva separador final.** `"Scripts\\x"` →
  `REAPERScripts\\x`, y `AddRemoveReaScript` devuelve 0 sin explicar. Usar
  `.. "/Scripts"`.
- **`CalculateNormalization` devuelve un factor lineal, no dB.**
  `medido_dB = objetivo_dB - 20*log10(retorno)`. Leerlo como dB publicaba −10
  donde era −20. Y `math.log10` **no existe** en el Lua de Reaper.
- **`InsertMedia` devuelve `integer` (éxito), no el item.** Buscarlo por índice.
- **La API tiene dos medidores y el puente usaba el peor.**
  `Track_GetPeakHoldDB` es un pico **retenido** (máximo histórico): no dice qué
  pasa ahora. `Track_GetPeakInfo` da el instantáneo y además **sonoridad en los
  canales 1024 y 1025**. El puente nunca llama a `Track_GetPeakInfo`.
  Ojo: la doc de `Track_GetPeakHoldDB` se contradice ("in dB*0.01" pero a la vez
  "-0.01 = -1dB"), así que sus unidades **no están verificadas**.
- **El render sale a 24 bits.** Está en el plan arreglarlo y verificar las
  unidades del metro con un test.
- **Un diálogo abierto no es un DAW parado.** El aviso de evaluación ignora
  `WM_CLOSE`, cambia el texto de su botón entre arranques (una vez fue
  "Buy Me [4]") y, aun así, **el DAW responde con las 162 acciones**. El
  criterio de bloqueo es "el DAW no contesta", no "hay una ventana".
- **`BrowseForOpenFiles` y `GetSetProjectInfo_String("RENDER_STATS")` sin la
  pref activada** abren modales que congelan el DAW.
- **17 de 162 acciones no cumplen `prefijo == módulo Lua`**
  (`chops_create_virtual_slice` está en el módulo `item`, con grupo `chops`).
  Cumplen la regla del **grupo**, que es la que usa `daw_catalog`. La afirmación
  anterior de que "los 162 cumplen sin excepción" era falsa.

### De medir

- **El render de Reaper es 24 bits** y el módulo `wave` de Python solo acepta
  1, 2 y 4 bytes por muestra. Leerlo como `int32 >> 8` mete **48,16 dB de
  error** (`20*log10(256)`), que es una cantidad que parece un nivel de signal y
  no un error de lectura. Y promediar L y R en vez de tomar el máximo cuesta
  3 dB más. `tools/wav_measure.py` tiene el lector correcto **y sus fixtures**,
  porque tres lectores erróneos concordaban entre ellos: lo que faltaba era una
  respuesta conocida contra la que comparar.

---

## 6. Lo que hay que instalar antes de tocar nada

```powershell
# con opencode CERRADO: compila el release y lo deja donde lo busca opencode
.\tools\install_release.ps1
# con la sesion abierta, para comprobar que compila sin tocar el binario vivo
.\tools\install_release.ps1 -Verify
# reiniciar Reaper de forma ordenada (el bridge necesita recargar el .lua)
.\tools\restart_reaper.ps1
```

**Sin `tools/mcp_call.py` no se puede comprobar nada de este repo**: los tests
unitarios pasan igual con las tools rotas. `mcp_call.py` habla MCP de verdad por
stdio y mantiene el pipe vivo; `llamar.py` es su envoltorio para PowerShell
(manda el JSON en un fichero con `@fichero`, porque PowerShell se come las
comillas de la línea de comandos).

El aviso de "abierto" de `install_release.ps1` usa
`[System.IO.File]::Open(..., FileShare::None)`. Medido: **`Copy-Item` sí
funciona con un fichero abierto**, así que copiar no dice nada.

---

## 7. Deuda conocida

1. **El puente no está en el repo.** Son 226 KB de Lua que viven en
   `%APPDATA%\REAPER\Scripts\`, y `gen_actions.py` los lee de ahí. El repo no
   puede reproducir su propio catálogo. **Decisión pendiente.**
2. **No hay validación de sintaxis de Lua.** No hay `luac` en la máquina, y un
   error de sintaxis solo se ve cuando Reaper abre el diálogo, que ya no se
   cierra. Idea: `daw_debug op=checklua` con `loadfile`, que da el error exacto.
3. **Las unidades del metro no están verificadas** (ver §5).
4. **El bridge ignora los parámetros desconocidos.** Debería rechazararlos, o
   al menos `daw_do` debería, ya que el catálogo sabe qué se espera.
5. **`fx_scan_params` arreglado pero sin probar contra un plugin de verdad.**
6. **El supervisor no relanza en caliente** a propósito; falta una forma
   cómoda de hacerlo que no destruya la instancia buena.

---

## 8. Plan de fases

### Fase 0 — Deuda ✅ *hecha el 2026-09-26*
- Reescrito `AGENTS.md` (esta versión): fuera lo que se demostró falso.
- `gen_actions.py` es ahora una **puerta**: no genera si el puente llama a una
  API inexistente. Caza `TrackFX_GetParameterStepCount`.
- El generador **sigue a los helpers**, así que `required` es real: 49 acciones
  lo ganaron, ninguna lo perdió. Antes 9 de 17 `midi_*` decían no pedir nada
  cuando `get_midi_take(p)` les exige `item_index`.
- Arregladas las 6 claves camelCase de las tools.
- Arreglado `daw_project op=render` (estaba muerto) y `op=new` (prometía lo que
  no existe).
- 5 tests nuevos que atan las tools al catálogo del bridge.
- `tools/wav_measure.py` con lector correcto y fixtures.
- **81 tests, 0 fallos.** Verificado contra Reaper: `daw_track op=rename`,
  `daw_fx op=get_chain` y `daw_project op=render` funcionan.

### Fase 1 — Verdad proactiva
- `daw_health` como snapshot: srate de proyecto y de render, `Audio_IsRunning`,
  transporte, bpm/compás, ítems, notas.
- `daw_master op=meter`: pico **instantáneo** (`Track_GetPeakInfo`, no el
  retenido) + sonoridad por los canales 1024/1025, master y pistas.
- El render devuelve un **resultado**: pico, LUFS, duración, y "silencio
  digital" explícito. Hoy devuelve `rendered: true` y nada más.
- `daw_master op=probe_instrument`: "¿suena?" en una llamada.

### Fase 2 — Notación (la innovación)
MIDI-en-JSON es la peor interfaz posible para un LLM: unidad equivocada (beats
absolutos en vez de compases), pitch numerado, y **no expresa estructura** —en
JSON no existe "igual que el compás 1"—. La capa de notación sería
bar-relativo y con nombres de nota, y **expandiría a la lista de notas que ya
funciona**. En Rust, que se testea; no en Lua.

### Fase 3 — Generadores y operaciones
Una pieza de piano es ~20 % melodía (exacta) y ~80 % figuración (repetitiva por
definición). Los generadores atacan el 80 %: `alberti16`, `arpegio8`, `escala`.
Y operaciones en vez de reescrituras, para que cambiar 8 notas no regenere 200.

### Fase 4 — Reorganización
El corte en 10 tools frente a 162 acciones está por justificar. Y el catálogo,
que debería ser la spec del agente, hoy miente en `required` y en tipos.

---

## 9. Reglas para futuras sesiones

- **Probar contra el DAW real.** Un test que pase intermitentemente es peor que
  uno que no pase; uno que no prueba nada es peor que no tener test.
- **La API no se escribe de memoria.** Se genera del HTML oficial. Si no está
  en el fichero generado, no se inventa. Catálogos generados **no se editan a
  mano**.
- **Toda cifra de este documento con cómo se midió**, o no vale.
- **Una decisión por pregunta.**
- Español en conversación, inglés en código y docs. **Brutalmente honesto** en
  auditorías: pintar nada bonito es lo que rompio este repo una vez.
- **Probar un script antes de darlo por bueno.** Los tres ultimos reviewers un `$raiz` con un `Split-Path` de más, un `assert` que contaba
  6 donde había 8, y un fichero de tests **pisado** por otro nuevo.
- Al tocar el puente: copia de seguridad, `loadfile` para la sintaxis, y
  `gen_actions.py` antes de compilar.

---

## 10. Referencias

- **Catálogo generado:** `crates/heretic-daw/data/reaper-api.json` (730
  funciones) y `docs/REAPER_API.md` (1.526 líneas, generado, no editar).
- **Generadores:** `tools/gen_api_docs.py` (HTML→JSON), `tools/gen_api_md.py`
  (JSON→MD), `tools/gen_actions.py` (bridge→Rust, con puerta).
- **Arnes:** `tools/mcp_call.py`, `tools/llamar.py`, `tools/wav_measure.py`.
- **Instalación:** `tools/install_release.ps1`, `tools/restart_reaper.ps1`.
- **API de Reaper:** https://www.reaper.fm/sdk/reaper/reaper_wwwroot.html
