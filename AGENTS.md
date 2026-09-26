# DAW Heretic MCP

> **Memoria viva del proyecto.** Cualquier sesión futura debe leer esto primero.

---

## 0. Qué es esto

Un servidor MCP en Rust que deja que un agente IA controle una DAW entera:
crear proyectos, pistas, meter plugins, escribir MIDI, mezclar, renderizar.

**Estado: el transporte funciona contra Reaper real.** 5/5 tests live
verdes: se escribe el tempo, se relee, se crean y borran pistas. El bridge
Lua está instalado y se autostartea.

Lo que no hay todavía: el FL Studio VSTi cargado en Reaper, y el E2E completo
a través del MCP.

**Historial:** este repo empezó siendo `FLHereticMCP`. El trabajo de FL Studio
está completo en `0930996` y el pivot a Reaper en `493f850`. Nada se perdió.

---

## 1. Por qué no FL Studio

**No es que la implementación fuera mala: es que la API no existe.** Medido
sobre FL Studio 2025 real (MIDI scripting v38),across sus 11 módulos:

| Necesidad | ¿FL la tiene? |
|---|---|
| Guardar proyecto con ruta | ✅ `midi.FPT_Save` (verificado, 90 ms) |
| Abrir proyecto | ❌ ni `FPT_Open` ni nada en `general` |
| Crear proyecto nuevo | ❌ ni `FPT_New` ni `general.newProject` |
| Cerrar proyecto | ❌ ni `FPT_Close` |
| Cambiar de proyecto | ⚠️ copiar el `.flp` + `CreateProcess` |

De las **79** constantes `midi.FPT_*` solo hay `FPT_Save` y `FPT_SaveNew`.
`FPT_SaveNew` abre un diálogo con campos `TQuickEdit` (controles Delphi
internos) que aceptan texto pero **cierran sin guardar** al confirmar.

El menú File es **owner-draw**: `GetMenu` da un handle pero
`GetMenuItemCount` da 0, así que sus items no se leen por Win32.

Y el Python del controller script va en un sandbox sin file I/O, sin sockets,
sin `subprocess` (todo medido). Por eso el transporte acababa siendo
ficheros + MIDI para despertarlo, con techo de 1.5KB por mensaje.

**No es un defecto de FL.** La API de Cubase tiene las mismas limitaciones:
sandboxed, sin file I/O, sin red, sin crear pistas, sin insertar plugins. Los
DAWs modernos exponen scripting para *control de hardware*, no para
*producción programática*.

Coste real que [↑ pagamos por eso](/AUDIT.md):
- El bridge solo corría cuando FL recibía MIDI, porque `OnIdle` no dispara.
- Un `TWelcomeWizard` ("Welcome to FL Studio") o un "Save changes?" congelan
  el bridge entero y hacen que **toda** escritura falle con
  `Operation unsafe at current time`.
- Eso hizo que "abrir un proyecto" costara tantas pruebas. Los dos modales
  se confundían: el wizard sale al arrancar sin proyecto, el "Save changes?"
  al abrir un `.flp` con cambios sin guardar.

## 2. Por qué Reaper

| | REAPER | FL Studio |
|---|---|---|
| Funciones de API | **900+** | crippled |
| Control externo | file-RPC o `python-reapy` (TCP) | MIDI 1.5KB, sandbox |
| File I/O desde el DAW | ✅ libre | ❌ bloqueado |
| Crear pistas / insertar plugins | ✅ | ❌ |
| MCP existentes | varios | ninguno |
| Licencia | 60 días gratis → **$60** | $99 |

Reaper 7.80 instalado en `C:\Program Files\REAPER (x64)\`.

## 3. Los plugins de FL Studio

Los **nativos** (FLEX, Sytrus, Harmor) no son VST3: son FL-only. Pero existe
`FL Studio VSTi (Multi).dll` en:
```
D:\Program Files\Image-Line\FL Studio 2025\System\Plugin\VSTi\x64\
```
que carga **FL Studio entero como VST2** dentro de Reaper → FLEX y todo el
bundle siguen disponibles.

Trampas (de foros, confirmadas): pista de **instrumento** (no de audio), FL en
modo "song", output a "FL 1".

---

## 4. Arquitectura: sin daemon

```
opencode ──stdio MCP──> daw-heretic-mcp (FlojoMCP, Rust)
                              │ file-RPC: command.json <-> response.json
                              ▼
                         [Reaper]  ←── ReaScript Lua
```

**El daemon se eliminó a propósito.** El diseño anterior asumía "el agente MCP
no es de confianza" y ponía HMAC entre el agente y el daemon. Eso es
**seguridad de teatro**: el agente *es* el proceso de opencode, ya puede
escribir en `%TEMP%` por su cuenta, y puede leer el fichero del token. No hay
frontera de privilegio que cruzar.

Lo que el daemon sí resolvía, y cómo se resuelve ahora:

| Lo que resolvía | Ahora |
|---|---|
| HMAC / ACL / rate limit | ❌ fuera, no aplica |
| **Serializar llamadas paralelas** | `Mutex` dentro de `heretic-daw` |
| **Saber si el bridge vive** | `daw_health` lo comprueba de verdad |
| Reconexión tras reinicio | el MCP server es longevo, no hace falta |

Menos procesos (de 4 a 2) y fuera los 3 bugs de Named Pipe que costaron una
tarde entera (`ERROR_PIPE_BUSY` con una sola instancia de pipe, brazo duplicado
en el dispatch, y rutas de lifecycle que no rutaban a su handler).

## 5. Transporte: file-RPC

El bridge Lua (`reference/xDarkzx/reaper_mcp_server.lua`, 6049 líneas, 165
funciones de la API) usa IPC por ficheros:

```text
MCP (Rust)  --escribe-->  command.json    [dentro de Reaper]
MCP (Rust)  <--lee--     response.json   [dentro de Reaper]
```

Es el mismo patrón que el file-RPC de FL, con **una diferencia que lo cambia
todo**: el bridge de Reaper corre su propio bucle `defer()` a ~30 Hz. **No hay
wake.** Eso elimina de raíz el puerto MIDI persistente, el techo de 1.5KB y
toda la clase de fallos "el DAW no responde".

Reglas que no se pueden romper:

- **Atomicidad**: escribir en `.tmp` y renombrar, con 20 reintentos. En
  Windows el rename da `PermissionError` si el bridge tiene el destino abierto.
- **Ids monotónicos**, no timestamps: el bridge compara contra el último id
  visto, y un id que no avance se pierde en silencio.
- **camelCase, no snake_case**: `trackIndex`, `fxIndex`, `paramIndex`.
  Reaper **no da error** con un param desconocido: usa el valor por defecto y
  parece que funcionó. Hay un test que lo fija.
- **La carpeta de IPC se crea desde el cliente.** Si no, el error dice "io" en
  vez de "el bridge no está", que es lo que el LLM necesita para saber qué hacer.

## 6. Las 7 tools

Se midió el MCP de referencia con su AST:

| | |
|---|---|
| Tools | **181** |
| descripciones | 74.157 chars (~18.500 tokens) |
| firmas + params | 15.334 chars (~3.800 tokens) |
| **coste en schemas** | **~22.400 tokens, en cada request** |

Y las descripciones están infladas: `setup_fx_chain` tiene 3.740 chars de
descripción para una función de **un** parámetro (935 tokens). Es
documentación de blog metida en un schema.

Ese MCP tiene perfiles (`full` 180 … `minimal` 43) porque su autor sabe que
el problema existe, pero el default sigue siendo 180 y cambiar de perfil
exige reiniciar el servidor. Es parchear el síntoma.

Aquí al revés: **7 por defecto, y se expande si se pide.**

| Tool | Qué cubre |
|---|---|
| `daw_health()` | ¿Reaper vivo? ¿bridge respondiendo? |
| `daw_catalog(domain?)` | Descubrimiento: qué acciones hay y qué params |
| `daw_do(action, params)` | Escape hatch: **las 181 acciones** |
| `daw_project(op, …)` | new / save / saveAs / open / render / info / paths |
| `daw_track(op, …)` | create / list / info / rename / volume / pan / mute / solo / arm / color |
| `daw_fx(op, …)` | search / add / remove / list / paramInfo / getParam / setParam / preset / toggle |
| `daw_midi(op, …)` | createItem / addNote / addNotes / getNotes / clear / read |

**Las tipadas existen por un motivo concreto:** el bug más caro de la etapa de
FL no fue del DAW, fue nuestro diseño. Con 17 wrappers finos cada uno
reconstruía los params a su manera, y el LLM adivinó mal un nombre (`ms`
cuando el daemon leía `position`). Con `daw_fx("add", track=0, fx="ReaEQ")`
el schema dice el nombre exacto. `daw_do` deja la larga cola abierta, pero el
error ya no es silencioso.

## 5b. Instalar el release (y por que casi no se puede desde dentro)

```powershell
# con opencode CERRADO
.\tools\install_release.ps1
# con la sesion abierta, para comprobar que compila sin tocar el binario vivo
.\tools\install_release.ps1 -Verify
```

opencode ejecuta `target\release\daw-heretic-mcp.exe` y lo mantiene abierto, y
Windows no deja reemplazar un fichero abierto. Por eso `cargo build --release`
falla con `failed to remove ... daw-heretic-mcp.exe` y parece un problema de
compilacion cuando lo que esta bloqueado es el propio servidor.

Durante esa temporada se compila a `target/verify` y se verifica por
`tools/mcp_call.py`, que habla MCP de verdad por stdio. **Sin `mcp_call.py` no
se puede probar nada de este repo**: un test unitario pasa y el tool puede
estar roto.

El aviso de "abierto" del script usa `[System.IO.File]::Open(..., FileShare::None)`,
medido: **`Copy-Item` si funciona con un fichero abierto**, asi que copiar no
dice nada. Solo el open exclusivo falla.

## 6b. Trampas de Reaper (medidas, no supuestas)

Cada una costó tiempo o dejó el DAW inservible. Todas están en el código con su
porqué, pero esta lista es el aviso rápido.

### Congelan el DAW entero

| trampa | qué pasa |
|---|---|
| un error de ReaScript abre un **diálogo modal** | Reaper deja de leer `command.json`; el agente solo ve "no respondió en 10 s". Por eso existen `daw_debug` y el guardián con `pcall` |
| `GetSetProjectInfo_String("RENDER_STATS")` sin la preferencia activada | abre un modal pidiendo permiso. La pref va en Opciones > Preferencias > Rendering > Stats/Charts |
| `BrowseForOpenFiles` | selector de ficheros modal. El MCP ya tiene la ruta: no se usa nunca |
| `Main_OnCommand` sobre un ReaScript **en marcha** | no es no-op: se lleva la instancia que funciona y la sustituye. Y la sustituta no arranca porque el bridge no mira el lock. **Nunca en caliente** |

### Fallan en silencio (lo peor)

| trampa | real |
|---|---|
| `CalculateNormalization` devuelve un **factor lineal**, no dB | `medido_dB = objetivo_dB - 20*log10(retorno)`. Leerlo como dB desplaza todo y parece una medición |
| `InsertMedia` devuelve `integer` (éxito), no el item | buscar el item por índice en la pista |
| `math.log10` **no existe** en el Lua de Reaper | `math.log` sí. De ahí `math.log(x)/math.log(10)` |
| `GetSetProjectInfo` con clave desconocida devuelve nil sin avisar | la clave no se asigna y desaparece del JSON |
| params del bridge en **snake_case** | `track_index`, `fx_name`. Nada de `trackIndex` |
| nota MIDI usa `end`, no `length`; velocity 0-127 | y las posiciones van en **beats** |

### Rutas y scripts

- `reaper.GetResourcePath()` **no lleva separador final**. `"Scripts\\x"` sale
  `REAPERScripts\\x`, `AddRemoveReaScript` devuelve 0 sin decir por qué, y el
  supervisor se iba por su `return`. El bridge lo hace bien con `.. "/Scripts"`.
- El supervisor escribe en `%TEMP%\\reaper_mcp\\supervisor.log`. Antes su rastro
  estaba en la consola de ReaScript, que es un RichEdit y **no se puede leer
  desde fuera**: nadie podría enterarse de un fallo.
- `__startup.lua` corre antes de que la lista de acciones esté lista: sin
  `defer` doble, nada arranca.

### Lo que el catálogo generado evita

- 730 funciones reales; el bridge envuelve 161. Catálogo en
  `crates/heretic-daw/data/reaper-api.json` (versionado), .md generado en
  `docs/REAPER_API.md`.
- Ha pillado **funciones inventadas** 4 veces: `GetTrackChannelInfo`,
  `GetActiveTrack`, `GetTrackNumber`, `TimeMap_TimeToBeats` (real:
  `TimeMap2_timeToBeats`). Cada una, si llega al DAW, congela Reaper.
- Regenerar: `python tools/gen_api_docs.py` (HTML -> JSON) y
  `python tools/gen_api_md.py` (JSON -> MD). **El .md no se edita a mano.**

### Un dialogo abierto NO es un DAW parado

Medido con el aviso de evaluacion de Reaper en pantalla: `daw_health` decia
`bridge: ok` con las 162 acciones mientras `daw_debug` decia CONGELADO. Los dos
no pueden ser verdad.

El aviso sale **despues** de que el bridge arranque y el bucle de ReaScript
sigue corriendo: no bloquea nada. Un agente no puede distinguir "este dialogo
me tapa" de "este dialogo esta aqui" mirando la ventana; solo preguntándoselo
al DAW. Por eso `daw_debug` pregunta antes de clasificar, y el criterio es:

- DAW contesta + dialogo abierto → informativo
- DAW mudo + dialogo abierto → bloqueante, con su texto

### El aviso de evaluacion de Reaper

- Cambia el **texto del boton** entre arranques: "Still Evaluating" un dia,
  "Buy Me [4]" otro. Por eso la huella mira el cuerpo ("REAPER IS NOT FREE"),
  que no varia, y no el boton.
- **Ignora `WM_CLOSE`**: medido, sigue en pantalla despues de enviarlo. No hay
  forma automatica de quitarlo.
- No bloquea el DAW (ver arriba), asi que no hace falta quitarlo. El skip es
  una red, no un paso del arranque.
- Nunca se pulsa un boton suyo: "buscar un boton conocido" en un dialogo de
  licencia acaba metiendo al usuario en la tienda. Se cierra la ventana, y solo
  si el DAW esta mudo.

### El fixture que se autoniega

`wav.rs` genera 4 señales con respuesta conocida. Lo importante no son los
ficheros: es que se **verifican por fuera** (módulo `wave` de Python) y no
con el código que las genera. Es lo único que cazó el bug de los dB, porque un
test que comprobara "devuelve un número" habría pasado igual.

Y una expectativa mía que era **falsa** y corregí: el fichero recortado NO
tiene "mucha menos sonoridad que su pico". Al revés: recortar **aplasta las
crestas y acerca el RMS al pico** (factor de cresta 1.07 dB frente a 3.01 de un
sano). De ahí salió la detección de recorte.

## 7. `reference/`: los MCPs que estudiar

Clonados, en `.gitignore` (material de referencia, no código nuestro):

| | Tools | Commit | Notas |
|---|---|---|---|
| **xDarkzx/Reaper-MCP** | 181 | hace días | **El mejor.** CI en 3 SO × 4 versiones de Python, bridge Lua de 6049 líneas, perfiles. Base de referencia |
| shiehn/total-reaper-mcp | 600+ | 8 semanas | Cobertura casi total, DSL natural, bridge Lua único |
| T-Rzeznik/reaper-mcp | 55 | 2 meses | Bridge TCP propio, `Undo_BeginBlock` por operación, prefijo `reaper_`. Diseño más limpio |

## 7b. El catálogo se genera del bridge, no se escribe

`tools/gen_actions.py` produce `crates/heretic-daw/src/actions.rs` leyendo el
bridge real (`%APPDATA%\REAPER\Scripts\reaper_mcp_server.lua`).

Por cada handler saca dos cosas, y las dos **del código**, no de documentación:

- **params**: los campos que el handler lee de `p` (`p.bpm`, `p["x"]`).
- **required**: los que el propio handler rechaza si faltan, vía
  `return nil, "Missing parameter: X"`.

**162 acciones, 24 grupos.** Verificado en vivo:

```text
daw_catalog(domain=midi) -> 17 acciones
  midi_delete_cc        required=['cc_index']
  midi_delete_note      required=['note_index']
  daw_project(info)     -> bpm 120, sample_rate 44100, 0 pistas
  daw_health            -> bridge=ok
```

Que salga del código importa por una razón concreta: **el bridge no tiene
docstrings por handler**, y adivinar los parámetros es el bug que más caro
salió en FL (mandar `ms` cuando el daemon leía `position`). Reaper no avisa de
un parámetro desconocido: usa el valor por defecto y parece que funcionó.

### Trampas del naming del bridge

| trampa | ejemplo |
|---|---|
| El prefijo va **duplicado** | `function fx.fx_add(p)` → clave pública `fx_add` |
| Separador `_`, no punto | `transport_set_bpm`, no `transport.setTempo` |
| Verbos en pasado | `track_create`, `project_get_info` |

`crates/heretic-mcp/src/tools_tests.rs` ata las acciones que usan las tools al
catálogo generado, para que un nombre inventado falle en `cargo test` y no
tres horas después contra Reaper. Ya atrapó uno: `project_get_paths`, que no
existe (es `project_get_metadata`).

### Parámetros que el bridge exige de otra forma

Descubiertos porque da error, no por leerlo:

- `track_delete_batch` quiere `entries`, y cada entry es un **objeto**:
  `{"track_index": 3}`. Con un índice suelto responde
  `Entry must be an object` y no borra nada.

## 8. Decisiones

| Decisión | Por qué |
|---|---|
| Sin daemon | El agente es el proceso del cliente MCP; el HMAC entre procesos del mismo usuario no protege nada |
| `heretic-core` se conserva | Auth, audit, tipos y protocolo ya están escritos y son reutilizables |
| file-RPC, no `python-reapy` | Las dos dan las mismas 900+ funciones. file-RPC no mete Python dentro de Reaper, que es lo que se rompe primero (DTM, versión, bits) |
| 7 tools, no 181 | 22.400 tokens de schemas, en cada request, para siempre |
| Tipadas + escape hatch | El error de nombre de parámetro es silencioso en Reaper; el schema lo evita en el camino común |
| `reference/` fuera del repo | Es material de estudio, no código nuestro |

## 9. Estado

### Hecho
- [x] Investigación de alternativas: Reaper es la vía viable
- [x] Reaper 7.80 instalado
- [x] 3 MCPs clonados en `reference/` y analizados (coste real medido)
- [x] Andamiaje `heretic-daw` con file-RPC (9/9 tests)
- [x] 7 tools MCP compilando, verificadas contra Reaper real
- [x] Repo renombrado a `daw-heretic-mcp`
- [x] Bridge Lua instalado y autostarteando (`__startup.lua`)
- [x] 162 acciones **generadas** del bridge, con params y obligatorios
- [x] Tests que atan las tools al catálogo generado
- [x] MCP de FL Studio y magda desconfigurados de opencode

### Pendiente, por orden
1. [ ] **FL Studio VSTi en Reaper** para tener FLEX. Es lo que decide si esto
       sirve para algo: sin los sounds, es un DAW vacío
2. [ ] E2E completo por el MCP (ya funciona a mano, falta el test)
3. [ ] Installer automático: bridge + `__startup.lua` + config de opencode
4. [ ] Decidir qué queda de `heretic-core` (el audit log pasa de "seguridad"
       a "observabilidad": qué le cambió el agente a mi DAW)

## 10. Reglas para futuras sesiones

- **Español** en conversación, **inglés** en código y docs.
- **Probar contra el DAW real, no contra mocks.** El 100% de los bugs caros
  pasaron los tests en local y fallaron contra FL.
- **No compilar ni mover cosas sin luz verde** del usuario.
- Un test que pasa unas veces y otras no es peor que uno que no pasa:
  entrena a ignorarlo. Si algo es intermitente, se aísla o se documenta por qué.
- Los catálogos se **generan** del bridge real, nunca se escriben a mano.
- Lo de FL está en `git show 0930996`, no en el repo.

## 11. Referencias

- **ReaScript API**: https://www.reaper.fm/sdk/reascript/reascripthelp.html
- **python-reapy**: https://python-reapy.readthedocs.io/
- **FL Studio como plugin**: https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/flstudio_vst_plugin.htm
- **Por qué Cubase tampoco vale**: https://forums.steinberg.net/t/1026258
- **FlojoMCP** (framework): `D:\Mis Juegos\ClaudeMCPs\FlojoMCP\`
