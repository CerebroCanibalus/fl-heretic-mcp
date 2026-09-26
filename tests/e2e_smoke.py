#!/usr/bin/env python3
"""Prueba de humo de extremo a extremo: LLM -> MCP -> daemon -> bridge -> FL.

Se escriben a mano las peticiones MCP por stdin de `fl-heretic-mcp.exe` (mismo
formato NDJSON que usaria un cliente real) y se comprueba que lo que vuelve
tiene sentido. Asi se prueba la cadena ENTERA, no el bridge aislado:

  - el MCP server arranca y responde `initialize`
  - anuncia las tools en `tools/list`, con sus params
  - el daemon esta en el pipe que dice FL_HERETIC_PIPE
  - el daemon autentica el handshake HMAC
  - la peticion llega al bridge dentro de FL
  - el cambio se ve de vuelta por el bridge

Ademas manda las tools EN PARALELO, porque el fallo mas caro que se ha
encontrado ahi (ERROR_PIPE_BUSY con una sola instancia del pipe) solo
aparece con concurrencia, nunca de una en una.

Uso:  python tests/e2e_smoke.py
Sale 0 si todo cuadra, 1 si algo falla.
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
    """Manda un lote de peticiones MCP y devuelve {id: response}."""
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
        print("  el MCP server se quedo colgado")
        return {}, ""
    got = {}
    for line in out.splitlines():
        line = line.strip()
        if not line:
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
    """Saca el texto de un resultado MCP, o la mensagem de error."""
    r = resp.get("result")
    if r is None:
        return json.dumps(resp.get("error", {}), ensure_ascii=False)
    for c in r.get("content", []):
        if c.get("type") == "text":
            return c["text"]
    return ""


def body_json(resp):
    """El texto del resultado MCP parseado como JSON.

    Comparar con `'"bpm": 111.0' in texto` es fragil: depende de como serde
    imprima los espacios y de si sale flot o entero. Parsear y mirar el VALOR.
    """
    try:
        return json.loads(body(resp))
    except (json.JSONDecodeError, TypeError):
        return {}


def main():
    for f in (MCP,):
        if not os.path.isfile(f):
            print("no existe %s (compila primero)" % f)
            return 1

    print("")
    print("=" * 70)
    print("  E2E: LLM -> MCP -> daemon -> bridge -> FL Studio")
    print("  pipe: %s" % PIPE)
    print("=" * 70)
    print("")

    # ---------------------------------------------------------------- 1
    print("  -- 1. handshake y catalogo --")
    r, _ = run(INIT + [
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
    ])
    if not check(1 in r, "initialize responde"):
        print("    no hubo initialize; revisar el servidor MCP")
        return 1
    check(r[1].get("result", {}).get("serverInfo", {}).get("name") == "fl-heretic-mcp",
          "serverInfo correcto")

    tools = r.get(2, {}).get("result", {}).get("tools", [])
    if not check(len(tools) == 17, "tools/list devuelve 17 tools (di %d)" % len(tools)):
        return 1
    names = {t["name"]: t for t in tools}
    for must in ("fl_ping", "fl_status", "fl_call", "fl_create_project",
                 "fl_set_song_position"):
        check(must in names, "expone %s" % must)

    # Los params del schema: aqui se esconden los bugs de "param obligatorio
    # que en realidad es opcional". Que `dir` aparezca en `properties` es
    # correcto (es opcional de verdad); lo que no puede ser es estar en
    # `required`, porque entonces el LLM esta obligado a inventarlo.
    for tool_name, must_props, must_not_required in (
        ("fl_create_project", {"name"}, {"dir", "template"}),
        ("fl_set_song_position", {"position", "unit"}, {"ms"}),
    ):
        props = set(names[tool_name].get("inputSchema", {}).get("properties", {}))
        req = set(names[tool_name].get("inputSchema", {}).get("required", []))
        check(not (must_not_required & req),
              "%s no exige %s como obligatorio (required=%s)"
              % (tool_name, sorted(must_not_required), sorted(req)))
        check(must_props.issubset(props),
              "%s declara %s (params=%s)" % (tool_name, sorted(must_props), sorted(props)))
    # `ms` fue el nombre viejo y no debe seguir apareciendo por ningun lado.
    check("ms" not in names["fl_set_song_position"].get("inputSchema", {})
          .get("properties", {}),
          "fl_set_song_position ya no usa el param 'ms'")

    # ---------------------------------------------------------------- 2
    print("")
    print("  -- 2. ida y vuelta por el bridge (secuencial) --")
    r, _ = run(INIT + [
        tool(3, "fl_ping", {}),
        tool(4, "fl_status", {}),
        tool(5, "fl_set_tempo", {"bpm": 111}),
        tool(6, "fl_set_song_position", {"position": 1000, "unit": "ms"}),
    ])
    check(body_json(r.get(3, {})).get("pong") is True,
          "fl_ping: el bridge dentro de FL responde")
    check(body_json(r.get(4, {})).get("fl_running") is True,
          "fl_status: el daemon ve FL Studio corriendo")
    check(5 in r and not r[5].get("result", {}).get("isError", False),
          "fl_set_tempo no falla")
    check(6 in r and not r[6].get("result", {}).get("isError", False),
          "fl_set_song_position ya no falla ('missing position')")

    # La ida y vuelta de verdad va en su propio test: una sesion MCP por paso
    # y espera entre medias, para que el valor que se lee sea el que se
    # escribio y no una carrera con el siguiente comando de la tanda.
    rt = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                      "test_tempo_roundtrip.py")
    rr = subprocess.run([sys.executable, rt], capture_output=True, text=True,
                        env=dict(os.environ))
    check(rr.returncode == 0,
          "ida y vuelta del tempo real (ver test_tempo_roundtrip.py)")
    if rr.returncode != 0:
        for line in rr.stdout.splitlines():
            if line.strip():
                print("      " + line)

    # ---------------------------------------------------------------- 3
    print("")
    print("  -- 3. crear proyecto (la tool que no rutaba) --")
    r, _ = run(INIT + [
        tool(9, "fl_create_project", {"name": "e2e-humo"}),
    ])
    ok = 9 in r and not r[9].get("result", {}).get("isError", False)
    if check(ok, "fl_create_project no falla ('metodo transport desconocido')"):
        b = body(r[9])
        check("e2e-humo.flp" in b, "creo el fichero e2e-humo.flp")
        check("bridge_ready" in b, "espero a que el bridge estuviera listo")

    # ---------------------------------------------------------------- 4
    print("")
    print("  -- 4. dejar FL limpio --")
    print("     el E2E cambia el tempo, asi que el proyecto queda SUCIO. Si la")
    print("     siguiente corrida abre otro .flp encima, FL pide 'Save changes?'")
    print("     y ese modal congela el bridge. Por eso se guarda al terminar.")
    r, _ = run(INIT + [tool(30, "fl_save", {})])
    if check(30 in r and not r[30].get("result", {}).get("isError", False),
             "fl_save deja el proyecto sin cambios pendientes"):
        r, _ = run(INIT + [tool(31, "fl_project_info", {})])
        changed = body_json(r.get(31, {})).get("changed")
        check(changed is False, "changed == False tras guardar (leyo %r)" % changed)

    print("")
    print("  -- 5. CONCURRENCIA (8 tools en paralelo) --")
    print("     el fallo ERROR_PIPE_BUSY solo sale aqui, nunca en serie")
    batch = INIT + [tool(20 + i, "fl_call",
                         {"action": "meta.ping", "params_json": "{}"})
                    for i in range(8)]
    r, _ = run(batch, timeout=90)
    llegaron = [i for i in range(20, 28) if i in r]
    check(len(llegaron) == 8,
          "las 8 tools en paralelo respondieron (%d/8)" % len(llegaron))
    # `meta.ping` del bridge no devuelve `pong`: devuelve su ficha de estado
    # (bridge_version, fl_version, in_fl...). Lo que hay que comprobar es que
    # respondio con una ficha real, no con un error.
    for i in llegaron:
        b = body_json(r[i])
        if "bridge_version" not in b or "in_fl" not in b:
            check(False, "llamada %d no llego al bridge: %s" % (i, body(r[i])[:160]))
            break
    else:
        if len(llegaron) == 8:
            check(True, "las 8 llegaron al bridge dentro de FL")

    # ---------------------------------------------------------------- 5
    print("")
    if fails:
        print("  %d comprobacion(es) FALLIDA(S):" % len(fails))
        for f in fails:
            print("    - " + f)
        return 1
    print("  E2E COMPLETO: la cadena entera funciona, incluida la concurrencia.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
