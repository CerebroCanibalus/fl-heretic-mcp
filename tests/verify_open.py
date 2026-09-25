import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
r = call("meta.ping", timeout=5)
log("  meta.ping -> " + ("OK" if r and r.get("ok") else "SIN RESPUESTA"))
m = call("project.metadata", timeout=5)
if m and m.get("ok"):
    d = m["result"]
    log("  title=%r has_file=%s changed=%s" % (d.get("title"), d.get("has_file"), d.get("changed")))
    log("  canales=%s patrones=%s pistas=%s tempo=%s" % (
        d.get("channel_count"), d.get("pattern_count"), d.get("mixer_tracks"), d.get("tempo")))
