#!/usr/bin/env python3
"""Cierra el TWelcomeWizard de FL si esta abierto.

FL Studio 2025 muestra un 'Welcome to FL Studio' al arrancar SIN proyecto. Es
modal: mientras esta, el controller script no recibe eventos (el pump se
congela) y FL rechaza toda escritura con 'Operation unsafe at current time'.

Medido: por eso 'abrir un proyecto' fallaba de forma tan lenta e
intermitente. Reiniciar FL a ciegas lo dispara siempre, y un reinicio pedido
por el propio agente lo dispara tambien. No es un dialogo de guardar: es el
wizard, y aparece con proyecto vacio.

Este script lo cierra de forma acotada: busca SOLO la ventana de clase
TWelcomeWizard del proceso FL64 y le manda WM_CLOSE. No toca nada mas.
"""
import ctypes
import subprocess
import time
from ctypes import wintypes

user32 = ctypes.WinDLL("user32", use_last_error=True)
user32.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.IsWindowVisible.argtypes = [wintypes.HWND]
user32.IsWindow.argtypes = [wintypes.HWND]
user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND,
                                            ctypes.POINTER(wintypes.DWORD)]
user32.PostMessageW.argtypes = [wintypes.HWND, ctypes.c_uint,
                                ctypes.c_ulong, ctypes.c_ulong]
user32.SendMessageW.argtypes = [wintypes.HWND, ctypes.c_uint,
                                ctypes.c_ulong, ctypes.c_ulong]

WM_CLOSE = 0x0010
ENUMPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)


def fl_pid():
    out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq FL64.exe",
                          "/FO", "CSV", "/NH"],
                         capture_output=True, text=True).stdout
    for line in out.splitlines():
        c = line.split('"')
        if len(c) >= 4:
            return int(c[3])
    return None


def find_welcome(pid):
    found = []

    def cb(h, _):
        p = wintypes.DWORD()
        user32.GetWindowThreadProcessId(h, ctypes.byref(p))
        if p.value != pid:
            return True
        c = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(h, c, 256)
        if c.value.strip() == "TWelcomeWizard" and user32.IsWindowVisible(h):
            t = ctypes.create_unicode_buffer(256)
            user32.GetWindowTextW(h, t, 256)
            found.append((h, t.value.strip()))
        return True

    user32.EnumWindows(ENUMPROC(cb), 0)
    return found


def main():
    pid = fl_pid()
    if not pid:
        print("  FL no corre: nada que cerrar")
        return 0

    wizards = find_welcome(pid)
    if not wizards:
        print("  no hay TWelcomeWizard abierto: FL esta usable")
        return 0

    for h, title in wizards:
        print("  TWelcomeWizard abierto (%r), cerrando..." % title)
        user32.PostMessageW(h, WM_CLOSE, 0, 0)

    # El cierre es asincrono: dar tiempo a que FL lo procese.
    for _ in range(20):
        time.sleep(0.25)
        if not find_welcome(pid):
            print("  cerrado. FL deberia estar usable ya")
            return 0
        # Si no se cierra con WM_CLOSE (a veces el wizard pide confirmacion),
        # se reintenta unas veces antes de rendirse.
        for h, _t in find_welcome(pid):
            user32.PostMessageW(h, WM_CLOSE, 0, 0)

    left = find_welcome(pid)
    if left:
        print("  [X] el wizard sigue abierto tras varios intentos")
        print("      cierralo a mano: es la ventana 'Welcome to FL Studio'")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
