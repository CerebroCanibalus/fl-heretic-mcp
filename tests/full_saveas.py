#!/usr/bin/env python3
"""Ruta completa de save_as con FPT_SaveNew:
abrir dialogo -> escribir ruta en el TQuickEdit de 'Name and location'
-> pulsar el boton de guardar (se busca por texto entre los controles).
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
    ("SetForegroundWindow", [wintypes.HWND], wintypes.BOOL),
    ("keybd_event", [ctypes.c_ubyte, ctypes.c_ubyte, wintypes.DWORD, ctypes.c_size_t], None),
]:
    f = getattr(user32, fn)
    f.argtypes = args
    f.restype = res

WM_CLOSE, WM_SETTEXT, WM_COMMAND, WM_GETTEXT = 0x10, 0x0C, 0x111, 0x0D
TARGET = os.path.join(os.environ["USERPROFILE"], "Documents", "Image-Line",
                       "FL Studio", "Projects", "heretic-test.flp")
BM_CLICK = 0x00F5


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


def descendants(h, depth=0):
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
        print("no abrio el dialogo")
        return 1
    print("  dialogo abierto: %r" % text_of(dlg))

    kids = descendants(dlg)
    print("  controles (todos): %s" % [(cls_of(k), text_of(k)) for k in kids])

    # 1. escribir la ruta
    written = False
    for k in kids:
        if cls_of(k) == "TQuickEdit":
            set_text(k, TARGET)
            time.sleep(0.25)
            if TARGET in read_text(k):
                written = True
                print("  ruta escrita OK")
                break
    if not written:
        print("  no se pudo escribir la ruta")
        user32.PostMessageW(dlg, WM_CLOSE, 0, 0)
        return 1

    # 2. buscar el boton de guardar
    botones = [(cls_of(k), text_of(k)) for k in kids]
    print("  botones: %s" % [b for b in botones if "Btn" in b[0] or "Button" in b[0]])

    # intentar pulsar cada boton por texto
    guardado = None
    for k in kids:
        c = cls_of(k)
        if "Btn" not in c and "Button" not in c:
            continue
        t = text_of(k).strip().lower()
        if any(x in t for x in ("guardar", "save", "aceptar", "ok")) and "como" not in t:
            guardado = k
            break
    if guardado is None:
        # el primero que no sea cancelar/cancel/close
        for k in kids:
            c = cls_of(k)
            if "Btn" not in c:
                continue
            t = text_of(k).strip().lower()
            if not any(x in t for x in ("cancelar", "cancel", "cerrar", "close")):
                guardado = k
                break

    if guardado is not None:
        print("  pulsando boton %r (%s)" % (text_of(guardado), cls_of(guardado)))
        user32.SendMessageW(guardado, BM_CLICK, 0, 0)
    else:
        # FL no expone los botones como ventanas propias: los dibuja dentro
        # del panel. La via fiable es Enter, que activa el boton por defecto.
        # Es UNA tecla y no depende del layout ni del idioma.
        print("  sin boton: SetForegroundWindow + Enter")
        user32.SetForegroundWindow(dlg)
        time.sleep(0.25)
        user32.keybd_event(0x0D, 0, 0, 0)   # VK_RETURN down
        time.sleep(0.05)
        user32.keybd_event(0x0D, 0, 2, 0)   # keyup
        print("  Enter enviado")

    time.sleep(2.0)
    vis = bool(user32.IsWindowVisible(dlg))
    print("  dialogo sigue visible: %s" % vis)
    if vis:
        print("  (lo cierro)")
        user32.PostMessageW(dlg, WM_CLOSE, 0, 0)
        time.sleep(0.5)

    ex = os.path.exists(TARGET)
    print("\n  FICHERO: existe=%s tamano=%s" % (ex, os.path.getsize(TARGET) if ex else 0))
    return 0 if ex else 1


if __name__ == "__main__":
    sys.exit(main())
