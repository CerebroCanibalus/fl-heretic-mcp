import json, io
d = json.load(io.open("tests/fl_api_dump.json", encoding="utf-8"))
keys = ["save", "path", "file", "name", "project", "export", "render", "write", "open", "dir"]
for mod, fns in d.items():
    if mod == "midi":
        continue
    hits = [f for f in fns if any(k in f.lower() for k in keys)]
    if hits:
        print("%-12s %s" % (mod, ", ".join(hits)))
