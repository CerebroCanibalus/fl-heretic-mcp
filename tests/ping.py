import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
r = call("meta.ping", timeout=4)
log("bridge: " + ("OK" if r and r.get("ok") else "NO RESPONDE " + str(r)))
if r and r.get("ok"):
    log("pumps: " + str(r["result"].get("pump_count")))
