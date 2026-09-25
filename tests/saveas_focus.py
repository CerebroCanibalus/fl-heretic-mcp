#!/usr/bin/env python3
"""Ultimo intento para save_as: usar GetFocus para saber que campo es el de
la ruta, escribir ahi, y mandarle el Enter al propio control.
"""
import ctypes
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
    ("GetFocus", [], wintypes.HWND),
    ("SetFocus", [wintypes.HWND], wintypes.HWND),
    ("GetDlgCtrlID", [wintypes.HWND], ctypes.c_int),
    ("EnumWindows", [ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM), wintypes.LPARAM], wintypes.BOOL),
    ("EnumChildWindows", [wintypes.HWND, ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM), wintypes.LPARAM], wintypes.BOOL),
]:
    f = getattr(user32, fn)
    f.argtypes = args
    f.restype = res

WM_CLOSE, WM_SETTEXT, WM_GETTEXT = 0x10, 0x0C, 0x0D
WM_KEYDOWN, WM_KEYUP = 0x100, 0x101
WM_COMMAND, BM_CLICK = 0x111, 0xF5
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


def kids(h):
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
    buf = ctypes.create_unicode_buffer(2048)
    if user32.SendMessageW(h, WM_GETTEXT, 2044, ctypes.cast(buf, ctypes.c_void_p).value):
        return buf.value
    return ""


def main():
    pid = fl_pid()
    if not pid:
        print("FL no corre")
        return 1
    if os.path.exists(TARGET):
        os.remove(TARGET)
    base = {h for h, _, _ in windows_of(pid)}
    call("meta.exec", {"code": "transport.globalTransport(midi.FPT_SaveNew, 1)"})
    time.sleep(1.5)
    dlg = next((h for h, c, t in windows_of(pid) if h not in base), None)
    if not dlg:
        print("no abrio dialogo")
        return 1

    print("  dialogo %r" % text_of(dlg))
    foc = user32.GetFocus()
    print("  GetFocus -> %s (%s)  esta en el dialogo: %s"
          % (foc, cls_of(foc) if foc else "?", "si" if foc and foc in kids(dlg) else "no"))

    # Escribir en el que tiene el foco; si no esta en el dialogo, probar
    # el TQuickEdit del grupo 'Name and location'.
    # El campo de ruta NO es el primer TQuickEdit. El dialogo tiene grupos
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
    if not objetivo:
        print("  sin control editable")
        user32.PostMessageW(dlg, WM_CLOSE, 0, 0)
        return 1

    user32.SetFocus(objetivo)
    time.sleep(0.15)
    set_text(objetivo, TARGET)
    time.sleep(0.3)
    print("  escrito: %r" % read_text(objetivo)[:100])

    # Enviar Enter al propio control
    user32.PostMessageW(objetivo, WM_KEYDOWN, 0x0D, 0)
    time.sleep(0.05)
    user32.PostMessageW(objetivo, WM_KEYUP, 0x0D, 0)
    time.sleep(2.0)

    vis = bool(user32.IsWindowVisible(dlg))
    print("  dialogo visible tras Enter: %s" % vis)
    ex = os.path.exists(TARGET)
    print("  FICHERO existe=%s" % ex)

    if not ex and vis:
        # Ultimo recurso: buscar TODOS los controles de FL, incluidos los no
        # visibles, por si el boton vive en otra ventana.
        print("  --- todas las ventanas de FL (visibles e invisibles) ---")
        for h, c, t in windows_of(pid):
            if c not in ("TFruityLoopsMainForm", "TApplication"):
                print("    %s / %r  hijos=%s" % (c, t, [cls_of(k) for k in kids(h)]))

    if vis:
        user32.PostMessageW(dlg, WM_CLOSE, 0, 0)
    return 0 if ex else 1


if __name__ == "__main__":
    sys.exit(main())
