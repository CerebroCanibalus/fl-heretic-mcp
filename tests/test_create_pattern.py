#!/usr/bin/env python3
"""Prueba real del workaround de crear patrones.

FL no expone createPattern. El truco es que setPatternName sobre el indice
siguiente al ultimo crea el patron. Esta prueba lo verifica de verdad: crea
un patron con nombre reconocible, lo lee de vuelta, y comprueba que
patternCount sube. Deja basura que hay que borrar a mano en FL (no hay
deletePattern en la API).
"""
import sys
import json
import time

sys.path.insert(0, "tests")
from introspect_api import call, log  # noqa: E402

TEST_NAME = "ZZ_HERETIC_TEST"


def patterns_state():
    """Snapshot del estado de patrones, de una sola llamada."""
    code = (
        "out = {'count': patterns.patternCount()}\n"
        "out['names'] = {}\n"
        "for i in range(0, 32):\n"
        "    try:\n"
        "        out['names'][i] = patterns.getPatternName(i)\n"
        "    except Exception as e:\n"
        "        out['names'][i] = '<' + type(e).__name__ + '>'\n"
        "out"
    )
    r = call("meta.exec", {"code": code})
    if not r or not r.get("ok"):
        return None
    res = r.get("result") or {}
    if not res.get("ok", True):
        log(f"  ERROR leyendo estado: {res.get('error')}")
        return None
    return res.get("result")


def main():
    log("=" * 74)
    log("  Workaround de creacion de patrones — PRUEBA REAL")
    log("=" * 74)
    log("")
    log(f"  Se va a crear un patron llamado {TEST_NAME!r}.")
    log("  No se puede borrar desde la API: borralo tu en FL luego.")
    log("")

    before = patterns_state()
    if not before:
        log("  no se pudo leer el estado inicial")
        return 1
    log(f"  ANTES: patternCount() = {before['count']}")
    for i, nm in sorted(before["names"].items(), key=lambda kv: int(kv[0])):
        if nm and not nm.startswith("<"):
            log(f"    [{i}] {nm!r}")
    log("")

    # El patron real, via la action del bridge (no via exec, para probar de
    # verdad el handler).
    r = call("patterns.create", {"name": TEST_NAME})
    log("  action patterns.create -> " + (json.dumps(r)[:300] if r else "TIMEOUT"))
    log("")

    after = patterns_state()
    if not after:
        return 1
    log(f"  DESPUES: patternCount() = {after['count']}")
    for i, nm in sorted(after["names"].items(), key=lambda kv: int(kv[0])):
        if nm and not nm.startswith("<"):
            log(f"    [{i}] {nm!r}")
    log("")

    log("--- Veredicto ---")
    found = [(i, nm) for i, nm in after["names"].items() if nm == TEST_NAME]
    if found:
        idx = found[0][0]
        log(f"  FUNCIONA. El patron {TEST_NAME!r} existe en el indice {idx}.")
        log(f"  patternCount: {before['count']} -> {after['count']}")
        if after["count"] > before["count"]:
            log("  patternCount() aumento: es un patron de verdad, no un rename.")
        else:
            log("  AVISO: patternCount() no aumento. Puede que FL lo considere")
            log("  el mismo slot. Comprueba en FL si hay un patron NUEVO en la lista.")
    else:
        log("  FALLO. No aparece el patron con ese nombre.")
        log("  Puede que FL haya ignorado el setPatternName sobre un indice libre.")

    # Bonus: probar el rename sobre el patron que acabamos de crear, para
    # confirmar que el indice existe de verdad.
    if found:
        idx = found[0][0]
        log("")
        log(f"--- Extra: rename del indice {idx} ---")
        r2 = call("patterns.rename", {"index": idx, "name": TEST_NAME + "_RENAMED"})
        log("  " + (json.dumps(r2)[:200] if r2 else "TIMEOUT"))
        r3 = call("patterns.findByName", {"name": TEST_NAME + "_RENAMED"})
        log("  findByName -> " + (json.dumps(r3)[:200] if r3 else "TIMEOUT"))
    log("")
    log("=" * 74)


if __name__ == "__main__":
    sys.exit(main())
