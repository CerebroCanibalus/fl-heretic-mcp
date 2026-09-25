import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
for label, code in [
    ("dir(ui) con menu", "[k for k in dir(ui) if 'enu' in k.lower() or 'FPT' in k]"),
    ("FPT_Menu", "getattr(midi, 'FPT_Menu', 'AUSENTE')"),
    ("dir(device) con menu", "[k for k in dir(device) if 'enu' in k.lower()]"),
    ("midi con FPT_ (todos)", "len([k for k in dir(midi) if k.startswith('FPT_')])"),
]:
    r = call("meta.exec", {"code": code})
    res = (r or {}).get("result") or {}
    log("  %-26s %s" % (label, json.dumps(res.get("result"))[:200] if res.get("ok", True) else "ERR " + str(res.get("error"))[:100]))
