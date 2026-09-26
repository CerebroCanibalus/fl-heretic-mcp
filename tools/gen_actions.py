#!/usr/bin/env python3
"""Genera `crates/heretic-daw/src/actions.rs` desde el bridge real.

Extrae, por handler:

- **nombre publico**: el segundo campo de `function <mod>.<nombre>(p)`.
  Ojo: el prefijo va duplicado, `fx.fx_add` -> `fx_add`.
- **dominio**: el modulo.
- **params que el handler lee** de `p`, y cuales son obligatorios.

Lo obligatorio se saca de los `return nil, "Missing parameter: X"` y
`error("Missing required parameter: X")`. Es la unica fuente fiable: el bridge
no tiene docstrings por handler, y adivinar los parametros es exactamente el bug
que mas caro salio en la etapa de FL (mandar `ms` cuando el daemon leia
`position`).

    python tools/gen_actions.py [ruta_al_bridge.lua]

Salida: un fichero Rust con ACTIONS, ACTION_GROUPS y ACTION_DOCS.

# Por que sigue a los helpers

Porque si no, `required` sale incompleto y el incompleto es peor que no
existir. `midi_insert_notes_batch` no menciona `item_index` en su cuerpo: lo
exige `get_midi_take(p)`, que es una funcion local. Antes de este cambio el
catalogo decia que `midi_insert_notes_batch` no necesita nada, y el agente lo
descubria cometiendo el error. Medido: 17 de las 17 acciones `midi_*` pedian
`item_index` y el catalogo no lo declaraba en ninguna.

Tres formas de exigir un parametro que hay que reconocer:

1. `if p.x == nil then ... "Missing parameter: x"`
2. `get_track(params, key)`, donde el nombre viene en el segundo argumento y el
   default esta en `key = key or "track_index"`
3. `require_int(p, "param_index")`, que exige lo que se le pase como nombre

# La puerta: ninguna API inventada

Ademas de generar, **comprueba que todas las funciones `reaper.X` que el bridge
llama existen en el catalogo oficial** (`crates/heretic-daw/data/reaper-api.json`,
generado del HTML de REAPER 7.80). Si el bridge llama a algo que no existe, esto
no genera y sale con codigo 1.

Medido: el bridge llama a 165 funciones y una estaba inventada,
`TrackFX_GetParameterStepCount`, que hacia que `fx_scan_params` fallara con
`attempt to call a nil value`. El nombre real es
`TrackFX_GetParameterStepSizes` y devuelve otra cosa. Sin esta puerta, un
parametro mal escrito parece un fallo de Lua y no un error de tipografia.
"""
import collections
import io
import json
import os
import re
import sys

DEFAULT_BRIDGE = os.path.join(
    os.environ.get("APPDATA", ""), "REAPER", "Scripts", "reaper_mcp_server.lua"
)
RAIZ = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(RAIZ, "crates", "heretic-daw", "src", "actions.rs")
API_JSON = os.path.join(RAIZ, "crates", "heretic-daw", "data", "reaper-api.json")

MODS = (
    "transport", "track", "project", "fx", "item", "marker", "selection",
    "send", "midi", "compose", "envelope", "tempo", "script", "val",
)

# `reaper.defer` no es una API: es el idiom de reaper para diferir un ciclo.
# Cualquier otra excepcion tiene que.justificarse aqui con un motivo, porque
# esta lista es la unica puerta que deja pasar una API inventada.
PERDONADAS = {
    "defer": "idiom de reaper, no una API",
}

# Funciones locales que reciben un dict y de las que hay que copiar lo que leen
# y lo que exigen. El nombre del dict puede ser `p` o `params`; da igual,
# porque lo que importa es que el handler les pasa su propio `p`.
DICT_ARG = re.compile(r"local function (\w+)\s*\(([^)]*)\)")


def helpers_locales(src):
    """Mapa nombre -> (cuerpo, posicion_del_argumento_dict)."""
    salida = {}
    for m in DICT_ARG.finditer(src):
        nombre, args = m.group(1), m.group(2)
        # El cuerpo va hasta la siguiente `local function` o `function mod.x`.
        nxt = re.search(r"^(?:local function|function\s+\w+\.)", src[m.end():], re.M)
        cuerpo = src[m.end(): m.end() + nxt.start()] if nxt else src[m.end():]
        pos = None
        for i, a in enumerate(args.split(",")):
            if a.strip() in ("p", "params"):
                pos = i
                break
        if pos is not None:
            salida[nombre] = (cuerpo, pos)
    return salida


def params_de_un_cuerpo(cuerpo, nombre_dict):
    """Lo que un cuerpo lee y exige de su argumento dict."""
    lee = set(re.findall(r"\b%s\.(\w+)" % nombre_dict, cuerpo))
    lee |= set(re.findall(r'\b%s\["(\w+)"\]' % nombre_dict, cuerpo))
    # `params[key or "track_index"]`: el default del nombre es lo que se exige.
    for alt in re.findall(r'\b%s\[key or "(\w+)"\]' % nombre_dict, cuerpo):
        lee.add(alt)
    for alt in re.findall(r'key = key or "(\w+)"', cuerpo):
        lee.add(alt)
    # `params[algo]` sin default no dice nada del nombre: no se puede inventar.
    req = set()
    for r in re.findall(r'Missing (?:required )?parameter:\s*(?:"|\*\()?([\w./"]+)', cuerpo):
        req.add(r.strip(' "'))
    for r in re.findall(r'Missing (?:required )?parameter: "\s*\.\.\s*\(?key or "(\w+)"', cuerpo):
        req.add(r)
    for r in re.findall(r'Missing required parameter: "\s*\.\.\s*key', cuerpo):
        # `key = key or "X"` ya metio X en `lee`; aqui se marca obligatorio.
        for alt in re.findall(r'key = key or "(\w+)"', cuerpo):
            req.add(alt)
    req = {r for r in req if r and r not in ("/p", "p", "params", "")}
    # Lo que se marca con `p.x == nil` antes de un "Missing parameter" tambien
    # es obligatorio, aunque el mensaje venga de un helper.
    return lee, req


def params_de_un_handler(bucle, cuerpo):
    lee, req = params_de_un_cuerpo(cuerpo, "p")
    # `require_int(p, "x")` / `require_num(p, "x")` exigen lo que se les pase.
    for r in re.findall(r'require_(?:int|num)\(\s*\w+\s*,\s*"([\w.]+)"', cuerpo):
        req.add(r)
    # Y entrar en los helpers que reciben el dict.
    for nombre, (cuerpo_h, pos) in bucle.items():
        if not re.search(r"(?<![\w.])%s\s*\(" % re.escape(nombre), cuerpo):
            continue
        lee_h, req_h = params_de_un_cuerpo(cuerpo_h, "p")
        # `get_track(params, key)`: su dict se llama `params`, no `p`.
        if not lee_h and req_h:
            lee_h, req_h = params_de_un_cuerpo(cuerpo_h, "params")
        lee |= lee_h
        req |= req_h
    return sorted(lee), sorted(req)


def puerta_api(src):
    """Ninguna API inventada. Devuelve la lista de infracciones."""
    with io.open(API_JSON, encoding="utf-8") as fh:
        cat = json.load(fh)
    funcs = cat if isinstance(cat, list) else (cat.get("functions") or [])
    oficiales = {f.get("name") for f in funcs}
    print("  catalogo oficial: %d funciones" % len(oficiales))

    llamadas = sorted(set(re.findall(r"reaper\.([A-Za-z_][A-Za-z0-9_]*)", src)))
    inventadas = [c for c in llamadas if c not in oficiales and c not in PERDONADAS]
    perdonadas = [c for c in llamadas if c in PERDONADAS]
    print("  el bridge llama a %d funciones: %d en el catalogo, %d perdonadas, %d INVENTADAS"
          % (len(llamadas), len(llamadas) - len(inventadas) - len(perdonadas), len(perdonadas), len(inventadas)))
    import difflib
    for c in inventadas:
        print("    INVENTADA  reaper.%s" % c)
        for sug in difflib.get_close_matches(c, sorted(oficiales), n=2, cutoff=0.6):
            print("               quiza quisiste decir: reaper.%s" % sug)
    return inventadas


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_BRIDGE
    src = io.open(path, encoding="utf-8", errors="replace").read()
    print("  bridge: %s (%d bytes)" % (path, len(src)))

    print()
    print("  --- puerta: APIs que no existen ---")
    if puerta_api(src):
        print()
        print("  NO SE GENERA. Arregla el bridge y vuelve a intentarlo.")
        return 1
    print("  ok: todas las funciones que el bridge llama existen en el catalogo")

    bucle = helpers_locales(src)
    print("  helpers con dict: %d" % len(bucle))

    # Cortar el source en bloques por handler: desde `function mod.nombre(p)`
    # hasta el `end` que cierra al mismo nivel de indentacion.
    heads = list(re.finditer(r"^function\s+(\w+)\.(\w+)\s*\(p\)\s*$", src, re.M))
    handlers = []
    for i, m in enumerate(heads):
        mod, name = m.group(1), m.group(2)
        if mod not in MODS:
            continue
        start = m.end()
        end = heads[i + 1].start() if i + 1 < len(heads) else len(src)
        handlers.append({"module": mod, "name": name, "body": src[start:end]})

    if not handlers:
        print("  ERROR: no se encontro ningun handler. ¿ruta correcta?")
        return 1

    for h in handlers:
        h["params"], h["required"] = params_de_un_handler(bucle, h["body"])

    names = sorted({h["name"] for h in handlers})
    print("  acciones: %d" % len(names))

    groups = collections.defaultdict(list)
    for h in sorted(handlers, key=lambda x: x["name"]):
        groups[h["name"].split("_")[0]].append(h)
    print("  grupos: %d" % len(groups))

    # ---- generar Rust -----------------------------------------------
    L = []
    L.append("//! Catalogo de acciones del bridge, con sus parametros.")
    L.append("//!")
    L.append("//! **GENERADO con `tools/gen_actions.py`. No editar a mano.**")
    L.append("//!")
    L.append("//! Se extrae del bridge real (`%APPDATA%\\\\REAPER\\\\Scripts\\\\reaper_mcp_server.lua`):")
    L.append("//!")
    L.append("//! ```text")
    L.append("//! python tools/gen_actions.py")
    L.append("//! ```")
    L.append("//!")
    L.append("//! Para cada handler se leen dos cosas del codigo, no de documentacion:")
    L.append("//!")
    L.append("//! - **params**: los campos que el handler lee de `p` (`p.bpm`, `p[x]`),")
    L.append("//!   entrando tambien en los helpers locales que reciben ese `p`.")
    L.append("//! - **required**: los que el propio handler rechaza si faltan, via")
    L.append("//!   `return nil, 'Missing parameter: X'`, `require_int(p, 'X')`, o un")
    L.append("//!   helper como `get_midi_take(p)` que los exige por el llamante.")
    L.append("//!")
    L.append("//! Importa que salga del codigo y no de un docstring: el bridge no tiene")
    L.append("//! docstrings por handler, y **adivinar los parametros es el bug que mas")
    L.append("//! caro salio en la etapa de FL Studio** (mandar `ms` cuando el daemon")
    L.append("//! leia `position`). Reaper no avisa de un parametro desconocido: usa el")
    L.append("//! valor por defecto y parece que funciono.")
    L.append("//!")
    L.append("//! El generador ademas es una puerta: si el bridge llama a una funcion")
    L.append("//! `reaper.*` que no esta en el catalogo oficial, no genera.")
    L.append("//!")
    L.append("//! Al actualizar el bridge, hay que regenerar esto.")
    L.append("")
    L.append("/// Una accion del bridge, con lo que se sabe de ella leyendo su codigo.")
    L.append("#[derive(Debug, Clone, Copy)]")
    L.append("pub struct ActionDoc {")
    L.append("    /// Nombre publico, tal cual lo registra el bridge.")
    L.append("    pub name: &'static str,")
    L.append("    /// Modulo del bridge donde vive.")
    L.append("    pub module: &'static str,")
    L.append("    /// Params que el handler lee. Ninguno es opcional por defecto: el")
    L.append("    /// bridge no avisa si sobra uno.")
    L.append("    pub params: &'static [&'static str],")
    L.append("    /// Params que el handler exige.")
    L.append("    pub required: &'static [&'static str],")
    L.append("}")
    L.append("")
    L.append("/// Documentacion de cada accion, indexada por nombre.")
    L.append("pub static ACTION_DOCS: &[ActionDoc] = &[")

    def arr(items):
        return "[" + ", ".join('"%s"' % i for i in items) + "]"

    for h in sorted(handlers, key=lambda x: x["name"]):
        L.append("    ActionDoc {")
        L.append('        name: "%s",' % h["name"])
        L.append('        module: "%s",' % h["module"])
        L.append("        params: &%s," % arr(h["params"]))
        L.append("        required: &%s," % arr(h["required"]))
        L.append("    },")
    L.append("];")
    L.append("")
    L.append("/// Buscar la documentacion de una accion.")
    L.append("pub fn doc_of(name: &str) -> Option<&'static ActionDoc> {")
    L.append("    ACTION_DOCS.iter().find(|d| d.name == name)")
    L.append("}")
    L.append("")
    L.append("/// Nombres de todas las acciones.")
    L.append("pub const ACTIONS: &[&str] = &[")
    for n in names:
        L.append('    "%s",' % n)
    L.append("];")
    L.append("")
    L.append("/// Acciones agrupadas por prefijo, para `daw_catalog`.")
    L.append("pub const ACTION_GROUPS: &[(&str, &[&str])] = &[")
    for g in sorted(groups):
        L.append('    ("%s", &[' % g)
        for h in groups[g]:
            L.append('        "%s",' % h["name"])
        L.append("    ]),")
    L.append("];")

    io.open(OUT, "w", encoding="utf-8", newline="\n").write("\n".join(L) + "\n")
    print("  escrito: %s (%d bytes)" % (OUT, os.path.getsize(OUT)))

    print()
    print("  required de las acciones midi (antes vacio, ahora lo que el bridge exige):")
    for h in sorted(handlers, key=lambda x: x["name"]):
        if h["module"] == "midi":
            print("    %-30s %s" % (h["name"], ", ".join(h["required"]) or "-"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
