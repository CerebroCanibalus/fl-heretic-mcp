# FL Heretic Bridge — VST3 plugin

Plugin VST3 que expone la FL Studio API al daemon FL Heretic MCP via TCP JSON-RPC en `127.0.0.1:9790`.

## Build (Windows)

### Requisitos

- **Visual Studio 2022 Build Tools** (MSVC, CMake) — [descargar](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022)
- **CMake 3.20+** — [descargar](https://cmake.org/download/)
- **VST3 SDK 3.7.7** — se descarga automáticamente via CMake FetchContent

### Compilar

```powershell
cd vst3-bridge
cmake -S . -B build -G "Visual Studio 17 2022" -A x64
cmake --build build --config Release
```

Output: `build/Release/FL Heretic Bridge.vst3`

### Instalar

El instalador automático (`scripts/install_windows.ps1`) copia el plugin a `%COMMONPROGRAMFILES%\VST3\`.

Si lo haces manualmente:
```powershell
Copy-Item "build\Release\FL Heretic Bridge.vst3" "$env:COMMONPROGRAMFILES\VST3\"
```

## Cargar en FL Studio (una vez)

1. Abrir FL Studio 2025
2. Menú **Options → Manage plugins**
3. Click **Start scan** (FL debe detectar el plugin en `%COMMONPROGRAMFILES%\VST3\`)
4. Confirmar que "FL Heretic Bridge" aparece con check ✓
5. Cerrar el manager

## Usar el plugin

Una vez escaneado, el plugin aparece en el navegador de FL:
- **Instrumentos**: arrastrar a un slot del Channel Rack
- **Effects**: arrastrar a un slot del Mixer (cualquier insert slot)

El plugin es un **Fx pass-through**: el audio pasa intacto, sin latencia añadida.

Al cargar el plugin, **abre un servidor TCP en `127.0.0.1:9790`** automáticamente. El daemon FL Heretic MCP se conecta a este puerto para hablar con FL.

## Protocolo

JSON-RPC sobre TCP con framing `[BE u32 length][body JSON]` (compatible con fLMCP Bridge).

Request:
```json
{"id": 1, "action": "meta.ping", "params": {}}
```

Response:
```json
{"id": 1, "ok": true, "result": {"ok": true, "bridge_version": "0.3.0", ...}}
```

## Actions implementadas (Fase 2 MVP)

| Action | Handler | Estado |
|---|---|---|
| `meta.ping` | meta::ping | ✅ |
| `meta.info` | meta::info | ✅ |
| `transport.start` | transport::start | ✅ (mock — acceso real FL API en Fase 3) |
| `transport.stop` | transport::stop | ✅ (mock) |
| `transport.status` | transport::status | ✅ (mock) |
| `transport.set_tempo` | transport::set_tempo | ✅ (mock) |
| `transport.set_position` | transport::set_position | ✅ (mock) |
| `mixer.set_volume` | mixer::set_volume | ✅ (mock) |
| `mixer.get_peaks` | mixer::get_peaks | ✅ (placeholder) |

## Limitaciones actuales (a resolver en Fase 3)

- **Sin acceso directo a FL API** — los plugins VST3 NO pueden llamar FL API directamente. Por ahora los handlers devuelven datos simulados.
- **Audio pass-through** — el plugin no procesa audio, solo lo pasa.
- **Sin GUI** — el plugin es headless (no ventana de configuración).

## Licencia

GPL-3.0-or-later