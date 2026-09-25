# AGENTS.md — FL Heretic MCP

> **Memoria viva del proyecto.** Cualquier sesión futura debe leer esto primero.
> Actualizar tras cada iteración completada (formato changelog al final).

---

## 0. Identidad del proyecto

- **Nombre:** FL Heretic MCP
- **Propósito:** MCP server + **daemon blindado** que controla FL Studio desde agentes IA, rompiendo el walled garden de FL con un modelo de seguridad fuerte y concurrencia real.
- **Tagline:** *"Daemons blindados para un DAW amurallado."*
- **Rename:** este repo se llamaba `FLStudioMCP` (Python, FastMCP). El propósito nuevo (Rust + FlojoMCP + daemon blindado + Named Pipes) implica rename físico del directorio y de los artefactos. Ver §10.
- **Licencia:** GPL-3.0-or-later
- **Stack objetivo:** Cargo workspace, Rust 2024, `flojo-mcp` + `flojo-macros` (path deps), `tokio`, `rusqlite`, `hmac`, `clap`.
- **Path raíz:** `D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\`

---

## 1. Visión

FL Studio expone un sandbox Python muy limitado en controller scripts: ni file I/O, ni sockets, ni subprocess. El único canal bidireccional always-on es MIDI SysEx con un techo físico de ~1.5KB por mensaje (loopMIDI dropea silenciosamente más grande). El FLStudioMCP original resolvió esto con ingenio (MIDI SysEx + daemon TCP fallback + paginación por presupuesto), pero carga con 4 problemas estructurales:

1. **Detección de plugins = primitiva** → solo plugins ya cargados en slots 0-9 del mixer. Sin índice de librería. Sin parse de `.fst`. Sin fabricante/formato.
2. **Detección de proyectos = primitiva** → `get_project_state` devuelve 7 campos. Sin parse del `.flp`. Sin samples referenciados.
3. **Transporte MIDI = techo 1.5KB** → paginación obligatoria de TODO, round-trip latency, ports loopback frágiles.
4. **Runtime Python pesado** → 50-76MB RAM, GIL, deploy con venv + native deps.

**FL Heretic MCP** ataca los 4 con un diseño en 3 capas:

```
[MCP server stdio]  ←FlojoMCP→  [Daemon blindado Named Pipe]  ←MIDI+parse→  [FL Studio]
     Rust 7MB              Rust mismo binary           FL Python sandbox + .flp
```

El daemon es el único proceso autorizado a tocar FL. Autentica cada request del agente, rate-limitea, audita, supervisa FL con watchdog, cachea el proyecto en SQLite, y maneja reconexión tras crashes sin que el MCP server se entere.

---

## 2. Auditoría del FLStudioMCP original (referencia histórica)

Esto ya está preservado en `AUDIT.md`. Resumen ejecutivo:

### Lo que está BIEN (no se tira, se re-implementa)
- SysEx wire format (F0/F7 framing, base64, magic `MCP`, request-id, heartbeat 500ms)
- Safety layer (`safe_write`, `safe_write_group`, changelog JSONL, dry-run)
- Mix Doctor v3 (30KB, snapshot/diagnose/plan separados, full-song peak watch)
- Plugin→intent calibration pattern (sweep+readback)
- Limits honestos (no carga plugins nuevos, no crea patterns, no coloca clips)

### Lo que está PODRIDO (los 4 problemas)
1. **Plugin detection = basura** → ver §1 arriba
2. **Project detection = primitiva**
3. **MIDI SysEx = techo 1.5KB**
4. **Python runtime = pesado**

### Inventario del código legacy a preservar como referencia
- `fl_controller/FLStudioMCP/device_FLStudioMCP.py` (37KB) → portar lógica de handlers a Rust
- `docs/SERUM_PROBE_FINDING.md`, `docs/VST_PROBE_FINDING.md`, `docs/ARRANGEMENT_FINDING.md` → findings críticos para el port
- `docs/FIX_REPORT.md` → bugs conocidos del controller script (OnSysEx, set_tempo flags)
- `docs/MIXING_ROUTING_REPORT.md`, `docs/COMPRESSION_CALIBRATION_REPORT.md`, `docs/PHASE1A_REPORT.md`, `docs/PHASE1B_REPORT.md` → contexto de las decisiones de diseño

---

## 3. Decisiones arquitectónicas (con razón)

| Decisión | Por qué |
|---|---|
| **Cargo workspace, mismo repo, mismo binary** | Un solo binario `fl-heretic.exe` con subcommands. Path deps a `flojo-mcp`/`flojo-macros`. Cero overhead, cero duplicación de tipos. |
| **Daemon blindado en Rust** | El agente MCP NO es de confianza: validamos todo. Auth HMAC + rate limit + audit log + circuit breaker + ACL + watchdog. |
| **Named Pipes en Windows** | Single-client perfecto para este caso, más rápido que TCP loopback, ACL de Windows nativa, no expone a la red. `\\.\pipe\fl-heretic-<pid>` |
| **JSON-RPC 2.0 sobre Named Pipe** | Framing NDJSON (un JSON por línea). Handshake + auth + comandos + heartbeats. |
| **Auth HMAC-SHA256 Bearer** | Token generado al `init`, guardado en `%LOCALAPPDATA%\fl-heretic\token` con ACL usuario. Cada request firma el payload. |
| **Rate limit por tool (token bucket)** | `governor` crate (ya en FlojoMCP con feature `rate-limit`). Configurable por tool. Default razonable. |
| **Audit log SQLite append-only** | WAL mode, tabla `events`, triggers que bloquean UPDATE/DELETE. Retention 30 días rotativo. |
| **Capability ACL** | Lista de tools permitidas por scope. Default "todo permitido", modo "safe" / "demo" / "readonly". |
| **Circuit breaker** | 3 heartbeats perdidos → open 5s → half-open 1 prueba → close. Backoff exponencial hasta 60s. |
| **Watchdog tokio task** | Supervisa FL alive, .pyscript armed, disk space, port conflicts. Auto-recovery best-effort. |
| **FlojoMCP stdio + `rate-limit` + `session`** | El MCP server usa el framework. Std IO NDJSON (compatible con OpenCode). |
| **API tools idéntico `fl_*`** | Drop-in replacement para el FLStudioMCP actual. Cliente MCP no nota el cambio. |

---

## 4. Topología final (Fase 4+ completa)

```
┌──────────────────────────────────────────────────────────────┐
│ OpenCode / Claude (MCP client stdio NDJSON)                  │
└────────────────┬─────────────────────────────────────────────┘
                 │
┌────────────────▼─────────────────────────────────────────────┐
│ fl-heretic mcp (FlojoMCP, Rust)                              │
│   • Tool registry + schema validation                        │
│   • Bearer token al handshake                                │
│   • Reenvía comandos al daemon via Named Pipe                │
└────────────────┬─────────────────────────────────────────────┘
                 │ Named Pipe \\.\pipe\fl-heretic-<pid>
                 │ JSON-RPC 2.0 NDJSON + Bearer HMAC
┌────────────────▼─────────────────────────────────────────────┐
│ fl-heretic daemon (Rust, hardened)                           │
│   ┌─────────────────────────────────────────────────────┐   │
│   │ Security & control                                  │   │
│   │   • HMAC Bearer auth │   │
│   │   • Rate limit por tool (governor)                  │   │
│   │   • Audit log SQLite WAL append-only                │   │
│   │   • Capability ACL │   │
│   │   • Circuit breaker (heartbeat)                     │   │
│   └─────────────────────────────────────────────────────┘   │
│   ┌─────────────────────────────────────────────────────┐   │
│   │ State & cache                                       │   │
│   │   • Project snapshot (typed, versioned)             │   │
│   │   • Plugin library index (SQLite, persisted)        │   │
│   │   • .flp file watch (notify changes)                │   │
│   │   • Calibration cache (fingerprint → curves)        │   │
│   │   • Session manager (per-project)                          │   │
│   └─────────────────────────────────────────────────────┘   │
│   ┌─────────────────────────────────────────────────────┐   │
│   │ Transport to FL                                     │   │
│   │   • MIDI SysEx (primary hoy)                        │   │
│   │   • .pyscript file-watch bridge (heavy writes)      │   │
│   │   • .flp direct read (no FL touch)                  │   │
│   │   • VST3 FlojoBridge (FUTURO Phase 2)               │   │
│   └─────────────────────────────────────────────────────┘   │
│   ┌─────────────────────────────────────────────────────┐   │
│   │ Watchdog                                            │   │
│   │   • FL alive (heartbeat 500ms)                      │   │
│   │   • .pyscript armed (re-arm if lost)               │   │
│   │   • Disk space, port conflicts                      │   │
│   │   • Auto-restart on crash                           │   │
│   └─────────────────────────────────────────────────────┘   │
└────────────────┬─────────────────────────────────────────────┘
                 │ MIDI loopback / .pyscript / .flp
┌────────────────▼─────────────────────────────────────────────┐
│ FL Studio 25 (controller + .pyscript + .flp)                 │
└──────────────────────────────────────────────────────────────┘
```

---

## 5. Estructura del repo

```
FLHereticMCP/
├── Cargo.toml                              # workspace
├── README.md                               # descripción + quickstart
├── LICENSE                                 # GPL-3.0
├── .gitignore                              # build, secrets, runtime data
├── AGENTS.md                               # este archivo (memoria viva)
├── AUDIT.md                                # auditoría del FLStudioMCP original
├── crates/
│   ├── heretic-core/                       # tipos compartidos, error, auth, audit, protocol
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── error.rs                    # HereticError enum
│   │       ├── auth.rs                     # HMAC Bearer
│   │       ├── audit.rs                    # SQLite WAL append-only
│   │       ├── protocol.rs                 # JSON-RPC envelope sobre Named Pipe
│   │       ├── ratelimit.rs                # token bucket por tool
│   │       ├── circuit.rs                  # circuit breaker (heartbeat)
│   │       └── acl.rs                      # capabilities JSON
│   └── heretic-daemon/                     # el daemon blindado
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── main.rs                     # binario con subcommand dispatcher (clap)
│           ├── pipe.rs                     # Named Pipe server (Windows)
│           ├── commands.rs                 # dispatch JSON-RPC
│           ├── watchdog.rs                 # supervisor tokio task
│           └── handlers/
│               ├── mod.rs
│               ├── ping.rs                 # ✅ Fase 1
│               ├── auth.rs                 # handshake + verificación token
│               └── health.rs               # estado del daemon
├── examples/
│   └── ping/                               # cliente mínimo que conecta al daemon
│       ├── Cargo.toml
│       └── src/main.rs
└── scripts/
    ├── install_windows.ps1                 # installer (controller script, .pyscript, token)
    └── doctor.ps1                          # diagnóstico
```

Crate futuro (no en Fase 0+1):
- `heretic-mcp` — el MCP server (Fase 2)
- `heretic-fl` — el bridge a FL (controller + MIDI SysEx + .flp parser) (Fase 2)
- `heretic-flp` — parser de `.flp` (ZIP + project.xml) (Fase 4)

---

## 6. Plan de fases

### Fase 0 — Setup ⏳ (esta sesión)
- [x] Auditoría brutal del FLStudioMCP → `AUDIT.md`
- [ ] Crear estructura de directorios (`FLHereticMCP/`)
- [ ] `Cargo.toml` workspace con path deps a FlojoMCP
- [ ] `.gitignore`, `LICENSE` (GPL-3.0), `README.md` inicial
- [ ] `AGENTS.md` (este archivo)
- [ ] Rename físico del directorio `FLStudioMCP` → `FLHereticMCP`

### Fase 1 — Daemon básico ✅ target
- [ ] `heretic-core` con tipos compartidos
  - [ ] `error.rs` — `HereticError` enum con `thiserror`
  - [ ] `auth.rs` — HMAC Bearer + token store
  - [ ] `audit.rs` — SQLite WAL append-only + retention
  - [ ] `protocol.rs` — JSON-RPC envelope (request, response, error)
- [ ] `heretic-daemon` con Named Pipe server
  - [ ] `pipe.rs` — servidor Named Pipe Windows + auth handshake
  - [ ] `commands.rs` — dispatch JSON-RPC con handlers tipados
  - [ ] `watchdog.rs` — stub por ahora, implementación en Fase 5
- [ ] Binario `fl-heretic` con subcommand dispatcher (clap)
  - [ ] `fl-heretic mcp` — arranca MCP server (stub)
  - [ ] `fl-heretic daemon` — arranca el daemon blindado
  - [ ] `fl-heretic doctor` — diagnóstico
  - [ ] `fl-heretic token generate|rotate|show`
  - [ ] `fl-heretic init` — setup inicial (Fase 5)
- [ ] Comando `ping` funcional via Named Pipe + audit
- [ ] Tests con `FlojoTester`-style (sin Named Pipe, mock auth)
- [ ] `examples/ping/` — cliente que conecta al daemon y manda `ping`
- [ ] Compila, todos los tests pasan

### Fase 2 — Bridge MIDI + tools transport
- [ ] Port del controller script (`device_FLStudioMCP.py`) → handlers Rust
- [ ] `heretic-fl` con `midi.rs` — MIDI SysEx bridge (mido-equivalent en Rust: `midir` crate)
- [ ] Heartbeat detection (500ms)
- [ ] Tools transport: `fl_ping`, `fl_get_tempo`, `fl_set_tempo`, `fl_play`, `fl_stop`, `fl_get_song_position`, `fl_set_song_position`
- [ ] `heretic-mcp` — MCP server con FlojoMCP (path dep), reenvía al daemon via Named Pipe
- [ ] End-to-end: Claude → MCP server → daemon → MIDI → FL → respuesta

### Fase 3 — Port completo de tools (drop-in replacement)
- [ ] Port de las 67 tools del FLStudioMCP al MCP server Rust
- [ ] Mantener API `fl_*` idéntico
- [ ] Tests de integración contra FL real

### Fase 4 — Features nuevas (lo que el FLStudioMCP no tiene)
- [ ] Plugin indexer (SQLite cache + parallel scan)
- [ ] `.flp` parser (ZIP + project.xml)
- [ ] Project snapshot completo tipado
- [ ] Mix Doctor paralelo (peaks en paralelo via `tokio::spawn`)
- [ ] Calibration engine tipado
- [ ] Preset catalog + sample indexer
- [ ] Resources MCP: `fl://status`, `fl://project/inspect`, etc.

### Fase 5 — Maduración
- [ ] Rate limit real (governor)
- [ ] Circuit breaker con heartbeat
- [ ] Capability ACL por defecto "safe"
- [ ] Watchdog completo
- [ ] Prompts MCP para workflows
- [ ] Installer PowerShell
- [ ] Benchmarks vs Python version
- [ ] Documentación + SKILL.md

### Fase 6 — VST3 FlojoBridge (game changer)
- [ ] Plugin VST3 en C++ con SDK de Steinberg
- [ ] WebSocket server local cuando se carga
- [ ] MIDI bridge como fallback legacy
- [ ] Auto-detección de cuál usar

---

## 7. Estado actual (changelog)

### 2026-09-24 — Auditoría + decisiones
- ✅ Auditoría completa del FLStudioMCP → `AUDIT.md`
- ✅ Decisión: daemon blindado en Rust, mismo repo, Named Pipes, blindaje PRO
- ✅ Decisión: nombre "FL Heretic MCP"
- ✅ Decisión: Fase 0+1 primero (setup + daemon básico)
- ⏳ Creando estructura de directorios + Cargo workspace

---

## 8. Bugs abiertos / descubrimientos críticos

### Bugs del FLStudioMCP que el port debe arreglar
- B1. **`OnSysEx` missing en controller script legacy** → ya parcheado en upstream (FIX_REPORT §3a), pero el port debe usar `OnSysEx` + `OnMidiMsg` ambos
- B2. **`midi.REC_Updated` no existe en FL 25+** → usar `midi.REC_UpdateValue`. **Crítico**: usar `REC_FromMIDI` colapsa tempo a ~10 BPM
- B3. **Sample rate mismatch** → siempre trabajar con tempo `bpm * 1000` interno de FL
- B4. **MIDI SysEx 1.5KB hard cap** → no se puede saltarse en MIDI; bypass con `.flp` parser + VST3 plugin
- B5. **Plugin names truncated a 24 chars en listados** → bypass con `truncate_string` configurable, no en disco
- B6. **No hay forma de cargar plugins nuevos via API** → limitación real de FL, no bug. Documentar y usar `fl_suggest_plugin`
- B7. **Serum presets walled off** → FL solo expone 128 programas MIDI, no la librería real `.fxp`. Confirmado dead-end (SERUM_PROBE_FINDING). No retry.
- B8. **No se pueden colocar clips en playlist via API** → confirmado dead-end (ARRANGEMENT_FINDING). Preparar patterns + markers, usuario arrastra.

### Bugs del FL controller script sandbox
- S1. `open("...", "w")` → `SystemError: <class '_io.FileIO'> returned NULL`
- S2. `os.open(..., O_WRONLY|O_CREAT)` → `TypeError: bad argument type for built-in operation`
- S3. `os.makedirs(...)` → `SystemError: mkdir returned NULL without setting an exception`
- S4. `socket`, `subprocess`, `urllib` → no están en built-in module list

→ **Conclusión**: MIDI SysEx es el único canal bidireccional always-on en el controller script. El `.pyscript` del piano roll tiene file I/O pero solo corre on-demand (UX horrible).

---

## 9. Próximos pasos inmediatos

1. **Rename físico** del directorio `FLStudioMCP` → `FLHereticMCP` (ver §10)
2. **Crear el workspace Cargo** completo
3. **Implementar `heretic-core`** (tipos, error, auth, audit, protocol)
4. **Implementar `heretic-daemon`** (Named Pipe server + comando `ping`)
5. **Compilar y testear** el path completo MCP-less (cliente ping → daemon)
6. **Documentar** resultados en §7

---

## 10. Rename: FLStudioMCP → FLHereticMCP

### Acción
- Mover `D:\Mis Juegos\ClaudeMCPs\FLStudioMCP\` → `D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\`
- Actualizar referencias internas (pyproject.toml legacy → cargo.toml nuevo, README, etc.)
- El historial git se preserva si es un repo git (verificar con `git log` antes)
- El código Python legacy se mantiene como `legacy/` para referencia histórica

### Estructura después del rename
```
FLHereticMCP/
├── Cargo.toml                  # nuevo (workspace Rust)
├── AGENTS.md                   # este archivo
├── AUDIT.md                    # auditoría
├── README.md                   # nuevo
├── LICENSE                     # GPL-3.0
├── .gitignore                  # nuevo
├── crates/                     # nuevo (Rust)
├── examples/                   # nuevo (Rust)
├── scripts/                    # nuevo (PowerShell)
├── docs/                       # preservado del legacy
│   ├── CHANGELOG.md            # histórico v0.1/v0.2
│   ├── FIX_REPORT.md           # bugs del controller script
│   ├── SERUM_PROBE_FINDING.md  # findings críticos
│   ├── VST_PROBE_FINDING.md
│   ├── ARRANGEMENT_FINDING.md
│   └── ...
└── legacy/                     # preservado (código Python legacy)
    ├── pyproject.toml          # fl-studio-mcp (ya no se desarrolla)
    ├── src/fl_studio_mcp/      # código Python
    ├── fl_controller/          # controller script .py
    └── ...
```

### Decisión pendiente
- [ ] Confirmar con General: ¿rename físico AHORA o primero terminar Fase 0+1 conceptual y rename después?
- [ ] Si rename AHORA: ¿qué hago con el código Python legacy? (¿a `legacy/`? ¿lo borramos?)

---

## 11. Comandos útiles (referencia)

```bash
# Build
cargo build --release                    # workspace completo
cargo build -p heretic-daemon --release  # solo daemon

# Test
cargo test --workspace
cargo test -p heretic-core               # solo core
cargo test -p heretic-daemon             # solo daemon

# Lint
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

# Run
./target/release/fl-heretic.exe daemon   # arranca daemon
./target/release/fl-heretic.exe mcp      # arranca MCP server (Fase 2+)
./target/release/fl-heretic.exe doctor   # diagnóstico
./target/release/fl-heretic.exe token generate

# Examples
cargo run -p ping                        # cliente ping → daemon
```

### Variables de entorno
- `RUST_LOG=info` — nivel de tracing
- `FL_HERETIC_DATA_DIR` — override de `%LOCALAPPDATA%\fl-heretic` (default Windows)
- `FL_HERETIC_PIPE_NAME` — override de `\\.\pipe\fl-heretic-<pid>`
- `FL_HERETIC_TOKEN_PATH` — override del path del token

### Paths importantes
- Token: `%LOCALAPPDATA%\fl-heretic\token`
- Audit DB: `%LOCALAPPDATA%\fl-heretic\audit.db`
- Logs: `%LOCALAPPDATA%\fl-heretic\logs\`
- Project snapshots: `%LOCALAPPDATA%\fl-heretic\snapshots\<project-hash>\`

---

## 12. Referencias externas

- **FlojoMCP** (framework): `D:\Mis Juegos\ClaudeMCPs\FlojoMCP\`
  - `DESIGN.md` — diseño completo del framework
  - `AGENTS.md` — convenciones del framework (build.bat, Rust 2024, etc.)
  - `crates/flojo-mcp/` — runtime, errores, FlojoTester
  - `crates/flojo-macros/` — proc-macros `#[tool]`, `#[flojo_mcp]`
- **FLStudioMCP original** (legacy): preservado en `legacy/` o en git history
  - `AUDIT.md` — auditoría completa
  - `docs/` — hallazgos críticos del comportamiento de FL
- **FL Studio MIDI scripting**: https://www.image-line.com/fl-studio-download/fl-studio-2025/
- **MIDI SysEx spec**: https://www.midi.org/specifications

---

## 13. Notas de estilo (para futuras sesiones)

- **Español** para conversación, **inglés** para código/comments/docs.
- **Brutalmente honesto** en auditorías y reviews — no pintar nada bonito.
- **Reglas globales** de `C:\Users\Admin\.config\opencode\AGENTS.md` aplican.
- **Notación simbólica** (`→`, `⊘`, `⊕`, `∴`, `∀`, `∃`) cuando el documento esté compactado.
- **Términos en español cuando aporten** (`q/`, `c/`, `s/`, `p/`, `d/`, `nec`, `imp`, `crit`, `cfg`, `dep`, `env`).
- **Probar antes de declarar terminado.** Tests con `FlojoTester`-style + smoke tests reales contra el daemon.
- **Documentar descubrimientos** en §8 inmediatamente.
- **Una decisión por pregunta.** No apilar decisiones en una sola.