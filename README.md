# FL Heretic MCP

> **Daemon blindado + MCP server para FL Studio sobre FlojoMCP.**
> *Daemons blindados para un DAW amurallado.*

[![version](https://img.shields.io/badge/version-0.1.0--alpha-orange)](https://github.com/CerebroCanibalus/fl-heretic-mcp)
[![license](https://img.shields.io/badge/license-GPL--3.0-blue)](LICENSE)
[![rust](https://img.shields.io/badge/rust-2024-orange)](https://www.rust-lang.org)
[![platform](https://img.shields.io/badge/platform-Windows%2010%2F11-blue)](https://github.com/CerebroCanibalus/fl-heretic-mcp)

![FL Heretic MCP architecture](docs/architecture.png)

---

## ¿Qué es esto?

FL Heretic MCP es la reescritura en Rust del [FLStudioMCP](legacy/) original (Python + FastMCP + MIDI SysEx). El proyecto original tenía una arquitectura ingeniosa pero cargaba con 4 problemas estructurales que lo condenaban a ser un parche sobre parche:

1. **Detección de plugins primitiva** → solo lo que FL reporta en runtime, sin índice de librería.
2. **Detección de proyectos primitiva** → 7 campos. Sin parse del `.flp`. Sin samples.
3. **Transporte MIDI SysEx con techo de 1.5KB** → paginación obligatoria de TODO.
4. **Runtime Python pesado** → 50-76MB RAM, GIL, deploy con venv + native deps.

FL Heretic MCP ataca los 4 con un diseño en 3 capas, escrito en Rust sobre el framework [FlojoMCP](https://github.com/CerebroCanibalus/FlojoMCP):

```
[MCP server stdio]  ←FlojoMCP→  [Daemon blindado Named Pipe]  ←MIDI+parse→  [FL Studio]
     Rust 7MB              Rust mismo binary           FL Python sandbox + .flp
```

El daemon es el único proceso autorizado a tocar FL Studio. Autentica cada request del agente, rate-limitea, audita, supervisa FL con watchdog, cachea el proyecto en SQLite, y maneja reconexión tras crashes sin que el MCP server se entere.

## Estado actual

**Fase 0+1** (esta iteración): workspace Cargo + daemon básico con auth HMAC + audit log SQLite + Named Pipe server + comando `ping`.

Ver [`AGENTS.md`](AGENTS.md) §6 para el plan de fases completo.

## Quickstart

```powershell
# Clonar
git clone https://github.com/CerebroCanibalus/fl-heretic-mcp
cd fl-heretic-mcp

# Build
cargo build --release

# Generar token + iniciar daemon
target\release\fl-heretic.exe token generate
target\release\fl-heretic.exe daemon

# Probar conexión
cargo run -p ping
```

## Arquitectura

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
│   │   • VST3 FlojoBridge (FUTURO Phase 6)               │   │
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

## Estructura

```
FLHereticMCP/
├── Cargo.toml                  # workspace
├── AGENTS.md                   # memoria viva (leer primero)
├── AUDIT.md                    # auditoría del FLStudioMCP original
├── README.md                   # este archivo
├── LICENSE                     # GPL-3.0
├── .gitignore                  # Rust + secrets
├── crates/
│   ├── heretic-core/           # tipos compartidos, error, auth, audit, protocol
│   └── heretic-daemon/         # el daemon blindado
├── examples/
│   └── ping/                   # cliente mínimo
├── scripts/                    # installer PowerShell (próximamente)
├── docs/                       # hallazgos del FLStudioMCP original (referencia)
└── legacy/                     # código Python preservado (no se desarrolla)
```

## Roadmap

| Fase | Estado | Descripción |
|---|---|---|
| 0 — Setup | ✅ done | Auditoría, rename, workspace |
| 1 — Daemon básico | 🚧 en curso | Named Pipe + HMAC + audit + `ping` |
| 2 — Bridge MIDI | ⏳ | Controller script port + transport tools |
| 3 — Port completo | ⏳ | 67 tools del FLStudioMCP re-implementados |
| 4 — Features nuevas | ⏳ | Plugin indexer, .flp parser, Mix Doctor paralelo |
| 5 — Maduración | ⏳ | Rate limit, ACL, watchdog, prompts, docs |
| 6 — VST3 FlojoBridge | ⏳ futuro | Game changer: WebSocket sin cap de payload |

Ver `AGENTS.md` §6 para el detalle de cada fase.

## Stack

- **Rust 2024** + **Cargo workspace**
- [FlojoMCP](https://github.com/CerebroCanibalus/FlojoMCP) — framework MCP (path dep)
- [tokio](https://tokio.rs) — async runtime
- [rusqlite](https://github.com/rusqlite/rusqlite) — audit log SQLite
- [hmac](https://github.com/RustCrypto/MACs) + [sha2](https://github.com/RustCrypto/hashes) — Bearer tokens
- [clap](https://github.com/clap-rs/clap) — CLI subcommands
- [tracing](https://github.com/tokio-rs/tracing) — observabilidad
- (Fase 2) [midir](https://github.com/Boddlnagg/midir) — MIDI bridge

## Seguridad

El daemon implementa blindaje PRO:

- **Auth HMAC-SHA256 Bearer** — token generado al `init`, guardado con ACL Windows. Cada request firma el payload.
- **Rate limit por tool** (token bucket) — configurable por tool.
- **Audit log append-only** — SQLite WAL con triggers que bloquean UPDATE/DELETE. Retention 30 días.
- **Capability ACL** — lista de tools permitidas. Modos `safe` / `demo` / `readonly`.
- **Circuit breaker** — si FL se congela 3s, el daemon deja de enviar comandos.
- **Watchdog tokio** — supervisa FL alive, .pyscript armed, port conflicts.

## Documentación

- [`AGENTS.md`](AGENTS.md) — memoria viva del proyecto (decisiones, bugs, próximos pasos)
- [`AUDIT.md`](AUDIT.md) — auditoría brutal del FLStudioMCP original
- [`docs/`](docs/) — hallazgos críticos del comportamiento de FL (referencia histórica)
- [`legacy/`](legacy/) — código Python preservado, no se desarrolla

## Licencia

GPL-3.0-or-later — ver [`LICENSE`](LICENSE).

## Contribuir

Ver [`CONTRIBUTING.md`](CONTRIBUTING.md) (legacy, aún no actualizado al nuevo propósito).

---

> *Daemons blindados para un DAW amurallado.*