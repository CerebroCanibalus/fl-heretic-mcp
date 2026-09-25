import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
code = "out = []\nfor i in range(0, 40):\n    try:\n        patterns.isPatternDefault(i)\n        out.append(i)\n    except Exception as e:\n        out.append('STOP@%d' % i)\n        break\nout"
r = call("meta.exec", {"code": code})
res = (r or {}).get("result") or {}
log("  " + json.dumps(res.get("result"))[:500] if res.get("ok", True) else "  ERR " + str(res.get("error"))[:200])
