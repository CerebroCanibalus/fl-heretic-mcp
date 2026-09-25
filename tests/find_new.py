import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log

log("=== TODAS las FPT_ (79) ===")
r = call("meta.exec", {"code": "sorted([k for k in dir(midi) if k.startswith('FPT_')])"})
if r and r.get("ok"):
    for k in (r["result"].get("result") or []):
        log("  " + k)

log("")
log("=== hay FPT para New / Open / Close? ===")
r = call("meta.exec", {"code": "sorted([k for k in dir(midi) if k.startswith('FPT_') and any(x in k.lower() for x in ('new','open','close','file','project'))])"})
if r and r.get("ok"):
    log("  " + json.dumps(r["result"].get("result")))

log("")
log("=== general: alguna forma de proyecto nuevo? ===")
r = call("meta.exec", {"code": "sorted([k for k in dir(general) if any(x in k.lower() for x in ('new','open','close','project','file','save'))])"})
if r and r.get("ok"):
    log("  " + json.dumps(r["result"].get("result")))

log("")
log("=== transport: globalTransport yFriends? ===")
r = call("meta.exec", {"code": "sorted([k for k in dir(transport) if 'global' in k.lower() or 'file' in k.lower()])"})
if r and r.get("ok"):
    log("  " + json.dumps(r["result"].get("result")))
