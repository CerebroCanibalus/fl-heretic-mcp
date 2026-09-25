#!/usr/bin/env python3
"""Compara TCP 9876 vs file-RPC contra el fLMCP Bridge real en FL Studio.

Uso:  python tests/test_transports.py

Salida: resultado + latencia de cada transporte.
"""
import json
import os
import socket
import struct
import sys
import time
from pathlib import Path

BRIDGE_HOST = "127.0.0.1"
BRIDGE_PORT = 9876
HEADER = struct.Struct(">I")
MAX_FRAME = 16 * 1024 * 1024


def script_dir() -> Path:
    base = Path(os.environ.get("USERPROFILE", str(Path.home()))) / "Documents" / "Image-Line" / "FL Studio" / "Settings"
    return base / "Hardware" / "fLMCP Bridge"


RPC_REQ = script_dir() / "rpc_request.json"
RPC_RESP = script_dir() / "rpc_response.json"

_id = [int(time.time() * 1_000_000_000)]


def next_id() -> int:
    _id[0] += 1
    return _id[0]


def unpack_response(data: bytes):
    if len(data) < HEADER.size:
        return None
    (length,) = HEADER.unpack(data[:HEADER.size])
    if len(data) < HEADER.size + length:
        return None
    return json.loads(data[HEADER.size : HEADER.size + length].decode("utf-8"))


# ---------------------------------------------------------------- TCP
def test_tcp(action="meta.ping", params=None, timeout=6.0):
    t0 = time.monotonic()
    try:
        s = socket.create_connection((BRIDGE_HOST, BRIDGE_PORT), timeout=timeout)
    except Exception as e:
        return {"ok": False, "latency_ms": None, "error": f"connect: {type(e).__name__}: {e}"}
    try:
        s.settimeout(timeout)
        rid = next_id()
        body = json.dumps({"id": rid, "action": action, "params": params or {}}).encode("utf-8")
        s.sendall(HEADER.pack(len(body)) + body)
        buf = b""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            chunk = s.recv(65536)
            if not chunk:
                break
            buf += chunk
            resp = unpack_response(buf)
            if resp is not None and resp.get("id") == rid:
                return {
                    "ok": bool(resp.get("ok")),
                    "latency_ms": round((time.monotonic() - t0) * 1000, 1),
                    "result": resp.get("result"),
                    "error": resp.get("error"),
                }
        return {"ok": False, "latency_ms": round((time.monotonic() - t0) * 1000, 1), "error": f"timeout tras {timeout}s, recibidos {len(buf)}B"}
    except Exception as e:
        return {"ok": False, "latency_ms": round((time.monotonic() - t0) * 1000, 1), "error": f"{type(e).__name__}: {e}"}
    finally:
        s.close()


# ---------------------------------------------------------- file-RPC
def test_file(action="meta.ping", params=None, timeout=6.0):
    t0 = time.monotonic()
    rid = next_id()
    req = {"id": rid, "action": action, "params": params or {}}
    try:
        tmp = RPC_REQ.with_suffix(".json.tmp")
        tmp.write_text(json.dumps(req, ensure_ascii=False), encoding="utf-8")
        os.replace(tmp, RPC_REQ)  # atomico: el daemon esta fuera del sandbox
    except Exception as e:
        return {"ok": False, "latency_ms": None, "error": f"write req: {type(e).__name__}: {e}"}

    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        time.sleep(0.02)
        try:
            raw = RPC_RESP.read_text(encoding="utf-8")
        except Exception:
            continue
        if not raw.strip():
            continue
        try:
            resp = json.loads(raw)  # puede haber escritura parcial
        except Exception:
            continue
        if resp.get("id") != rid:
            continue
        return {
            "ok": bool(resp.get("ok")),
            "latency_ms": round((time.monotonic() - t0) * 1000, 1),
            "result": resp.get("result"),
            "error": resp.get("error"),
        }
    return {"ok": False, "latency_ms": round((time.monotonic() - t0) * 1000, 1), "error": f"timeout tras {timeout}s"}


def main():
    print("=" * 66)
    print("  fLMCP Bridge — comparativa de transportes")
    print("=" * 66)
    print(f"  script_dir : {script_dir()}")
    print(f"  TCP        : {BRIDGE_HOST}:{BRIDGE_PORT}")
    print()

    # Puerto abierto?
    try:
        probe = socket.create_connection((BRIDGE_HOST, BRIDGE_PORT), timeout=1.0)
        probe.close()
        listening = True
    except Exception:
        listening = False
    print(f"  puerto {BRIDGE_PORT} escuchando : {'SI' if listening else 'NO'}")
    print()

    print("--- TCP ---")
    t = test_tcp()
    print(f"  ok={t['ok']}  latencia={t['latency_ms']}ms")
    if t.get("result"):
        print(f"  result: {json.dumps(t['result'])[:200]}")
    if t.get("error"):
        print(f"  error : {t['error'][:300]}")
    print()

    print("--- file-RPC ---")
    f = test_file()
    print(f"  ok={f['ok']}  latencia={f['latency_ms']}ms")
    if f.get("result"):
        print(f"  result: {json.dumps(f['result'])[:200]}")
    if f.get("error"):
        print(f"  error : {f['error'][:300]}")
    print()

    # 5 muestras cada uno para ver variacion
    print("--- 5 muestras transport.status ---")
    for name, fn in (("TCP", test_tcp), ("file", test_file)):
        lat = []
        oks = 0
        for _ in range(5):
            r = fn("transport.status")
            if r["ok"]:
                oks += 1
                lat.append(r["latency_ms"])
            time.sleep(0.15)
        if lat:
            print(f"  {name:5s} ok={oks}/5  min={min(lat)}ms  max={max(lat)}ms  media={round(sum(lat)/len(lat),1)}ms")
        else:
            print(f"  {name:5s} ok={oks}/5  (todos fallaron)")
    print()

    # Action pesada: lista de canales
    print("--- channels.list (respuesta grande) ---")
    for name, fn in (("TCP", test_tcp), ("file", test_file)):
        r = fn("channels.list")
        size = len(json.dumps(r.get("result"))) if r.get("result") else 0
        print(f"  {name:5s} ok={r['ok']}  latencia={r['latency_ms']}ms  payload={size}B  {r.get('error') or ''}")
    print()
    print("=" * 66)


if __name__ == "__main__":
    sys.exit(main())
