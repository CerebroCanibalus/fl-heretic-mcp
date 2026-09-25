#!/usr/bin/env python3
"""Fiabilidad del FL Heretic Bridge contra FL Studio real.

Manda un byte MIDI (wake) tras cada request, como hara el daemon Rust, y mide
cuantas de N requests responden. Esto decide si el wake MIDI es necesario o
si FL se despierta solo.
"""
import json
import os
import sys
import time
from pathlib import Path

if sys.platform == "win32":
    import ctypes
    winmm = ctypes.WinDLL("winmm")

    class MIDIHDR(ctypes.Structure):
        _fields_ = [("dwBytes", ctypes.c_ulong),
                    ("dwUser", ctypes.c_ulong),
                    ("dwFlags", ctypes.c_ulong),
                    ("lpData", ctypes.c_void_p),
                    ("dwBufferLength", ctypes.c_ulong),
                    ("dwBytesRecorded", ctypes.c_ulong),
                    ("dwUserP1", ctypes.c_ulong),
                    ("dwUserP2", ctypes.c_ulong)]

    winmm.midiOutOpen.argtypes = [ctypes.POINTER(ctypes.c_void_p), ctypes.c_ulong,
                                 ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong]
    winmm.midiOutShortMsg.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    winmm.midiOutClose.argtypes = [ctypes.c_void_p]
else:
    winmm = None


BRIDGE = Path(os.environ["USERPROFILE"]) / "Documents/Image-Line/FL Studio/Settings/Hardware/FL Heretic Bridge"
SLOTS = 8
REQ = [BRIDGE / f"hr_req_{i}.json" for i in range(SLOTS)]
RESP = [BRIDGE / f"hr_resp_{i}.json" for i in range(SLOTS)]
STATUS = BRIDGE / "hr_status.json"

# El puerto MIDI OUT se mantiene ABIERTO durante toda la vida del proceso.
# Abrirlo y cerrarlo en cada envio hace que FL no reciba nada: con el puerto
# persistente el pump se dispara (medido: pump_count 1 -> 7601), y con el
# efimero se queda en 0. Es el mismo motivo por el que el daemon Rust debe
# guardar el handle abierto.
_handles = []


def midi_open():
    global _handles
    if _handles:
        return _handles
    n = winmm.midiOutGetNumDevs()
    for dev in range(n):
        h = ctypes.c_void_p()
        if winmm.midiOutOpen(ctypes.byref(h), dev, None, None, 0) == 0:
            _handles.append(h)
    return _handles


def midi_wake():
    """Despierta a FL con un Note On en los 16 canales por cada puerto OUT.

    Los 16 canales: FL filtra el MIDI entrante por el canal asignado al
    controller, y no esta claro cual es. Mandarlos todos son 16 bytes."""
    handles = midi_open()
    for h in handles:
        for ch in range(16):
            winmm.midiOutShortMsg(h, 0x90 | ch | (0x00 << 8) | (0x40 << 16))
        time.sleep(0.02)
        for ch in range(16):
            winmm.midiOutShortMsg(h, 0x80 | ch)
    return len(handles)


def midi_close():
    for h in _handles:
        try:
            winmm.midiOutClose(h)
        except Exception:
            pass
    _handles.clear()

def heartbeat():
    try:
        return json.loads(STATUS.read_text(encoding="utf-8"))
    except Exception:
        return None


def call(action, params=None, rid=None, wake=True, timeout=6.0):
    if rid is None:
        rid = int(time.time() * 1e9)
    slot = rid % SLOTS
    payload = json.dumps({"id": rid, "action": action, "params": params or {}})
    # Atomicidad: temp + replace. En Windows replace() falla con
    # PermissionError si FL tiene el fichero destino abierto (y lo tiene, lo
    # acaba de leer), asi que se cae a escritura directa. El bridge descarta
    # un JSON ilegible y reintenta en el siguiente pump, asi que no se pierde.
    part = Path(str(REQ[slot]) + ".tmp")
    part.write_text(payload, encoding="utf-8")
    try:
        os.replace(part, REQ[slot])
    except PermissionError:
        with open(REQ[slot], "w", encoding="utf-8") as f:
            f.write(payload)
            f.flush()
    if wake:
        midi_wake()
    t0 = time.monotonic()
    while time.monotonic() - t0 < timeout:
        time.sleep(0.01)
        try:
            raw = RESP[slot].read_text(encoding="utf-8")
        except FileNotFoundError:
            continue
        if not raw.endswith("\n"):
            continue
        try:
            resp = json.loads(raw.split("\n", 1)[0])
        except Exception:
            continue
        if resp.get("id") != rid:
            continue
        return (time.monotonic() - t0) * 1000, resp
    return (time.monotonic() - t0) * 1000, None


def main():
    hb = heartbeat()
    if not hb:
        print("ERROR: no hay hr_status.json. El bridge no esta cargado en FL.")
        return 1
    print("=" * 66)
    print(f"  FL Heretic Bridge v{hb['bridge_version']}  FL={hb['fl_version']}  "
          f"in_fl={hb['in_fl']}  handlers={hb['handlers']}")
    print(f"  pump_count al inicio: {hb['pump_count']}")
    print("=" * 66)
    print()

    print("--- 1. meta.ping con wake MIDI ---")
    ms, r = call("meta.ping", wake=True)
    if r and r.get("ok"):
        res = r["result"]
        print(f"  OK  {ms:.1f}ms  bridge={res['bridge_version']} fl={res['fl_version']} "
              f"pumps={res['pump_count']}")
    else:
        print(f"  FAIL {ms:.1f}ms  {r}")
    print()

    print("--- 2. transporte real: transport.status ---")
    ms, r = call("transport.status")
    if r and r.get("ok"):
        st = r["result"]
        print(f"  OK  {ms:.1f}ms  bpm={st['bpm']}  playing={st['is_playing']}  "
              f"bars={st['position_bars']}")
    else:
        print(f"  FAIL {ms:.1f}ms  {r}")
    print()

    print("--- 3. lectura de estado: mixer + channels ---")
    ms, r = call("mixer.count")
    print(f"  mixer.count        {'OK ' if r and r.get('ok') else 'FAIL'}  "
          f"{r['result'] if r and r.get('ok') else (r or 'timeout')}")
    ms, r = call("channels.count")
    print(f"  channels.count     {'OK ' if r and r.get('ok') else 'FAIL'}  "
          f"{r['result'] if r and r.get('ok') else (r or 'timeout')}")
    ms, r = call("project.metadata")
    if r and r.get("ok"):
        print(f"  project.metadata   OK   name={r['result']['name']!r}")
    else:
        print(f"  project.metadata   FAIL  {r or 'timeout'}")
    print()

    print("--- 4. escritura real: setTempo + mixer.setVolume ---")
    ms, r = call("transport.status")
    bpm0 = r["result"]["bpm"] if r and r.get("ok") else 140.0
    newbpm = 128.0 if bpm0 != 128.0 else 132.0
    ms, r = call("transport.setTempo", {"bpm": newbpm})
    if r and r.get("ok"):
        got = r["result"]["bpm"]
        print(f"  setTempo({bpm0} -> {newbpm})  {'OK' if abs(got-newbpm)<0.6 else 'MISMATCH'}  "
              f"FL reporta {got:.2f}")
        call("transport.setTempo", {"bpm": bpm0})  # restaurar
    else:
        print(f"  setTempo  FAIL  {r or 'timeout'}")
    ms, r = call("mixer.setVolume", {"track": 0, "volume": 0.75})
    print(f"  mixer.setVolume    {'OK' if r and r.get('ok') else 'FAIL'}  "
          f"{(r['result']['name'] if r and r.get('ok') else (r or 'timeout'))}")
    print()

    print("--- 5. handlers: barrido de todos los de solo lectura ---")
    # Handlers que no modifican nada del proyecto (seguros de ejecutar).
    safe = [
        ("meta.ping", {}), ("meta.info", {}), ("meta.actions", {}),
        ("transport.status", {}), ("transport.length", {}),
        ("mixer.count", {}), ("mixer.allTracks", {}),
        ("mixer.fxSlots", {"track": 0}),
        ("channels.count", {}), ("channels.all", {}),
        ("patterns.count", {}), ("patterns.list", {}),
        ("playlist.trackCount", {}),
        ("plugins.isValid", {"track": 0, "slot": 0}),
        ("project.metadata", {}), ("project.version", {}),
        ("ui.focusedWindow", {}), ("ui.selectedChannel", {}),
        ("arrangement.current", {}), ("arrangement.list", {}),
    ]
    good, bad = [], []
    for act, prm in safe:
        ms, r = call(act, prm, timeout=5.0)
        if r and r.get("ok"):
            good.append((act, ms))
        else:
            err = r.get("error") if r else "TIMEOUT"
            bad.append((act, err))
    for act, ms in good:
        print(f"  OK    {act:<24} {ms:6.1f}ms")
    for act, err in bad:
        print(f"  FAIL  {act:<24} {err}")
    print()
    print(f"  {len(good)}/{len(safe)} handlers de solo lectura funcionan en FL real")
    print()

    print("--- 6. fiabilidad: 30 meta.ping seguidos ---")
    lat, ok, fail = [], 0, 0
    for i in range(30):
        ms, r = call("meta.ping", timeout=6.0)
        if r and r.get("ok"):
            ok += 1
            lat.append(ms)
        else:
            fail += 1
        time.sleep(0.05)
    print(f"  ok={ok}/30  fail={fail}/30")
    if lat:
        s = sorted(lat)
        print(f"  latencia: min={s[0]:.1f}  p50={s[len(s)//2]:.1f}  "
              f"p95={s[int(len(s)*0.95)]:.1f}  max={s[-1]:.1f} ms")
    print()

    print("--- 7. fiabilidad SIN wake MIDI (20 intentos) ---")
    lat2, ok2, fail2 = [], 0, 0
    for i in range(20):
        ms, r = call("meta.ping", wake=False, timeout=4.0)
        if r and r.get("ok"):
            ok2 += 1
            lat2.append(ms)
        else:
            fail2 += 1
    print(f"  ok={ok2}/20  fail={fail2}/20")
    if lat2:
        s = sorted(lat2)
        print(f"  latencia: min={s[0]:.1f}  p50={s[len(s)//2]:.1f}  max={s[-1]:.1f} ms")
    else:
        print(f"  latencia: n/a (nunca respondio sin wake)")
    print()

    hb2 = heartbeat()
    print("=" * 66)
    if hb2:
        print(f"  pumps: {hb['pump_count']} -> {hb2['pump_count']}")
        print(f"  last_action: {hb2['last_action']!r}  last_error: {hb2['last_error']!r}")
    print("=" * 66)
    return 0 if (ok >= 27 and not bad) else 1


if __name__ == "__main__":
    sys.exit(main())
