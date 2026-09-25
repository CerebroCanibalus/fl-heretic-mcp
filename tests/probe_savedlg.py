#!/usr/bin/env python3
"""Con FPT_SaveNew el dialogo aparece (TNewProjForm 'Save as'), pero sus
controles son TQuickEdit, no 'Edit' de Win32. Este script prueba cada
TQuickEdit para ver cual es el de la ruta.
"""
import ctypes
import json
import os
import sys
import time
from ctypes import wintypes

sys.path.insert(0, "tests")
from introspect_api import call  # noqa: E402

user32 = ctypes.WinDLL("user32", use_last_error=True)
for fn, args, res in [
    ("GetClassNameW", [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int], ctypes.c_int),
    ("GetWindowTextW", [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int], ctypes.c_int),
    ("GetWindowTextLengthW", [wintypes.HWND], ctypes.c_int),
    ("IsWindowVisible", [wintypes.HWND], wintypes.BOOL),
    ("PostMessageW", [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM], wintypes.BOOL),
    ("SendMessageW", [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM], ctypes.c_longlong),
    ("GetWindowThreadProcessId", [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)], wintypes.DWORD),
    ("EnumWindows", [ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM), wintypes.LPARAM], wintypes.BOOL),
    ("EnumChildWindows", [wintypes.HWND, ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM), wintypes.LPARAM], wintypes.BOOL),
]:
    f = getattr(user32, fn)
    f.argtypes = args
    f.restype = res

WM_CLOSE, WM_SETTEXT, WM_COMMAND, WM_GETTEXT = 0x10, 0x0C, 0x111, 0x0D
TARGET = os.path.join(os.environ["USERPROFILE"], "Documents", "Image-Line",
                       "FL Studio", "Projects", "heretic-test.flp")


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


def text_of(h):
    n = user32.GetWindowTextLengthW(h)
    if n <= 0:
        return ""
    b = ctypes.create_unicode_buffer(n + 2)
    user32.GetWindowTextW(h, b, n + 2)
    return b.value


def windows_of(pid):
    out = []
    CB = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def cb(h, l):
        p = wintypes.DWORD()
        user32.GetWindowThreadProcessId(h, ctypes.byref(p))
        if p.value == pid and user32.IsWindowVisible(h):
            out.append((h, cls_of(h), text_of(h)))
        return True

    user32.EnumWindows(CB(cb), 0)
    return out


def children(h):
    out = []
    CB = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def cb(c, l):
        out.append(c)
        return True

    user32.EnumChildWindows(h, CB(cb), 0)
    return out


def set_text(h, t):
    raw = (t + "\0").encode("utf-16-le")
    buf = ctypes.create_string_buffer(raw, len(raw))
    user32.SendMessageW(h, WM_SETTEXT, 0, ctypes.cast(buf, ctypes.c_void_p).value)


def read_text(h):
    buf = ctypes.create_unicode_buffer(1024)
    if user32.SendMessageW(h, WM_GETTEXT, 1024, ctypes.cast(buf, ctypes.c_void_p).value):
        return buf.value
    return ""


def main():
    pid = fl_pid()
    if not pid:
        print("FL no corre")
        return 1
    base = {h for h, _, _ in windows_of(pid)}

    # Si ya hay un 'Save as' abierto de la prueba anterior, cerrarlo.
    for h, c, t in windows_of(pid):
        if t == "Save as":
            user32.PostMessageW(h, WM_CLOSE, 0, 0)
            time.sleep(0.5)

    call("meta.exec", {"code": "transport.globalTransport(midi.FPT_SaveNew, 1)"})
    time.sleep(1.5)

    dlg = None
    for h, c, t in windows_of(pid):
        if h not in base:
            dlg = h
            print("  dialogo: %s / %r" % (c, t))
            break
    if not dlg:
        print("  no abrio el dialogo")
        return 1

    kids = children(dlg)
    print("  %d controles:" % len(kids))
    for i, k in enumerate(kids):
        print("    [%d] %-16s texto=%r" % (i, cls_of(k), text_of(k)))

    # Probar a escribir en cada TQuickEdit y ver cual lo acepta.
    print("")
    for i, k in enumerate(kids):
        if cls_of(k) != "TQuickEdit":
            continue
        set_text(k, TARGET)
        time.sleep(0.25)
        got = read_text(k)
        ok = TARGET in got
        print("    [%d] TQuickEdit -> %s   contiene=%r" % (i, "ACEPTADO" if ok else "rechazado", got[:80]))
        if ok:
            print("")
            print("  >>> EL EDIT DE LA RUTA ES EL INDICE %d <<<" % i)
            user32.PostMessageW(dlg, WM_COMMAND, 1, 0)
            time.sleep(1.5)
            print("  dialogo sigue visible: %s" % bool(user32.IsWindowVisible(dlg)))
            if user32.IsWindowVisible(dlg):
                user32.PostMessageW(dlg, WM_CLOSE, 0, 0)
            time.sleep(0.5)
            ex = os.path.exists(TARGET)
            print("  FICHERO: existe=%s tamano=%s" % (ex, os.path.getsize(TARGET) if ex else 0))
            return 0 if ex else 1

    # Si ninguno acepto texto plano, al menos tenemos el mapa.
    print("")
    print("  Ningun TQuickEdit acepto texto plano. Puede hacer falta")
    print("  WM_SETTEXT al control padre, o el dialogo tiene su propia ruta.")
    user32.PostMessageW(dlg, WM_CLOSE, 0, 0)
    return 1


if __name__ == "__main__":
    sys.exit(main())
