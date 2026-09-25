#!/usr/bin/env python3
"""Verificacion de los workarounds para guardar y crear patrones.

Todo lo de esta fase es de SOLO LECTURA: comprueba que las funciones y
constantes existen y que las firmas son las que asumimos. La parte que crea
patrones (que no se puede deshacer, FL no expone deletePattern) va aparte, en
test_create_pattern.py, y hay que lanzarla a proposito.
"""
import sys
import json

sys.path.insert(0, "tests")
from introspect_api import call, log  # noqa: E402


def check(label, expr):
    r = call("meta.exec", {"code": expr})
    if not r:
        log(f"  {label:<42} TIMEOUT")
        return None
    # meta.exec devuelve ok en el ENVELOPE cuando la llamada RPC funciono, y
    # ok/error/result dentro de result para el codigo ejecutado.
    res = r.get("result") or {}
    if not r.get("ok"):
        log(f"  {label:<42} RPC ERROR: {str(r.get('error'))[:90]}")
        return None
    if not res.get("ok", True):
        log(f"  {label:<42} EXEC ERROR: {res.get('error', '?')[:90]}")
        return None
    v = res.get("result")
    if res.get("stdout"):
        log(f"  {label:<42} {v}   (stdout: {res['stdout'].strip()[:120]})")
    else:
        log(f"  {label:<42} {json.dumps(v)[:140]}")
    return v


def main():
    log("=" * 78)
    log("  Workarounds: solo lectura")
    log("=" * 78)
    log("")

    log("--- A. Constantes FPT para guardar proyecto ---")
    check("midi.FPT_Save existe", "getattr(midi, 'FPT_Save', 'AUSENTE')")
    check("midi.FPT_SaveNew existe", "getattr(midi, 'FPT_Save', 'AUSENTE') and getattr(midi, 'FPT_SaveNew', 'AUSENTE')")
    check("todas las FPT_ disponibles", "[k for k in dir(midi) if k.startswith('FPT_')]")
    log("")

    log("--- B. globalTransport: firma y disponibilidad ---")
    check("transport.globalTransport existe", "hasattr(transport, 'globalTransport')")
    log("")

    log("--- C. Firmas reales de channels (aceptan argumentos?) ---")
    check("channelCount(True)", "channels.channelCount(True)")
    check("selectedChannel() sin args", "channels.selectedChannel()")
    check("selectedChannel kwargs", "channels.selectedChannel(canBeNone=True, indexGlobal=True)")
    log("")

    log("--- D. Patrones: indices y clone ---")
    check("patternCount()", "patterns.patternCount()")
    check("patternNumber() (actual)", "patterns.patternNumber()")
    check("getPatternName(0)", "patterns.getPatternName(0)")
    check("getPatternName(1)", "patterns.getPatternName(1)")
    check("patternMax", "patterns.patternMax")
    log("")

    log("--- E. Undo / redo: cual es cual ---")
    check("getUndoLevelHint()", "general.getUndoLevelHint()")
    check("getUndoHistoryCount()", "general.getUndoHistoryCount()")
    check("getUndoHistoryPos()", "general.getUndoHistoryPos()")
    check("getUndoHistoryLast()", "general.getUndoHistoryLast()")
    log("  (undoUp/undoDown se prueban en la fase destructiva:necesitan un cambio real)")
    log("")

    log("--- F. Metadatos de proyecto (lo que si se puede leer) ---")
    check("getProjectTitle()", "general.getProjectTitle()")
    check("getProjectAuthor()", "general.getProjectAuthor()")
    check("getChangedFlag()", "general.getChangedFlag()")
    check("getRecPPQ()", "general.getRecPPQ()")
    check("getUseMetronome()", "general.getUseMetronome()")
    log("")

    log("--- G. Peaks del master (util para metering) ---")
    check("getTrackPeaks(0, 1)", "mixer.getTrackPeaks(0, 1)")
    check("getTrackPeaks(0, 2)", "mixer.getTrackPeaks(0, 2)")
    check("getLastPeakVol(0)", "mixer.getLastPeakVol(0)")
    log("")

    log("--- H. Lo que sigue SIN existir (limites duros de FL) ---")
    for name, expr in [
        ("general.saveProject", "hasattr(general, 'saveProject')"),
        ("general.newProject", "hasattr(general, 'newProject')"),
        ("general.getProjectFilePath", "hasattr(general, 'getProjectFilePath')"),
        ("patterns.createPattern", "hasattr(patterns, 'createPattern')"),
        ("patterns.deletePattern", "hasattr(patterns, 'deletePattern')"),
        ("arrangement.countMarkers", "hasattr(arrangement, 'countMarkers')"),
        ("playlist.getClipNum", "hasattr(playlist, 'getClipNum')"),
    ]:
        check(name, expr)
    log("")
    log("=" * 78)


if __name__ == "__main__":
    sys.exit(main())
