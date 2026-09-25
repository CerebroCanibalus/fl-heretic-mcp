import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
r = call("meta.exec", {"code": "[k for k in dir(midi) if k.startswith('FPT_')]"})
res = (r or {}).get("result") or {}
v = res.get("result") if res.get("ok", True) else None
if v:
    log("  FPT_ disponibles (%d):" % len(v))
    for i in range(0, len(v), 4):
        log("    " + "".join("%-26s" % x for x in v[i:i+4]))
log("")
r = call("meta.exec", {"code": "[k for k in dir(midi) if k.startswith('FPN_')]"})
res = (r or {}).get("result") or {}
v2 = res.get("result") if res.get("ok", True) else None
if v2:
    log("  FPN_ (funciones de parametro) (%d): %s" % (len(v2), ", ".join(v2)))
