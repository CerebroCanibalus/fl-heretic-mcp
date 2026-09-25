import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
for label, code in [
    ("isPatternDefault(0..12)", "[patterns.isPatternDefault(i) for i in range(0,13)]"),
    ("isPatternSelected(0..5)", "[patterns.isPatternSelected(i) for i in range(0,6)]"),
    ("patternCount otra vez", "patterns.patternCount()"),
    ("getPatternGroupCount", "patterns.getPatternGroupCount()"),
    ("patternMax()", "patterns.patternMax()"),
]:
    r = call("meta.exec", {"code": code})
    res = (r or {}).get("result") or {}
    log("%-30s %s" % (label, json.dumps(res.get("result"))[:200] if res.get("ok", True) else "ERR " + str(res.get("error"))[:150]))
