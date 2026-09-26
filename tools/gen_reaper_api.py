#!/usr/bin/env python3
"""Extrae la API REAL de Reaper que usa el bridge, leyendo su Lua.

Por que esto y no la documentacion:

El bridge expone 162 acciones con NOMBRES PROPIOS (`track_set_volume`,
`fx_get_params`) que no existen en Reaper. La API real de Reaper es un
namespace plano de ~900 funciones con su propia nomenclatura
(`SetMediaTrackInfo_Value`, `TrackFX_AddByName`, `GetSetProjectInfo`...).

Traducir de uno a otro a mano es inventar. Este script saca del propio Lua del
bridge **que funciones reales llama y con que parametros**, que es la unica
fuente verificable: si el bridge la llama, existe y funciona.

Salida: catalogo de la API real, para decidir si se expone la API nativa en
vez de la capa inventada del bridge.

    python tools/gen_reaper_api.py [ruta_al_bridge.lua]
"""
import collections
import io
import os
import re
import sys

DEFAULT = os.path.join(
    os.environ.get("APPDATA", ""), "REAPER", "Scripts", "reaper_mcp_server.lua"
)

# `reaper.Foo_Bar(a, b)`  y  `reaper.Foo_Bar{...}` (llamada de metodo)
CALL = re.compile(r"\breaper\.([A-Z][A-Za-z0-9_]*)\s*\(")
STRING = re.compile(r'"([^"]{0,80})"')


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else DEFAULT
    src = io.open(path, encoding="utf-8", errors="replace").read()

    calls = collections.Counter(CALL.findall(src))
    total = sum(calls.values())

    print("  bridge: %s" % path)
    print("  llamadas a la API real de Reaper: %d (%d funciones distintas)"
          % (total, len(calls)))
    print()

    # Familias: el prefijo antes del primer underscore, que es como Reaper
    # agrupa de facto (Track*, TrackFX*, GetSet*, MIDI*, ...).
    fam = collections.defaultdict(list)
    for fn, n in calls.items():
        if fn.startswith("GetSet"):
            fam["GetSet*"].append((fn, n))
        elif "_" in fn:
            fam[fn.split("_")[0] + "*"].append((fn, n))
        else:
            fam["(sin prefijo)"].append((fn, n))

    print("  por familia:")
    for k in sorted(fam, key=lambda x: -sum(n for _, n in fam[x])):
        total_f = sum(n for _, n in fam[k])
        print("    %-22s %3d funciones  %4d llamadas" % (k, len(fam[k]), total_f))
    print()

    print("  las 30 mas usadas:")
    for fn, n in calls.most_common(30):
        print("    %-40s %4d" % (fn, n))
    print()

    # Parametros con nombre: Reaper los pasa como cadenas ("D_VOL", "B_MUTE").
    # Esos son los que el LLM tiene que acertar, y los que no se pueden
    # adivinar: un "D_VOL" mal puesto no da error, cambia otra cosa.
    named = collections.Counter(STRING.findall(src))
    claves = [k for k, n in named.most_common(80)
              if re.fullmatch(r"[A-Z]_[A-Z0-9_]+", k)]
    print("  parametros con nombre (tipo \"D_VOL\"): %d distintos" % len(claves))
    for k in claves[:30]:
        print("    %-16s %4d" % (k, named[k]))
    print()

    # ¿Hay una accion generica en el bridge para llamar API arbitraria?
    print("  ¿el bridge tiene un eval generico?")
    for fn in ("script_run_start", "script_list", "script_read_result"):
        print("    %-20s %s" % (fn, "SI" if fn in src else "no"))
    gen = [m for m in re.findall(r"function\s+\w+\.(\w*eval\w*)\s*\(", src, re.I)]
    print("    acciones *eval*: %s" % (gen or "ninguna"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
