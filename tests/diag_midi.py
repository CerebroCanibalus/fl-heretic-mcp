#!/usr/bin/env python3
"""Diagnostico minimo: el MIDI que mandamos llega al controller script?

No enumera nombres de dispositivo (midiOutGetDevCapsW con struct erronea
provoca access violation). Solo envia y comprueba el pump_count del bridge.
"""
import ctypes
import json
import os
import sys
import time
from pathlib import Path

winmm = ctypes.WinDLL("winmm")
winmm.midiInGetNumDevs.restype = ctypes.c_ulong
winmm.midiOutGetNumDevs.restype = ctypes.c_ulong
winmm.midiOutOpen.argtypes = [ctypes.POINTER(ctypes.c_void_p), ctypes.c_ulong,
                              ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong]
winmm.midiOutClose.argtypes = [ctypes.c_void_p]
winmm.midiOutShortMsg.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
winmm.midiOutPrepareHeader.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong]
winmm.midiOutUnprepareHeader.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong]


class MIDIHDR(ctypes.Structure):
    _fields_ = [("dwBytes", ctypes.c_ulong),
                ("dwUser", ctypes.c_ulong),
                ("dwFlags", ctypes.c_ulong),
                ("lpData", ctypes.c_void_p),
                ("dwBufferLength", ctypes.c_ulong),
                ("dwBytesRecorded", ctypes.c_ulong),
                ("dwUserP1", ctypes.c_ulong),
                ("dwUserP2", ctypes.c_ulong)]


BRIDGE = Path(os.environ["USERPROFILE"]) / "Documents/Image-Line/FL Studio/Settings/Hardware/FL Heretic Bridge"
STATUS = BRIDGE / "hr_status.json"
MIDIERR = {0: "sin error", 1: "ONDE ABIERTO", 2: "OUT SIN DISPOSITIVOS", 3: "SIN MEMORIA",
           4: "INVALID HANDLE", 5: "SIN FUNCION", 6: "NO HAY SENTIDO", 7: "MEM NO LOCKED",
           8: "MEM HANDLE", 9: "PARAM", 10: "HARDWARE", 11: "MEMORY", 12: "HANDLED"}


def pumps():
    for _ in range(3):
        try:
            return json.loads(STATUS.read_text(encoding="utf-8"))["pump_count"]
        except Exception:
            time.sleep(0.2)
    return None


def log(msg):
    sys.stdout.write(msg + "\n")
    sys.stdout.flush()


def main():
    log("=" * 66)
    n_in = winmm.midiInGetNumDevs()
    n_out = winmm.midiOutGetNumDevs()
    log(f"  MIDI IN  devices: {n_in}")
    log(f"  MIDI OUT devices: {n_out}")
    log("")

    base = pumps()
    log(f"  pump_count inicial: {base}")
    log("")

    # --- Intento 1: short messages a cada puerto OUT ---
    log("[1] midiOutShortMsg, todos los puertos, canales 0-15")
    opened = []
    for dev in range(n_out):
        h = ctypes.c_void_p()
        rc = winmm.midiOutOpen(ctypes.byref(h), dev, None, None, 0)
        if rc == 0:
            opened.append((dev, h))
            log(f"    OUT {dev}: abierto")
        else:
            log(f"    OUT {dev}: fallo rc={rc} {MIDIERR.get(rc, '?')}")
    for dev, h in opened:
        for ch in range(16):
            rc = winmm.midiOutShortMsg(h, 0x90 | ch)          # note on
            winmm.midiOutShortMsg(h, 0x80 | ch)              # note off
        # control change, a veces los scripts filtran solo CC
        for cc in (0, 7, 64, 123):
            winmm.midiOutShortMsg(h, 0xB0 | (cc << 8) | (0x7F << 16))
        time.sleep(0.1)
    time.sleep(2)
    after1 = pumps()
    log(f"    pump_count: {base} -> {after1}  {'SE MOVIO' if after1 != base else 'sin cambio'}")
    for dev, h in opened:
        winmm.midiOutClose(h)
    log("")

    # --- Intento 2: long message (SysEx-like) via midiOutLongMsg ---
    log("[2] midiOutLongMsg (buffer de datos, no short msg)")
    base2 = after1
    for dev in range(n_out):
        h = ctypes.c_void_p()
        if winmm.midiOutOpen(ctypes.byref(h), dev, None, None, 0) != 0:
            continue
        data = (ctypes.c_ubyte * 4)(0xF0, 0x7E, 0x7F, 0xF7)  # SysEx realtime-ish
        buf = ctypes.create_string_buffer(bytes(data), 4)
        hdr = MIDIHDR()
        hdr.dwBytes = 32
        hdr.lpData = ctypes.cast(buf, ctypes.c_void_p)
        hdr.dwBufferLength = 4
        rc = winmm.midiOutPrepareHeader(h, ctypes.byref(hdr), 32)
        if rc == 0:
            winmm.midiOutUnprepareHeader(h, ctypes.byref(hdr), 32)
            log(f"    OUT {dev}: long msg enviada")
        else:
            log(f"    OUT {dev}: prepare fallo rc={rc} {MIDIERR.get(rc, '?')}")
        # ademas short msg
        for ch in range(16):
            winmm.midiOutShortMsg(h, 0x90 | ch)
            winmm.midiOutShortMsg(h, 0x80 | ch)
        time.sleep(0.1)
        winmm.midiOutClose(h)
    time.sleep(2)
    after2 = pumps()
    log(f"    pump_count: {base2} -> {after2}  {'SE MOVIO' if after2 != base2 else 'sin cambio'}")
    log("")

    # --- Intento 3: request directa, sin MIDI, por si el pump viene de otro sitio ---
    log("[3] request directa sin MIDI (por si el pump no depende del MIDI)")
    base3 = after2
    rid = int(time.time() * 1e9)
    slot = rid % 8
    part = BRIDGE / f"hr_req_{slot}.json.tmp"
    part.write_text(json.dumps({"id": rid, "action": "meta.ping", "params": {}}), encoding="utf-8")
    os.replace(part, BRIDGE / f"hr_req_{slot}.json")
    time.sleep(3)
    after3 = pumps()
    log(f"    pump_count: {base3} -> {after3}  {'SE MOVIO' if after3 != base3 else 'sin cambio'}")
    r = BRIDGE / f"hr_resp_{slot}.json"
    log(f"    hr_resp_{slot}.json: {r.read_text(encoding='utf-8')[:150] if r.exists() and r.stat().st_size else '(vacio)'}")
    log("")

    log("=" * 66)
    if after3 == base:
        log("  VEREDICTO: ningun MIDI llega al script. pump_count no se mueve.")
        log("  -> problema de configuracion MIDI (loopMIDI loopback / puerto / FL).")
    elif after3 != base and after1 == base:
        log("  VEREDICTO: el pump se dispara SOLO sin MIDI. Hay una fuente de")
        log("  eventos interna de FL, y el wake MIDI que mando no llega.")
    else:
        log("  VEREDICTO: el MIDI si mueve el pump (intento 1 funciono).")
    log("=" * 66)


if __name__ == "__main__":
    main()
