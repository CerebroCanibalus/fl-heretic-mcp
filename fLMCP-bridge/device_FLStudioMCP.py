# name=FL Heretic Bridge
# url=https://github.com/CerebroCanibalus/fl-heretic-mcp
# receiveFrom=FL Heretic Bridge
"""FL Heretic Bridge — controller script limpio para FL Heretic MCP.

Versión DEBLOATED del fLMCP Bridge (v0.2.0):
- Solo file-RPC (no TCP, no thread, no socket — VST3 plugin maneja IPC)
- Solo handlers esenciales (Fase 2: meta + transport + mixer + channels + plugins)
- Sin automation, sin piano roll, sin projects, sin UI
- ~600 líneas vs 2049 del original

El VST3 plugin (vst3-bridge/) actúa como PROXY:
- Recibe TCP del daemon
- Re-empaqueta como file-RPC a este script
- Devuelve la respuesta por TCP al daemon

OnIdle() procesa file-RPC. Mismo patrón que el bridge original.
"""

import json
import os
import sys
import time
import traceback
from pathlib import Path

# FL Studio API — disponible solo dentro de FL Studio.
try:
    import channels
    import device
    import general
    import midi
    import mixer
    import patterns
    import playlist
    import plugins
    import transport
    import ui
    _IN_FL = True
except ImportError:
    _IN_FL = False


# ============================================================================
# Config
# ============================================================================

def _script_dir():
    userprofile = os.environ.get("USERPROFILE", str(Path.home()))
    return Path(userprofile) / "Documents" / "Image-Line" / "FL Studio" / "Settings" / "Hardware" / "fLMCP Bridge"

SCRIPT_DIR = _script_dir()
RPC_REQ = SCRIPT_DIR / "rpc_request.json"
RPC_RESP = SCRIPT_DIR / "rpc_response.json"

BRIDGE_VERSION = "0.3.0-fl-heretic"
_started_at = time.monotonic()
_last_rpc_id = 0


# ============================================================================
# Helpers
# ============================================================================

def _log(msg):
    print(f"[FL Heretic Bridge] {msg}", flush=True)


def _safe(fn, *a, **kw):
    try:
        return fn(*a, **kw)
    except Exception:
        return None


def _color_to_int(color):
    if isinstance(color, bool):
        return 0
    if isinstance(color, int):
        return color
    if isinstance(color, (list, tuple)) and len(color) >= 3:
        r, g, b = int(color[0]), int(color[1]), int(color[2])
        return (r << 16) | (g << 8) | b
    s = str(color).strip()
    if s.startswith("#"):
        s = s[1:]
        if len(s) == 6:
            r = int(s[0:2], 16); g = int(s[2:4], 16); b = int(s[4:6], 16)
            return (r << 16) | (g << 8) | b
    try:
        return int(s, 0)
    except Exception:
        return 0


# ============================================================================
# Handlers — Fase 2: meta + transport + mixer + channels + plugins (subset)
# ============================================================================

def h_meta_ping(_):
    return {
        "ok": True,
        "bridge_version": BRIDGE_VERSION,
        "fl_version": _safe(general.getVersion) or "unknown",
        "uptime_sec": round(time.monotonic() - _started_at, 1),
        "script_dir": str(SCRIPT_DIR),
    }


def h_meta_info(_):
    return {
        "bridge_version": BRIDGE_VERSION,
        "fl_version": _safe(general.getVersion) or "unknown",
        "api_modules": ["transport", "mixer", "channels", "patterns",
                        "plugins", "general", "ui", "midi"],
        "note": "FL Heretic Bridge — version limpia para FL Heretic MCP",
    }


# ---- transport ----

def h_transport_start(_):
    transport.start()
    return {"is_playing": transport.isPlaying() == 1}


def h_transport_stop(_):
    transport.stop()
    return {"stopped": True}


def h_transport_status(_):
    return {
        "is_playing": transport.isPlaying() == 1,
        "is_recording": transport.isRecording() == 1,
        "position_ticks": transport.getSongPos(2),
        "position_bars": transport.getSongPos(3),
        "position_seconds": transport.getSongPos(1),
        "bpm": mixer.getCurrentTempo() / 1000.0,
    }


def h_transport_set_tempo(p):
    bpm = float(p.get("bpm", 140))
    if bpm < 10 or bpm > 999:
        raise ValueError(f"bpm fuera de rango: {bpm}")
    general.processRECEvent(
        midi.REC_Tempo,
        int(round(bpm * 1000)),
        midi.REC_Control | midi.REC_UpdateControl,
    )
    return {"bpm": mixer.getCurrentTempo() / 1000.0}


def h_transport_set_position(p):
    unit = p.get("unit", "bars")
    fl_unit = {"ms": 0, "seconds": 1, "ticks": 2, "bars": 3, "steps": 4}.get(unit, 3)
    transport.setSongPos(p.get("position", 0), fl_unit)
    return {"position_bars": transport.getSongPos(3)}


# ---- mixer ----

def h_mixer_set_volume(p):
    track = int(p.get("track", 0))
    value = float(p.get("value", 0.8))
    mixer.setTrackVolume(track, max(0.0, min(1.0, value)))
    return {"track": track, "vol_norm": mixer.getTrackVolume(track)}


def h_mixer_get_volume(p):
    track = int(p.get("track", 0))
    vol = mixer.getTrackVolume(track)
    return {
        "track": track,
        "vol_norm": vol,
        "vol_db": 20.0 * (vol / 0.8 - 1.0) if vol > 0 else -120.0,
    }


def h_mixer_set_pan(p):
    track = int(p.get("track", 0))
    value = float(p.get("value", 0.0))
    mixer.setTrackPan(track, max(-1.0, min(1.0, value)))
    return {"track": track, "pan": mixer.getTrackPan(track)}


def h_mixer_mute(p):
    track = int(p.get("track", 0))
    state = bool(p.get("state", True))
    if mixer.isTrackMuted(track) != (1 if state else 0):
        mixer.muteTrack(track)
    return {"track": track, "muted": bool(mixer.isTrackMuted(track))}


# ---- channels ----

def h_channels_list(_):
    out = []
    for i in range(channels.channelCount()):
        try:
            tgt = channels.getTargetFxTrack(i)
        except Exception:
            tgt = -1
        out.append({
            "index": i,
            "name": channels.getChannelName(i),
            "vol_norm": channels.getChannelVolume(i),
            "pan": channels.getChannelPan(i),
            "muted": bool(channels.isChannelMuted(i)),
            "target_fx_track": tgt,
        })
    return {"channels": out}


def h_channels_set_volume(p):
    idx = int(p.get("channel", 0))
    value = float(p.get("value", 0.8))
    channels.setChannelVolume(idx, max(0.0, min(1.0, value)))
    return {"channel": idx, "vol_norm": channels.getChannelVolume(idx)}


# ---- plugins (básico) ----

def h_plugins_name(p):
    track = int(p.get("track", 0))
    slot = int(p.get("slot", -1))
    if plugins.isValid(track, slot):
        return {"track": track, "slot": slot, "name": plugins.getPluginName(track, slot)}
    return {"track": track, "slot": slot, "name": None}


def h_plugins_get_param(p):
    track = int(p.get("track", 0))
    slot = int(p.get("slot", -1))
    param = int(p.get("param", 0))
    if not plugins.isValid(track, slot):
        return {"error": "no plugin"}
    return {
        "track": track, "slot": slot, "param": param,
        "name": plugins.getParamName(param, track, slot),
        "value": plugins.getParamValue(param, track, slot),
        "value_str": plugins.getParamValueString(param, track, slot) or "",
    }


def h_plugins_set_param(p):
    track = int(p.get("track", 0))
    slot = int(p.get("slot", -1))
    param = int(p.get("param", 0))
    value = float(p.get("value", 0.0))
    if not plugins.isValid(track, slot):
        return {"error": "no plugin"}
    plugins.setParamValue(value, param, track, slot)
    return {"track": track, "slot": slot, "param": param,
            "value": plugins.getParamValue(param, track, slot)}


# ============================================================================
# Dispatch
# ============================================================================

HANDLERS = {
    "meta.ping": h_meta_ping,
    "meta.info": h_meta_info,
    "transport.start": h_transport_start,
    "transport.stop": h_transport_stop,
    "transport.status": h_transport_status,
    "transport.set_tempo": h_transport_set_tempo,
    "transport.set_position": h_transport_set_position,
    "mixer.set_volume": h_mixer_set_volume,
    "mixer.get_volume": h_mixer_get_volume,
    "mixer.set_pan": h_mixer_set_pan,
    "mixer.mute": h_mixer_mute,
    "channels.list": h_channels_list,
    "channels.set_volume": h_channels_set_volume,
    "plugins.name": h_plugins_name,
    "plugins.get_param": h_plugins_get_param,
    "plugins.set_param": h_plugins_set_param,
}


def _execute(action, params):
    h = HANDLERS.get(action)
    if h is None:
        raise ValueError(f"unknown action: {action}")
    return h(params or {})


# ============================================================================
# File-RPC pump (corre en OnIdle)
# ============================================================================

def _pump_file_rpc():
    global _last_rpc_id
    try:
        raw = RPC_REQ.read_text(encoding="utf-8")
    except FileNotFoundError:
        return
    except Exception as e:
        _log(f"file-RPC read error: {e}")
        return
    if not raw.strip():
        return
    try:
        req = json.loads(raw)
    except Exception:
        return  # partial write
    req_id = req.get("id", 0)
    if not req_id or req_id == _last_rpc_id:
        return
    _last_rpc_id = req_id
    action = req.get("action", "")
    params = req.get("params", {}) or {}
    try:
        result = _execute(action, params)
        resp = {"id": req_id, "ok": True, "result": result}
    except Exception as e:
        resp = {"id": req_id, "ok": False,
                "error": f"{type(e).__name__}: {e}",
                "traceback": traceback.format_exc(limit=3)}
        _log(f"action {action} error: {e}")
    try:
        RPC_RESP.write_text(json.dumps(resp), encoding="utf-8")
    except Exception as e:
        _log(f"file-RPC write resp failed: {e}")


# ============================================================================
# FL Studio lifecycle callbacks
# ============================================================================

def OnInit():
    global _started_at
    _started_at = time.monotonic()
    _log(f"v{BRIDGE_VERSION} ready (file-RPC @ {SCRIPT_DIR})")


def OnDeInit():
    _log("shutting down")


def OnIdle():
    _pump_file_rpc()
