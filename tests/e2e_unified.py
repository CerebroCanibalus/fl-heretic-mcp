#!/usr/bin/env python3
"""E2E de la surface UNIFICADA de 4 tools.

Mismo objetivo que antes (cadena completa LLM -> MCP -> daemon -> bridge -> FL)
pero contra `fl_do` / `fl_project` / `fl_diagnose` / `fl_exec` en vez de las 17
tools finas.

Comprueba ademas lo que la unificacion tiene que garantizar:
- solo 4 tools, y cada una hace lo que dice;
- `fl_do` sin action devuelve el catalogo (para que el LLM no adivine);
- una accion invalida devuelve un error util, no un fallo opaco;
- las tools en paralelo funcionan en paralelo (el bug de ERROR_PIPE_BUSY);
- fl_project create/open/save funcionan contra FL de verdad.

Uso:  python tests/e2e_unified.py
"""
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MCP = os.path.join(ROOT, "target", "release", "fl-heretic-mcp.exe")
PIPE = os.environ.get("FL_HERETIC_PIPE", r"\\.\pipe\fl-heretic-e2e")

fails = []


def check(cond, msg):
    print("  %s %s" % ("[OK]" if cond else "[XX]", msg))
    if not cond:
        fails.append(msg)
    return cond


def run(batch, timeout=90):
    data = "".join(json.dumps(r) + "\n" for r in batch)
    p = subprocess.Popen(
        [MCP], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, env=dict(os.environ), text=True,
        encoding="utf-8", errors="replace",
    )
    try:
        out, err = p.communicate(data, timeout=timeout)
    except subprocess.TimeoutExpired:
        p.kill()
        print("  el MCP server se colgo")
        return {}, ""
    got = {}
    for line in out.splitlines():
        if not line.strip():
            continue
        try:
            m = json.loads(line)
        except json.JSONDecodeError:
            continue
        if "id" in m:
            got[m["id"]] = m
    return got, err


INIT = [
    {"jsonrpc": "2.0", "id": 1, "method": "initialize",
     "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                "clientInfo": {"name": "e2e", "version": "1"}}},
    {"jsonrpc": "2.0", "method": "notifications/initialized"},
]


def tool(i, name, args):
    return {"jsonrpc": "2.0", "id": i, "method": "tools/call",
            "params": {"name": name, "arguments": args}}


def body(resp):
    r = resp.get("result")
    if r is None:
        return json.dumps(resp.get("error", {}), ensure_ascii=False)
    for c in r.get("content", []):
        if c.get("type") == "text":
            return c["text"]
    return ""


def bj(resp):
    try:
        return json.loads(body(resp))
    except (json.JSONDecodeError, TypeError):
        return {}


def main():
    print("")
    print("=" * 70)
    print("  E2E de la surface unificada (4 tools)")
    print("=" * 70)
    print("")

    # ---------------------------------------------------------------- 1
    print("  -- 1. superficie: exactamente 4 tools --")
    r, _ = run(INIT + [{"jsonrpc": "2.0", "id": 2, "method": "tools/list",
                        "params": {}}])
    tools = r.get(2, {}).get("result", {}).get("tools", [])
    names = sorted(t["name"] for t in tools)
    check(names == ["fl_diagnose", "fl_do", "fl_exec", "fl_project"],
          "las 4 tools unificadas y nada mas: %s" % names)

    # Los params que se tranquilidad al LLM
    sch = {t["name"]: t.get("inputSchema", {}) for t in tools}
    check(set(sch.get("fl_do", {}).get("properties", {})) >= {"action", "params"},
          "fl_do expone action + params")
    check(not ({"action", "params"} & set(sch.get("fl_do", {}).get("required", []))),
          "fl_do no obliga a dar action (para poder pedir el catalogo)")
    check("op" in sch.get("fl_project", {}).get("properties", {}),
          "fl_project expone op")
    req = set(sch.get("fl_project", {}).get("required", []))
    check(req == {"op"},
          "fl_project solo exige op; el resto son opcionales (required=%s)" % sorted(req))

    # ---------------------------------------------------------------- 2
    print("")
    print("  -- 2. el catalogo (para que el LLM no adivine action) --")
    r, _ = run(INIT + [tool(3, "fl_do", {})])
    cat = bj(r.get(3, {}))
    compiled = cat.get("compiled_catalog") or []
    check(len(compiled) >= 60,
          "fl_do sin action devuelve el catalogo (%d actions)" % len(compiled))
    for must in ("transport.setTempo", "channels.all", "patterns.list"):
        check(must in compiled, "el catalogo incluye %s" % must)

    # ---------------------------------------------------------------- 3
    print("")
    print("  -- 3. action invalida: error util, no opaco --")
    r, _ = run(INIT + [tool(4, "fl_do",
                            {"action": "no.existe.esta", "params": {}})])
    txt = body(r.get(4, {}))
    check(bool(txt), "responde algo ante una action invalida")
    check("action" in txt.lower() or "desconoc" in txt.lower(),
          "el error menciona la action: %s" % txt[:120])

    # ---------------------------------------------------------------- 4
    print("")
    print("  -- 4. ida y vuelta real por FL --")
    r, _ = run(INIT + [
        tool(5, "fl_do", {"action": "transport.status", "params": {}}),
        tool(6, "fl_do", {"action": "channels.all", "params": {}}),
    ])
    st = bj(r.get(5, {}))
    check("bpm" in st, "fl_do transport.status devuelve bpm (%s)" % st.get("bpm"))
    ch = bj(r.get(6, {}))
    check("channels" in ch, "fl_do channels.all devuelve canales (%d)"
          % len(ch.get("channels", [])))

    # escritura + relectura, en sesiones separadas para que no compitan
    bpm0 = st.get("bpm") or 130.0
    target = 111.0 if abs(bpm0 - 111) > 1 else 122.0
    r, _ = run(INIT + [tool(7, "fl_do",
                            {"action": "transport.setTempo",
                             "params": {"bpm": target}})])
    check("bpm" in bj(r.get(7, {})), "escribir el tempo funciona (%s)" % target)
    import time as _t
    _t.sleep(0.7)
    r, _ = run(INIT + [tool(8, "fl_do",
                            {"action": "transport.status", "params": {}})])
    got = bj(r.get(8, {})).get("bpm")
    check(isinstance(got, (int, float)) and abs(got - target) < 0.01,
          "y se lee de vuelta desde FL (puso %s, leyo %r)" % (target, got))
    run(INIT + [tool(9, "fl_do",
                     {"action": "transport.setTempo", "params": {"bpm": bpm0}})])

    # ---------------------------------------------------------------- 5
    print("")
    print("  -- 5. fl_diagnose --")
    r, _ = run(INIT + [tool(10, "fl_diagnose", {})])
    dg = bj(r.get(10, {}))
    check(dg.get("daemon") == "ok", "el daemon responde")
    check("escrituras" in dg, "fl_diagnose dice si se puede escribir (%s)"
          % dg.get("escrituras"))
    check("veredicto" in dg, "fl_diagnose da un veredicto: %s"
          % str(dg.get("veredicto"))[:80])

    # ---------------------------------------------------------------- 6
    print("")
    print("  -- 6. fl_project create + save --")
    r, _ = run(INIT + [tool(11, "fl_project",
                            {"op": "create", "name": "e2e-unified"})])
    cp = bj(r.get(11, {}))
    created = cp.get("created", "")
    check(created.endswith("e2e-unified.flp"),
          "fl_project create creo el fichero (%s)" % created.rsplit("\\", 1)[-1])
    check(cp.get("bridge_ready") is not False,
          "create espero al bridge (ready=%s)" % cp.get("bridge_ready"))

    r, _ = run(INIT + [tool(12, "fl_project", {"op": "save"})])
    check(not r.get(12, {}).get("result", {}).get("isError", False),
          "fl_project save no falla")

    # ---------------------------------------------------------------- 7
    print("")
    print("  -- 7. concurrencia (8 fl_do en paralelo) --")
    print("     el fallo ERROR_PIPE_BUSY solo sale aqui")
    batch = INIT + [tool(20 + i, "fl_do",
                         {"action": "meta.ping", "params": {}}) for i in range(8)]
    r, _ = run(batch, timeout=90)
    llegaron = [i for i in range(20, 28) if i in r]
    check(len(llegaron) == 8,
          "las 8 en paralelo respondieron (%d/8)" % len(llegaron))

    # ---------------------------------------------------------------- 8
    print("")
    print("  -- 8. fl_exec (Python crudo) --")
    r, _ = run(INIT + [tool(30, "fl_exec",
                            {"code": "mixer.getCurrentTempo() / 1000.0"})])
    v = bj(r.get(30, {}))
    check("result" in v, "fl_exec devuelve el valor: %s" % json.dumps(v)[:120])

    # limpiar
    try:
        if created and os.path.exists(created):
            os.remove(created)
    except OSError:
        pass

    print("")
    if fails:
        print("  %d comprobacion(es) FALLIDA(S):" % len(fails))
        for f in fails:
            print("    - " + f)
        return 1
    print("  E2E unificado COMPLETO.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
