import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log

log("--- estado via la action patterns.list (indices reales) ---")
r = call("patterns.list")
log("  " + json.dumps(r)[:400])
log("")
log("--- probar setPatternName en indice alto y ver si persiste ---")
for idx in (1, 50, 100):
    r = call("patterns.rename", {"index": idx, "name": "PROBE_%d" % idx})
    ok = r and r.get("ok")
    log("  rename idx=%-4d -> %s" % (idx, json.dumps(r.get("result")) if ok else "ERR " + str((r or {}).get("error"))[:120]))
log("")
log("--- re-leer los nombres ---")
r = call("patterns.list")
log("  " + json.dumps(r)[:600])
