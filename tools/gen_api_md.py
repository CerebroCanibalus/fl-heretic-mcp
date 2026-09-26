#!/usr/bin/env python3
"""Genera `docs/REAPER_API.md` desde los datos ya parseados.

El .md esta GENERADO. Si lo editas a mano, la proxima vez que se ejecute este
script tus cambios se pierden; y si no se ejecuta, el .md miente. Por eso
sale de `reaper-api.json` (que sale del HTML de REAPER) y no de mi cabeza.

    python tools/gen_api_docs.py && python tools/gen_api_md.py
"""
import io
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
DATA = os.path.join(ROOT, "crates", "heretic-daw", "data", "reaper-api.json")
OUT = os.path.join(ROOT, "docs", "REAPER_API.md")
BRIDGE = os.path.join(ROOT, "reference", "xDarkzx", "reaper_scripts",
                      "reaper_mcp_server.lua")

# Temas para las tools de musically. Cada entrada: (titulo, regex sobre la
# descripcion, por que le importa al agente).
TEMAS = [
    ("Medir sonoridad (LUFS / pico / RMS)", r"loudness|lufs|normali[sz]|\bpeaks?\b",
     "Remasterizar empieza por medir. Sin esto el agente ajusta a ojo."),
    ("Leer y escribir buffers de audio", r"PCM_|AudioAccessor|Samples",
     "Acceso a los samples de verdad: analisis de espectro y deteccion de clipping."),
    ("Notas, tono y afinacion", r"notename|note ?name|tuning|key ?signature",
     "Convierte 'Eb2' en el numero de nota correcto sin que el agente calcule."),
    ("Tempo, comps y curvas de tiempo", r"tempo|time ?map|timemap",
     "Remaster rapido: llevar la cancion al tempo del proyecto."),
    ("Renderizar y exportar", r"\brender\b",
     "Entregar el remaster. Sin esto no hay final."),
    ("Undo y state chunks", r"undo|statechunk|statechunk",
     "CTransactions y volcado de estado: el escape cuando no hay API directa."),
    ("MIDI en general", r"\bmidi\b|note ?on|note ?off|ppq",
     "Escribir y leer notas. 94 funciones, el bridge solo usa 25."),
]


def main():
    d = json.load(io.open(DATA, encoding="utf-8"))
    F = d["functions"]
    by = {f["name"]: f for f in F}
    usadas = set(re.findall(
        r"reaper\.([A-Z][A-Za-z0-9_]*)\s*\(",
        io.open(BRIDGE, encoding="utf-8", errors="replace").read(),
    ))
    cubiertas = {n for n in by if n in usadas}
    libres = [f for f in F if f["name"] not in usadas]

    L = []
    w = L.append
    w("# API de Reaper (ReaScript) â€” referencia compacta")
    w("")
    w("> **GENERADO. No editar a mano.** Sale de "
      "`reference/reaper/reascripthelp.html` (el doc oficial de REAPER %s, "
      "%d funciones) via `tools/gen_api_docs.py` + `tools/gen_api_md.py`."
      % (d["version"], d["count"]))
    w("> Editarlo a mano es la forma mas rapida de que este fichero mienta.")
    w("")
    w("```")
    w("# regenerar (una vez descargado el HTML)")
    w("curl -o reference/reaper/reascripthelp.html \\")
    w("  https://www.reaper.fm/sdk/reascript/reascripthelp.html")
    w("python tools/gen_api_docs.py    # HTML -> data/reaper-api.json")
    w("python tools/gen_api_md.py      # JSON -> este .md")
    w("```")
    w("")

    # ---- 1. el numero que explica por que existen las tools -----------------
    w("## 1. Cuanto de esto usan las tools hoy")
    w("")
    w("| | funciones |")
    w("|---|---|")
    w("| API real de REAPER %s | **%d** |" % (d["version"], d["count"]))
    w("| llama el bridge Lua (`reaper.X(...)`) | %d |" % len(cubiertas))
    w("| **inalcanzables hoy** | **%d** |" % len(libres))
    w("")
    w("Las %d inalcanzables no son un defecto del bridge: son la API que este "
      "no envuelve. Es exactamente donde estan las funciones de analisis y "
      "mastering (seccion 4). El escape es `script_run_start` + Lua propia."
      % len(libres))
    w("")

    # ---- 2. las trampas de naming ------------------------------------------
    w("## 2. Las trampas (las que ya han costado un bug cada una)")
    w("")
    w("| trampa | real | por duele |")
    w("|---|---|---|")
    w("| el bridge usa guion bajo, no punto | `track_create` | `track.create` "
      "-> `Unknown command` sin decir cual era el bueno |")
    w("| los params van en `snake_case` | `track_index`, `fx_name` | "
      "`trackIndex` -> `Missing parameter` |")
    w("| nota MIDI usa `end`, no `length` | `{\"start\":0,\"end\":0.5}` | "
      "el bridge ignora en silencio la nota si falta `end` |")
    w("| velocity va en 0-127, no 0-1 | `vel = 112` | 1.0 es inaudible |")
    w("| hay prefijos duplicados en el catalogo | `fx.fx_add` -> `fx_add` | "
      "poner `fx_fx_add` no existe |")
    w("| prefijo duplicado tambien en midi | `midi.midi_insert_note` -> "
      "`midi_insert_note` | `midi_midi_insert_note` no existe |")
    w("| posiciones MIDI en BEATS, no segundos | `start = 4` es el compas 2 | "
      "se escribe todo aplastado al principio |")
    w("")
    w("Regla que sustituye a acordarse de todo esto: **si no esta en el "
      "catalogo, no se inventa**. `daw_catalog` y `daw_api` lo dicen, y el "
      "error de un op desconocido lista los parecidos.")
    w("")

    # ---- 3. como se llega a lo que no cubre el bridge -----------------------
    w("## 3. Como se llega a las %d funciones sin cubrir" % len(libres))
    w("")
    w("`script_run_start` ejecuta un `.lua` propio. Ahi cabe cualquier "
      "`reaper.X(...)`, que es como se llega a la API sin envolver. El "
      "problema de siempre son los punteros opacos (`MediaTrack`, "
      "`PCM_source`), que no viajan en JSON: hay que resolverlos dentro del "
      "Lua a partir de indices.")
    w("")
    w("| lo que pide el agente | como se resuelve en Lua |")
    w("|---|---|")
    w("| `track: 0` | `reaper.GetTrack(reaper.GetActiveProject(), 0)` |")
    w("| `item: 0` en `track: 0` | `reaper.GetTrackMediaItem(tr, 0)` |")
    w("| `take: 0` de un item | `reaper.GetActiveTake(item)` |")
    w("| `source` de un take | `reaper.GetMediaItemTake_Source(take)` |")
    w("| `proj: 0` = proyecto actual | casi toda funcion acepta `0` |")
    w("")

    # ---- 4. lo que sirve para componer y remasterizar -----------------------
    w("## 4. Lo que sirve para componer y remasterizar")
    w("")
    w("De aqui sale el diseno de `daw_music` y `daw_master`: cada funcion de "
      "esta tabla es una capacidad que el agente no tiene hoy. Las marcadas "
      "*(libre)* no las usa el bridge, o sea que hay que llamarlas por Lua.")
    w("")
    for titulo, pat, por in TEMAS:
        hits = [f for f in F if re.search(pat, f["desc"], re.I)]
        if not hits:
            continue
        libres_t = [f for f in hits if f["name"] not in usadas]
        w("### %s" % titulo)
        w("")
        w("_%s_" % por)
        w("")
        w("| funcion | libre | firma | que hace |")
        w("|---|---|---|---|")
        for f in sorted(hits, key=lambda x: (x["name"] in usadas, x["name"]))[:14]:
            sig = re.sub(r"^[\w, ]*reaper\.", "", f["lua"])
            sig = re.sub(r"\s*=\s*reaper\.", " = ", f["lua"])
            w("| `%s` | %s | `%s` | %s |" % (
                f["name"], "si" if f["name"] not in usadas else "",
                sig[:74], (f["desc"] or "sin descripcion en el doc")[:78].replace("|", "/"),
            ))
        if len(hits) > 14:
            w("")
            w("_... y %d mas; `daw_api` las encuentra por palabra._" % (len(hits) - 14))
        w("")

    # ---- 5. las claves con nombre -------------------------------------------
    w("## 5. Los parametros con nombre (475 en 26 funciones)")
    w("")
    w("Un `D_VOL` mal puesto no da error: cambia otra cosa en silencio. "
      "Estos son los que hay que mirar antes de escribir.")
    w("")
    con = sorted((f for f in F if f["named"]),
                 key=lambda f: -len(f["named"]))
    for f in con:
        w("### `%s` â€” %d claves" % (f["name"], len(f["named"])))
        w("")
        w("`%s`" % f["lua"][:150])
        w("")
        items = sorted(f["named"].items())
        # dos columnas para que no ocupe 500 lineas verticales
        half = (len(items) + 1) // 2
        izq, der = items[:half], items[half:]
        w("| clave | que es | clave | que es |")
        w("|---|---|---|---|")
        for i in range(half):
            a = izq[i]
            b = der[i] if i < len(der) else ("", "")
            w("| `%s` | %s | `%s` | %s |" % (
                a[0], a[1][:46].replace("|", "/"),
                b[0], b[1][:46].replace("|", "/")))
        w("")

    # ---- 6. cheat sheet de los 730 nombres ---------------------------------
    w("## 6. Los %d nombres, por familia" % d["count"])
    w("")
    w("Para saber si algo existe sin preguntar. `daw_api(query=...)` lo hace "
      "por descripcion, esto es por prefijo.")
    w("")
    fam = {}
    for f in F:
        k = f["name"].split("_")[0]
        fam.setdefault(k, []).append(f["name"])
    for k in sorted(fam, key=lambda x: (-len(fam[x]), x)):
        nombres = sorted(fam[k])
        w("**%s** (%d) â€” %s" % (k, len(nombres),
                                ", ".join("`%s`" % n for n in nombres)))
        w("")

    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    io.open(OUT, "w", encoding="utf-8", newline="\n").write("\n".join(L) + "\n")
    print("  %s  (%d lineas, %.0f KB)" % (
        os.path.relpath(OUT, ROOT), len(L), os.path.getsize(OUT) / 1024))
    print("  %d/%d funciones documentadas, %d claves con nombre" % (
        d["count"], d["count"], sum(len(f["named"]) for f in F)))
    return 0


if __name__ == "__main__":
    sys.exit(main())

