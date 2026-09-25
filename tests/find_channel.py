#!/usr/bin/env python3
"""Descubre que canal MIDI wake mueve el pump del FL Heretic Bridge.

test_live mandaba MIDI solo por el canal 0 y no despertaba nada; el
diagnostico mandaba por los 16 canales y si. FL filtra el MIDI entrante por
el canal asignado al controller script en MIDI Settings, asi que hay que
descubrir cual es. Este test recorre los 16 canales.
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
STATUS = BRIDGE / "hr_status.json"


def pumps():
    try:
        return json.loads(STATUS.read_text(encoding="utf-8"))["pump_count"]
    except Exception:
        return None


def log(m):
    sys.stdout.write(m + "\n")
    sys.stdout.flush()


def send_all(channels, note=0, vel=100):
    n_out = winmm.midiOutGetNumDevs()
    for dev in range(n_out):
        h = ctypes.c_void_p()
        if winmm.midiOutOpen(ctypes.byref(h), dev, None, None, 0) != 0:
            continue
        for ch in channels:
            winmm.midiOutShortMsg(h, 0x90 | (ch & 0x0F) | ((note & 0x7F) << 8) | ((vel & 0x7F) << 16))
        time.sleep(0.05)
        for ch in channels:
            winmm.midiOutShortMsg(h, 0x80 | (ch & 0x0F))
        winmm.midiOutClose(h)


def probe(channels, timeout=3.0):
    rid = int(time.time() * 1e9)
    slot = rid % 8
    resp = BRIDGE / f"hr_resp_{slot}.json"
    resp.write_text("", encoding="utf-8")
    part = BRIDGE / f"hr_req_{slot}.json.tmp"
    part.write_text(json.dumps({"id": rid, "action": "meta.ping", "params": {}}), encoding="utf-8")
    os.replace(part, BRIDGE / f"hr_req_{slot}.json")
    send_all(channels)
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
            return True
    return False


def main():
    log("=" * 66)
    log("  Descubrimiento del canal MIDI wake")
    log("=" * 66)
    log(f"  pump_count inicial: {pumps()}")
    log("")
    log("[control] los 16 canales")
    ok = probe(list(range(16)))
    log(f"  {'OK' if ok else 'FAIL'}  wake multi-canal")
    log("")
    log("[individual] cada canal por separado")
    good = []
    for ch in range(16):
        ok = probe([ch], timeout=2.5)
        log(f"  canal {ch:2d}: {'OK  <<<' if ok else 'no'}")
        if ok:
            good.append(ch)
        time.sleep(0.1)
    log("")
    log("[reconfirmacion] canal 0 dos veces")
    for i in range(2):
        ok = probe([0], timeout=2.5)
        log(f"  intento {i+1}: {'OK' if ok else 'no'}")
    log("")
    log("=" * 66)
    if good:
        log(f"  CANALES QUE FUNCIONAN: {good}")
    else:
        log("  NINGUN canal aislado funciona, pero el multi-canal si.")
        log("  -> hay que despertar con un burst de los 16 canales.")
    log("=" * 66)
    return 0 if good else 1


if __name__ == "__main__":
    sys.exit(main())
