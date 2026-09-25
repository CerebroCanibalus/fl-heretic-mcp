#!/usr/bin/env python3
"""Intenta leer la barra de menus de FL (TNewMenu) via Win32.

Si se puede enumerar, se busca 'New' / 'Nuevo' por su texto y se lanza su
command id. Eso daria un File > New programatico, que es lo unico que falta
para poder crear proyectos VACIOS.
"""
import ctypes
import os
import sys
import time
from ctypes import wintypes

sys.path.insert(0, "tests")
from introspect_api import call, log  # noqa: E402

user32 = ctypes.WinDLL("user32", use_last_error=True)
for fn, args, res in [
    ("GetClassNameW", [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int], ctypes.c_int),
    ("GetWindowTextW", [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int], ctypes.c_int),
    ("GetWindowTextLengthW", [wintypes.HWND], ctypes.c_int),
    ("IsWindowVisible", [wintypes.HWND], wintypes.BOOL),
    ("GetWindowThreadProcessId", [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)], wintypes.DWORD),
    ("GetMenu", [wintypes.HWND], wintypes.HMENU),
    ("GetMenuItemCount", [wintypes.HMENU], ctypes.c_int),
    ("GetMenuStringW", [wintypes.HMENU, wintypes.UINT, wintypes.LPWSTR, ctypes.c_int, wintypes.UINT], ctypes.c_int),
    ("GetMenuItemID", [wintypes.HMENU, ctypes.c_int], wintypes.UINT),
    ("GetSubMenu", [wintypes.HMENU, ctypes.c_int], wintypes.HMENU),
    ("EnumWindows", [ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM), wintypes.LPARAM], wintypes.BOOL),
    ("EnumChildWindows", [wintypes.HWND, ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM), wintypes.LPARAM], wintypes.BOOL),
]:
    f = getattr(user32, fn)
    f.argtypes = args
    f.restype = res

MF_BYPOSITION = 0x400


def fl_pid():
    import subprocess
    out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq FL64.exe", "/FO", "CSV", "/NH"],
                         capture_output=True, text=True).stdout
    for line in out.splitlines():
        c = line.split('"')
        if len(c) >= 4:
            return int(c[3])
    return None


def cls_of(h):
    b = ctypes.create_unicode_buffer(256)
    user32.GetClassNameW(h, b, 256)
    return b.value


def windows_of(pid):
    out = []
    CB = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def cb(h, l):
        p = wintypes.DWORD()
        user32.GetWindowThreadProcessId(h, ctypes.byref(p))
        if p.value == pid and user32.IsWindowVisible(h):
            out.append(h)
        return True

    user32.EnumWindows(CB(cb), 0)
    return out


def kids(h):
    out = []
    CB = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def cb(c, l):
        out.append(c)
        return True

    user32.EnumChildWindows(h, CB(cb), 0)
    return out


def dump_menu(hmenu, depth=0, limit=40):
    n = user32.GetMenuItemCount(hmenu)
    if n <= 0:
        return 0
    shown = 0
    for i in range(n):
        if shown >= limit:
            break
        buf = ctypes.create_unicode_buffer(256)
        ln = user32.GetMenuStringW(hmenu, i, buf, 256, MF_BYPOSITION)
        name = buf.value if ln > 0 else "<separador>"
        mid = user32.GetMenuItemID(hmenu, i)
        sub = user32.GetSubMenu(hmenu, i)
        log("    %s[%d] id=%-8s %r%s" % ("  " * depth, i, mid if mid != 0 else "-",
                                        name, " [submenu]" if sub else ""))
        shown += 1
        if sub:
            dump_menu(sub, depth + 1, max(0, limit - shown))
    return shown


def main():
    pid = fl_pid()
    if not pid:
        log("FL no corre")
        return 1

    main_h = next((h for h in windows_of(pid) if cls_of(h) == "TFruityLoopsMainForm"), None)
    if not main_h:
        log("sin ventana principal")
        return 1
    log("ventana principal: %s" % main_h)

    # 1. GetMenu directo sobre la ventana principal
    hm = user32.GetMenu(main_h)
    log("GetMenu(ventana principal) = %s" % hm)
    if hm:
        log("  items:")
        dump_menu(hm)

    # 2. buscar el TNewMenu entre los hijos
    menus = [k for k in kids(main_h) if cls_of(k) == "TNewMenu"]
    log("")
    log("hijos TNewMenu: %s" % menus)
    for m in menus:
        hm2 = user32.GetMenu(m)
        log("  GetMenu(TNewMenu) = %s" % hm2)
        if hm2:
            log("  items:")
            dump_menu(hm2, limit=30)

    # 3. ver si algun hijo tiene menu propio
    log("")
    log("hijos con GetMenu no nulo:")
    for k in kids(main_h):
        h = user32.GetMenu(k)
        if h:
            log("  %-24s -> menu %s" % (cls_of(k), h))
            dump_menu(h, depth=1, limit=15)
    return 0


if __name__ == "__main__":
    sys.exit(main())
