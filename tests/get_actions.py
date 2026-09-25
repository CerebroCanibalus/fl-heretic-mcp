import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
r = call("meta.actions")
log(json.dumps(r)[:400])
acts = r.get("result", {}).get("actions") or r.get("result", {}).get("result", {}).get("actions")
if acts:
    with open("tests/bridge_actions.json", "w", encoding="utf-8") as f:
        json.dump(acts, f, indent=1)
    log("TOTAL: %d" % len(acts))
