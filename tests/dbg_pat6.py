import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
log("--- limite real del pool, escaneando desde 1 ---")
code = ("out = []\n"
        "for i in range(1, 120):\n"
        "    try:\n"
        "        d = patterns.isPatternDefault(i)\n"
        "        out.append((i, d))\n"
        "    except Exception as e:\n"
        "        out.append(('STOP', i, type(e).__name__))\n"
        "        break\n"
        "out")
r = call("meta.exec", {"code": code})
res = (r or {}).get("result") or {}
v = res.get("result") if res.get("ok", True) else "ERR " + str(res.get("error"))[:200]
log("  " + json.dumps(v)[:700])
if isinstance(v, list) and v:
    log("")
    log("  primeros 12: " + json.dumps(v[:12]))
    log("  ultimos 3 : " + json.dumps(v[-3:]))
