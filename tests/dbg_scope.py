import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log
for label, code in [
    ("A: que hay en el scope", "sorted([k for k in dir() if not k.startswith('_')])"),
    ("B: dir(mixer) crudo", "dir(mixer)[:25]"),
    ("C: len(dir(mixer))", "len(dir(mixer))"),
    ("D: mixer.count() directo", "mixer.count()"),
    ("E: patterns.count() directo", "patterns.count()"),
    ("F: general.getProjectFilePath", "general.getProjectFilePath"),
]:
    log("--- %s ---" % label)
    r = call("meta.exec", {"code": code})
    log("  " + (json.dumps(r)[:500] if r else "TIMEOUT"))
