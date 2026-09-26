#!/usr/bin/env python3
"""Cliente MCP por stdio que MANTIENE stdin abierto.

Por que existe: `subprocess.run(input=...)` cierra stdin en cuanto ha escrito
todo, y el servidor de FlojoMCP responde "input stream terminated" y se va a los
5 s ("timed out draining in-flight responses"). Cualquier tool que tarde mas de
5 s pierde la respuesta y parece un fallo. En production el cliente MCP mantiene
el pipe vivo toda la sesion; este arnes solo lo hacia mal.

    python tools/mcp_call.py daw_debug '{"op":"status"}'
    python tools/mcp_call.py daw_do   '{"action":"track_get_all","params":{}}'
"""
import json
import os
import subprocess
import sys
import threading

MCP = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "target", "verify", "release", "daw-heretic-mcp.exe",
)


def llama(nombre, argumentos, timeout=90):
    """Una llamada, con stdin vivo hasta recibir la respuesta."""
    lote = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize",
         "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                    "clientInfo": {"name": "mcp_call", "version": "1"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 42, "method": "tools/call",
         "params": {"name": nombre, "arguments": argumentos}},
    ]
    p = subprocess.Popen([MCP], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True,
                         env=dict(os.environ), encoding="utf-8")

    # Leer en un hilo evita el pipe bloqueado mientras se escribe.
    salida = []
    def leer():
        for linea in p.stdout:
            if linea.strip():
                salida.append(linea)
    t = threading.Thread(target=leer, daemon=True)
    t.start()

    for m in lote:
        p.stdin.write(json.dumps(m) + "\n")
        p.stdin.flush()
    # El ultimo mensaje NO lleva newline extra que cierre el flujo: hay que
    # dejar el pipe abierto hasta tener la respuesta.

    import time
    limite = time.time() + timeout
    while time.time() < limite:
        for linea in list(salida):
            try:
                m = json.loads(linea)
            except Exception:
                continue
            if m.get("id") == 42:
                p.kill()
                r = m.get("result") or {}
                txt = "".join(c.get("text", "") for c in r.get("content", []))
                try:
                    return json.loads(txt)
                except Exception:
                    return {"_texto": txt, "_error": m.get("error")}
        time.sleep(0.05)
    p.kill()
    return {"_timeout": f"sin respuesta en {timeout}s"}


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print(__doc__)
        sys.exit(1)
    args = json.loads(sys.argv[2])
    print(json.dumps(llama(sys.argv[1], args), ensure_ascii=False, indent=1))
