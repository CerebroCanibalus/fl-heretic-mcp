#!/usr/bin/env python3
"""save_as PRUEBA REAL: FPT_SaveNew + solo el NOMBRE del fichero.

La carpeta por defecto de FL ya es la que quiere el usuario, asi que no hay
que navegar: solo el nombre. Se escribe en el TQuickEdit que va tras el panel
'Name and location' y se confirma con Enter.
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
    ("EnumWindows", [ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM), wintypes.LPARAM], wintypes.BOOL),
    ("EnumChildWindows", [wintypes.HWND, ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM), wintypes.LPARAM], wintypes.BOOL),
]:
    f = getattr(user32, fn)
    f.argtypes = args
    f.restype = res

WM_CLOSE, WM_SETTEXT, WM_GETTEXT, WM_KEYDOWN, WM_KEYUP = 0x10, 0x0C, 0x0D, 0x100, 0x101
NAME = "heretic-test"
EXPECTED = os.path.join(os.environ["USERPROFILE"], "Documents", "Image-Line",
                        "FL Studio", "Projects", NAME + ".flp")


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
    buf = ctypes.create_unicode_buffer(1024)
    if user32.SendMessageW(h, WM_GETTEXT, 1020, ctypes.cast(buf, ctypes.c_void_p).value):
        return buf.value
    return ""


def main():
    pid = fl_pid()
    if not pid:
        print("FL no corre")
        return 1
    if os.path.exists(EXPECTED):
        os.remove(EXPECTED)
        print("  (borrado el anterior)")

    base = {h for h, _, _ in windows_of(pid)}
    print("  >>> FPT_SaveNew")
    call("meta.exec", {"code": "transport.globalTransport(midi.FPT_SaveNew, 1)"})
    time.sleep(1.6)

    dlg = next((h for h, c, t in windows_of(pid) if h not in base), None)
    if not dlg:
        print("  no abrio el dialogo")
        return 1
    print("  dialogo: %r" % text_of(dlg))

    ks = kids(dlg)
    # El campo de nombre: el TQuickEdit justo tras 'Name and location'
    objetivo, tras = None, False
    for i, k in enumerate(ks):
        if cls_of(k) == "TVectorPanel" and "name and location" in text_of(k).lower():
            tras = True
            continue
        if tras and cls_of(k) == "TQuickEdit":
            objetivo = k
            print("  campo de nombre: TQuickEdit idx %d" % i)
            break
    if objetivo is None:
        objetivo = next((k for k in ks if cls_of(k) == "TQuickEdit"), None)
        print("  fallback: primer TQuickEdit")

    set_text(objetivo, NAME)
    time.sleep(0.3)
    print("  escrito: %r" % read_text(objetivo))

    user32.PostMessageW(objetivo, WM_KEYDOWN, 0x0D, 0)
    time.sleep(0.05)
    user32.PostMessageW(objetivo, WM_KEYUP, 0x0D, 0)
    time.sleep(2.5)

    vis = bool(user32.IsWindowVisible(dlg))
    print("  dialogo visible: %s" % vis)
    if vis:
        user32.PostMessageW(dlg, WM_CLOSE, 0, 0)
        time.sleep(0.4)

    ex = os.path.exists(EXPECTED)
    print("\n  esperado: %s" % EXPECTED)
    print("  existe  : %s   tamano: %s" % (ex, os.path.getsize(EXPECTED) if ex else 0))
    if not ex:
        print("  --- .flp recientes en el disco ---")
        import subprocess
        out = subprocess.run(
            ["powershell", "-NoProfile", "-Command",
             "Get-ChildItem \"$env:USERPROFILE\\Documents\\Image-Line\" -Filter *.flp "
             "-Recurse -EA SilentlyContinue | Sort-Object LastWriteTime -Desc | "
             "Select -First 3 | % { $_.FullName + ' | ' + $_.LastWriteTime }"],
            capture_output=True, text=True).stdout
        print("  " + out.strip())
    return 0 if ex else 1


if __name__ == "__main__":
    sys.exit(main())
