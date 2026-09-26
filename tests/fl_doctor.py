#!/usr/bin/env python3
"""Diagnostico UNICO del estado de FL, en un comando.

Este script sustituye a 6 comandos de PowerShell queanum a uno. Cuando algo
va mal, la pregunta siempre es la misma: ¿por que el bridge no responde?
Antes habia que averiguarlo a mano (pump congelado, FL sin foco, dialogo
modal, puerto MIDI mal)... y por eso abrir un proyecto costaba tantas pruebas.

Aqui se distinguen los DOS fallos que se confunden:

  A) FL tiene un DIALOGO MODAL abierto. Se leen bien pero rechaza las
     escrituras con 'Operation unsafe at current time'. El bridge esta sano
     pero inutilizable hasta que se cierra el dialogo.

  B) El bridge esta DORMIDO: el pump no avanza ni con wake MIDI. Suele
     pasar si FL no tiene el foco o si el puerto MIDI de entrada no esta
     conectado al controller script.

Se imprime el veredicto y la accion concreta. Sin adivinar.
"""
import ctypes
import json
import os
import sys
import time
from ctypes import wintypes
from pathlib import Path

BRIDGE = (Path(os.environ["USERPROFILE"])
          / "Documents/Image-Line/FL Studio/Settings/Hardware/FL Heretic Bridge")
STATUS = BRIDGE / "hr_status.json"


def log(m=""):
    print(m)


def fl_pid():
    import subprocess
    out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq FL64.exe",
                          "/FO", "CSV", "/NH"],
                         capture_output=True, text=True).stdout
    for line in out.splitlines():
        c = line.split('"')
        if len(c) >= 4:
            return int(c[3])
    return None


def status():
    try:
        return json.loads(STATUS.read_text(encoding="utf-8"))
    except Exception:
        return None


def pump():
    s = status()
    return s.get("pump_count") if s else None


# --- win32: ventanas de FL -------------------------------------------------
user32 = ctypes.WinDLL("user32", use_last_error=True)
user32.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.IsWindowVisible.argtypes = [wintypes.HWND]
user32.IsWindowEnabled.argtypes = [wintypes.HWND]
user32.IsWindow.argtypes = [wintypes.HWND]
user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND,
                                            ctypes.POINTER(wintypes.DWORD)]
user32.GetForegroundWindow.restype = wintypes.HWND

ENUMPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)


def fl_windows(pid):
    out = []

    def cb(h, _):
        p = wintypes.DWORD()
        user32.GetWindowThreadProcessId(h, ctypes.byref(p))
        if p.value == pid:
            out.append(h)
        return True

    user32.EnumWindows(ENUMPROC(cb), 0)
    return out


def win_info(h):
    c = ctypes.create_unicode_buffer(256)
    user32.GetClassNameW(h, c, 256)
    t = ctypes.create_unicode_buffer(256)
    user32.GetWindowTextW(h, t, 256)
    return {
        "class": c.value.strip(),
        "title": t.value.strip(),
        "visible": bool(user32.IsWindowVisible(h)),
        "enabled": bool(user32.IsWindowEnabled(h)),
    }


def modal_dialogs(pid):
    """Ventana visible pero DESHABILITADA = hay un modal encima.

    Cuando una ventana modal esta abierta, la ventana principal queda
    deshabilitada. Es la firma que se vio en el fallo real.
    """
    vis = [win_info(h) for h in fl_windows(pid)]
    visibles = [w for w in vis if w["visible"]]
    deshabilitadas = [w for w in visibles if not w["enabled"]]
    return visibles, deshabilitadas


def has_focus(pid):
    fg = user32.GetForegroundWindow()
    p = wintypes.DWORD()
    user32.GetWindowThreadProcessId(fg, ctypes.byref(p))
    return p.value == pid


# --- MIDI: intentar despertar ----------------------------------------------
winmm = ctypes.WinDLL("winmm")
winmm.midiOutGetNumDevs.restype = ctypes.c_ulong
winmm.midiOutOpen.argtypes = [ctypes.POINTER(ctypes.c_void_p), ctypes.c_ulong,
                              ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong]
winmm.midiOutClose.argtypes = [ctypes.c_void_p]
winmm.midiOutShortMsg.argtypes = [ctypes.c_void_p, ctypes.c_ulong]


def midi_wake():
    hs = []
    for d in range(winmm.midiOutGetNumDevs()):
        h = ctypes.c_void_p()
        if winmm.midiOutOpen(ctypes.byref(h), d, None, None, 0) == 0:
            hs.append(h)
    for h in hs:
        for ch in range(16):
            winmm.midiOutShortMsg(h, 0x90 | ch)
            winmm.midiOutShortMsg(h, 0x80 | ch)
        time.sleep(0.02)
    for h in hs:
        winmm.midiOutClose(h)
    return len(hs)


def main():
    log("=" * 70)
    log("  Diagnostico de FL")
    log("=" * 70)
    log("")

    pid = fl_pid()
    if not pid:
        log("  [X] FL Studio no esta corriendo.")
        log("      Arrancalo con:  fl-heretic open <proyecto.flp>")
        return 1
    log("  FL Studio corre, pid %d" % pid)

    s = status()
    if not s:
        log("  [X] no hay hr_status.json: el controller script nunca arranco.")
        log("      Selecciona 'FL Heretic Bridge' en Options > MIDI Settings.")
        return 1
    log("  bridge: v%s  fl=%s  pump=%s  uptime=%.0fs  handlers=%s"
        % (s.get("bridge_version"), s.get("fl_version"), s.get("pump_count"),
           s.get("uptime_sec") or 0, s.get("handlers")))
    if s.get("last_error"):
        log("  ultimo error: %s" % s.get("last_error")[:90])
    log("")

    # --- el wake funciona? ---
    before = pump()
    ports = midi_wake()
    time.sleep(1.2)
    after = pump()
    despierte = (after is not None and before is not None and after > before)

    log("  -- wake --")
    log("  puertos MIDI OUT: %d" % ports)
    log("  pump: %s -> %s  %s" % (before, after,
                                   "SE DESPIERTA" if despierte else "NO SE DESPIERTA"))
    log("")

    # --- dialogos modales ---
    visibles, deshab = modal_dialogs(pid)
    log("  -- ventanas de FL --")
    for w in visibles:
        log("    visible enabled=%-6s class=%-12s title='%s'"
            % (w["enabled"], w["class"], w["title"]))
    log("")

    focus = has_focus(pid)
    log("  -- foco --")
    log("    FL tiene el foco: %s" % ("SI" if focus else "NO"))
    log("")

    # --- veredicto ---
    log("  " + "-" * 66)
    if deshab:
        log("  VEREDICTO: FL tiene un DIALOGO MODAL abierto.")
        log("    Por eso rechazan las escrituras con 'Operation unsafe at")
        log("    current time'. El bridge esta SANO, pero FL no deja tocar el")
        log("    proyecto hasta que se cierre el dialogo.")
        log("")
        log("    ACCION: mira la ventana de FL y cierra el dialogo")
        log("    (normalmente un 'Guardar cambios?' que se quedo de una")
        log("    sesion anterior). El bridge se recuperara solo.")
        return 2

    if not despierte:
        log("  VEREDICTO: el bridge esta DORMIDO (el pump no avanza).")
        log("    FL no recibe el wake por MIDI. Causas probables:")
        log("      - FL no tiene el foco y OnRefresh no corre;")
        log("      - el puerto MIDI de entrada no esta conectado al script.")
        log("")
        log("    ACCION: dale el foco a la ventana de FL. Si sigue, en")
        log("    Options > MIDI Settings comprueba que el puerto asignado a")
        log("    'FL Heretic Bridge' tiene Enabled=1.")
        return 3

    log("  VEREDICTO: todo bien. El bridge esta vivo y despierto.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
