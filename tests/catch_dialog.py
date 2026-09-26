#!/usr/bin/env python3
"""Pesca el dialogo 'Confirm' que aparece al abrir un proyecto, y lee su texto.

El TMsgForm sale y se va muy rapido: hay que pollar mientras se lanza
`new-project`. Este script existe solo para DIAGNOSTICO (leer que dice el
dialogo). No forma parte del producto.
"""
import ctypes
import os
import subprocess
import sys
import threading
import time
from ctypes import wintypes

user32 = ctypes.WinDLL("user32", use_last_error=True)
user32.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.IsWindowVisible.argtypes = [wintypes.HWND]
user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND,
                                            ctypes.POINTER(wintypes.DWORD)]
ENUMPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

# Delphi owner-draw: GetWindowText puede venir vacio. Se prueba WM_GETTEXT,
# que a veces si responde, y luego el nombre del control como pista.
user32.SendMessageW.restype = ctypes.c_ssize_t
user32.SendMessageW.argtypes = [wintypes.HWND, ctypes.c_uint, ctypes.c_size_t,
                                 ctypes.c_void_p]
WM_GETTEXT = 0x000D


def send_gettext(h):
    buf = ctypes.create_unicode_buffer(1024)
    rc = user32.SendMessageW(h, WM_GETTEXT, 1024, ctypes.cast(buf, ctypes.c_void_p))
    return buf.value if rc > 0 else ""

EXE = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\target\release\fl-heretic.exe"
FL_PROJECTS = os.path.join(os.environ["USERPROFILE"], "Documents", "Image-Line",
                           "FL Studio", "Projects")


def fl_pid():
    out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq FL64.exe",
                          "/FO", "CSV", "/NH"],
                         capture_output=True, text=True).stdout
    for line in out.splitlines():
        c = line.split('"')
        if len(c) >= 4:
            return int(c[3])
    return None


def cls_of(h):
    b = ctypes.create_unicode_buffer(256)
    user32.GetClassNameW(h, b, 256)
    return b.value.strip()


def txt_of(h):
    b = ctypes.create_unicode_buffer(512)
    user32.GetWindowTextW(h, b, 512)
    return b.value.strip()


def top_windows(pid, only_class=None):
    out = []

    def cb(h, _):
        p = wintypes.DWORD()
        user32.GetWindowThreadProcessId(h, ctypes.byref(p))
        if p.value != pid:
            return True
        if not user32.IsWindowVisible(h):
            return True
        c = cls_of(h)
        if only_class is None or c == only_class:
            out.append((h, c, txt_of(h)))
        return True

    user32.EnumWindows(ENUMPROC(cb), 0)
    return out


def children(h):
    out = []

    def cb(c, _):
        out.append((c, cls_of(c), txt_of(c)))
        return True

    user32.EnumChildWindows(h, ENUMPROC(cb), 0)
    return out


def main():
    name = "probe-confirm"
    path = os.path.join(FL_PROJECTS, name + ".flp")
    if os.path.exists(path):
        os.remove(path)

    pid = fl_pid()
    if not pid:
        print("  FL no corre")
        return 1

    caught = {}

    def poller():
        deadline = time.time() + 12
        while time.time() < deadline:
            for h, c, t in top_windows(pid, "TMsgForm"):
                if h not in caught:
                    caught[h] = (c, t, children(h))
                    return
            time.sleep(0.05)

    th = threading.Thread(target=poller, daemon=True)
    th.start()
    time.sleep(0.2)

    p = subprocess.Popen([EXE, "new-project", name], stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, text=True)
    th.join(timeout=13)
    p.wait(timeout=30)

    print("")
    print("  === dialogos capturados ===")
    if not caught:
        print("  no se capturo ninguno (o se escaparon antes del poll)")
    for h, (c, t, kids) in caught.items():
        print("  %s '%s' (hwnd=%s)" % (c, t, h))
        for ch, cc, tt in kids:
            extra = tt or ""
            if not extra and cc in ("TQuickMemo", "TQuickFocusBtn", "TLabel"):
                extra = send_gettext(ch)
            line = "    %-22s %s" % (cc, ("'%s'" % extra) if extra else "(vacio)")
            print(line)

    if os.path.exists(path):
        os.remove(path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
