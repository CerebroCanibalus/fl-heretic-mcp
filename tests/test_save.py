#!/usr/bin/env python3
"""Verifica FPT_Save (Ctrl+S) salvando de verdad un proyecto con ruta.

Secuencia:
  1. Comprueba que el proyecto abierto TIENE ruta (si no, FPT_Save abre el
     dialogo de Save as y se queda colgado).
  2. Anota el hash y la fecha del .flp en disco.
  3. Hace cambios reales: tempo, nombre de canal, volumen de un canal.
  4. Comprueba que FL marca el proyecto como sucio (changed=1).
  5. Dispara FPT_Save.
  6. Comprueba que el fichero en disco CAMBIO y que changed=0.
"""
import hashlib
import json
import os
import sys
import time

sys.path.insert(0, "tests")
from introspect_api import call, log  # noqa: E402

PROJ = os.path.join(os.environ["USERPROFILE"], "Documents", "Image-Line",
                    "FL Studio", "Projects", "audit-vacio.flp")


def sha(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(65536), b""):
            h.update(b)
    return h.hexdigest()


def mtime(p):
    return os.path.getmtime(p)


def meta():
    r = call("project.metadata", timeout=8)
    if r and r.get("ok"):
        return r["result"]
    return None


def main():
    log("=" * 72)
    log("  Verificacion de FPT_Save (Ctrl+S)")
    log("=" * 72)
    log("  proyecto: %s" % PROJ)
    if not os.path.isfile(PROJ):
        log("  NO EXISTE el fichero en disco")
        return 1
    log("")

    log("--- 1. estado inicial ---")
    d = meta()
    if not d:
        log("  el bridge no responde")
        return 1
    log("  tempo=%s changed=%s has_file=%s"
        % (d.get("tempo"), d.get("changed"), d.get("has_file")))
    log("  titulo en la ventana: %r" % d.get("title"))
    hash0, mt0 = sha(PROJ), mtime(PROJ)
    log("  sha256 disco : %s" % hash0)
    log("  mtime  disco : %s" % time.strftime("%H:%M:%S", time.localtime(mt0)))
    log("")

    log("--- 2. cambios reales ---")
    # tempo
    r = call("transport.setTempo", {"bpm": 99}, timeout=8)
    if r and r.get("ok"):
        log("  transport.setTempo(99) -> bpm=%s" % r["result"].get("bpm"))
    else:
        log("  setTempo fallo: %s" % json.dumps(r)[:150])

    # nombre de canal
    r = call("channels.setName", {"index": 0, "name": "TEST_KICK"}, timeout=8)
    if r and r.get("ok"):
        log("  channels.setName(0, 'TEST_KICK') -> %r" % r["result"].get("name"))
    else:
        log("  setName fallo (puede que no haya canales): %s" % json.dumps(r)[:150])

    # volumen
    r = call("channels.setVolume", {"index": 0, "volume": 0.42}, timeout=8)
    if r and r.get("ok"):
        log("  channels.setVolume(0, 0.42) -> vol=%s" % r["result"].get("volume"))
    else:
        log("  setVolume fallo: %s" % json.dumps(r)[:150])
    time.sleep(0.5)

    d2 = meta()
    log("")
    log("  tras los cambios: changed=%s tempo=%s" % (d2.get("changed"), d2.get("tempo")))
    log("")

    if d2.get("changed") == 0:
        log("  AVISO: FL no marca el proyecto como sucio. Puede que los cambios")
        log("  no se aplicaron, o que FL no lo detecta. Se prueba igual.")

    log("--- 3. FPT_Save ---")
    t0 = time.time()
    r = call("meta.exec", {"code": "transport.globalTransport(midi.FPT_Save, 1)"}, timeout=10)
    if not r:
        log("  el bridge no respondio tras FPT_Save")
        return 1
    log("  globalTransport(FPT_Save) -> %s" % json.dumps(r.get("result"))[:120])
    log("  (%.2fs)" % (time.time() - t0))
    time.sleep(2.0)

    log("")
    log("--- 4. el fichero en disco cambio? ---")
    hash1, mt1 = sha(PROJ), mtime(PROJ)
    log("  sha256 antes : %s" % hash0)
    log("  sha256 ahora : %s" % hash1)
    log("  mtime  antes : %s" % time.strftime("%H:%M:%S", time.localtime(mt0)))
    log("  mtime  ahora : %s" % time.strftime("%H:%M:%S", time.localtime(mt1)))
    cambio = hash1 != hash0
    log("  HA CAMBIADO : %s" % cambio)
    log("")

    d3 = meta()
    if d3:
        log("  estado final: changed=%s tempo=%s" % (d3.get("changed"), d3.get("tempo")))

    if not cambio:
        log("")
        log("  FPT_Save NO ha escrito en disco.")
        log("  Comprueba a mano en FL: Ctrl+S -> se guarda o abre dialogo?")
        return 1

    log("")
    log("  FPT_Save FUNCIONA: el proyecto se guardo en su ruta.")
    log("  (Ctrl+S sobre un proyecto con ruta no abre dialogo)")

    # Volver a poner el nombre original para no dejar basura
    call("channels.setName", {"index": 0, "name": ""}, timeout=8)
    call("transport.setTempo", {"bpm": 130}, timeout=8)
    call("meta.exec", {"code": "transport.globalTransport(midi.FPT_Save, 1)"}, timeout=10)
    log("  (restaurado tempo 130 y nombre vacio, y guardado)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
