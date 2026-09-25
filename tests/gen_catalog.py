#!/usr/bin/env python3
"""Genera el catalogo Rust de ACTIONS a partir del bridge real."""
import io
import json

acts = json.load(io.open(r"tests\bridge_actions.json", encoding="utf-8"))
acts = sorted(acts)

# Agrupar por categoria para el comentario
groups = {}
for a in acts:
    groups.setdefault(a.split(".")[0], []).append(a)

lines = []
lines.append("/// Catálogo de actions que declara el FL Heretic Bridge.")
lines.append("///")
lines.append("/// Generado desde el bridge real con `tests/get_actions.py` (no a mano),")
lines.append("/// que lista lo que el bridge vivo publica via `meta.actions`. Si añades")
lines.append("/// un handler al bridge, regenera esto y el catálogo lo refleja.")
lines.append("///")
lines.append("/// Se usa para *advertir* cuando llega una action desconocida, no para")
lines.append("/// bloquear: el bridge puede llevar más actions de las que este crate conoce")
lines.append("/// (va por delante), y un catálogo desactualizado no debe impedir reaches")
lines.append("/// que sí funcionan.")
lines.append("pub const ACTIONS: &[&str] = &[")
for g in sorted(groups):
    lines.append("    // %s (%d)" % (g, len(groups[g])))
    for a in groups[g]:
        lines.append('    "%s",' % a)
lines.append("];")

block = "\n".join(lines) + "\n"

PATH = r"crates\heretic-fl\src\bridge.rs"
src = io.open(PATH, encoding="utf-8").read()
start = src.index("/// Las 133 actions del fLMCP Bridge v0.2.0")
end = src.index("/// Config del cliente.")
src = src[:start] + block + "\n" + src[end:]
io.open(PATH, "w", encoding="utf-8", newline="\n").write(src)
print("catalogo regenerado: %d actions en %d categorias" % (len(acts), len(groups)))
