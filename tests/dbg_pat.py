import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
log("--- patronCount ---")
r = call("meta.exec", {"code": "patterns.patternCount()"})
log(json.dumps(r)[:300])
log("")
log("--- bloque multi-linea simple ---")
code = "out = {}\nout['a'] = 1\nfor i in range(3):\n    out[str(i)] = i*i\nout"
r = call("meta.exec", {"code": code})
log(json.dumps(r)[:400])
log("")
log("--- getPatternName en bucle ---")
code2 = "[ (i, patterns.getPatternName(i)) for i in range(0,8) ]"
r = call("meta.exec", {"code": code2})
log(json.dumps(r)[:500])
