#!/usr/bin/env python3
"""Prueba real: abrir el menu File de FL con FPT_Menu, bajar N posiciones y
confirmar, para ver que dialogo sale. Sirve para localizar 'Guardar como'.

Hay que estar atento: esto manipula la interfaz de FL Studio.
"""
import json
import sys
import time

sys.path.insert(0, "tests")
from introspect_api import call, log  # noqa: E402


def send(code):
    r = call("meta.exec", {"code": code})
    if not r or not r.get("ok"):
        return {"rpc": "timeout"}
    res = r.get("result") or {}
    if not res.get("ok", True):
        return {"exec_err": res.get("error")}
    return res.get("result")


def state():
    return send("out = {'popup': ui.isInPopupMenu()}\n"
                "out['caption'] = ui.getFocusedFormCaption()\n"
                "out")


def main():
    log("=" * 72)
    log("  Localizar 'Guardar como' en el menu File de FL")
    log("=" * 72)
    log("  Si aparece un dialogo, cierralo con Escape.")
    log("")

    log("  estado inicial: " + json.dumps(state())[:200])
    log("")

    n = int(sys.argv[1]) if len(sys.argv) > 1 else 5
    print(f"  >>> FPT_Menu, luego {n}x FPT_Down, luego FPT_Enter")
    print("  FPT_Menu = " + str(send("transport.globalTransport(midi.FPT_Menu, 1)")))
    time.sleep(0.5)
    log("  popup tras abrir: " + json.dumps(state())[:200])
    for i in range(n):
        send("transport.globalTransport(midi.FPT_Down, 1)")
        time.sleep(0.15)
    print("  >>> FPT_Enter")
    send("transport.globalTransport(midi.FPT_Enter, 1)")
    time.sleep(1.2)
    log("  estado tras confirmar: " + json.dumps(state())[:200])
    log("")
    log("  MIRA FL STUDIO: que se ha abierto? (dialogo, submenu, nada)")
    log("  Si se abrio algo pulsalo con Escape para cerrarlo.")


if __name__ == "__main__":
    main()
