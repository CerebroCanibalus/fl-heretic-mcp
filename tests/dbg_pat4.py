import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
log("--- donde salta isPatternDefault ---")
r = call("meta.exec", {"code": "patterns.isPatternDefault(0)"})
log("  idx 0 -> " + json.dumps((r or {}).get("result"))[:200])
r = call("meta.exec", {"code": "patterns.isPatternDefault(1)"})
log("  idx 1 -> " + json.dumps((r or {}).get("result"))[:200])
log("")
log("--- buscar el limite con try/except (ahora meta.exec devuelve el bloque) ---")
code = "out = []\nfor i in range(0, 40):\n    try:\n        patterns.isPatternDefault(i)\n        out.append(i)\n    except Exception:\n        out.append('stop@%d' % i)\n        break\nout"
r = call("meta.exec", {"code": code})
log("  indices validos: " + json.dumps((r or {}).get("result"))[:400])
