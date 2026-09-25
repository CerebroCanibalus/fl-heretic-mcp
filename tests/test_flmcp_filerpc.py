"""Test file-RPC directo: enviar request, esperar response."""
import json
import sys
import time
from pathlib import Path

SCRIPT_DIR = Path.home() / "Documents" / "Image-Line" / "FL Studio" / "Settings" / "Hardware" / "fLMCP Bridge"
RPC_REQ = SCRIPT_DIR / "rpc_request.json"
RPC_RESP = SCRIPT_DIR / "rpc_response.json"

# 1. Limpiar respuesta previa para detectar nueva
print("[1] Limpiando rpc_response.json...")
RPC_RESP.write_text("", encoding="utf-8")

# 2. Construir y escribir request (id debe ser MAYOR que el último procesado por FL)
# Usamos timestamp en nanosegundos como id
import time as _t
req_id = int(_t.time() * 1_000_000_000)
request = {
    "id": req_id,
    "action": "meta.ping",
    "params": {},
}
print(f"[2] Escribiendo rpc_request.json con id={req_id}, action=meta.ping...")
RPC_REQ.write_text(json.dumps(request, ensure_ascii=False), encoding="utf-8")

# 3. Esperar respuesta
print("[3] Esperando hasta 10s por respuesta...")
start = time.time()
response_text = ""
deadline = start + 10.0
while time.time() < deadline:
    time.sleep(0.2)
    if RPC_RESP.exists():
        text = RPC_RESP.read_text(encoding="utf-8").strip()
        if text and str(req_id) in text:
            response_text = text
            break

elapsed = time.time() - start
print(f"[4] Esperado {elapsed:.1f}s")

if response_text:
    print(f"[5] RESPUESTA RECIBIDA:")
    try:
        resp = json.loads(response_text)
        print(json.dumps(resp, indent=2, ensure_ascii=False))
    except Exception as e:
        print(f"    (parse error: {e})")
        print(f"    raw: {response_text}")
else:
    print(f"[5] NO HAY RESPUESTA después de 10s")
    print("    El archivo rpc_response.json contiene:")
    if RPC_RESP.exists():
        print(f"    {RPC_RESP.read_text(encoding='utf-8')}")
    print("\n    Diagnostico: OnIdle no se esta disparando en este FL build.")
    print("    Ver FL Studio > View > Script output para ver logs de fLMCP.")
    sys.exit(1)