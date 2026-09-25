#!/usr/bin/env python3
"""Lista la FL API real por modulo, via meta.exec.

Sirve para arreglar a mano los handlers que usan nombres de funcion
inventados. Una llamada por modulo con un comprehension simple: los bloques
try/except multi-linea se comportan mal en el exec().
"""
import sys
import json

sys.path.insert(0, "tests")
from introspect_api import call, log  # noqa: E402

MODULES = [
    "mixer", "patterns", "general", "ui", "arrangement",
    "transport", "plugins", "channels", "playlist", "device", "midi",
]


def main():
    log("=" * 74)
    log("  FL API real — funciones disponibles por modulo")
    log("=" * 74)
    total = 0
    data = {}
    for mod in MODULES:
        r = call("meta.exec", {"code": f"[k for k in dir({mod}) if not k.startswith('_')]"})
        if not r or not r.get("ok"):
            log(f"  {mod}: ERROR {json.dumps(r)[:200] if r else 'TIMEOUT'}")
            continue
        fns = r["result"]["result"]
        data[mod] = fns
        total += len(fns)
        log("")
        log(f"  {mod} ({len(fns)}):")
        for i in range(0, len(fns), 5):
            log("    " + "".join(f"{x:<26}" for x in fns[i:i+5]))
    log("")
    log("=" * 74)
    log(f"  TOTAL: {total} funciones en {len(data)} modulos")
    log("=" * 74)
    with open("tests/fl_api_dump.json", "w", encoding="utf-8") as f:
        json.dump(data, f, indent=1, ensure_ascii=False)
    log("  volcado en tests/fl_api_dump.json")
    return 0


if __name__ == "__main__":
    sys.exit(main())
