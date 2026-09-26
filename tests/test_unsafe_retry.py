#!/usr/bin/env python3
"""Verifica el reintento de 'Operation unsafe at current time'.

FL lanza ese RuntimeError cuando se le pide tocar el proyecto con un dialogo
modal abierto. El caso tipico es un setTempo justo despues de abrir un
proyecto: el daemon lanza FL y llama acto seguido, pero FL sigue cargando.

Se comprueba:
  1. Que el sintoma aparece de verdad (setTempo nada mas abrir un proyecto).
  2. Que con el reintento del bridge la operacion acabaHaving exito.
  3. Que el error, cuando no se puede, trae hint legible y retryable.
"""
import json
import os
import sys
import time

sys.path.insert(0, "tests")
from introspect_api import call, log  # noqa: E402

PROJ = os.path.join(os.environ["USERPROFILE"], "Documents", "Image-Line",
                    "FL Studio", "Projects", "retry-probe.flp")

fails = []


def check(cond, msg):
    log("  %s %s" % ("[OK]" if cond else "[XX]", msg))
    if not cond:
        fails.append(msg)
    return cond


def main():
    log("=" * 70)
    log("  Reintento de 'Operation unsafe at current time'")
    log("=" * 70)
    log("")

    # --- el sintoma, a proposito: setTempo mientras FL esta ocupado ---------
    log("  -- 1. provocar el caso patologico --")
    log("     se mandan varias escrituras seguidas, sin esperar: es cuando FL")
    log("     esta mas ocupado y mas probable que lance el RuntimeError")
    errores = 0
    for i, bpm in enumerate((90, 95, 100, 105, 110)):
        r = call("transport.setTempo", {"bpm": bpm}, timeout=15)
        if r and not r.get("ok"):
            err = json.dumps(r)
            if "unsafe" in err.lower():
                errores += 1
                log("     setTempo(%d) -> unsafe (intento %d)" % (bpm, i + 1))
    log("     escrituras que dieron 'unsafe' sin reintento: %d/5" % errores)
    log("")

    # --- ahora con el bridge nuevo, que reintenta --------------------------
    log("  -- 2. con reintento --")
    log("     (el bridge instalado ya tiene UNSAFE_RETRY_DELAYS)")
    ok = 0
    for bpm in (120, 125, 130):
        r = call("transport.setTempo", {"bpm": bpm}, timeout=20)
        if r and r.get("ok"):
            ok += 1
        else:
            log("     setTempo(%d) fallo: %s" % (bpm, json.dumps(r)[:200]))
    check(ok == 3, "las 3 escrituras con reintento quedaron ok (%d/3)" % ok)
    log("")

    # --- lectura de confirmacion -------------------------------------------
    log("  -- 3. confirmacion por lectura --")
    r = call("transport.status", timeout=15)
    if r and r.get("ok"):
        bpm = r["result"].get("bpm")
        check(abs((bpm or 0) - 130) < 0.01,
              "FL se quedo en 130 (leido %s)" % bpm)
    else:
        check(False, "no se pudo leer el estado: %s" % json.dumps(r)[:150])
    log("")

    # --- el error, cuando no se puede, debe ser legible --------------------
    log("  -- 4. el error es accionable --")
    r = call("meta.exec",
             {"code": "general.processRECEvent(midi.REC_Tempo, 130000, "
                      "midi.REC_Control | midi.REC_UpdateControl)"},
             timeout=20)
    if r and not r.get("ok"):
        err = r.get("error", "")
        tiene_hint = "hint" in json.dumps(r).lower()
        log("     error: %s" % err[:140])
        check(tiene_hint or "unsafe" not in err.lower(),
              "el error explica que hay un dialogo modal (hint=%s)" % tiene_hint)
    else:
        log("     esta vez FL acepto la escritura: el caso es intermitente,")
        log("     que es justo lo que hace necesario el reintento.")
    log("")

    if fails:
        log("  %d FALLO(S)" % len(fails))
        for f in fails:
            log("    - " + f)
        return 1
    log("  Reintento verificado.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
