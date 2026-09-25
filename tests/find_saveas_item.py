#!/usr/bin/env python3
"""Busca la posicion de 'Guardar como' en el menu File de FL, probando todas.

Por cada posicion: FPT_Menu, N x FPT_Down, FPT_Enter, y se mira con Win32 si
ha aparecido una ventana nueva de FL. Si aparece, se cierra sola para no dejar
a FL bloqueado.

Cuando una posicion abre el dialogo de guardado (tiene un Edit), escribe la
ruta y confirma: eso es el save_as completo, sin simular teclas.
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
user32.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.GetWindowTextLengthW.argtypes = [wintypes.HWND]
user32.IsWindowVisible.argtypes = [wintypes.HWND]
user32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
user32.SendMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
user32.EnumWindows.argtypes = [ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM), wintypes.LPARAM]
user32.EnumChildWindows.argtypes = [wintypes.HWND, ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM), wintypes.LPARAM]

WM_CLOSE = 0x0010
WM_SETTEXT = 0x000C
WM_COMMAND = 0x0111
WM_GETTEXT = 0x000D

TARGET = os.path.join(
    os.environ["USERPROFILE"],
    "Documents", "Image-Line", "FL Studio", "Projects", "heretic-test.flp"
)


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


def send(code):
    r = call("meta.exec", {"code": code})
    if not r or not r.get("ok"):
        return {"err": "timeout"}
    res = r.get("result") or {}
    if not res.get("ok", True):
        return {"err": res.get("error")}
    return res.get("result")


def set_text(h, text):
    wide = (text + "\0").encode("utf-16-le")
    buf = ctypes.create_string_buffer(wide)
    user32.SendMessageW(h, WM_SETTEXT, 0, ctypes.cast(buf, ctypes.c_void_p).value)


def read_text(h):
    n = 1024
    buf = ctypes.create_unicode_buffer(n)
    got = user32.SendMessageW(h, WM_GETTEXT, n, ctypes.cast(buf, ctypes.c_void_p).value)
    return buf.value if got else ""


def main():
    pid = fl_pid()
    if not pid:
        print("FL Studio no esta corriendo")
        return 1
    print("=" * 74)
    print("  Buscando 'Guardar como' en el menu File de FL   (pid %d)" % pid)
    print("=" * 74)
    print("  Destino: %s" % TARGET)
    print("")

    base = {h for h, _, _ in windows_of(pid)}
    print("  ventanas al inicio: %d" % len(base))
    print("")

    for n in range(0, 13):
        # reset: cerrar cualquier popup
        send("transport.globalTransport(midi.FPT_Escape, 1)")
        time.sleep(0.2)

        send("transport.globalTransport(midi.FPT_Menu, 1)")
        time.sleep(0.35)
        for _ in range(n):
            send("transport.globalTransport(midi.FPT_Down, 1)")
            time.sleep(0.12)
        send("transport.globalTransport(midi.FPT_Enter, 1)")
        time.sleep(0.9)

        now = windows_of(pid)
        new = [(h, c, t) for h, c, t in now if h not in base]
        desc = ", ".join("%s/%r" % (c, t) for h, c, t in new) if new else "-"
        print("  N=%2d  ventanas nuevas: %s" % (n, desc))

        if new:
            # Mira si alguna tiene un Edit (dialogo de guardado)
            for h, c, t in new:
                kids = children(h)
                classes = [cls_of(k) for k in kids]
                edit = next((k for k in kids if cls_of(k) == "Edit"), None)
                print("        -> %s  controls: %s" % (c, classes))
                if edit is not None:
                    print("        >>> ES EL DIALOGO DE GUARDADO (n=%d) <<<" % n)
                    set_text(edit, TARGET)
                    time.sleep(0.3)
                    got = read_text(edit)
                    print("        edit contiene: %r" % got)
                    if TARGET in got:
                        user32.PostMessageW(h, WM_COMMAND, 1, 0)
                        time.sleep(1.0)
                        still = user32.IsWindowVisible(h)
                        print("        confirmada; dialogo sigue visible: %s" % still)
                        if still:
                            user32.PostMessageW(h, WM_CLOSE, 0, 0)
                    else:
                        user32.PostMessageW(h, WM_CLOSE, 0, 0)
                    print("")
                    print("  RESULTADO: n=%d" % n)
                    time.sleep(0.6)
                    ex = os.path.exists(TARGET)
                    sz = os.path.getsize(TARGET) if ex else 0
                    print("  fichero existe: %s  tamano: %d" % (ex, sz))
                    return 0 if ex else 1
                else:
                    user32.PostMessageW(h, WM_CLOSE, 0, 0)
                    time.sleep(0.3)

    print("")
    print("  RESULTADO: ninguna posicion abrio el dialogo de guardado.")
    return 1


if __name__ == "__main__":
    sys.exit(main())
