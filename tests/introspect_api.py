#!/usr/bin/env python3
"""Introspeccion de la FL API real, via meta.exec del bridge.

7 handlers fallaron con AttributeError porque use nombres de funcion que no
existen ('mixer.getTrackNum' en vez de 'mixer.count', etc.). En vez de adivinar,
se le pide al propio FL que las liste. Esto tambien valida meta.exec, que es
la palanca de acceso a toda la API.
"""
import ctypes
import json
import os
import sys
import time
from pathlib import Path

winmm = ctypes.WinDLL("winmm")
winmm.midiOutGetNumDevs.restype = ctypes.c_ulong
winmm.midiOutOpen.argtypes = [ctypes.POINTER(ctypes.c_void_p), ctypes.c_ulong,
                              ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong]
winmm.midiOutClose.argtypes = [ctypes.c_void_p]
winmm.midiOutShortMsg.argtypes = [ctypes.c_void_p, ctypes.c_ulong]

BRIDGE = Path(os.environ["USERPROFILE"]) / "Documents/Image-Line/FL Studio/Settings/Hardware/FL Heretic Bridge"
_handles = []


def midi_open():
    if _handles:
        return _handles
    for dev in range(winmm.midiOutGetNumDevs()):
        h = ctypes.c_void_p()
        if winmm.midiOutOpen(ctypes.byref(h), dev, None, None, 0) == 0:
            _handles.append(h)
    return _handles


def wake():
    for h in midi_open():
        for ch in range(16):
            winmm.midiOutShortMsg(h, 0x90 | ch)
            winmm.midiOutShortMsg(h, 0x80 | ch)
        time.sleep(0.02)


def call(action, params=None, timeout=6.0):
    rid = int(time.time() * 1e9)
    slot = rid % 8
    resp = BRIDGE / f"hr_resp_{slot}.json"
    resp.write_text("", encoding="utf-8")
    payload = json.dumps({"id": rid, "action": action, "params": params or {}})
    target = BRIDGE / f"hr_req_{slot}.json"
    part = BRIDGE / f"hr_req_{slot}.json.tmp"
    part.write_text(payload, encoding="utf-8")
    try:
        os.replace(part, target)
    except PermissionError:
        with open(target, "w", encoding="utf-8") as f:
            f.write(payload)
            f.flush()
    wake()
    t0 = time.monotonic()
    while time.monotonic() - t0 < timeout:
        time.sleep(0.02)
        try:
            raw = resp.read_text(encoding="utf-8")
        except Exception:
            continue
        if not raw.endswith("\n"):
            continue
        try:
            d = json.loads(raw.split("\n", 1)[0])
        except Exception:
            continue
        if d.get("id") == rid:
            return d
    return None


def log(m):
    sys.stdout.write(str(m) + "\n")
    sys.stdout.flush()


def main():
    log("=" * 70)
    log("  Introspeccion de la FL API real (via meta.exec)")
    log("=" * 70)

    # Prueba de que meta.exec funciona.
    r = call("meta.exec", {"code": "1 + 1"})
    log(f"  meta.exec('1+1') -> {json.dumps(r)[:200] if r else 'TIMEOUT'}")
    if not r or not r.get("ok"):
        log("  meta.exec NO funciona. No se puede introspeccionar.")
        return 1
    log("")

    # Listado de funciones por modulo, filtrado a las que nos interesan.
    wanted = {
        "mixer": ["count", "num", "track", "solo", "mute", "volume", "pan", "fx", "send", "eq", "arm", "name", "color", "select", "route"],
        "patterns": ["count", "num", "current", "select", "name", "clone", "create", "color", "find"],
        "general": ["project", "file", "name", "path", "title", "version", "dirty", "undo", "redo", "save", "new"],
        "ui": ["focus", "form", "caption", "window", "show", "hide", "hint", "scroll", "select"],
        "arrangement": ["marker", "time", "selection", "count", "current", "jump"],
        "transport": ["song", "play", "record", "loop", "tempo", "length", "pos"],
        "plugins": ["param", "plugin", "preset", "valid", "name", "count"],
        "channels": ["count", "num", "name", "volume", "pan", "pitch", "color", "mute", "solo", "select", "target"],
        "playlist": ["track", "clip", "marker", "count", "num"],
        "device": ["volume", "mixer", "master"],
    }

    code_lines = ["out = {}"]
    for mod, keys in wanted.items():
        cond = " or ".join(f"(k.lower().find({k!r}) >= 0)" for k in keys)
        code_lines.append(f"try:\n    out[{mod!r}] = sorted([k for k in dir({mod}) if {cond}])\nexcept Exception as e:\n    out[{mod!r}] = ['ERROR: ' + str(e)]")
    code_lines.append("out")
    code = "\n".join(code_lines)

    r = call("meta.exec", {"code": code})
    if not r or not r.get("ok"):
        log(f"  introspeccion fallo: {json.dumps(r)[:400]}")
        return 1

    data = r["result"]
    for mod in wanted:
        fns = data.get(mod, [])
        log(f"  {mod} ({len(fns)}):")
        for i in range(0, len(fns), 6):
            log("      " + "  ".join(f"{x:<22}" for x in fns[i:i+6]))
        log("")

    log("=" * 70)
    return 0


if __name__ == "__main__":
    sys.exit(main())
