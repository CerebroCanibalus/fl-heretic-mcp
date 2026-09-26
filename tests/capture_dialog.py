#!/usr/bin/env python3
"""Captura el dialogo 'Confirm' en PANTALLA mientras aparece.

Los intentos anteriores fallaron porque el TMsgForm se autodescarta en
segundos cuando no hay nadie tocando FL. Aqui se queda mirando en bucle
mientras se dispara la operacion que lo provoca (abrir un proyecto nuevo), y
en cuanto lo pilla le saca una captura PNG para leer que dice de verdad.
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
user32.IsWindowVisible.argtypes = [wintypes.HWND]
user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND,
                                            ctypes.POINTER(wintypes.DWORD)]
user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.c_void_p]
ENUMPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

EXE = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\target\release\fl-heretic.exe"
OUT = os.path.join(os.environ.get("TEMP", "."), "confirm.png")


class RECT(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]


def fl_pid():
    out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq FL64.exe",
                          "/FO", "CSV", "/NH"],
                         capture_output=True, text=True).stdout
    for line in out.splitlines():
        c = line.split('"')
        if len(c) >= 4:
            return int(c[3])
    return None


def grab(hwnd, path):
    r = RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    w, h = r.right - r.left, r.bottom - r.top
    if w <= 0 or h <= 0:
        return None
    import ctypes.wintypes as wt
    gdi = ctypes.WinDLL("gdi32")
    # Los handles de Win32 (HDC, HBITMAP) no caben en un int con signo.
    # Hay que declararlos como puntero opaco en TODAS las funciones, o
    # ctypes los convierte a int y revienta con OverflowError.
    P = ctypes.c_void_p
    user32.GetDC.argtypes = [P]
    user32.GetDC.restype = P
    user32.ReleaseDC.argtypes = [P, P]
    gdi.CreateCompatibleDC.argtypes = [P]
    gdi.CreateCompatibleDC.restype = P
    gdi.CreateCompatibleBitmap.argtypes = [P, ctypes.c_int, ctypes.c_int]
    gdi.CreateCompatibleBitmap.restype = P
    gdi.SelectObject.argtypes = [P, P]
    gdi.SelectObject.restype = P
    # BitBlt vive en gdi32, NO en user32.
    gdi.BitBlt.argtypes = [P, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                          ctypes.c_int, P, ctypes.c_int, ctypes.c_int, ctypes.c_uint32]
    gdi.BitBlt.restype = ctypes.c_int

    hdc = user32.GetDC(None)
    mdc = gdi.CreateCompatibleDC(hdc)
    bmp = gdi.CreateCompatibleBitmap(hdc, w, h)
    gdi.SelectObject(mdc, bmp)
    # SRCCOPY = 0x00CC0020
    gdi.BitBlt(mdc, 0, 0, w, h, hdc, r.left, r.top, 0x00CC0020)

    class BITMAPINFOHEADER(ctypes.Structure):
        _fields_ = [("biSize", ctypes.c_uint32), ("biWidth", ctypes.c_int32),
                    ("biHeight", ctypes.c_int32), ("biPlanes", ctypes.c_ushort),
                    ("biBitCount", ctypes.c_ushort), ("biCompression", ctypes.c_uint32),
                    ("biSizeImage", ctypes.c_uint32), ("biXPelsPerMeter", ctypes.c_int32),
                    ("biYPelsPerMeter", ctypes.c_int32), ("biClrUsed", ctypes.c_uint32),
                    ("biClrImportant", ctypes.c_uint32)]

    bmi = BITMAPINFOHEADER()
    bmi.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bmi.biWidth = w
    bmi.biHeight = -h
    bmi.biPlanes = 1
    bmi.biBitCount = 32
    bmi.biCompression = 0
    buf = ctypes.create_string_buffer(w * h * 4)
    # HDC/HBITMAP tienen que ser punteros opacos sin signo: si no,
    # ctypes_trata el handle como entero con signo y revienta con OverflowError.
    gdi.GetDIBits.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32,
                              ctypes.c_uint32, ctypes.c_void_p,
                              ctypes.c_void_p, ctypes.c_uint32]
    gdi.GetDIBits(mdc, bmp, 0, h, buf, ctypes.byref(bmi), 0)

    from PIL import Image
    img = Image.frombuffer("RGBA", (w, h), buf, "raw", "BGRA", 0, 1)
    img.convert("RGB").save(path)
    return path


def find_msg(pid, results):
    def cb(h, _):
        p = wintypes.DWORD()
        user32.GetWindowThreadProcessId(h, ctypes.byref(p))
        if p.value != pid:
            return True
        if not user32.IsWindowVisible(h):
            return True
        b = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(h, b, 256)
        if b.value.strip() == "TMsgForm":
            results.append(h)
            return False
        return True
    user32.EnumWindows(ENUMPROC(cb), 0)


def main():
    try:
        import PIL  # noqa
    except ImportError:
        print("  hace falta Pillow: pip install pillow")
        return 1

    name = "probe-cap"
    path = os.path.join(os.environ["USERPROFILE"], "Documents", "Image-Line",
                        "FL Studio", "Projects", name + ".flp")
    if os.path.exists(path):
        os.remove(path)

    pid = fl_pid()
    if not pid:
        print("  FL no corre")
        return 1

    hit = []

    def watch():
        deadline = time.time() + 25
        while time.time() < deadline and not hit:
            find_msg(pid, hit)
            time.sleep(0.03)

    th = threading.Thread(target=watch, daemon=True)
    th.start()
    time.sleep(0.15)
    subprocess.run([EXE, "new-project", name], capture_output=True, timeout=60)
    th.join(timeout=26)

    if hit:
        print("  TMsgForm capturado, guardando captura...")
        try:
            out = grab(hit[0], OUT)
            print("  captura: %s" % out)
        except Exception as e:
            print("  fallo al capturar: %s" % e)
    else:
        print("  no se pillo a tiempo")

    if os.path.exists(path):
        os.remove(path)
    return 0 if hit else 1


if __name__ == "__main__":
    sys.exit(main())
