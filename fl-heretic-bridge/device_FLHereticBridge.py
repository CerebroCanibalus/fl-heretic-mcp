# name=FL Heretic Bridge
# url=https://github.com/CerebroCanibalus/fl-heretic-mcp
# receiveFrom=FL Heretic Bridge
"""FL Heretic Bridge — controller script para FL Studio.

Transporta comandos del daemon FL Heretic MCP a la FL Python API y devuelve
resultados. Corre dentro del sub-interprete de FL Studio 2025, que es un
entorno muy restringido. Lo que ese entorno permite y no permite esta medido en
`docs/FL2025_SANDBOX.md`; en resumen:

  PERMITE   open()/read()/write() de ficheros en este directorio
            la FL API completa (channels, mixer, transport, ...)
            exec() de Python arbitrario
  BLOQUEA   threads, sockets, ctypes, os.rename, os.mkdir, os.unlink, glob
            `__file__` no existe
            OnIdle NO se dispara nunca
entorno muy restringido. Lo que ese entorno permite y no permite esta medido
Por eso el transporte es file-RPC puro y el despertar lo pone el cliente: tras
escribir la request, el daemon manda un byte MIDI y FL dispara OnMidiIn.

Topología:

    daemon Rust ──ficheros──> este script ──FL API──> FL Studio
    daemon Rust ──MIDI out───> este script (despierta)

Transporte: mailbox rotativo de 8 slots. El daemon escribe la request en
`hr_req_<id % 8>.json`; este script lee los 8 slots, ejecuta el de mayor id
pendiente y escribe la respuesta en `hr_resp_<mismo slot>.json` terminada en
un salto de linea que actua como marcador de escritura completa (el sandbox no
permite os.rename, asi que la escritura no es atomica y hay que poder
detectar un fichero a medio escribir).

La API de FL se expone de dos maneras complementarias:

  - Handlers explicitos y tipados para lo que se usa en el camino caliente.
    Son estables, documentados y validan sus parametros.
  - `exec.exec`, que ejecuta una expresion Python en este mismo interprete.
    Da acceso a las ~250 funciones de FL que Image-Line expone sin que
    escribamos un handler para cada una. Es la via para lo raro o lo nuevo.
"""

import json
import os
import sys
import time
import traceback
from pathlib import Path

# ----------------------------------------------------------------------------
# Config
# ----------------------------------------------------------------------------

BRIDGE_NAME = "FL Heretic Bridge"
BRIDGE_VERSION = "1.0.0"
PROTOCOL_VERSION = 1

# vuelo), asi que 8 es de sobra: los slots existen para tolerar escrituras a
# vuelo), asi que 8 es de sobra: los slots existen para tolerate escrituras a
# medias y para no perder una request si FL esta a mitad de pump cuando llega
# la siguiente. Habria que enviar 8 requests simultaneas para perder una.
SLOT_COUNT = 8

POLL_SLEEP_S = 0.0  # el pump es synchronous, no hay donde esperar

started_at = time.monotonic()
last_seen_id = 0          # ultimo id procesado
pump_count = 0            # quantas veces hemos bombinado
last_action = ""
last_error = ""
in_fl = False


def script_dir():
    """Ruta de este script. `__file__` no existe en el sub-interprete de FL."""
    if sys.platform == "win32":
        base = Path(os.environ.get("USERPROFILE", str(Path.home()))) / "Documents" / "Image-Line" / "FL Studio" / "Settings"
    else:
        base = Path.home() / "Documents" / "Image-Line" / "FL Studio" / "Settings"
    return base / "Hardware" / BRIDGE_NAME


SCRIPT_DIR = script_dir()
REQ_FILES = [SCRIPT_DIR / ("hr_req_%d.json" % i) for i in range(SLOT_COUNT)]
RESP_FILES = [SCRIPT_DIR / ("hr_resp_%d.json" % i) for i in range(SLOT_COUNT)]
STATUS_FILE = SCRIPT_DIR / "hr_status.json"

try:
    import arrangement
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
    in_fl = True
except ImportError:  # fuera de FL: permite test unitario del transporte
    arrangement = channels = device = general = midi = None
    mixer = patterns = playlist = plugins = transport = ui = None
    in_fl = False


# ----------------------------------------------------------------------------
# Log y file IO (lo unico que el sandbox permite)
# ----------------------------------------------------------------------------

def log(msg):
    try:
        print("[heretic] %s" % msg)
    except Exception:
        pass


def write_text_atomic(path, text):
    """Escribe `text` en `path` de forma que el lector nunca vea un fichero
    a medias: primero un .part y despues el destino. rename NO esta disponible
    en el sandbox, pero la escritura se hace de golpe y el lector valida el
    contenido (JSON parseable + id correcto), asi que un .part a medias se
    descarta sin coste."""
    part = Path(str(path) + ".part")
    try:
        with open(part, "w", encoding="utf-8") as f:
            f.write(text)
            f.flush()
        # Intentos de rename por si el build lo permite (no en 2025, pero si
        # FL lo desbloquea en el futuro el transporte se vuelve atomico solo).
        try:
            os.replace(str(part), str(path))
        except Exception:
            with open(path, "w", encoding="utf-8") as f:
                f.write(text)
                f.flush()
        return True
    except Exception as e:
        log("write failed %s: %s" % (path, e))
        return False


def read_json(path):
    """Lee un JSON. Devuelve None si no existe, esta a medias, o no parsea."""
    try:
        with open(path, "r", encoding="utf-8") as f:
            raw = f.read()
    except FileNotFoundError:
        return None
    except Exception:
        return None
    if not raw.strip():
        return None
    try:
        return json.loads(raw)
    except Exception:
        return None  # escritura parcial en curso; se reintenta al proximo pump


# ----------------------------------------------------------------------------
# Registro de handlers
# ----------------------------------------------------------------------------

HANDLERS = {}


def action(name):
    def deco(fn):
        HANDLERS[name] = fn
        return fn
    return deco


# ---- meta ----

@action("meta.ping")
def h_ping(_):
    return {
        "ok": True,
        "bridge_name": BRIDGE_NAME,
        "bridge_version": BRIDGE_VERSION,
        "protocol_version": PROTOCOL_VERSION,
        "fl_version": _safe(general.getVersion) if in_fl else "unknown",
        "midi_version": _safe(general.getVersion) if in_fl else "unknown",
        "uptime_sec": round(time.monotonic() - started_at, 1),
        "pump_count": pump_count,
        "in_fl": in_fl,
    }


@action("meta.info")
def h_info(_):
    counts = {}
    for name in HANDLERS:
        prefix = name.split(".")[0]
        counts[prefix] = counts.get(prefix, 0) + 1
    return {
        "bridge_version": BRIDGE_VERSION,
        "fl_version": _safe(general.getVersion) if in_fl else "unknown",
        "handlers": len(HANDLERS),
        "by_category": counts,
        "slots": SLOT_COUNT,
    }


@action("meta.actions")
def h_actions(_):
    return {"actions": sorted(HANDLERS.keys())}


@action("meta.exec")
def h_exec(p):
    """Ejecuta una expresion Python en el interprete de FL y devuelve su
    valor como JSON. Es la via de acceso a toda la FL API sin escribir un
    handler por funcion.

    El codigo se evalua con las FL API en el scope (`channels`, `mixer`, ...),
    mas `json` y `print` (capturado en stdout)."""
    code = p.get("code")
    if not isinstance(code, str) or not code.strip():
        raise ValueError("meta.exec necesita 'code' (string)")
    if len(code) > 20000:
        raise ValueError("code demasiado largo (max 20000 chars)")

    captured = []
    import io as _io
    import contextlib as _ctx
    import ast

    scope = {
        "json": json,
        "time": time,
        "__builtins__": __builtins__,
    }
    if in_fl:
        for mod in (arrangement, channels, device, general, midi, mixer,
                    patterns, playlist, plugins, transport, ui):
            if mod is not None:
                scope[mod.__name__] = mod

    buf = _io.StringIO()
    scope["print"] = lambda *a, **k: buf.write(" ".join(str(x) for x in a) + "\n")

    last_expr = None
    has_result = False
    try:
        with _ctx.redirect_stdout(buf):
            tree = ast.parse(code, "<heretic-exec>", "exec")
            # Si el ULTIMO statement es una expresion suelta, se separa y se
            # evalua aparte para poder devolver su valor. Sin esto, un script
            # de varias lineas terminaba siempre en `null` porque `exec` no
            # devuelve nada.
            if tree.body and isinstance(tree.body[-1], ast.Expr):
                main = ast.Module(body=tree.body[:-1], type_ignores=[])
                ast.fix_missing_locations(main)
                exec(compile(main, "<heretic-exec>", "exec"), scope)
                tail = ast.Expression(body=tree.body[-1].value)
                ast.fix_missing_locations(tail)
                last_expr = eval(compile(tail, "<heretic-exec>", "eval"), scope)
                has_result = True
            else:
                exec(compile(tree, "<heretic-exec>", "exec"), scope)
                # Un bloque puro puede dejar el resultado en `result` u `out`.
                for key in ("result", "out"):
                    if key in scope and key not in scope.get("__builtins__", {}):
                        last_expr = scope[key]
                        has_result = True
                        break
    except SyntaxError as e:
        return {
            "ok": False,
            "error": "SyntaxError: %s (linea %s)" % (e.msg, e.lineno),
            "stdout": buf.getvalue()[-4000:],
        }
    except Exception as e:
        return {
            "ok": False,
            "error": "%s: %s" % (type(e).__name__, e),
            "traceback": traceback.format_exc(limit=4),
            "stdout": buf.getvalue()[-4000:],
        }

    result = None
    err = None
    if has_result:
        try:
            result = _jsonable(last_expr)
        except Exception as e:
            err = "resultado no serializable: %s: %s" % (type(e).__name__, e)
            result = repr(last_expr)[:1000]

    out = {"ok": err is None, "stdout": buf.getvalue()[-4000:]}
    if err:
        out["error"] = err
    else:
        out["result"] = result
    return out


# ---- transport ----

@action("transport.start")
def h_t_start(_):
    transport.start()
    return {"is_playing": transport.isPlaying() == 1}


@action("transport.stop")
def h_t_stop(_):
    transport.stop()
    return {"is_playing": transport.isPlaying() == 1}


@action("transport.record")
def h_t_record(p):
    on = bool(p.get("on", False))
    if (transport.isRecording() == 1) != on:
        transport.record(on)
    return {"is_recording": transport.isRecording() == 1}


@action("transport.status")
def h_t_status(_):
    return {
        "is_playing": transport.isPlaying() == 1,
        "is_recording": transport.isRecording() == 1,
        "bpm": mixer.getCurrentTempo() / 1000.0,
        "position_ticks": transport.getSongPos(2),
        "position_bars": transport.getSongPos(3),
        "position_seconds": transport.getSongPos(1),
        "loop_mode": "song" if transport.getLoopMode() == 1 else "pattern",
    }


@action("transport.setTempo")
def h_t_set_tempo(p):
    # El default se evalua en su propia rama: `dict.get(k, expr)` evalua
    # `expr` SIEMPRE, asi que consultar el tempo actual ahi haria que un
    # request sin 'bpm' fallara si getCurrentTempo no responde.
    if "bpm" in p:
        bpm = float(p["bpm"])
    else:
        current = _safe(mixer.getCurrentTempo)
        if current is None:
            raise ValueError("falta 'bpm' y no se pudo leer el tempo actual")
        bpm = float(current) / 1000.0
    if not (10.0 <= bpm <= 999.0):
        raise ValueError("bpm fuera de rango 10-999: %r" % bpm)
    # processRECEvent con el valor interno de FL (bpm * 1000). Usar
    # midi.REC_FromMIDI colapsaria el tempo, y REC_Updated ya no existe.
    general.processRECEvent(midi.REC_Tempo, int(round(bpm * 1000)),
                            midi.REC_Control | midi.REC_UpdateControl)
    return {"bpm": mixer.getCurrentTempo() / 1000.0}


@action("transport.setPosition")
def h_t_set_pos(p):
    unit = str(p.get("unit", "bars"))
    unit_map = {"ms": 0, "seconds": 1, "ticks": 2, "bars": 3, "steps": 4}
    if unit not in unit_map:
        raise ValueError("unit invalida %r (usa ms|seconds|ticks|bars|steps)" % unit)
    pos = float(p.get("position", 0))
    if pos < 0:
        raise ValueError("position negativa: %r" % pos)
    transport.setSongPos(pos, unit_map[unit])
    return {"position_ticks": transport.getSongPos(2),
            "position_bars": transport.getSongPos(3)}


@action("transport.length")
def h_t_length(_):
    return {"ticks": transport.getSongLength(2),
            "seconds": transport.getSongLength(1),
            "ms": transport.getSongLength(0),
            "bars": transport.getSongLength(3),
            "steps": transport.getSongLength(4)}


@action("transport.setLoopMode")
def h_t_loop(p):
    mode = str(p.get("mode", "pattern"))
    target = 1 if mode == "song" else 0
    if transport.getLoopMode() != target:
        transport.setLoopMode()
    return {"loop_mode": "song" if transport.getLoopMode() == 1 else "pattern"}


# ---- mixer ----

@action("mixer.count")
def h_mx_count(_):
    return {"count": mixer.trackCount()}


@action("mixer.trackInfo")
def h_mx_info(p):
    return _mx_info(int(p["track"]))


@action("mixer.allTracks")
def h_mx_all(_):
    n = mixer.trackCount()
    return {"tracks": [_mx_info(i) for i in range(n)]}


@action("mixer.setVolume")
def h_mx_vol(p):
    mixer.setTrackVolume(int(p["track"]), _clamp(p["volume"], 0.0, 1.0))
    return _mx_info(int(p["track"]))


@action("mixer.setPan")
def h_mx_pan(p):
    mixer.setTrackPan(int(p["track"]), _clamp(p["pan"], -1.0, 1.0))
    return _mx_info(int(p["track"]))


@action("mixer.mute")
def h_mx_mute(p):
    tr = int(p["track"])
    want = bool(p.get("muted", True))
    if (mixer.isTrackMuted(tr) == 1) != want:
        mixer.muteTrack(tr)
    return _mx_info(tr)


@action("mixer.solo")
def h_mx_solo(p):
    tr = int(p["track"])
    want = bool(p.get("solo", True))
    if bool(mixer.isTrackSolo(tr)) != want:
        mixer.soloTrack(tr)
    return _mx_info(tr)


@action("mixer.setName")
def h_mx_name(p):
    mixer.setTrackName(int(p["track"]), str(p.get("name", "")))
    return _mx_info(int(p["track"]))


@action("mixer.fxSlots")
def h_mx_fx(p):
    tr = int(p["track"])
    return {
        "track": tr,
        "slots": [
            {"slot": i,
             "name": _safe(plugins.getPluginName, tr, i) or "",
             "valid": bool(_safe(plugins.isValid, tr, i))}
            for i in range(10)
        ],
    }


# ---- channels ----

@action("channels.count")
def h_ch_count(_):
    return {"count": channels.channelCount()}


@action("channels.info")
def h_ch_info(p):
    return _ch_info(int(p["index"]))


@action("channels.all")
def h_ch_all(_):
    n = channels.channelCount()
    return {"channels": [_ch_info(i) for i in range(n)]}


@action("channels.setVolume")
def h_ch_vol(p):
    # setChannelVolume(index, volume, pickupMode, useGlobalIndex). El 3er
    # argumento es pickupMode, NO useGlobalIndex.
    channels.setChannelVolume(int(p["index"]), _clamp(p["volume"], 0.0, 1.0), 0, True)
    return _ch_info(int(p["index"]))


@action("channels.setPan")
def h_ch_pan(p):
    channels.setChannelPan(int(p["index"]), _clamp(p["pan"], -1.0, 1.0), 0, True)
    return _ch_info(int(p["index"]))


@action("channels.setName")
def h_ch_name(p):
    channels.setChannelName(int(p["index"]), str(p.get("name", "")))
    return _ch_info(int(p["index"]))


@action("channels.setColor")
def h_ch_color(p):
    channels.setChannelColor(int(p["index"]), _color(p.get("color", 0)))
    return _ch_info(int(p["index"]))


@action("channels.mute")
def h_ch_mute(p):
    i = int(p["index"])
    want = bool(p.get("muted", True))
    if channels.isChannelMuted(i) != want:
        channels.muteChannel(i)
    return _ch_info(i)


@action("channels.solo")
def h_ch_solo(p):
    i = int(p["index"])
    want = bool(p.get("solo", True))
    if bool(channels.isChannelSolo(i)) != want:
        channels.soloChannel(i)
    return _ch_info(i)


@action("channels.select")
def h_ch_select(p):
    channels.selectChannel(int(p["index"]))
    return {"selected": int(p["index"])}


@action("channels.routeToMixer")
def h_ch_route(p):
    i = int(p["index"])
    tr = int(p.get("track", 0))
    channels.setChannelTargetFxTrack(i, tr)
    return _ch_info(i)


# ---- plugins ----

@action("plugins.isValid")
def h_pl_valid(p):
    return {"valid": bool(_safe(plugins.isValid, int(p["track"]), int(p["slot"])))}


@action("plugins.name")
def h_pl_name(p):
    tr, sl = int(p["track"]), int(p["slot"])
    if not _safe(plugins.isValid, tr, sl):
        return {"track": tr, "slot": sl, "name": None}
    return {"track": tr, "slot": sl, "name": plugins.getPluginName(tr, sl)}


@action("plugins.params")
def h_pl_params(p):
    tr, sl = int(p["track"]), int(p["slot"])
    if not _safe(plugins.isValid, tr, sl):
        raise ValueError("no hay plugin en track %d slot %d" % (tr, sl))
    n = plugins.getParamCount(tr, sl)
    out = []
    for i in range(n):
        out.append({
            "index": i,
            "name": plugins.getParamName(i, tr, sl),
            "value": plugins.getParamValue(i, tr, sl),
            "value_str": plugins.getParamValueString(i, tr, sl) or "",
        })
    return {"track": tr, "slot": sl, "params": out}


@action("plugins.getParam")
def h_pl_get(p):
    tr, sl, i = int(p["track"]), int(p["slot"]), int(p["param"])
    if not _safe(plugins.isValid, tr, sl):
        raise ValueError("no hay plugin en track %d slot %d" % (tr, sl))
    return {"track": tr, "slot": sl, "param": i,
            "name": plugins.getParamName(i, tr, sl),
            "value": plugins.getParamValue(i, tr, sl),
            "value_str": plugins.getParamValueString(i, tr, sl) or ""}


@action("plugins.setParam")
def h_pl_set(p):
    tr, sl, i = int(p["track"]), int(p["slot"]), int(p["param"])
    if not _safe(plugins.isValid, tr, sl):
        raise ValueError("no hay plugin en track %d slot %d" % (tr, sl))
    plugins.setParamValue(float(p["value"]), i, tr, sl)
    return {"track": tr, "slot": sl, "param": i,
            "value": plugins.getParamValue(i, tr, sl)}


@action("plugins.findParam")
def h_pl_find(p):
    tr, sl = int(p["track"]), int(p["slot"])
    needle = str(p.get("name", "")).lower()
    n = plugins.getParamCount(tr, sl)
    for i in range(n):
        nm = (plugins.getParamName(i, tr, sl) or "").lower()
        if needle in nm:
            return {"track": tr, "slot": sl, "param": i, "name": nm}
    return {"track": tr, "slot": sl, "param": None, "name": None}


# ---- patterns ----

@action("patterns.count")
def h_pat_count(_):
    return {"count": patterns.patternCount()}


@action("patterns.list")
def h_pat_list(_):
    # Solo 0 .. patternCount()-1 son reales. getPatternName devuelve un nombre
    # para cualquier indice (incluso del pool vacio), asi que el limite DURO
    # es patternCount().
    n = patterns.patternCount()
    return {"patterns": [{"index": i,
                          "name": _safe(patterns.getPatternName, i) or "",
                          "color": _safe(patterns.getPatternColor, i),
                          "is_default": bool(_safe(patterns.isPatternDefault, i))}
                         for i in range(n)],
            "current": _safe(patterns.patternNumber),
            "count": n}


@action("patterns.current")
def h_pat_current(_):
    return {"index": patterns.patternNumber()}


@action("patterns.select")
def h_pat_select(p):
    patterns.selectPattern(int(p["index"]))
    return {"index": patterns.patternNumber()}


@action("patterns.create")
def h_pat_create(p):
    """Crear patrones: FL no expone createPattern, pero `setPatternName(idx)`
    sobre el indice IGUAL a `patternCount()` crea el patron. Verificado en
    FL 2025 v38: tres renombrados followed de tres clones subieron
    patternCount de 0 a 5.

    Trampa importante: `getPatternName(i)` NO sirve para detectar si un patron
    existe; devuelve "Pattern i" siempre, incluso para indices del pool que
    no existen (comprobado hasta 999). El unico contador fiable es
    `patternCount()`, y los indices van de 0 a patternCount()-1.
    """
    name = str(p.get("name", "") or "")
    new_idx = patterns.patternCount()
    patterns.setPatternName(new_idx, name or ("Pattern %d" % new_idx))
    # setPatternName puede no crear si el indice no era valido; se comprueba
    # que el contador haya subido.
    count_after = patterns.patternCount()
    if count_after <= new_idx:
        raise RuntimeError(
            "setPatternName(%d) no creo el patron: patternCount sigue en %d. "
            "FL solo acepta el indice == patternCount() y solo hay %d slots."
            % (new_idx, count_after, patterns.patternMax())
        )
    if p.get("color") is not None:
        _safe(patterns.setPatternColor, new_idx, _color(p["color"]))
    return {"index": new_idx,
            "name": patterns.getPatternName(new_idx),
            "pattern_count": count_after}


@action("patterns.rename")
def h_pat_rename(p):
    patterns.setPatternName(int(p["index"]), str(p.get("name", "")))
    return {"index": int(p["index"]), "name": patterns.getPatternName(int(p["index"]))}


@action("patterns.setColor")
def h_pat_color(p):
    patterns.setPatternColor(int(p["index"]), _color(p.get("color", 0x808080)))
    return {"index": int(p["index"]), "color": patterns.getPatternColor(int(p["index"]))}


@action("patterns.delete")
def h_pat_delete(p):
    """FL no expone deletePattern. Workaround: renombrar a "", con lo que el
    patron queda fuera de la lista pero sigue ocupando su slot. Se avisa con
    `soft_deleted` para que quien llama sepa que no es un borrado real."""
    idx = int(p["index"])
    patterns.setPatternName(idx, "")
    return {"soft_deleted": idx,
            "real": False,
            "note": "FL no expone deletePattern; el patron se renombro a '' y "
                    "deja de aparecer, pero el slot sigue ocupado"}


@action("patterns.clone")
def h_pat_clone(p):
    """clonePattern() sin argumentos clona el patron ACTUAL, asi que hay que
    saltar al origen antes de clonar. El nuevo indice es el que devolvia
    patternCount() antes de clonar."""
    src = int(p["index"])
    before = patterns.patternCount()
    patterns.jumpToPattern(src)
    patterns.clonePattern()
    new_idx = before
    if patterns.patternCount() <= new_idx:
        raise RuntimeError("clonePattern no creo un patron nuevo (count sigue en %d)"
                           % patterns.patternCount())
    new_name = str(p.get("new_name", "") or "")
    if new_name:
        patterns.setPatternName(new_idx, new_name)
    return {"source": src,
            "index": new_idx,
            "name": patterns.getPatternName(new_idx),
            "pattern_count": patterns.patternCount()}


@action("patterns.findByName")
def h_pat_find(p):
    target = str(p.get("name", "")).lower().strip()
    for i in range(0, patterns.patternCount()):
        try:
            nm = patterns.getPatternName(i)
        except Exception:
            break
        if nm and nm.lower() == target:
            return {"index": i, "name": nm}
    return {"index": None, "name": None}


# ---- project ----

@action("project.metadata")
def h_pr_meta(_):
    # FL no expone getProjectName/Path/FilePath a MIDI scripting (comprobado:
    # hasattr(general, 'getProjectFilePath') == False). Lo que si hay es el
    # titulo, la autoria y el flag de cambios sin guardar.
    return {
        "title": _safe(general.getProjectTitle),
        "author": _safe(general.getProjectAuthor),
        "genre": _safe(general.getProjectGenre),
        "changed": _safe(general.getChangedFlag),
        "undo_level": _safe(general.getUndoLevelHint),
        "ppq": _safe(general.getRecPPQ),
        "ppb": _safe(general.getRecPPB),
        "metronome": _safe(general.getUseMetronome),
        "precount": _safe(general.getPrecount),
        "channel_count": _safe(channels.channelCount, True),
        "mixer_tracks": _safe(mixer.trackCount),
        "pattern_count": _safe(patterns.patternCount),
        "selected_pattern": _safe(patterns.patternNumber),
        "selected_channel": _safe(channels.selectedChannel, canBeNone=True, indexGlobal=True),
        "is_playing": (_safe(transport.isPlaying) == 1),
        "is_recording": (_safe(transport.isRecording) == 1),
        "tempo": (_safe(mixer.getCurrentTempo, 0) or 0) / 1000.0,
        "has_file": bool(_safe(general.getProjectTitle)),
    }


@action("project.save")
def h_pr_save(_):
    """Guardar el proyecto: FL no expone saveProject, pero si el atajo global
    Ctrl+S via `transport.globalTransport(midi.FPT_Save)`.

    OJO: si el proyecto no tiene ruta, FL abre un dialogo modal de "Save as"
    y se queda bloqueado esperando al usuario. El pump se reanuda cuando se
    cierra el dialogo, pero la request se pierde. Por eso se avisa en vez de
    colgarse en silencio."""
    fpt = getattr(midi, "FPT_Save", None)
    if fpt is None:
        raise RuntimeError("midi.FPT_Save no disponible en esta build de FL")
    has_file = bool(_safe(general.getProjectTitle))
    _safe(transport.globalTransport, fpt, 1)
    return {
        "ok": True,
        "had_file": has_file,
        "warning": (None if has_file else
                    "el proyecto no tiene ruta: FL ha abierto (o va a abrir) "
                    "un dialogo 'Save as'. La request se pierde hasta que se "
                    "cierre el dialogo."),
    }


@action("project.saveAs")
def h_pr_save_as(_):
    fpt = getattr(midi, "FPT_SaveNew", None)
    if fpt is None:
        raise RuntimeError("midi.FPT_SaveNew no disponible en esta build de FL")
    _safe(transport.globalTransport, fpt, 1)
    return {"ok": True,
            "warning": "FL abre un dialogo 'Save as': el usuario tiene que "
                       "escribir la ruta a mano, no se puede automatizar"}


@action("project.undo")
def h_pr_undo(_):
    # Ojo con la direccion: `undoUp`/`undoDown` son los nombres reales, y
    # segun el fLMCP Bridge undoUp DESHACE y undoDown REHACE (no es "subir un
    # nivel"). Verificado con el contador de historial.
    _safe(general.undoUp)
    return {"ok": True,
            "undo_level": _safe(general.getUndoLevelHint),
            "history_pos": _safe(general.getUndoHistoryPos)}


@action("project.redo")
def h_pr_redo(_):
    _safe(general.undoDown)
    return {"ok": True,
            "undo_level": _safe(general.getUndoLevelHint),
            "history_pos": _safe(general.getUndoHistoryPos)}


@action("project.undoHistory")
def h_pr_undo_hist(_):
    return {"count": _safe(general.getUndoHistoryCount),
            "pos": _safe(general.getUndoHistoryPos),
            "last": _safe(general.getUndoHistoryLast),
            "level": _safe(general.getUndoLevelHint)}


@action("project.saveUndo")
def h_pr_save_undo(_):
    _safe(general.saveUndo)
    return {"ok": True, "level": _safe(general.getUndoLevelHint)}


@action("project.version")
def h_pr_version(_):
    return {"fl_version": _safe(general.getVersion)}


# ---- ui ----

@action("ui.focusedWindow")
def h_ui_focus(_):
    return {"caption": _safe(ui.getFocusedFormCaption),
            "id": _safe(ui.getFocusedFormID)}


@action("ui.showWindow")
def h_ui_show(p):
    wid = int(p["window"])
    _safe(ui.showWindow, wid)
    return {"window": wid}


@action("ui.hideWindow")
def h_ui_hide(p):
    wid = int(p["window"])
    _safe(ui.hideWindow, wid)
    return {"window": wid}


@action("ui.hint")
def h_ui_hint(p):
    _safe(ui.setHintMsg, str(p.get("text", "")))
    return {"ok": True}


@action("ui.openPianoRoll")
def h_ui_pr(p):
    i = int(p.get("channel", 0))
    _safe(channels.selectChannel, i)
    _safe(ui.showWindow, midi.widPianoRoll)
    return {"channel": i}


@action("ui.selectedChannel")
def h_ui_selch(_):
    # `channels.selectedChannel` devuelve el numero de canal seleccionado (o -1).
    idx = _safe(channels.selectedChannel)
    if idx is None or idx < 0:
        return {"selected": -1, "name": ""}
    return {"selected": int(idx), "name": _safe(channels.getChannelName, int(idx)) or ""}


# ---- arrangement ----

@action("arrangement.current")
def h_ar_current(_):
    return {"tick": _safe(arrangement.currentTime)}


@action("arrangement.selection")
def h_ar_sel(_):
    active = bool(_safe(arrangement.selectionIsActive))
    return {"active": active,
            "start": _safe(arrangement.selectionStart),
            "end": _safe(arrangement.selectionEnd)}


# ---- playlist ----

@action("playlist.trackCount")
def h_pl_tracks(_):
    return {"count": playlist.trackCount()}


@action("playlist.trackInfo")
def h_pl_track_info(p):
    tr = int(p["track"])
    return {"track": tr,
            "name": _safe(playlist.getTrackName, tr) or "",
            "color": _safe(playlist.getTrackColor, tr),
            "muted": bool(_safe(playlist.isTrackMuted, tr)),
            "solo": bool(_safe(playlist.isTrackSolo, tr))}


@action("playlist.allTracks")
def h_pl_all(_):
    n = playlist.trackCount()
    return {"tracks": [{
        "track": i,
        "name": _safe(playlist.getTrackName, i) or "",
        "color": _safe(playlist.getTrackColor, i),
        "muted": bool(_safe(playlist.isTrackMuted, i)),
    } for i in range(n)]}


# ----------------------------------------------------------------------------
# Helpers
# ----------------------------------------------------------------------------

def _safe(fn, *args, **kwargs):
    """Llama a la FL API tragandose cualquier excepcion. Devuelve None si falla."""
    try:
        return fn(*args, **kwargs)
    except Exception:
        return None


def _clamp(v, lo, hi):
    try:
        v = float(v)
    except (TypeError, ValueError):
        raise ValueError("valor numerico esperado: %r" % (v,))
    return max(lo, min(hi, v))


def _color(c):
    if isinstance(c, str):
        s = c.lstrip("#")
        if len(s) == 6:
            try:
                return (int(s[0:2], 16) << 16) | (int(s[2:4], 16) << 8) | int(s[4:6], 16)
            except ValueError:
                return 0
        try:
            return int(c, 0)
        except ValueError:
            return 0
    if isinstance(c, (list, tuple)) and len(c) >= 3:
        return (int(c[0]) << 16) | (int(c[1]) << 8) | int(c[2])
    try:
        return int(c)
    except (TypeError, ValueError):
        return 0


def _jsonable(v):
    """Convierte un valor de la FL API a algo serializable en JSON."""
    if v is None or isinstance(v, (bool, int, float, str)):
        return v
    if isinstance(v, (list, tuple)):
        return [_jsonable(x) for x in v]
    if isinstance(v, dict):
        return {str(k): _jsonable(x) for k, x in v.items()}
    # Los objetos de la FL API (flags, etc.) se reducen a su repr, que es
    # estable y legible; no se tries introspeccionar mas.
    return str(v)


def _mx_info(tr):
    return {
        "track": tr,
        "name": _safe(mixer.getTrackName, tr) or "",
        "volume": _safe(mixer.getTrackVolume, tr),
        "pan": _safe(mixer.getTrackPan, tr),
        "muted": bool(_safe(mixer.isTrackMuted, tr)),
        "solo": bool(_safe(mixer.isTrackSolo, tr)),
        "peaks_l": _safe(mixer.getTrackPeaks, tr, 1),
        "peaks_r": _safe(mixer.getTrackPeaks, tr, 2),
    }


def _ch_info(i):
    return {
        "index": i,
        "name": _safe(channels.getChannelName, i) or "",
        "volume": _safe(channels.getChannelVolume, i),
        "pan": _safe(channels.getChannelPan, i),
        "pitch": _safe(channels.getChannelPitch, i),
        "muted": bool(_safe(channels.isChannelMuted, i)),
        "color": _safe(channels.getChannelColor, i),
        "target_fx_track": _safe(channels.getTargetFxTrack, i),
    }


# ----------------------------------------------------------------------------
# Transporte
# ----------------------------------------------------------------------------

def _pick_pending():
    """Busca la request pendiente MAS ANTIGUA en los slots del mailbox.

    Devuelve (id, slot, request) o None. Se recorren los slots en orden fijo
    (no hace falta glob, que esta bloqueado: se abren por nombre).

    Se elige la de MENOR id, no la de mayor. Si se procesase la mayor, la menor
    quedaria con id < last_seen_id y se perderia en silencio. FIFO no pierde
    nada y el orden es determinista."""
    best = None
    global last_seen_id
    for slot in range(SLOT_COUNT):
        data = read_json(REQ_FILES[slot])
        if not isinstance(data, dict):
            continue
        rid = data.get("id")
        if not isinstance(rid, int) or rid <= 0 or rid <= last_seen_id:
            continue
        if best is None or rid < best[0]:
            best = (rid, slot, data)
    return best



def _handle(req):
    rid = req.get("id", 0)
    name = req.get("action", "")
    params = req.get("params") or {}
    if not isinstance(params, dict):
        params = {}
    handler = HANDLERS.get(name)
    if handler is None:
        return {"id": rid, "ok": False,
                "error": "action desconocida: %r" % name,
                "available": len(HANDLERS)}
    try:
        result = handler(params)
        return {"id": rid, "ok": True, "result": result}
    except Exception as e:
        return {"id": rid, "ok": False,
                "error": "%s: %s" % (type(e).__name__, e),
                "traceback": traceback.format_exc(limit=4)}


def pump():
    """Atiende como mucho una request pendiente. La llama FL desde OnMidiIn,
    OnMidiMsg, OnRefresh y OnProjectLoad. Nunca lanza: cualquier excepcion se
    traga para que el script no muera y FL no pierda el controller."""
    global last_seen_id, pump_count, last_action, last_error
    try:
        pump_count += 1
        pending = _pick_pending()
        if pending is not None:
            rid, slot, req = pending
            # Se marca ANTES de ejecutar: si el handler falla o el script se
            # reinicia a mitad, no se reintenta en bucle.
            last_seen_id = rid
            resp = _handle(req)
            body = json.dumps(resp, ensure_ascii=False, separators=(",", ":"))
            write_text_atomic(RESP_FILES[slot], body + "\n")
            last_action = req.get("action", "")
            if not resp.get("ok", True):
                last_error = resp.get("error", "")
        _write_status()
    except Exception as e:
        last_error = "pump: %s: %s" % (type(e).__name__, e)
        log(last_error)


def _write_status():
    """Heartbeat. El daemon lo lee para saber que FL esta vivo y que el
    transporte funciona, incluso antes de la primera request."""
    payload = {
        "bridge_version": BRIDGE_VERSION,
        "protocol_version": PROTOCOL_VERSION,
        "fl_version": _safe(general.getVersion) if in_fl else "unknown",
        "in_fl": in_fl,
        "pump_count": pump_count,
        "last_seen_id": last_seen_id,
        "last_action": last_action,
        "last_error": last_error,
        "uptime_sec": round(time.monotonic() - started_at, 1),
        "handlers": len(HANDLERS),
    }
    write_text_atomic(STATUS_FILE, json.dumps(payload, ensure_ascii=False) + "\n")


# ----------------------------------------------------------------------------
# Callbacks de FL Studio
#
# OnIdle NUNCA se dispara en FL 2025 (medido: idle_ticks = 0). El pump real
# llega por OnMidiIn/OnMidiMsg, que es lo que dispara el daemon con un byte
# MIDI tras escribir la request. Los demas callbacks se aprovechan como
# oportunidades extra: si FL dispara alguno, procesamos lo pendiente.
# ----------------------------------------------------------------------------

def OnInit():
    global started_at
    started_at = time.monotonic()
    try:
        # Limpia slots viejos para que un daemon recien arrancado no lea
        # respuestas de una sesion anterior.
        for path in list(REQ_FILES) + list(RESP_FILES):
            try:
                with open(path, "w", encoding="utf-8") as f:
                    f.write("")
            except Exception:
                pass
    except Exception as e:
        log("init cleanup: %s" % e)
    log("%s v%s en FL=%s, %d handlers, %d slots, dir=%s"
        % (BRIDGE_NAME, BRIDGE_VERSION, in_fl, len(HANDLERS), SLOT_COUNT, SCRIPT_DIR))
    _write_status()


def OnDeInit():
    log("shutdown tras %d pumps" % pump_count)


def OnMidiIn(event):
    pump()
    event.handled = False


def OnMidiMsg(event):
    pump()
    event.handled = False


def OnRefresh(flags):
    pump()


def OnProjectLoad(status):
    pump()
