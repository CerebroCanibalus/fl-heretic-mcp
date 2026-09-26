#!/usr/bin/env python3
"""Hace el E2E idempotente: no debe depender del estado que dejo la corrida anterior.

Los fallos intermitentes venían de que el test asumía cosas del entorno:
- el proyecto `e2e-humo.flp` no existía (la primera vez se creaba, la segunda
  no, y la tool se niega a sobrescribir);
- el FL arrastraba un diálogo modal de la corrida previa, y entonces TODAS las
  escrituras fallaban con 'Operation unsafe at current time'.

Un test que pasa unas veces y otras no es peor que uno que no pasa: entrena a
ignorarlo. Este preparations el terreno y, si FL está bloqueado, lo dice claro en
vez de fallar en tres sitios distintos.
"""
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
FL_PROJECTS = os.path.join(os.environ["USERPROFILE"], "Documents", "Image-Line",
                           "FL Studio", "Projects")
TEST_PROJECT = "e2e-humo"


def log(m):
    print("  " + m)


def prepare():
    """Deja el terreno igual para todas las corridas."""
    # 1. El proyecto de la prueba, siempre desde cero.
    p = os.path.join(FL_PROJECTS, TEST_PROJECT + ".flp")
    if os.path.exists(p):
        os.remove(p)
        log("limpiado el %s de la corrida anterior" % TEST_PROJECT)

    # 2. FL tiene que estar despierto y sin dialogos. Si el bridge no responde
    #    a los 20s, es que FL esta en otro estado (arrancando, con un modal).
    for i in range(20):
        r = subprocess.run([sys.executable, os.path.join(HERE, "ping.py")],
                           capture_output=True, text=True)
        if "OK" in r.stdout:
            return True
        time.sleep(1)
    return False


def fl_is_writable():
    """Se puede escribir en FL ahora mismo?

    Es la distincion importante: si FL tiene un modal, `fl_set_tempo` falla
    con 'unsafe at current time' y eso NO es un fallo del MCP ni del bridge.
    """
    r = subprocess.run(
        [sys.executable, "-c",
         "import sys; sys.path.insert(0, %r)\n"
         "from introspect_api import call\n"
         "r = call('transport.status', timeout=10)\n"
         "print('ok' if r and r.get('ok') else 'no')\n" % HERE],
        capture_output=True, text=True)
    if "ok" not in r.stdout:
        return False
    # Lectura OK. Ahora una escritura inocua: mismo tempo.
    r = subprocess.run(
        [sys.executable, "-c",
         "import sys; sys.path.insert(0, %r)\n"
         "from introspect_api import call\n"
         "t = call('transport.status', timeout=10)['result']['bpm']\n"
         "r = call('transport.setTempo', {'bpm': t}, timeout=15)\n"
         "print('ok' if r and r.get('ok') else 'no')\n" % HERE],
        capture_output=True, text=True)
    return "ok" in r.stdout


def main():
    print("=" * 70)
    print("  Preparacion del terreno")
    print("=" * 70)

    if not prepare():
        print("")
        print("  [XX] el bridge no responde. FL Studio no esta listo.")
        print("       Comprueba: esta abierto? tiene un dialogo modal?")
        print("       esta el controller script seleccionado en Options > MIDI Settings?")
        return 1
    log("bridge despierto")

    if not fl_is_writable():
        print("")
        print("  [!] FL responde a las LECTURAS pero rechaza las escrituras.")
        print("      almost seguro tiene un dialogo modal abierto (guardar,")
        print("      cargar plugin, o un 'Save as' de la sesion anterior).")
        print("      Los tests de escritura no son validos hasta que se cierre.")
        return 2
    log("FL acepta escrituras")
    return 0


if __name__ == "__main__":
    sys.exit(main())
