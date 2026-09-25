#!/usr/bin/env python3
"""Arregla el script de localizacion: usa print en vez de log con 2 args."""
import io

P = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\tests\locate_saveas.py"
s = io.open(P, encoding="utf-8").read()
s = s.replace('    log(f"  >>> FPT_Menu, luego {n}x FPT_Down, luego FPT_Enter")',
              '    print(f"  >>> FPT_Menu, luego {n}x FPT_Down, luego FPT_Enter")')
s = s.replace('    log("  FPT_Menu =", send("transport.globalTransport(midi.FPT_Menu, 1)"))',
              '    print("  FPT_Menu = " + str(send("transport.globalTransport(midi.FPT_Menu, 1)")))')
s = s.replace('    log("  >>> FPT_Enter")', '    print("  >>> FPT_Enter")')
s = s.replace('    log("  estado inicial:", json.dumps(state())[:200])',
              '    log("  estado inicial: " + json.dumps(state())[:200])')
s = s.replace('    log("  popup tras abrir:", json.dumps(state())[:200])',
              '    log("  popup tras abrir: " + json.dumps(state())[:200])')
s = s.replace('    log("  estado tras confirmar:", json.dumps(state())[:200])',
              '    log("  estado tras confirmar: " + json.dumps(state())[:200])')
io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("locate_saveas.py arreglado")
