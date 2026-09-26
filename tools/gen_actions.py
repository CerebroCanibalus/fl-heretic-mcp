#!/usr/bin/env python3
"""Genera `crates/heretic-daw/src/actions.rs` desde el bridge real.

Extrae, por handler:

- **nombre publico**: el segundo campo de `function <mod>.<nombre>(p)`.
  Ojo: el prefijo va duplicado, `fx.fx_add` -> `fx_add`.
- **dominio**: el modulo.
- **params que el handler lee** de `p`, y cuales son obligatorios.

Lo obligatorio se saca de los `return nil, "Missing parameter: X"` y
`error("Missing required parameter: X")` que hay DENTRO del cuerpo de cada
handler. Es la unica fuente fiable: el bridge no tiene docstrings por
handler, y adivinar los parametros es exactamente el bug que mas caro salio
en la etapa de FL (mandar `ms` cuando el daemon leia `position`).

    python tools/gen_actions.py [ruta_al_bridge.lua]

Salida: un fichero Rust con ACTIONS, ACTION_GROUPS y ACTION_DOCS.
"""
import collections
import io
import os
import re
import sys

DEFAULT_BRIDGE = os.path.join(
    os.environ.get("APPDATA", ""), "REAPER", "Scripts", "reaper_mcp_server.lua"
)
OUT = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "crates", "heretic-daw", "src", "actions.rs",
)

MODS = (
    "transport", "track", "project", "fx", "item", "marker", "selection",
    "send", "midi", "compose", "envelope", "tempo", "script", "val",
)


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_BRIDGE
    src = io.open(path, encoding="utf-8", errors="replace").read()
    print("  bridge: %s (%d bytes)" % (path, len(src)))

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
        body = src[start:end]
        handlers.append({"module": mod, "name": name, "body": body})

    if not handlers:
        print("  ERROR: no se encontro ningun handler. ¿ruta correcta?")
        return 1

    for h in handlers:
        b = h["body"]
        # Params que lee del dict `p`. Todos los `p.campo` y `p["campo"]`.
        read = set(re.findall(r"\bp\.(\w+)", b))
        read |= set(re.findall(r'\bp\["(\w+)"\]', b))
        # Obligatorios: los que el propio handler exige.
        req = set(
            re.findall(r'Missing (?:required )?parameter:\s*(?:"|\*\()?([\w./"]+)', b)
        )
        req |= set(re.findall(r'Missing required parameter: " \. \. ([^"]+)', b))
        req = {r.strip(' "') for r in req if r and r.strip(' "') not in ("/p", "p", "")}
        h["params"] = sorted(read)
        h["required"] = sorted(req)

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
    L.append("//! - **params**: los campos que el handler lee de `p` (`p.bpm`, `p[x]`).")
    L.append("//! - **required**: los que el propio handler rechaza si faltan, via")
    L.append("//!   `return nil, 'Missing parameter: X'`.")
    L.append("//!")
    L.append("//! Importa que salga del codigo y no de un docstring: el bridge no tiene")
    L.append("//! docstrings por handler, y **adivinar los parametros es el bug que mas")
    L.append("//! caro salio en la etapa de FL Studio** (mandar `ms` cuando el daemon")
    L.append("//! leia `position`). Reaper no avisa de un param desconocido: usa el valor")
    L.append("//! por defecto y parece que funciono.")
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
    print("  escrito: %s" % OUT)
    print("  (%d bytes)" % os.path.getsize(OUT))

    # muestra de lo generado, para revisar que tiene sentido
    print()
    print("  muestra:")
    for h in sorted(handlers, key=lambda x: x["name"])[:6]:
        req = ", ".join(h["required"]) or "-"
        prm = ", ".join(h["params"][:6]) or "-"
        print("    %-28s req: %-18s params: %s" % (h["name"], req, prm))
    return 0


if __name__ == "__main__":
    sys.exit(main())
