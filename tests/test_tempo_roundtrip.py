#!/usr/bin/env python3
"""Aisla el paso de ida y vuelta del tempo en su propia sesion MCP.

El problema: en el E2E completo, la misma tanda que hace setTempo(111) ->
get_tempo termina con setTempo(130). Como el MCP server las procesa y el
bridge tiene una latencia de ~10 ms, el 130 puede aplicarse ANTES de que se
lea el 111, y la comprobacion lee 130 y falla sin que haya bug.

Aqui se hace en una sesion propia, y con una espera entre medias. Si asi
falla, el fallo es real.
"""
import json
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MCP = os.path.join(ROOT, "target", "release", "fl-heretic-mcp.exe")

INIT = [
    {"jsonrpc": "2.0", "id": 1, "method": "initialize",
     "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                "clientInfo": {"name": "tempo", "version": "1"}}},
    {"jsonrpc": "2.0", "method": "notifications/initialized"},
]


def call(ids, name, args):
    """Una tool por sesion MCP, para que no se solapen."""
    batch = INIT + [{"jsonrpc": "2.0", "id": i, "method": "tools/call",
                     "params": {"name": name, "arguments": args}} for i in ids]
    data = "".join(json.dumps(r) + "\n" for r in batch)
    p = subprocess.run([MCP], input=data, capture_output=True, text=True,
                       env=dict(os.environ), timeout=60)
    out = {}
    for line in p.stdout.splitlines():
        if not line.strip():
            continue
        m = json.loads(line)
        if "id" in m:
            out[m["id"]] = m
    return out


def value(resp):
    r = resp.get("result", {})
    for c in r.get("content", []):
        if c.get("type") == "text":
            try:
                return json.loads(c["text"])
            except json.JSONDecodeError:
                return c["text"]
    return resp.get("error", {})


def main():
    print("=" * 70)
    print("  Ida y vuelta del tempo, aislada")
    print("=" * 70)

    # Lee el tempo de partida.
    r = call([2], "fl_get_tempo", {})
    start = value(r.get(2, {})).get("bpm")
    print("  tempo de partida: %s" % start)

    # Elige un valor distinto al actual para que el cambio sea observable.
    target = 111.0 if abs((start or 0) - 111) > 1 else 122.0
    print("  escribiendo %s ..." % target)
    r = call([3], "fl_set_tempo", {"bpm": target})
    err = value(r.get(3, {}))
    ok = isinstance(err, dict) and "bpm" in err
    print("  set_tempo devolvio: %s" % json.dumps(err)[:120])
    if not ok:
        print("  [XX] la escritura fallo")
        return 1

    time.sleep(0.8)
    r = call([4], "fl_get_tempo", {})
    got = value(r.get(4, {})).get("bpm")
    print("  releyendo ... %s" % got)

    if isinstance(got, (int, float)) and abs(got - target) < 0.01:
        print("  [OK] la ida y vuelta real: %s -> %s" % (start, got))
    else:
        print("  [XX] se escribio %s y se leyo %s" % (target, got))
        return 1

    # Restaurar.
    call([5], "fl_set_tempo", {"bpm": start})
    print("  restaurado a %s" % start)
    return 0


if __name__ == "__main__":
    sys.exit(main())
