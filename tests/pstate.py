import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
r = call("project.metadata")
if r and r.get("ok"):
    d = r["result"]
    for k in ("title","has_file","changed","channel_count","pattern_count"):
        log("  %-14s = %s" % (k, d.get(k)))
