import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log

log("=" * 72)
log("  AUDITORIA: que contiene realmente TemplateProject.flp")
log("=" * 72)
log("")

r = call("channels.all")
if r and r.get("ok"):
    chs = r["result"].get("channels", [])
    log("  canales: %d" % len(chs))
    for c in chs:
        log("    [%d] %-20r vol=%s pan=%s pitch=%s color=%s target=%s"
            % (c["index"], c["name"], c["volume"], c["pan"], c["pitch"],
               c["color"], c["target_fx_track"]))
log("")

r = call("mixer.allTracks")
if r and r.get("ok"):
    tr = r["result"].get("tracks", [])
    log("  pistas de mixer: %d" % len(tr))
    for t in tr:
        log("    [%d] %-22r vol=%s" % (t["track"], t["name"], t["volume"]))
log("")

r = call("patterns.list")
if r and r.get("ok"):
    d = r["result"]
    log("  patrones: %d  actual=%s" % (d.get("count", 0), d.get("current")))
    for p in d.get("patterns", []):
        log("    [%d] %r default=%s" % (p["index"], p["name"], p.get("is_default")))
log("")

r = call("playlist.trackCount")
if r and r.get("ok"):
    log("  pistas de playlist: %s" % r["result"].get("count"))
log("")

# tipo de canal: hay instrumentos cargados?
code = ("out = []\n"
        "for i in range(channels.channelCount(True)):\n"
        "    try:\n"
        "        out.append((i, channels.getChannelType(i), channels.getChannelName(i)))\n"
        "    except Exception as e:\n"
        "        out.append((i, 'ERR', str(e)))\n"
        "out")
r = call("meta.exec", {"code": code})
if r and r.get("ok"):
    log("  tipo de cada canal (None = generico/vacio, otro = instrumento cargado):")
    for i, t, n in (r["result"].get("result") or []):
        log("    [%d] type=%-30r name=%r" % (i, t, n))
log("")

# hay muestras en el browser?
code2 = ("out = {}\n"
         "out['browser_root'] = None\n"
         "out['project_title'] = general.getProjectTitle()\n"
         "out")
r = call("meta.exec", {"code": code2})
if r and r.get("ok"):
    log("  project_title: %r" % (r["result"].get("result") or {}).get("project_title"))
