# fl-studio-mcp — Guía de uso (Windows)

Servidor MCP instalado para controlar FL Studio 2025 desde OpenCode.
Proyecto: https://github.com/rosasynthesiz/flstudio-mcp — licencia MIT.

## Arquitectura instalada

```
OpenCode (MCP client)
  └── fl-studio-mcp.exe        (server stdio → TCP 127.0.0.1:9787)
        └── fl-studio-mcp-daemon (.vbs oculto → python -m fl_studio_mcp.daemon)
              └── loopMIDI (puertos virtuales FLStudioMCP RX / TX)
                    └── FL Studio 2025 (controller script FLStudioMCP)
```

- El **daemon** mantiene los puertos MIDI (TCP 127.0.0.1:9787) y se re-lanza en cada
  inicio de sesión (autostart vía `fl-studio-mcp-daemon.vbs` en la clave `Run`).
- El **server** se registra en `~/.config/opencode/opencode.jsonc` con
  `FLSTUDIO_MCP_TRANSPORT=tcp`.
- Sin daemon → las herramientas fallan. Relánzalo con
  `D:\Mis Juegos\ClaudeMCPs\FLStudioMCP\iniciar-daemon.bat`.

## Componentes instalados

| Componente | Ruta |
|---|---|
| Controller script | `Documents\Image-Line\FL Studio\Settings\Hardware\FLStudioMCP\device_FLStudioMCP.py` |
| Note-bridge (piano roll) | `Documents\Image-Line\FL Studio\Settings\Piano roll scripts\MCP_Apply.pyscript` |
| Repo | `D:\Mis Juegos\ClaudeMCPs\FLStudioMCP` |
| Server | `C:\Users\Admin\AppData\Roaming\Python\Python314\Scripts\fl-studio-mcp.exe` |
| Daemon | `fl-studio-mcp-daemon.vbs` (wrapper oculto → `python -m fl_studio_mcp.daemon`, TCP 9787) |
| Puertos MIDI | loopMIDI: `FLStudioMCP RX`, `FLStudioMCP TX` (autostart loopMIDI OK) |

## Overrides de entorno (opcionales)

- `FLSTUDIO_MCP_PORT_TO_FL` / `FLSTUDIO_MCP_PORT_FROM_FL` — nombres de puerto
- `FLSTUDIO_MCP_TCP_HOST` / `FLSTUDIO_MCP_TCP_PORT`
- `FLSTUDIO_MCP_PITCH_ENGINE=crepe|pyin|auto` — motor de transcripción
- `FLSTUDIO_MCP_PLUGIN_DB` / `FLSTUDIO_MCP_PRESETS` — rutas de librería/patches

## Límites reales de la API de FL Studio (no son bugs)

1. **No puede cargar plugins nuevos.** Solo controla plugins ya presentes en el proyecto
   (parámetros por nombre, presets). Para "usar todos tus VST": carga el plugin en
   FL y luego pide configurarlo/automatizarlo.
2. **No crea patterns desde cero.** Trabaja con patterns existentes o clona vía pyscript.
3. **Tiempo (tempo) a veces se ignora** si FL está en un diálogo modal.
4. Microtonal se redondea al semitono (limitación de MIDI de 12 tonos).

## Verificación

1. `fl_ping` → confirma bridge sano.
2. `fl_get_project_state` → lectura del proyecto.
3. `fl_diagnose_mix` → Mix Doctor sobre el proyecto abierto.

## Snippets de ejemplo

- "Configura una cadena vocal en la voz principal usando mis plugins" → `fl_setup_chain`
- "Dame un preset vintage de bass de Serum" → `fl_suggest_preset`
- "Escribe 8 compases de melodía en D Dorian en el canal seleccionado" → `fl_write_piano_roll_notes`
- "Mira mi mezcla y dime qué está mal" → `fl_diagnose_mix`
- "Exporta este arreglo a un archivo MIDI" → `fl_export_midi`