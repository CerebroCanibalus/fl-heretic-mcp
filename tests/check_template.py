#!/usr/bin/env python3
"""Comprueba si la plantilla esta configurada por defecto.

El daemon busca, por orden:
  1. FL_HERETIC_TEMPLATE_FLP
  2. el .flp mas reciente de la carpeta de FL que no sea backup ni autosave
  3. error con instrucciones

Esta plantilla esta fuera de la carpeta de FL, asi que sin configurarla el
step 2 no la encuentra. Este script dice que haria falta.
"""
import io
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, "tests")
from introspect_api import call, log  # noqa: E402

TPL = Path(r"D:\Mis Juegos\ClaudeMCPs\FLStudioMCP\TemplateProject\TemplateProject.flp")
FL_DIR = Path(os.environ["USERPROFILE"]) / "Documents/Image-Line/FL Studio/Projects"


def candidatas_en_fl():
    out = []
    if FL_DIR.is_dir():
        for p in FL_DIR.rglob("*.flp"):
            n = p.name.lower()
            if "autosav" in n or "overwritten" in n or "backup" in str(p).lower():
                continue
            out.append(p)
    return sorted(out, key=lambda p: p.stat().st_mtime, reverse=True)


def main():
    log("=" * 72)
    log("  Estado de la plantilla")
    log("=" * 72)
    log("")
    log("  plantilla indicada: %s" % TPL)
    log("  existe            : %s" % TPL.is_file())
    log("")

    env = os.environ.get("FL_HERETIC_TEMPLATE_FLP")
    log("  FL_HERETIC_TEMPLATE_FLP en el entorno: %s" % (env or "(no esta)"))
    log("")

    cands = candidatas_en_fl()
    log("  .flp usables en la carpeta de FL: %d" % len(cands))
    for c in cands[:5]:
        log("    %s" % c)
    log("")

    if env == str(TPL):
        log("  ESTA CONFIGURADA. create_project funcionara sin --template.")
    elif cands:
        log("  Se usaria como plantilla: %s" % cands[0])
        log("  (si quieres tu TemplateProject, define FL_HERETIC_TEMPLATE_FLP)")
    else:
        log("  HAY QUE CONFIGURARLA. Sin esto, create_project falla con:")
        log("    'no hay ningun .flp en %s para usar de plantilla'" % FL_DIR)
        log("")
        log("  Opciones:")
        log("    a) Copiar TemplateProject.flp a la carpeta de FL")
        log("    b) Definir FL_HERETIC_TEMPLATE_FLP=%s" % TPL)

    # Comprobar que el template es valido como .flp
    if TPL.is_file():
        head = TPL.read_bytes()[:4]
        log("")
        log("  primeros bytes de la plantilla: %r" % head)
        log("  (un .flp empieza por 'FLP' o es un ZIP 'PK')")
        log("  empieza por PK (zip): %s" % (head[:2] == b"PK"))
        log("  empieza por FLP     : %s" % (head[:3] == b"FLP"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
