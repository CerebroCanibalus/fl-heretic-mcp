#!/usr/bin/env python3
"""Prueba FPT_SaveNew: deberia abrir el dialogo de 'Save as' directamente,
sin menus ni teclas. Si aparece con un Edit, se rellena la ruta y confirma.
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
    buf = ctypes.create_buffer((t + "\0").encode("utf-16-le"))
    user32.SendMessageW(h, WM_SETTEXT, 0, ctypes.cast(buf, ctypes.c_void_p).value)


def read_text(h):
    buf = ctypes.create_unicode_buffer(1024)
    if user32.SendMessageW(h, WM_GETTEXT, 1024, ctypes.cast(buf, ctypes.c_void_p).value):
        return buf.value
    return ""


def main():
    pid = fl_pid()
    if not pid:
        print("FL Studio no corre")
        return 1
    print("=" * 72)
    print("  FPT_SaveNew  (midi.FPT_SaveNew = %s)" %
          json.dumps((call("meta.exec", {"code": "midi.FPT_SaveNew"}) or {})
                     .get("result", {}).get("result")))
    print("=" * 72)
    base = {h for h, _, _ in windows_of(pid)}
    print("  ventanas antes: %d" % len(base))

    r = call("meta.exec", {"code": "transport.globalTransport(midi.FPT_SaveNew, 1)"})
    print("  globalTransport(FPT_SaveNew) -> %s" % json.dumps(r)[:140])
    time.sleep(1.5)

    now = windows_of(pid)
    new = [(h, c, t) for h, c, t in now if h not in base]
    print("  ventanas nuevas: %s" % (", ".join("%s/%r" % (c, t) for h, c, t in new) or "-"))

    if not new:
        print("\n  NO aparecio ninguna ventana. Puede que el atajo este en otro sitio")
        print("  o que FL lo ignore si ya tiene ruta.")
        return 1

    for h, c, t in new:
        kids = children(h)
        print("\n  %s / %r" % (c, t))
        print("  controls: %s" % [cls_of(k) for k in kids])
        edit = next((k for k in kids if cls_of(k) == "Edit"), None)
        if edit:
            set_text(edit, TARGET)
            time.sleep(0.4)
            got = read_text(edit)
            print("  edit tras escribir: %r" % got)
            if TARGET in got:
                user32.PostMessageW(h, WM_COMMAND, 1, 0)
                time.sleep(1.5)
                vis = user32.IsWindowVisible(h)
                print("  confirmado; dialogo visible: %s" % vis)
                if vis:
                    user32.PostMessageW(h, WM_CLOSE, 0, 0)
                time.sleep(0.5)
    ex = os.path.exists(TARGET)
    print("\n  FICHERO: existe=%s tamano=%s" % (ex, os.path.getsize(TARGET) if ex else 0))
    return 0 if ex else 1


if __name__ == "__main__":
    sys.exit(main())
