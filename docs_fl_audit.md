# Auditoría brutal: fl-studio-mcp → FlojoMCP

> **Fecha:** 2026-09-24 · **Versión auditada:** 0.3.0 (FL 25.2.5 build 5319)
> **Auditor:** General Beria · **Veredicto:** Buena ingeniería, techo bajo.
> **Acción propuesta:** Port a Rust sobre FlojoMCP (`D:\Mis Juegos\ClaudeMCPs\FlojoMCP`).

---

## TL;DR

El proyecto actual **funciona**, está **bien mantenido**, tiene **tests** y la
arquitectura MIDI-SysEx es ingeniosa (FL sandboxea file I/O en controller
scripts, eso está bien resuelto). Pero tiene **4 problemas estructurales** que
lo condenan a ser un "MCP de mierda" para cualquier workflow serio:

1. **Detección de plugins y proyectos = primitiva.** Solo expone lo que FL
   reporta en runtime. Sin índice de librería, sin parsing de `.flp`, sin cache.
2. **Transporte MIDI SysEx = techo de 1.5KB/payload.** Limita cada respuesta y
   obliga a paginar TODO. Round-trip latency + base64 overhead + ports loopback
   frágiles.
3. **Python con venv + dependencias nativas (python-rtmidi) = pesadilla de
   deploy.** 50-76MB RAM, binarios por plataforma, GIL.
4. **No aprovecha concurrencia.** Las 125 tracks del mixer se leen una a una
   (secuencial). No hay batching real.

**El port a FlojoMCP** elimina los 4 problemas de un golpe porque el framework
ya está pensado para esto (`ws-server` para plugin↔server, `truncate_json` por
si acaso, `SessionManager` para proyectos, static typing, 4.2MB RAM).

---

## Lo que está BIEN (respeto al trabajo original)

- **Arquitectura MIDI-SysEx está justificada.** El sandbox de FL bloquea file
  I/O en controller scripts (confirmado con stack traces). MIDI es el único
  canal bidireccional siempre activo. Bien resuelto: F0/F7 framing, base64 para
  meter bytes en data MIDI, magic `MCP` para ignorar SysEx ajeno, request-id
  de 8 chars ASCII, heartbeat cada 500ms, paginación por presupuesto de bytes.
- **Safety layer es decente.** `safety.safe_write` con snapshot→log→write→
  readback→rollback. `safe_write_group` para writes atómicos. Persistencia en
  `~/.flstudio-mcp/changelog.jsonl`. Dry-run global. Tests cubren 13 paths.
- **Mix Doctor v3 está maduro.** 30KB de reglas puras con thresholds
  transparentes, snapshot gathering con paginación, full-song watch para peaks
  reales (no instantáneos), gain staging con banda healthy, reference match,
  findings ordenados por severidad. La separación snapshot/diagnose/plan es
  correcta.
- **Calibración plugin→intent está bien pensada.** Sweep+readback para mapear
  normalized↔unit por plugin. Funciona para FabFilter Pro-C 3 confirmado.
- **Project state honesto.** Reconoce límites reales: FL no carga plugins, no
  crea patterns, no coloca clips en playlist, no navega presets Serum, tempo
  ignorado en diálogos modales. Sin sobreventa.

**Esto NO se tira. Se re-implementa con mejor techo.**

---

## Los 4 problemas estructurales (lo podrido)

### 1. Detección de plugins = BASURA (el problema central de tu queja)

**Estado actual** (`tools/plugin.py`, `music/plugin_library.py`,
`fl_controller/.../device_FLStudioMCP.py`):

```python
# Controller: lista plugins en un mixer track (slots 0-9)
def _h_plugin_list(p):
    track = int(p["track"])
    slots = []
    for s in range(10):
        if plugins.isValid(track, s):
            slots.append({"slot": s, "name": plugins.getPluginName(track, s)})
    return {"track": track, "slots": slots}
```

**Lo que falta (lo que jode):**

| Lo que necesitas | Lo que hay | Gap |
|---|---|---|
| Buscar un plugin por nombre en tu librería | Solo nombres ya cargados en slots 0-9 de un track | **TOTAL** |
| Ver qué plugins están cargados en el proyecto (todos los channels) | `channel_list` NO expone `getChannelPluginName` | **TOTAL** |
| Ver fabricante/formato del plugin (VST3, AU, CLAP, native) | `.fst` solo tiene el basename | **TOTAL** |
| Buscar por categoría ("EQ", "compressor") con fuzzy match | `effects_by_role` keyword matching naive | **PARCIAL** |
| Parsear metadata del `.fst` (Fabricante, categoría IL) | Solo `os.path.splitext(f)[0]` | **TOTAL** |
| Buscar en `.dll` reales de `C:\Program Files\Common Files\VST3\` | No se escanea esa ruta | **TOTAL** |
| Cache persistente de la librería | Cada llamada `os.walk` desde cero | **TOTAL** |
| Inferir categoría desde el path (`Effects/Dynamics/`, `Generators/Synth`) | No se usa | **TOTAL** |
| Buscar presets por tags / carpeta | Solo nombres raw | **TOTAL** |
| Detectar samples cargados en plugins (Sampler, Drumaxx, etc.) | No expuesto | **TOTAL** |

**Consecuencia real:** Cuando dices "configura una cadena vocal con mi Pro-C
3", el server tiene que probar plugins en slots hasta que encuentra uno que
matchee "Pro-C 3". Y si NO está cargado en el proyecto actual → no existe
para el MCP.

### 2. Detección de proyectos = primitiva

`get_project_state` devuelve 7 campos:

```python
{
  "fl_version": "25.2.5 [build 5319]",
  "tempo_bpm": 145.0,
  "playing": False,
  "recording": False,
  "pattern_number": 1,
  "pattern_count": 4,
  "channel_count": 12,
  "mixer_track_count": 18,
}
```

**Lo que NO hay:**

- Path del archivo `.flp` abierto
- Samples referenciados (paths resueltos)
- BPM/signature en PPQ, time signature
- Markers y secciones con nombres
- Patterns content (notas por patrón)
- Automation lanes activas
- Playlist completo (clips colocados — aunque FL API no permite escribir aquí)
- Snapshot completo del proyecto para rollback de "todo el proyecto"
- Diff entre dos snapshots

**Un `.flp` es un ZIP con secciones.** Lo puedes parsear en disco sin tocar FL.
Eso da path, samples, mixer state, patterns → todo en una lectura.

### 3. Transporte MIDI SysEx = techo de 1.5KB

Cada `_paginate()` tiene `_LIST_BUDGET = 600 bytes` (→ ~843B en wire). Eso
significa:

- `mixer_list_tracks` con 125 tracks → **mínimo 2-3 páginas**
- `channel_routing_summary` → 3-5 páginas
- `plugin_get_params` en un VST monster → **decenas de páginas**
- Cada página = 1 SysEx + 1 roundtrip MIDI + base64 overhead 33% + heartbeat
  lock contention

**Más problemas:**

- Loopback MIDI (loopMIDI/IAC) es frágil: drivers se caen, conflictos de port
  number, sandboxing en MSIX Claude Desktop (motivo 1 del daemon).
- **1 FL Studio instance = 1 puerto** (no escala).
- Sin multidifusión: si tienes 2 MCP clients, ambos pelean por el puerto.
- Latency impredecible: heartbeat cada 500ms + coalesce de FL por script-tick.

**FlojoMCP ya tiene `ws-server` (WebSocket transport):** un plugin en FL
conecta vía WS al server MCP. Sin cap de payload, sin ports loopback, sin
nota-arm-por-sesión, multi-cliente nativo.

### 4. Python runtime = pesado

- FastMCP: ~50MB RAM en idle
- mido + python-rtmidi: bindings nativos por plataforma
- GIL bloquea concurrencia real
- Tests con mock MIDI bridge = boilerplate
- Deploy: venv + pip install + dependencias nativas

**FlojoMCP:** single 7MB binary, 4.2MB RAM, 73µs tool call, static typing,
testing sin transporte (`FlojoTester`), `truncate_json` para cuando aplique,
`SessionManager` por proyecto.

---

## Plan de port a FlojoMCP: arquitectura objetivo

```
┌─────────────────────────────────────────────────────────────┐
│ OpenCode / Claude (MCP client stdio / HTTP / WS)            │
└────────────────┬────────────────────────────────────────────┘
                 │
┌────────────────▼────────────────────────────────────────────┐
│ flojo-studio-mcp (Rust binary, 7MB, 4.2MB RAM)              │
│                                                             │
│   ┌─────────────────────────────────────────────────────┐   │
│   │ MCP layer (FlojoMCP)                                │   │
│   │   - #[tool] / #[resource] / #[prompt]               │   │
│   │   - stdio + HTTP + WS transports                    │   │
│   │   - FlojoTester para tests sin FL                   │   │
│   └─────────────────────────────────────────────────────┘   │
│                                                             │
│   ┌─────────────────────────────────────────────────────┐   │
│   │ Domain layer (Rust, typed)                          │   │
│   │                                                     │   │
│   │   ProjectInspector        - parse .flp (ZIP+JSON)   │   │
│   │   PluginLibraryIndexer    - parallel scan + SQLite  │   │
│   │   ChannelSnapshotter      - in-memory typed cache   │   │
│   │   MixerSnapshotter        - stateful, diff-friendly │   │
│   │   PresetCatalog           - serde-json catalog      │   │
│   │   SampleAssetIndexer      - path resolution + tags  │   │
│   │   CalibratedIntentEngine  - calibration cache       │   │
│   │   MixDoctor               - parallel peak analysis  │   │
│   │   SafetyLayer             - atomic changelog        │   │
│   │   SessionManager          - per-project state       │   │
│   └─────────────────────────────────────────────────────┘   │
│                                                             │
│   ┌─────────────────────────────────────────────────────┐   │
│   │ Bridges (3 paths, transparent fallback)             │   │
│   │                                                     │   │
│   │   FlControllerWs   - FL plugin → server (default)   │   │
│   │   FlControllerMidi - loopback MIDI (legacy)         │   │
│   │   FlpReader        - direct .flp parse (read-only)  │   │
│   └─────────────────────────────────────────────────────┘   │
└────────────────┬────────────────────────────────────────────┘
                 │
┌────────────────▼────────────────────────────────────────────┐
│ FL Studio 25 (3 hooks coexistentes)                         │
│   - Controller Script (legacy MIDI, retained for back-compat)│
│   - WebSocket plugin (NEW, primary)                          │
│   - .flp file on disk (read by Rust directly)               │
└─────────────────────────────────────────────────────────────┘
```

### Mejoras concretas que el port habilita

#### A. Plugin detection v2 (lo que pediste)

- **Scanner paralelo** (Rayon) de:
  - `<Documents>/Image-Line/FL Studio*/Presets/Plugin database/Installed/{Effects,Generators}/<format>/*.fst`
  - `C:\Program Files\Common Files\VST3\`, `VST2`, etc. → `walkdir` + `*.dll`
- **Parseo del header `.fst`** (texto plano con metadatos de FL) → fabricante,
  categoría, formato. Si falta, inferencia por keywords + path.
- **Cache SQLite** con hash del path de librería → invalidación incremental
  (no reescanea 5000 plugins cada vez). WAL mode para reads concurrentes.
- **Fuzzy search** con `strsim` (Levenshtein/Jaro-Winkler): "fabfilter proc"
  → "FabFilter Pro-C 3" sin ambigüedad.
- **Categorización tipada** (enum):
  `Eq / Compressor / Reverb / Delay / Saturator / Limiter / Synth / Sampler / ...`
- **Tool surface:**
  - `fl_plugin_search(query, category?, format?)` → fuzzy + filtros
  - `fl_plugin_inspect(track, slot)` → estructura completa (con cache TTL)
  - `fl_plugin_metadata(name)` → fabricante, formato, categoría inferida
  - `fl_get_loaded_plugins()` → TODOS los plugins cargados (channels + mixer)
  - `fl_sample_search(query)` → samples de plugins como Sampler/Drumaxx

#### B. Project detection v2

- **Parseo directo del `.flp`** (ZIP → leer `project.xml` o secciones):
  - Path del archivo
  - BPM, time signature, PPQ
  - Patterns (con notas)
  - Channels (nombre, target FX, generator plugin)
  - Mixer tracks (vol, pan, sends, plugins por slot)
  - Markers
  - Samples referenciados (paths resueltos si existen)
- **Resource `fl://project/inspect`** → snapshot completo (auto-pulled por Claude)
- **Tool `fl_project_diff(snapshot_a, snapshot_b)`** → qué cambió
- **Tool `fl_project_open(path)`** → carga el .flp (FL API lo soporta) +
  snapshot
- **Snapshot persistente** en `<project>/.flojo-mcp/snapshots/` con timestamp

#### C. Mixer/Channel snapshot con cache typed

- `FlMixerSnapshot` y `FlChannelSnapshot` como structs Rust (no dicts mágicos)
- Cache en memoria con TTL configurable (default 5s), invalidación por
  heartbeat write-detect
- `fl_mixer_full(track)` → todo en una llamada (sin paginación):
  `{name, vol, pan, mute, solo, color, plugins: [{slot, name, params_summary}], sends: [...]}`
- Lectura paralela de 125 tracks con `tokio::spawn` + `JoinSet`

#### D. Mix Doctor v2 (paralelo)

- Peaks: `fl_mixer_get_peaks` × 125 en paralelo (125 SysEx roundtrips → 125
  futures → 1 batch) vs actual 125 roundtrips secuenciales
- Reglas como enums Rust tipados (no strings mágicas):
  ```rust
  enum Finding {
    Clipping { track: u8, peak_db: f32, severity: Severity },
    MissingHpf { track: u8, chain: Vec<String> },
    Ungrouped { family: Family, tracks: Vec<u8> },
    EqClash { bucket_hz: f32, tracks: Vec<(u8, String)> },
    ...
  }
  ```
- Plan engine con `enum Plan { TrimVolume, GroupTracks, EqIntent, CompressorIntent, ... }`
- Apply atómico via `safe_write_group`

#### E. Calibrated intent engine v2

- `EqIntent`, `CompressorIntent`, `ReverbIntent`, `DelayIntent` como structs
  tipados con unidades (Hz, dB, ms, ratio)
- Calibration cache: `{plugin_fingerprint: {param_index: (norm_to_unit_fn)}}`
- Fingerprint = hash del plugin name + sample de 3 params → detecta drift
  tras update del plugin
- Re-calibration async en background cuando fingerprint cambia

#### F. Safety + rollback atómico

- Changelog en SQLite (no jsonl) → queryable, indexable, concurrente
- Multi-scope snapshots: `mixer_track`, `channel`, `plugin_param`, `route`,
  `project` (NEW)
- `fl_rollback_to_snapshot(id)` → restaura un snapshot completo
- Dry-run en todos los writes
- Diff visualization antes de aplicar

#### G. WebSocket transport (primary)

- Plugin FL Studio (Python + `websockets`) que conecta a `127.0.0.1:9788`
- Sin cap de payload (JSON crudo)
- Multi-instancia: 1 server MCP, N plugins FL (futuro)
- Auto-reconnect con backoff
- Fallback automático a MIDI si WS falla

#### H. Recursos MCP con schema descriptivo

- `fl://status` — bridge alive + heartbeat
- `fl://project/inspect` — snapshot completo del .flp
- `fl://project/channels` — typed snapshot
- `fl://project/mixer` — typed snapshot
- `fl://plugins/library` — indexado (cached)
- `fl://plugins/loaded` — cargado actualmente
- `fl://presets/catalog` — indexado (cached)

---

## Tabla comparativa

| Aspecto | Actual (Python) | Port (Rust/FlojoMCP) |
|---|---|---|
| Binary size | 50MB+ (venv+deps) | 7MB single .exe |
| RAM en idle | 52-76MB | 4.2MB |
| Tool call latency | ~800µs | 73µs |
| Transport payload cap | 1.5KB (SysEx) | Ilimitado (WS) |
| Plugin library index | Solo nombres cargados | Indexed + cached + searchable |
| Plugin metadata | Nada | Fabricante, formato, categoría |
| Sample detection | Nada | Path resolution + tags |
| Project file (.flp) | Solo state runtime | Parse directo + state |
| Concurrency | GIL | Tokio (async + paralelo) |
| Snapshot rollback | Por scope | Por scope + project-wide |
| Testing | Mock bridge | FlojoTester (sin bridge) |
| Type safety | Pydantic | Static + compile-time |
| Deploy | venv + pip + native | Copy .exe |

---

## Estructura de archivos propuesta (FlojoMCP workspace)

```
FlojoMCP/
├── crates/                          (existente, intacto)
│   ├── flojo-mcp/
│   ├── flojo-macros/
│   ├── flojo-cli/
│   └── flojo-session/
│
├── examples/                        (existente + nuevos)
│   ├── calculator/                  (existente)
│   ├── fl_studio_mcp/               ★ NUEVO: el port
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── main.rs              # FlojoMCP server boot
│   │   │   ├── tools/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── transport.rs     # play/stop/tempo/position
│   │   │   │   ├── channels.rs      # channel CRUD + snapshot
│   │   │   │   ├── mixer.rs         # mixer CRUD + snapshot
│   │   │   │   ├── patterns.rs      # pattern CRUD
│   │   │   │   ├── plugins.rs       # ★ plugin search/inspect/control
│   │   │   │   ├── presets.rs       # ★ preset catalog
│   │   │   │   ├── piano_roll.rs    # note writing
│   │   │   │   ├── compose.rs       # scales/melodies/chords
│   │   │   │   ├── mixing.rs        # EQ/comp/reverb/delay intents
│   │   │   │   ├── mix_doctor.rs    # diagnosis + plans
│   │   │   │   ├── chains.rs        # vocal/bass/drum chains
│   │   │   │   ├── color.rs         # track/channel coloring
│   │   │   │   ├── bulk.rs          # group mute/solo
│   │   │   │   ├── routing.rs       # sends + routing
│   │   │   │   ├── arrange.rs       # markers + patterns
│   │   │   │   └── export.rs        # MIDI export
│   │   │   ├── resources/
│   │   │   │   └── mod.rs           # fl:// status/project/...
│   │   │   ├── prompts/
│   │   │   │   └── mod.rs           # mix_doctor, vocal_chain, etc.
│   │   │   ├── bridges/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── fl_controller.rs # trait abstracto
│   │   │   │   ├── ws.rs            # WebSocket impl (primary)
│   │   │   │   └── midi.rs          # MIDI impl (legacy fallback)
│   │   │   ├── domain/              # tipos Rust (no dicts)
│   │   │   │   ├── mod.rs
│   │   │   │   ├── project.rs       # FlpProject, FlpChannel, FlpMixer
│   │   │   │   ├── plugin.rs        # PluginMetadata, PluginFormat, ...
│   │   │   │   ├── mixer.rs         # MixerTrack, MixerSlot, ...
│   │   │   │   ├── channel.rs       # Channel, ChannelGenerator, ...
│   │   │   │   ├── pattern.rs       # Pattern, Note, ...
│   │   │   │   ├── transport.rs     # Transport, Tempo, Position
│   │   │   │   └── intent.rs        # EqIntent, CompressorIntent, ...
│   │   │   ├── lib/                 # librerías read-only
│   │   │   │   ├── mod.rs
│   │   │   │   ├── plugin_indexer.rs   # SQLite + parallel scan
│   │   │   │   ├── preset_catalog.rs
│   │   │   │   ├── flp_parser.rs       # parse .flp ZIP
│   │   │   │   ├── sample_indexer.rs
│   │   │   │   ├── calibration.rs
│   │   │   │   ├── levels.rs           # peak measurement
│   │   │   │   └── watcher.rs          # full-song peak watch
│   │   │   ├── safety.rs            # changelog + rollback
│   │   │   ├── session.rs           # per-project session
│   │   │   └── error.rs             # error types
│   │   └── tests/                   # FlojoTester integration tests
│   └── fl_pyscript/                 # FL plugin Python (WS client)
│       ├── fl_mcp_ws_plugin.py      # WebSocket client + sys.expose
│       └── fl_mcp_midi_bridge.py    # Legacy MIDI fallback
```

---

## Plan de iteración (Fase 2 → Fase 3 → Fase 4)

### Fase 2 — Fundamentos (1 sesión)

1. Crear `examples/fl_studio_mcp/` con `cargo new`
2. Wire con `flojo-mcp` (path dep) + `tokio`, `serde`, `schemars`, `walkdir`,
   `rusqlite`, `strsim`, `zip`, `quick-xml`, `tracing`
3. Definir `domain/*` (structs tipados)
4. Bridges trait + WS impl (server-side)
5. Tools `transport` (play/stop/tempo/position) — los más simples
6. Smoke test: server arranca, FL plugin conecta, ping funciona

### Fase 2.5 — Iteración: features core

7. Tools `channels` + `mixer` (read + write)
8. Tools `patterns` + `arrange`
9. Tools `plugins` (read params, set params) — sin search todavía
10. Tools `piano_roll` (note writing)
11. Tools `mixing` (basic EQ intent)
12. Tests con `FlojoTester` (sin FL)

### Fase 3 — Las features nuevas que el actual no tiene

13. `PluginIndexer` con SQLite + parallel scan
14. `.flp` parser (ZIP + project.xml)
15. Tools `plugins/search`, `plugins/inspect`, `plugins/metadata`
16. `PresetCatalog` con tag inference
17. `SampleIndexer` con path resolution
18. `MixDoctor` parallel + typed rules
19. `CalibratedIntentEngine` v2
20. `SafetyLayer` SQLite changelog
21. `SessionManager` per-project
22. Resources MCP con `fl://project/inspect`, etc.

### Fase 4 — Maduración

23. Prompts MCP para workflows ("mix doctor", "vocal chain", ...)
24. WebSocket plugin Python + legacy MIDI bridge fallback
25. Installer scripts (PowerShell + Cargo)
26. Tests de integración contra FL real
27. Benchmarks vs FastMCP version (workflow completo)
28. Documentación + SKILL.md

---

## Compatibilidad: ¿rompemos el API?

**Decisión a tomar** (ver preguntas abajo). Opciones:

- **A. API idéntico** (`fl_ping`, `fl_set_mixer_volume`, etc.) → drop-in
  replacement. Cliente MCP no nota nada. Pero arrastra nombres flojos.
- **B. API nuevo** (`mix_ping`, `mixer_set_volume`, `plugin_search`, ...) →
  más limpio, pero requiere actualizar prompts / skill.
- **C. Híbrido**: tools nuevos con nombres limpios, tools legacy como alias
  deprecated.

---

## Riesgos del port

1. **FL WebSocket plugin**: hay que escribir un plugin FL en Python que use
   `websockets` (depuración: ¿está en FL built-in modules?). Si no está,
   hay que negociar con el sandbox. Alternativa: plugin CLAP o VST wrapper
   más complejo.

2. **`.flp` parsing**: el formato no está 100% documentado por Image-Line.
   Hay proyectos open-source que lo parsean (pyflp, flp-parser). Usar uno
   como referencia o implementar subset read-only.

3. **Calibration cache invalidation**: cuando un plugin se actualiza, los
   índices de parámetros pueden cambiar. Fingerprint + re-validate on
   connect. Coste aceptable.

4. **Migración de tests**: los 30+ scripts en `scripts/test_*.py` se
   re-implementan como `cargo test` con `FlojoTester`. El harness es
   superior (sin bridge mock).

5. **Tiempo**: el port completo es ~4-6 semanas de trabajo dedicado. Se
   puede hacer incremental: port Phase 0-3 primero (core funcional),
   luego Phase 4-5 (mejoras nuevas), luego Phase 6-7 (polish).

---

## Decisiones a tomar AHORA (antes de empezar)