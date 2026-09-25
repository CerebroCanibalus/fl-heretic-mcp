import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log

log("--- Understand the REAL pattern state ---")
code = ("out = {}\n"
        "out['patternCount'] = patterns.patternCount()\n"
        "out['patternNumber'] = patterns.patternNumber()\n"
        "out['patternMax'] = patterns.patternMax()\n"
        "out['getPatternGroupCount'] = patterns.getPatternGroupCount()\n"
        "out")
r = call("meta.exec", {"code": code})
log("  " + json.dumps((r or {}).get("result", {}).get("result"))[:300])
log("")

log("--- How many patterns does the pattern list ACTUALLY see? ---")
# Buscar el limite real probando isPatternDefault y getPatternName juntos,
# y tambien selectedChannel para ver si hay canales (indicador de proyecto real)
for label, code in [
    ("nombres 0..12", "[patterns.getPatternName(i) for i in range(0,12)]"),
    ("nombres 0..12 (2a llamada)", "[patterns.getPatternName(i) for i in range(0,12)]"),
    ("patternNumber tras selects", "patterns.patternNumber()"),
    ("clone del actual (patron %d)" % 1, "patterns.clonePattern()"),
    ("patternCount tras clone", "patterns.patternCount()"),
    ("nombres tras clone", "[patterns.getPatternName(i) for i in range(0,12)]"),
]:
    r = call("meta.exec", {"code": code})
    res = (r or {}).get("result") or {}
    v = json.dumps(res.get("result"))[:200] if res.get("ok", True) else "ERR " + str(res.get("error"))[:120]
    log("  %-34s %s" % (label, v))
