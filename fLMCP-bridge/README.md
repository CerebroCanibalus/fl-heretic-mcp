# FL Heretic Bridge — controller script limpio

Fork DEBLOATED del `fLMCP Bridge v0.2.0`, específico para **FL Heretic MCP daemon**.

## Qué es

Un controller script Python que corre dentro de FL Studio (en el sandbox del controller script MIDI). Expone **~17 handlers** del FL API (transport, mixer, channels, plugins, meta) via **file-RPC** sobre `rpc_request.json` / `rpc_response.json`.

## Diferencias con fLMCP Bridge original

| | fLMCP Bridge v0.2.0 | FL Heretic Bridge |
|---|---|---|
| Líneas | 2049 | **~300** |
| Transports | TCP + file-RPC + auto-thread | **Solo file-RPC** |
| Handlers | 133 | **~17** (Fase 2 essentials) |
| Categorías | 12 (todo) | **5** (meta, transport, mixer, channels, plugins) |
| Automation, projects, UI, playlist, arrangement | Sí | **No** (Fase 3+) |
| TCP server | Sí (falla en FL 2025 sandbox) | **No** (lo hace el VST3 plugin) |
| threading | Sí (falla) | **No** (single-thread OnIdle) |

## Arquitectura

```
[daemon Rust] ←─ TCP ─→ [VST3 plugin] ←─ file-RPC ─→ [ESTE BRIDGE]
                                                    ↓
                                                FL API (OnIdle)
```

El VST3 plugin actúa como **proxy TCP↔file-RPC**:
- Recibe TCP del daemon (en 127.0.0.1:9790)
- Re-empaqueta como file-RPC a este bridge
- Devuelve la respuesta por TCP al daemon

El bridge solo procesa en `OnIdle()` — la única función que FL llama automáticamente.

## Handlers implementados (Fase 2)

| Action | Params | Returns |
|---|---|---|
| `meta.ping` | — | `{bridge_version, fl_version, uptime_sec, script_dir}` |
| `meta.info` | — | `{bridge_version, fl_version, api_modules}` |
| `transport.start` | — | `{is_playing}` |
| `transport.stop` | — | `{stopped}` |
| `transport.status` | — | `{is_playing, is_recording, position_*, bpm}` |
| `transport.set_tempo` | `{bpm}` | `{bpm}` |
| `transport.set_position` | `{position, unit}` | `{position_bars}` |
| `mixer.set_volume` | `{track, value}` | `{track, vol_norm}` |
| `mixer.get_volume` | `{track}` | `{track, vol_norm, vol_db}` |
| `mixer.set_pan` | `{track, value}` | `{track, pan}` |
| `mixer.mute` | `{track, state}` | `{track, muted}` |
| `channels.list` | — | `{channels: [{...}]}` |
| `channels.set_volume` | `{channel, value}` | `{channel, vol_norm}` |
| `plugins.name` | `{track, slot}` | `{track, slot, name}` |
| `plugins.get_param` | `{track, slot, param}` | `{name, value, value_str}` |
| `plugins.set_param` | `{track, slot, param, value}` | `{value}` |

Fase 3 añadirá: channels (CRUD), patterns, playlist, automation, project, ui, pianoroll.

## Instalación

**Automática** vía `scripts/install_windows.ps1` (Phase 2.5).

**Manual** (no recomendada):
1. Copiar este directorio a `%USERPROFILE%\Documents\Image-Line\FL Studio\Settings\Hardware\fLMCP Bridge\`
2. Sobrescribir `device_FLStudioMCP.py` con el de este repo
3. Reiniciar FL Studio
4. En Options → MIDI Settings: el script `FL Heretic Bridge` debe aparecer

## Protocolo (file-RPC)

Request (escrito por el VST3 plugin a `rpc_request.json`):
```json
{"id": 12345, "action": "meta.ping", "params": {}}
```

Response (escrito por este bridge a `rpc_response.json`):
```json
{"id": 12345, "ok": true, "result": {"bridge_version": "0.3.0-fl-heretic", ...}}
```

**Regla del id**: estrictamente creciente. El bridge solo procesa requests con `id > _last_rpc_id`. Si reusas un id viejo, lo ignora.

## Licencia

GPL-3.0-or-later (mismo que FL Heretic MCP).