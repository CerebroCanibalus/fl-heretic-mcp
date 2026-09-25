import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
for label, code in [
    ("patternMax()", "patterns.patternMax()"),
    ("patternNumber()", "patterns.patternNumber()"),
    ("getPatternName hasta 20", "[patterns.getPatternName(i) for i in range(0,20)]"),
    ("getPatternName hasta 40", "[patterns.getPatternName(i) for i in range(0,40)]"),
    ("primer indice que lanza", "[i for i in range(0,60) if patterns.getPatternName(i) == '']"),
    ("patternCount vs count()", "(patterns.patternCount(), patterns.patternMax())"),
]:
    r = call("meta.exec", {"code": code})
    res = (r or {}).get("result") or {}
    log("%-32s %s" % (label, json.dumps(res.get("result"))[:400] if res.get("ok", True) else "ERR: " + str(res.get("error"))[:200]))
