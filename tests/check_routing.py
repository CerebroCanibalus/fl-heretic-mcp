#!/usr/bin/env python3
"""Audita el routing MCP tool -> metodo daemon -> handler.

Lee las listas REALES del codigo (pipe.rs y los dos handlers) en vez de
mantener una copia a mano. Una copia a mano de "que metodos existen" es
exactamente el tipo de cosa que deja de ser verdad en el commit siguiente y
hace pasar un bug por OK.

Uso:  python tests/check_routing.py
Sale con codigo 1 si algun tool MCP no aterriza en ningun handler.
"""
import io
import re
import sys

ROOT = r"D:\Mis Juntos"  # placeholder, no se usa
ROOT = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP"

TOOLS = io.open(ROOT + r"\crates\heretic-mcp\src\tools.rs", encoding="utf-8").read()
PIPE = io.open(ROOT + r"\crates\heretic-daemon\src\pipe.rs", encoding="utf-8").read()
TRANS = io.open(ROOT + r"\crates\heretic-daemon\src\handlers\transport.rs", encoding="utf-8").read()
LIFE = io.open(ROOT + r"\crates\heretic-daemon\src\handlers\lifecycle.rs", encoding="utf-8").read()


def arms(src):
    """Metodos atendidos por un `dispatch` de match con brazos `  "x" =>`."""
    body = src.split("pub async fn dispatch", 1)[-1]
    return set(re.findall(r'^\s{12}"(\w+)"\s*=>', body, re.M))


def life_list():
    block = re.search(r"const LIFECYCLE_METHODS[^=]*=\s*&\[(.*?)\];", PIPE, re.S).group(1)
    return set(re.findall(r'"(\w+)"', block))


# --- datos del codigo real --------------------------------------------------
life_arms = arms(LIFE)
trans_arms = arms(TRANS)
routed_to_life = life_list()

# Tools: nombre de la fn -> metodo que manda por el pipe.
pairs = re.findall(r"pub async fn (fl_\w+)", TOOLS)
methods = re.findall(r'\.call\(\s*"(\w+)"', TOOLS)
pairs = list(zip(pairs, methods))

# El dispatch de pipe hace strip_prefix("fl_") para los de lifecycle.
problems = []
print("  %-24s %-19s %-12s %s" % ("TOOL", "METODO", "RUTA", "HANDLER"))
print("  " + "-" * 66)
for tool, method in pairs:
    if method in routed_to_life:
        target = method[3:] if method.startswith("fl_") else method
        route = "lifecycle"
        landed = target in life_arms
    else:
        route = "transport"
        landed = method in trans_arms
    ok = "OK" if landed else "SIN HANDLER"
    if not landed:
        problems.append((tool, method, route))
    print("  %-24s %-19s %-12s %s" % (tool, method, route, ok))

# Metodos del daemon que ninguna tool usa (informativo, no es error).
used = {m for _, m in pairs}
orphans = (life_arms | trans_arms) - used
if orphans:
    print("\n  metodos del daemon sin tool MCP (accesibles por fl_call): %s"
          % ", ".join(sorted(orphans)))

print("")
if problems:
    print("  %d tool(s) sin handler:" % len(problems))
    for tool, method, route in problems:
        print("    - %s -> '%s' (%s)" % (tool, method, route))
    sys.exit(1)
print("  OK: las %d tools aterrizan en un handler real." % len(pairs))
