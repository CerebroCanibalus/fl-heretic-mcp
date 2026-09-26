# DAW Heretic MCP

MCP server en Rust para que un agente IA controle una DAW entera: crear
proyectos, pistas, meter plugins, escribir MIDI, mezclar y renderizar.

**Estado: andamiaje compilando.** Reaper 7.80 está instalado, pero el ReaScript
bridge todavía no se ha probado contra el DAW real. Ver `AGENTS.md` para el
estado exacto y los siguientes pasos.

## Por qué no FL Studio

FL Studio 2025 no expone API de proyecto: de las 79 constantes `midi.FPT_*`
solo hay `FPT_Save` y `FPT_SaveNew`, no hay forma de abrir, crear ni cerrar
proyectos, y su menú File es owner-draw. Su Python corre en un sandbox sin
file I/O ni red. Es la misma limitación que tiene Cubase.

La investigación completa, con medidas, está en `docs_fl_audit.md` y en el
historial (`git show 0930996`).

## Por qué Reaper

900+ funciones de API, control externo real, ReaScript con file I/O y red
libres, y varios MCPs existentes como base. $60 la licencia tras 60 días de
evaluación completa.

Los plugins nativos de FL Studio siguen accesibles: `FL Studio VSTi` carga FL
entero como VST2 dentro de Reaper, así que FLEX y el resto del bundle
continúan funcionando.

## Arquitectura

    opencode ──stdio MCP──> daw-heretic-mcp (FlojoMCP, Rust)
                              │ file-RPC
                              ▼
                         [Reaper] ←── ReaScript Lua

Sin daemon intermedio: el servidor MCP es el dueño del transporte.

## Tools

7, no 181. El MCP de referencia gasta ~22.400 tokens en schemas en cada
request; esta surface, ~1.800. `daw_do` sigue llegando a las 181 acciones del
DAW para lo que haga falta.

## Licencia

GPL-3.0-or-later
