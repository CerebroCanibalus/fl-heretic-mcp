import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
r = call("meta.ping")
log("  meta.ping -> " + (json.dumps(r)[:200] if r else "TIMEOUT"))
