#!/usr/bin/env python3
"""Recorre el menu File de FL con FPT_Down y lee el estado en cada paso,
para encontrar en que posicion esta 'Guardar como'.

Si esto funciona, el save_as deja de depender de teclas y de posiciones
supuestas: se cuenta hasta la posicion correcta y se confirma.
"""
import json
import sys
import time

sys.path.insert(0, "tests")
from introspect_api import call, log  # noqa: E402


def probe_state():
    """Que se ve ahora mismo: ventana enfocada y si hay un popup abierto."""
    code = ("out = {}\n"
            "out['popup'] = ui.isInPopupMenu()\n"
            "try:\n"
            "    out['caption'] = ui.getFocusedFormCaption()\n"
            "except Exception as e:\n"
            "    out['caption'] = 'ERR ' + str(e)\n"
            "out['focused'] = ui.getFocused()\n"
            "out")
    r = call("meta.exec", {"code": code})
    if not r or not r.get("ok"):
        return None
    res = r.get("result") or {}
    if not res.get("ok", True):
        return {"error": res.get("error")}
    return res.get("result")


def send(code):
    r = call("meta.exec", {"code": code})
    if not r or not r.get("ok"):
        return None
    res = r.get("result") or {}
    return res.get("result") if res.get("ok", True) else {"error": res.get("error")}


def main():
    log("=" * 72)
    log("  Recorrido del menu File de FL con FPT_*")
    log("=" * 72)
    log("")
    log("  valores de las constantes:")
    for k in ("FPT_Menu", "FPT_Down", "FPT_Up", "FPT_Enter", "FPT_Escape"):
        log(f"    midi.{k:<14} = {send('getattr(midi, \"%s\", None)' % k)}")
    log("")

    log("  estado ANTES de abrir el menu:")
    log("    " + json.dumps(probe_state())[:200])
    log("")

    log("  --- FPT_Menu ---")
    r = send("transport.globalTransport(midi.FPT_Menu, 1)")
    log(f"    retorno = {r}")
    time.sleep(0.5)
    log(f"    estado = {json.dumps(probe_state())[:200]}")
    log("")

    log("  --- 14 pasos de FPT_Down, leyendo en cada uno ---")
    for i in range(14):
        send("transport.globalTransport(midi.FPT_Down, 1)")
        time.sleep(0.18)
        st = probe_state()
        log(f"    {i+1:2d}: {json.dumps(st)[:170] if st else 'None'}")
    log("")

    log("  --- cerrar con Escape ---")
    send("transport.globalTransport(midi.FPT_Escape, 1)")
    time.sleep(0.4)
    log(f"    estado final = {json.dumps(probe_state())[:200]}")
    log("")
    log("  NOTA: esto no dice cual es cada item. Para eso habria que confirmar")
    log("  en un paso concreto y ver que aparece el dialogo. Lo que nos sirve")
    log("  aqui es saber si el menu se abre y si FPT_Down lo mueve.")


if __name__ == "__main__":
    sys.exit(main())
