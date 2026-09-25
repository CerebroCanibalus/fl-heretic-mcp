#!/usr/bin/env python3
"""Fiabilidad del file-RPC: 40 requests seguidos, sin mandar MIDI.

El pump real del bridge corre en OnMidiIn/OnMidiMsg (OnIdle nunca dispara en
FL 2025). Esta prueba mide si el wake-up ocurre sin que el cliente mande MIDI
nada, que es lo que decide si el daemon necesita enviar un byte de wake.
"""
import json
import os
import time
from pathlib import Path

RPC_REQ = Path(os.environ["USERPROFILE"]) / "Documents/Image-Line/FL Studio/Settings/Hardware/fLMCP Bridge/rpc_request.json"
RPC_RESP = RPC_REQ.with_name("rpc_response.json")

_id = int(time.time() * 1_000_000_000)


def call(action="meta.ping", params=None, timeout=8.0):
    global _id
    _id += 1
    rid = _id
    tmp = RPC_REQ.with_suffix(".json.tmp")
    tmp.write_text(json.dumps({"id": rid, "action": action, "params": params or {}}), encoding="utf-8")
    os.replace(tmp, RPC_REQ)
    t0 = time.monotonic()
    deadline = t0 + timeout
    while time.monotonic() < deadline:
        time.sleep(0.01)
        try:
            raw = RPC_RESP.read_text(encoding="utf-8")
        except Exception:
            continue
        if not raw.strip():
            continue
        try:
            resp = json.loads(raw)
        except Exception:
            continue
        if resp.get("id") != rid:
            continue
        return (time.monotonic() - t0) * 1000, bool(resp.get("ok"))
    return (time.monotonic() - t0) * 1000, False


def main():
    print("40 requests seguidos a meta.ping, SIN mandar MIDI")
    print("-" * 58)
    lat, ok, fail, worst = [], 0, 0, 0.0
    for i in range(40):
        ms, success = call()
        if success:
            ok += 1
            lat.append(ms)
            worst = max(worst, ms)
            bar = "#" * int(min(ms, 500) / 10)
            print(f"  {i+1:2d}  {ms:7.1f}ms  {bar}")
        else:
            fail += 1
            print(f"  {i+1:2d}  TIMEOUT (>8000ms)  <<< FALLO")
    print("-" * 58)
    print(f"  ok      : {ok}/40")
    print(f"  fallidos: {fail}/40")
    if lat:
        lat_sorted = sorted(lat)
        print(f"  latencia: min={min(lat):.1f}  p50={lat_sorted[len(lat)//2]:.1f}  "
              f"p95={lat_sorted[int(len(lat)*0.95)]:.1f}  max={max(lat):.1f} ms")
    print()
    print("VEREDICTO:", "FIABLE sin MIDI" if fail == 0 else f"NO FIABLE: {fail} timeouts -> hace falta wake por MIDI")


if __name__ == "__main__":
    main()
