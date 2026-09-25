#!/usr/bin/env python3
"""El MIDI de FL se agota: hay que mantener el puerto OUT abierto.

Los tests anteriores abrian y cerraban midiOutOpen en cada envio. El primer
burst funciono y los siguientes no, lo que apunta a que el puerto hay que
mantenerlo abierto (FL/loopMIDI se sincroniza al abrirlo) y no a cerrarlo
tras cada mensaje. Este test mantiene el puerto abierto y manda de forma
repetida, y ademas prueba la request sin MIDI entremedios.
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
winmm.midiOutReset.argtypes = [ctypes.c_void_p]

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


def write_req(action, params=None):
    rid = int(time.time() * 1e9)  # 64 bits, SIN mascara: un id recortado a 31 bits queda por debajo de last_seen_id y el bridge lo ignora para siempre
    slot = rid % 8
    resp = BRIDGE / f"hr_resp_{slot}.json"
    resp.write_text("", encoding="utf-8")
    part = BRIDGE / f"hr_req_{slot}.json.tmp"
    part.write_text(json.dumps({"id": rid, "action": action, "params": params or {}}), encoding="utf-8")
    os.replace(part, BRIDGE / f"hr_req_{slot}.json")
    return slot, resp, rid


def wait_resp(resp, rid, timeout):
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


def main():
    log("=" * 66)
    log("  Wake MIDI con el puerto SIEMPRE abierto")
    log("=" * 66)
    log(f"  pump_count inicial: {pumps()}")
    n_out = winmm.midiOutGetNumDevs()
    log(f"  MIDI OUT devices: {n_out}")
    log("")

    # Abrir TODOS los puertos OUT y mantenerlos abiertos durante el test.
    handles = []
    for dev in range(n_out):
        h = ctypes.c_void_p()
        rc = winmm.midiOutOpen(ctypes.byref(h), dev, None, None, 0)
        if rc == 0:
            handles.append((dev, h))
            log(f"  OUT {dev} abierto y se mantiene")
        else:
            log(f"  OUT {dev} fallo rc={rc}")
    log("")

    def burst(h, channels=range(16), note=0, vel=100, times=3):
        for _ in range(times):
            for ch in channels:
                winmm.midiOutShortMsg(h, 0x90 | (ch & 0x0F) | ((note & 0x7F) << 8) | ((vel & 0x7F) << 16))
            time.sleep(0.03)
            for ch in channels:
                winmm.midiOutShortMsg(h, 0x80 | (ch & 0x0F))
            time.sleep(0.03)

    # 1. Con MIDI, puerto abierto
    log("[1] 8 requests con MIDI, puerto persistente")
    ok = 0
    for i in range(8):
        slot, resp, rid = write_req("meta.ping")
        for dev, h in handles:
            burst(h, times=1)
        d = wait_resp(resp, rid, 3.0)
        if d and d.get("ok"):
            ok += 1
        time.sleep(0.05)
    log(f"  ok={ok}/8   pump_count={pumps()}")
    log("")

    # 2. Sin MIDI, con el puerto abierto (puede que FL bumpee por su cuenta)
    log("[2] 8 requests SIN MIDI")
    ok2 = 0
    for i in range(8):
        slot, resp, rid = write_req("meta.ping")
        d = wait_resp(resp, rid, 2.0)
        if d and d.get("ok"):
            ok2 += 1
        time.sleep(0.05)
    log(f"  ok={ok2}/8   pump_count={pumps()}")
    log("")

    # 3. MIDI solo por canal 1 (el "1" que se ve en el registro)
    log("[3] 8 requests MIDI solo canal 1")
    ok3 = 0
    for i in range(8):
        slot, resp, rid = write_req("meta.ping")
        for dev, h in handles:
            burst(h, channels=[1], times=1)
        d = wait_resp(resp, rid, 3.0)
        if d and d.get("ok"):
            ok3 += 1
        time.sleep(0.05)
    log(f"  ok={ok3}/8   pump_count={pumps()}")
    log("")

    # 4. MIDI continuo en background, requests sin parar
    log("[4] MIDI continuo, 10 requests")
    ok4 = 0
    stop = [False]

    def spam():
        while not stop[0]:
            for dev, h in handles:
                burst(h, times=1)
            time.sleep(0.1)

    import threading
    t = threading.Thread(target=spam, daemon=True)
    t.start()
    time.sleep(0.3)
    for i in range(10):
        slot, resp, rid = write_req("meta.ping")
        d = wait_resp(resp, rid, 3.0)
        if d and d.get("ok"):
            ok4 += 1
        time.sleep(0.05)
    stop[0] = True
    time.sleep(0.3)
    log(f"  ok={ok4}/10   pump_count={pumps()}")
    log("")

    for dev, h in handles:
        try:
            winmm.midiOutReset(h)
            winmm.midiOutClose(h)
        except Exception:
            pass

    log("=" * 66)
    best = max(ok, ok2, ok3, ok4)
    log(f"  mejor resultado: {best}/10  (con MIDI: {max(ok, ok3, ok4)}/10)")
    if max(ok, ok3, ok4) >= 8:
        log("  VEREDICTO: mantener el puerto MIDI OUT abierto resuelve el wake.")
    else:
        log("  VEREDICTO: el MIDI no despierta a FL de forma fiable.")
    log("=" * 66)


if __name__ == "__main__":
    main()
