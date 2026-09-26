# DAW Heretic MCP

> **Memoria viva del proyecto.** Cualquier sesión futura debe leer esto primero.

---

## 0. Estado actual: scaffolding para REAPER

**Punto de partida:** hoy el repo solo tiene el andamiaje. El trabajo de FL
Studio está en el historial (hasta `0930996`) y fue commiteado antes de
borrarlo, así que se puede recuperar con `git show`.

### Por qué se abandonó FL Studio

No es que la implementación fuera mala: **es que la API no existe.** Medido
sobre FL Studio 2025 real (MIDI scripting v38), contra sus 11 módulos:

| Necesidad | ¿FL la tiene? |
|---|---|
| Guardar proyecto con ruta | ✅ `midi.FPT_Save` (verificado, 90 ms) |
| Abrir proyecto | ❌ sin `FPT_Open` ni nada en `general` |
| Crear proyecto nuevo | ❌ sin `FPT_New` ni `general.newProject` |
| Cerrar proyecto | ❌ sin `FPT_Close` |
| Cambiar de proyecto | ⚠️ copiar el `.flp` + `CreateProcess` |

De las **79** constantes `midi.FPT_*`, solo hay `FPT_Save` y `FPT_SaveNew`.
`FPT_SaveNew` abre un diálogo cuyos campos son `TQuickEdit` (controles Delphi
internos) que aceptan texto pero **cierran sin guardar** al confirmar.

El menú File es **owner-draw**: `GetMenu` devuelve un handle pero
`GetMenuItemCount` da 0, así que sus items no se pueden leer por Win32.

Y el Python del controller script corre en un sandbox sin `file I/O`, sin
sockets, sin `subprocess` (todo medido). Por eso el transporte acabó siendo
ficheros + MIDI para despertarlo, con un techo de 1.5KB por mensaje.

**Esto no es un defecto de FL.** La API de Cubase tiene las mismas
limitaciones: sandboxed, sin file I/O, sin red, sin crear pistas, sin insertar
plugins. Los DAW modernos exponen scripting para *control de hardware*, no
para *producción programática*.

### Por qué REAPER

| | REAPER | FL Studio |
|---|---|---|
| Funciones de API | **900+** | crippled |
| Control externo | **`python-reapy` por TCP** o file-RPC | MIDI, 1.5KB, sandbox |
| File I/O desde el DAW | ✅ libre | ❌ bloqueado |
| Crear pistas | ✅ | ❌ |
| Insertar plugins | ✅ | ❌ |
| MCP existentes | varios, 55-600+ tools | ninguno |
| Licencia | 60 días gratis → **$60** | $99 |

Reaper 7.80 está instalado en `C:\Program Files\REAPER (x64)\`.

### Los plugins de FL Studio

Los **nativos** (FLEX, Sytrus, Harmor) no son VST3: son FL-only. Pero existe
`FL Studio VSTi (Multi).dll` en:
```
D:\Program Files\Image-Line\FL Studio 2025\System\Plugin\VSTi\x64\
```
que carga **FL Studio entero como VST2** dentro de REAPER → FLEX y todo el
bundle siguen disponibles. Reaper soporta VST2.

Trampas conocidas (de foros): hay que crear una pista de **instrumento** (no
de audio), FL debe estar en modo "song", y el output a "FL 1".

---

## 1. Qué hay en el repo ahora

```
DAWHereticMCP/
├── Cargo.toml                  # workspace: heretic-core, heretic-mcp, heretic-daw
├── AGENTS.md                   # este archivo
├── crates/
│   ├── heretic-core/           # tipOS compartidos, error, auth HMAC, audit SQLite, protocol
│   │                           # ESTO SE CONSERVA: la capa de blindaje
│   ├── heretic-daw/            # puente file-RPC a REAPER  (nuevo)
│   │   ├── src/lib.rs
│   │   ├── src/file_rpc.rs     # transporte: command.json -> response.json
│   │   └── src/reaper.rs       # cliente tipado + catalogo de acciones
│   └── heretic-mcp/            # server MCP stdio de FlojoMCP (andamiaje)
└── vendor-study/               # MCPs de Reaper clonados para estudiar
    ├── xDarkzx/                # 180 tools, CI, bridge Lua, commit hace 3 dias
    ├── total-reaper-mcp/       # 600+ tools, perfiles, bridge Lua
    └── T-Rzeznik/              # 55 tools, bridge TCP, diseño limpio
```

**Se conserva `heretic-core`:** es la capa de blindaje (HMAC Bearer, audit log
SQLite append-only, protocolo JSON-RPC). Ninguno de los MCPs de Reaper tiene
nada de eso: son bridges locales sin autenticación.

---

## 2. Diseño del transporte (heretic-daw)

El bridge Lua dentro de Reaper (`xDarkzx/Reaper-MCP`) usa **file-based IPC**:

```text
MCP (Rust)  --escribe-->  command.json    [dentro de Reaper]
MCP (Rust)  <--lee--     response.json   [dentro de Reaper]
```

Es el mismo patrón que el file-RPC de FL, pero con **una diferencia que lo
cambia todo**: el bridge de Reaper corre su propio bucle `defer()` a ~30 Hz.

**En FL había que mandar MIDI para despertar al DAW.** Eso obligaba a mantener
puertos MIDI abiertos de forma persistente,cía a tener un techo de 1.5KB, y
producía toda una clase de fallos "FL no responde" que costaron horas.

En Reaper no hay wake. Eso elimina de raíz la necesidad del MIDI entero.

Detalles que hay que respetar:
- **Atomicidad**: escribir en `.tmp` y renombrar, con 20 reintentos. En
  Windows el rename da `PermissionError` si el bridge tiene el destino abierto.
- **Ids monotónicos**, no timestamps: el bridge compara contra el último id
  visto, así que un id que no avance se pierde en silencio.
- **camelCase, no snake_case**. El bridge nombra los params `trackIndex`,
  `fxIndex`, `paramIndex`. Si el cliente manda snake_case, Reaper **no da
  error**: se queda con el valor por defecto y parece que funcionó. Es el peor
  modo de fallo posible, y hay un test que lo fija.
- **La carpeta de IPC se crea desde el cliente**, no se asume que el bridge ya
  la hizo. Si no, el error dice "io" en vez de "el bridge no está".

---

## 3. Decisiones

| Decisión | Por qué |
|---|---|
| `heretic-core` se conserva | La capa de blindaje es lo único que los MCPs de Reaper no tienen |
| `heretic-daw` separado de `heretic-mcp` | El transporte y el ciclo de vida son DAW; las tools son superficie |
| file-RPC, no `python-reapy` | Las dos dan las mismas 900+ funciones. file-RPC no mete Python dentro de Reaper, que es lo que se rompe primero (DTM, versión, bits) |
| 4 tools, no 17 | 17 wrappers finos sobre un escape hatch solo añadían mantenimiento, y cada wrapper reimplementaba la construcción de params |
| Un solo `command.json`, no mailbox de 8 slots | No hay wake ni concurrencia dentro del DAW: un slot basta y es más simple de depurar |
| vendor-study no se sube al repo | Es material de referencia, no código nuestro |

---

## 4. Lección de FL que se aplica a Reaper

El bug más caro no fue de FL: fue **nuestro diseño de tools**. En FL, 17
wrappers finos reconstruían los params cada uno, y ahí se colaron:

- `fl_set_song_position` mandaba `ms` cuando el daemon leía `position` (y el
  default del daemon era "bars" mientras la tool documentaba milisegundos: un
  desajuste de 4×).
- `create_project` no estaba en la tabla de ruteo, así que caía en el handler
  equivocado.
- `dir` y `template` eran obligatorios en el schema aunque el código los
  trataba como opcionales.

**Conclusión:** con N wrappers hay N sitios donde equivocarse. Reaper tiene
muchas más capacidades que FL, así que el riesgo escala. La mitigación es la
misma que se decidió para FL: pocas tools con superficies explícitas, y tests
que fijan el contrato de nombres.

---

## 5. Estado y próximos pasos

### Hecho
- [x] Investigación de alternativas: REAPER es la vía viable
- [x] Reaper 7.80 instalado
- [x] MCPs candidatos clonados y analizados (`vendor-study/`)
- [x] Andamiaje `heretic-daw` con file-RPC (9/9 tests)
- [x] Todo el trabajo de FL commiteado (`0930996`) antes de borrarlo

### Pendiente
- [ ] **Configurar el bridge Lua dentro de Reaper** y confirmar que
      file-RPC responde de verdad (el E2E de FL no tiene equivalente aún)
- [ ] Probar `python-reapy` por si compensa el setup de Lua
- [ ] **FL Studio VSTi en Reaper** para tener FLEX (pista de instrumento, modo
      song, output "FL 1")
- [ ] Decidir: usar `xDarkzx` tal cual, fork con la capa de blindaje, o
      reimplementar sobre `heretic-daw`
- [ ] Herramientas MCP: empezar por `track`, `fx`, `midi`, `project`
- [ ] `fl_diagnose` equivalente (¿Reaper vivo? ¿bridge respondiendo?)
- [ ] Installer automático (el de FL está en `0930996`, se puede portar)

---

## 6. Reglas para futuras sesiones

- **Español** en conversación, **inglés** en código/comments/docs.
- **Probar antes de declarar terminado.** Con Reaper, de verdad, no contra
  mocks: el 100% de los bugs caros de FL eran tests que pasaban en local y
  fallaban contra el DAW real.
- **No compilar/mover sin luz verde** del usuario.
- Un test que pasa unas veces y otras no es peor que uno que no pasa: entrena
  a ignorarlo. Si algo es intermitente, o se aísla, o se documenta por qué.
- Los docs de FL están en el historial (`git show 0930996`), no en el repo.

---

## 7. Referencias

- **REAPER ReaScript API**: https://www.reaper.fm/sdk/reascript/reascripthelp.html
- **python-reapy**: https://python-reapy.readthedocs.io/
- **xDarkzx/Reaper-MCP** (el candidato más sólido): https://github.com/xDarkzx/Reaper-MCP
- **FL Studio como plugin**: https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/flstudio_vst_plugin.htm
- **Por qué Cubase tampoco vale**: https://forums.steinberg.net/t/full-scripting-api-for-cubase-the-ai-integration-gap-is-now-a-competitive-threat/1026258
- **FlojoMCP** (framework): `D:\Mis Juegos\ClaudeMCPs\FlojoMCP\`
- **Historial de FL**: `git show 0930996` en este repo
