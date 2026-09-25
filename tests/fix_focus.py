#!/usr/bin/env python3
"""Apunta el campo de ruta al TQuickEdit que va justo despues del panel
'Name and location', en vez del primero (que cuelga de 'Information').
"""
import io

P = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\tests\saveas_focus.py"
s = io.open(P, encoding="utf-8").read()

old = '''    objetivo = None
    ks = kids(dlg)
    if foc and foc in ks:
        objetivo = foc
        print("  uso el control con el foco (idx %d)" % ks.index(foc))
    else:
        for i, k in enumerate(ks):
            if cls_of(k) == "TQuickEdit":
                objetivo = k
                print("  uso TQuickEdit idx %d" % i)
                break
    if not objetivo:'''
new = '''    # El campo de ruta NO es el primer TQuickEdit. El dialogo tiene grupos
    # ('Information', 'Name and location', 'Time settings') y el campo de la
    # ruta va DESPUES del panel 'Name and location'. El primero cuelga de
    # 'Information' y es otro campo (nombre/autor).
    objetivo = None
    ks = kids(dlg)
    tras_name = False
    for i, k in enumerate(ks):
        c = cls_of(k)
        if c == "TVectorPanel" and "name and location" in text_of(k).lower():
            tras_name = True
            continue
        if tras_name and c == "TQuickEdit":
            objetivo = k
            print("  uso el TQuickEdit idx %d (tras 'Name and location')" % i)
            break
    if objetivo is None:
        for i, k in enumerate(ks):
            if cls_of(k) == "TQuickEdit":
                objetivo = k
                print("  fallback: TQuickEdit idx %d" % i)
                break
    if not objetivo:'''
assert old in s
s = s.replace(old, new, 1)
io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("saveas_focus.py: ahora apunta al edit correcto")
