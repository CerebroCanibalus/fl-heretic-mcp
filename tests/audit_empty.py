import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
r = call("meta.ping", timeout=8)
log("bridge: " + ("OK" if r and r.get("ok") else "SIN RESPONDA"))
if r and r.get("ok"):
    m = call("project.metadata", timeout=8)
    if m and m.get("ok"):
        d = m["result"]
        log("")
        log("  AUDITORIA de audit-vacio.flp")
        log("  -------------------------------------")
        log("  title      = %r" % d.get("title"))
        log("  has_file   = %s" % d.get("has_file"))
        log("  changed    = %s" % d.get("changed"))
        log("  tempo      = %s" % d.get("tempo"))
        log("  canales    = %s" % d.get("channel_count"))
        log("  patrones   = %s" % d.get("pattern_count"))
        log("  pistas mix = %s" % d.get("mixer_tracks"))
    ch = call("channels.all", timeout=8)
    if ch and ch.get("ok"):
        chs = ch["result"].get("channels", [])
        log("")
        log("  canales: %d" % len(chs))
        for c in chs:
            log("    [%d] %r vol=%s" % (c["index"], c["name"], c["volume"]))
        if not chs:
            log("    (ninguno: proyecto realmente vacio)")
