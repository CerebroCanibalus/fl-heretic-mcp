#!/usr/bin/env python3
"""Envoltorio de mcp_call para usar desde PowerShell sin peleas con las comillas.

PowerShell se come las comillas dobles del JSON en la linea de comandos, y
json.loads recibe un '{op:status}' que no parsea. Esto hace lo mismo parseo pero
sin que la cita de PowerShell tenga algo que opinar.

    python tools/llamar.py daw_debug status
    python tools/llamar.py daw_debug op unblock
    python tools/llamar.py daw_do fx_list_installed
    python tools/llamar.py daw_midi '{"op":"insert_batch"}'   # si hace falta
    python tools/llamar.py daw_debug @_eval.json               # JSON en fichero
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mcp_call import llama  # noqa: E402

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print(__doc__)
        sys.exit(1)
    tool = sys.argv[1]
    if sys.argv[2].startswith("@"):
        # @fichero: el JSON va en un fichero porque PowerShell destroza las
        # comillas dobles de la linea de comandos. motives ya<TResultados de la verificación>

        with open(sys.argv[2][1:], encoding="utf-8") as fh:
            args = json.load(fh)
    elif sys.argv[2].strip().startswith("{"):
        args = json.loads(sys.argv[2])
    elif len(sys.argv) == 3:
        # Un solo token suelto es el op: `llamar.py daw_debug status`
        args = {"op": sys.argv[2]}
    else:
        # Atajo: pares sueltos "clave valor" desde sys.argv[2:]
        args = {}
        resto = sys.argv[2:]
        for i in range(0, len(resto) - 1, 2):
            clave = resto[i].lstrip("-")
            val = resto[i + 1]
            try:
                args[clave] = json.loads(val)
            except Exception:
                args[clave] = val
    print(json.dumps(llama(tool, args), ensure_ascii=False, indent=1))
