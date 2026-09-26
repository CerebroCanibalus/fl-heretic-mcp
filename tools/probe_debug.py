#!/usr/bin/env python3
"""Prueba `daw_debug` contra Reaper de verdad.

La prueba que importa no es la que funciona: es la que antes CONGELABA el DAW.
`daw_probe.lua:27: attempt to call a nil value (field 'GetTrackChannelInfo')`
dejo Reaper con un dialogo modal y el agente sin mas pista que "no respondio
en 10 s". Con el guardian, el mismo error tiene que volver como TEXTO.

    python tools/probe_debug.py
"""
import json
import os
import subprocess
import sys

MCP = os.path.join("target", "verify", "release", "daw-heretic-mcp.exe")


def mcp(*tools):
    """Manda un lote de llamadas MCP por stdio y devuelve las respuestas."""
    lote = [{
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "probe", "version": "1"}},
    }, {"jsonrpc": "2.0", "method": "notifications/initialized"}]
    for i, (nombre, args) in enumerate(tools):
        lote.append({"jsonrpc": "2.0", "id": 100 + i, "method": "tools/call",
                     "params": {"name": nombre, "arguments": args}})
    p = subprocess.run([MCP],
                       input="".join(json.dumps(r) + "\n" for r in lote),
                       capture_output=True, text=True, env=dict(os.environ),
                       timeout=180)
    fuera = {}
    for linea in p.stdout.splitlines():
        if not linea.strip():
            continue
        m = json.loads(linea)
        if isinstance(m.get("id"), int) and m["id"] >= 100:
            r = m.get("result") or {}
            txt = "".join(c.get("text", "") for c in r.get("content", []))
            try:
                fuera[m["id"] - 100] = json.loads(txt)
            except Exception:
                fuera[m["id"] - 100] = txt
    return fuera


def linea(t, ok, detalle):
    print("  %-46s %s  %s" % (t, "OK " if ok else "MAL", detalle))


def main():
    if not os.path.exists(MCP):
        print("  falta %s; compila antes" % MCP)
        return 1

    r = mcp(
        ("daw_debug", {"op": "status"}),
        ("daw_debug", {"op": "eval", "code": "return reaper.CountTracks(0)"}),
        # El fallo que antes congelaba el DAW, exacto:
        ("daw_debug", {"op": "eval", "code": "return reaper.GetTrackChannelInfo(0)"}),
        # Y uno de sintaxis, que tampoco debe abrir nada:
        ("daw_debug", {"op": "eval", "code": "return (("}),
        # Un valor compuesto, para ver que el codificador JSON aguanta:
        ("daw_debug", {"op": "eval", "code":
            "local t = reaper.GetTrack(0, 0)\n"
            "return {pistas = reaper.CountTracks(0),"
            " nombre = reaper.GetSetMediaTrackInfo_String(t, 'P_NAME', '', false),"
            " fx = reaper.TrackFX_GetCount(t),"
            " pico_db = reaper.Track_GetPeakHoldDB(t, 0, false)}"}),
        ("daw_debug", {"op": "modal"}),
        ("daw_debug", {"op": "log", "limit": 5}),
    )

    d = r.get(0, {})
    linea("status: bridge vivo", d.get("bridge", {}).get("ok") is True,
          str(d.get("diagnostico", ""))[:70])
    linea("status: sin dialogos bloqueando",
          not d.get("dialogos_bloqueando"),
          "ninguno" if not d.get("dialogos_bloqueando") else str(d["dialogos_bloqueando"]))

    ok = r.get(1, {})
    linea("eval que funciona", ok.get("ok") is True and ok.get("resultado") == 1,
          "CountTracks(0) = %s en %s ms" % (ok.get("resultado"), ok.get("ms")))

    # ESTA es la prueba. Antes: dialogo modal + DAW congelado + "no respondio".
    roto = r.get(2, {})
    es_error = "GetTrackChannelInfo" in json.dumps(roto, ensure_ascii=False)
    linea("eval con error -> TEXTO, no modal", es_error,
          (json.dumps(roto, ensure_ascii=False)[:90] if not ok else ""))

    sintaxis = r.get(3, {})
    linea("eval con error de sintaxis -> TEXTO",
          "sintaxis" in json.dumps(sintaxis, ensure_ascii=False),
          json.dumps(sintaxis, ensure_ascii=False)[:70])

    comp = r.get(4, {})
    val = comp.get("resultado") or {}
    linea("eval con tabla -> JSON", isinstance(val, dict) and "pistas" in val,
          json.dumps(val, ensure_ascii=False)[:100])

    mod = r.get(5, {})
    linea("modal: Reaper NO esta congelado", mod.get("bloqueando") == 0,
          "dialogos: %s" % mod.get("bloqueando"))

    print()
    print("  ultimo error recibido tal cual:")
    print("    %s" % json.dumps(roto, ensure_ascii=False)[:400])
    print()
    log = r.get(6, {})
    for l in (log.get("ultimas") or [])[-4:]:
        print("    log | %s" % l)
    return 0


if __name__ == "__main__":
    sys.exit(main())
