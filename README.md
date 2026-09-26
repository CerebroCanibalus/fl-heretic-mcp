# DAW Heretic MCP

Daemon blindado + MCP server para controlar un DAW desde un agente IA.

**Estado: scaffolding.** Solo REAPER, y sin bridge configurado todavia.
Ver `AGENTS.md` para el estado real, el analisis de por que se abandono
FL Studio, y los siguientes pasos.

## Por que no FL Studio

FL Studio 2025 no expone API de proyecto: no hay forma de abrir, crear ni
cerrar proyectos desde su API, y su menu File es owner-draw. Su Python va en
un sandbox sin file I/O ni red. La misma limitacion que tiene Cubase.

La investigacion completa, con las medidas, esta en el commit `0930996` del
historial.

## Por que REAPER

900+ funciones de API, control externo real (TCP o file-RPC), ReaScript con
file I/O y red libres, y varios MCPs existentes que se pueden tomar como
base. Los plugins nativos de FL Studio siguen accesibles cargando
`FL Studio VSTi` como VST2 dentro de Reaper.

## Arquitectura

    MCP (FlojoMCP, Rust)  ->  daemon blindado  ->  file-RPC  ->  Reaper

La capa de blindaje (auth HMAC, audit log SQLite, rate limit) vive en
`heretic-core` y es lo que ningun MCP de Reaper tiene.

## Licencia

GPL-3.0-or-later
