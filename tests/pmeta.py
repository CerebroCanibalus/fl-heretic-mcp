import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
r = call("project.metadata")
log(json.dumps(r, indent=1)[:700] if r else "timeout")
