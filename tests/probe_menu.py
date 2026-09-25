#!/usr/bin/env python3
"""Abre el menu File de FL y lista sus items, para ver si se puede leer
por Win32 y activar 'Guardar como' con WM_COMMAND en vez de navegar a ciegas.
"""
import ctypes
import json
import sys
import time
from ctypes import wintypes

sys.path.insert(0, "tests")
from introspect_api import call, log  # noqa: E402

user32 = ctypes.WinDLL("user32", use_last_error=True)

user32.FindWindowW.restype = wintypes.HWND
user32.GetMenu.restype = wintypes.HMENU
user32.GetMenuItemCount.argtypes = [wintypes.HMENU]
user32.GetMenuItemCount.restype = ctypes.c_int
user32.GetMenuStringW.argtypes = [wintypes.HMENU, wintypes.UINT, wintypes.LPWSTR, ctypes.c_int,
                                  wintypes.UINT]
user32.GetMenuStringW.restype = ctypes.c_int
user32.GetMenuItemID.argtypes = [wintypes.HMENU, ctypes.c_int]
user32.GetMenuItemID.restype = wintypes.UINT
user32.GetSubMenu.argtypes = [wintypes.HMENU, ctypes.c_int]
user32.GetSubMenu.restype = wintypes.HMENU

MF_BYPOSITION = 0x400


def fl_main_window():
    import subprocess
    out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq FL64.exe", "/FO", "CSV", "/NH"],
                         capture_output=True, text=True).stdout
    pid = None
    for line in out.splitlines():
        c = line.split('"')
        if len(c) >= 4:
            pid = int(c[3])
            break
    if pid is None:
        return None, None

    class E:
        pass
    found = []
    ctx = E()
    ctx.pid = pid
    ctx.out = found
    CB = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def cb(hwnd, lparam):
        p = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
        cls = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(hwnd, cls, 256)
        if p.value == ctx.pid and cls.value == "TFruityLoopsMainForm":
            found.append(hwnd)
        return True

    user32.EnumWindows(CB(cb), 0)
    return (found[0] if found else None), pid


def main():
    log("=" * 70)
    hwnd, pid = fl_main_window()
    log(f"  FL hwnd={hwnd} pid={pid}")
    if not hwnd:
        return 1

    log("")
    log("--- 1. Abrir el menu con FPT_Menu (via bridge, sin teclas) ---")
    r = call("meta.exec", {"code": "transport.globalTransport(midi.FPT_Menu, 1)"})
    log("  " + json.dumps(r)[:160])
    time.sleep(0.6)

    log("")
    log("--- 2. Leer el menu de esa ventana con Win32 ---")
    hmenu = user32.GetMenu(hwnd)
    log(f"  GetMenu -> {hmenu}")
    if hmenu:
        n = user32.GetMenuItemCount(hmenu)
        log(f"  items en el menu raiz: {n}")
        for i in range(max(0, n)):
            buf = ctypes.create_unicode_buffer(256)
            ln = user32.GetMenuStringW(hmenu, i, buf, 256, MF_BYPOSITION)
            item_id = user32.GetMenuItemID(hmenu, i)
            sub = user32.GetSubMenu(hmenu, i)
            name = buf.value if ln > 0 else "<separador>"
            log(f"    [{i:2d}] id={item_id:<6} sub={sub if sub else '-':<6} {name!r}")

    log("")
    log("--- 3. Cerrar el menu ---")
    user32.PostMessageW(wintypes.HWND(hwnd), 0x0012, 0, 0)  # WM_CANCELMODE
    return 0


if __name__ == "__main__":
    sys.exit(main())
