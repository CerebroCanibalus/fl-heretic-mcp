# name=Heretic Probe
# url=https://github.com/CerebroCanibalus/fl-heretic-mcp
# receiveFrom=Heretic Probe
"""Sonda de capacidades del sandbox de FL Studio 2025.

Responde a UNA pregunta: ¿puede el script abrir Named Pipes (o cualquier otro
IPC que no sea fichero)?

Escribe el resultado en probe_result.json (en su propio directorio, que el
sandbox SI permite) para que el daemon pueda leerlo. No necesita MIDI: se
autolanza en OnInit y en cada OnIdle.
"""

import json
import os
import sys
import time
from pathlib import Path


def _script_dir():
    """Ruta del script. NO usar __file__: el sub-interprete de FL no lo define."""
    if sys.platform == "win32":
        base = Path(os.environ.get("USERPROFILE", str(Path.home()))) / "Documents" / "Image-Line" / "FL Studio" / "Settings"
    else:
        base = Path.home() / "Documents" / "Image-Line" / "FL Studio" / "Settings"
    return base / "Hardware" / "HereticProbe"


SCRIPT_DIR = _script_dir()
RESULT = SCRIPT_DIR / "probe_result.json"

# FL Studio API — solo para confirmar que estamos dentro de FL.
try:
    import general
    _IN_FL = True
except ImportError:
    general = None
    _IN_FL = False

PIPE_NAME = r"\\.\pipe\fl-heretic-probe"
_fifo = None


def _out():
    return SCRIPT_DIR / "probe_stdout.log"


def _log(msg):
    # print() va al log de FL (View > Script output), que es el canal fiable.
    try:
        print("[probe] %s" % msg)
    except Exception:
        pass
    try:
        with open(_out(), "a", encoding="utf-8") as f:
            f.write("[%s] %s\n" % (time.strftime("%H:%M:%S"), msg))
    except Exception:
        pass


def _t(name, fn):
    """Ejecuta una prueba y registra ok/error sin dejar que una excepción tumbe el script."""
    try:
        value = fn()
        return {"test": name, "ok": True, "value": str(value)[:300]}
    except Exception as e:
        return {"test": name, "ok": False, "error": "%s: %s" % (type(e).__name__, e)}


# ======================================================================
# Pruebas
# ======================================================================

def t_in_fl():
    return "dentro de FL, version %s" % (general.getVersion() if _IN_FL else "?")


def t_open_normal():
    """Control: escribir/leer un fichero normal. Debe funcionar."""
    p = SCRIPT_DIR / "probe_scratch.txt"
    with open(p, "w", encoding="utf-8") as f:
        f.write("hola")
    with open(p, "r", encoding="utf-8") as f:
        data = f.read()
    return "write+read = %r" % data


def t_import_ctypes():
    import ctypes
    return "ctypes importado, %d attrs" % len(dir(ctypes))


def t_ctypes_loadlibrary():
    import ctypes
    k32 = ctypes.windll.kernel32
    return "kernel32 cargado, %d símbolos" % len(dir(k32))


def t_ctypes_createnamedpipe():
    """ESTE es el test clave: crear un named pipe SERVIDOR."""
    import ctypes
    from ctypes import wintypes
    k32 = ctypes.windll.kernel32
    k32.CreateNamedPipeW.restype = wintypes.HANDLE
    k32.CreateNamedPipeW.argtypes = [
        wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, wintypes.DWORD,
        wintypes.DWORD, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p,
    ]
    # PIPE_ACCESS_DUPLEX=3, PIPE_TYPE_BYTE=0, PIPE_READMODE_BYTE=0,
    # PIPE_WAIT=0, PIPE_UNLIMITED_INSTANCES=255
    h = k32.CreateNamedPipeW(PIPE_NAME, 3, 0, 0, 0, 0, 255, None)
    if h in (0, -1, 0xFFFFFFFFFFFFFFFF):
        raise OSError("CreateNamedPipeW devolvio %r" % h)
    return "named pipe CREADO, handle=%r" % h


def t_open_pipe_write():
    r"""Abrir el named pipe como CLIENTE en escritura.

    En Windows `open()` sobre \\.\pipe\\ SI es legal (usa CreateFileW), pero
    solo como cliente: si no hay servidor, falla con ERROR_PIPE_BUSY o
    ERROR_FILE_NOT_FOUND. Probamos igual: si el pipe existe de antes y hay
    alguien escuchando, funcionaria.
    """
    with open(PIPE_NAME, "wb") as f:
        f.write(b'{"probe":true}')
    return "abierto y escrito"


def t_socket():
    """Ya sabemos que falla, pero lo medimos con el script real."""
    import socket
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.close()
    return "socket creado"


def t_mkfifo():
    """Solo POSIX. En Windows dara AttributeError; lo dejamos por completitud."""
    os.mkfifo(str(SCRIPT_DIR / "probe_fifo"))
    return "fifo creado"


def t_os_rename():
    a = SCRIPT_DIR / "probe_a.txt"
    b = SCRIPT_DIR / "probe_b.txt"
    with open(a, "w", encoding="utf-8") as f:
        f.write("x")
    os.rename(str(a), str(b))
    return "rename OK"


def t_glob():
    hits = list(Path(SCRIPT_DIR).glob("*.txt"))
    return "%d ficheros .txt" % len(hits)


TESTS = [
    ("in_fl", t_in_fl),
    ("open_normal_CONTROL", t_open_normal),
    ("import_ctypes", t_import_ctypes),
    ("ctypes_loadlibrary", t_ctypes_loadlibrary),
    ("ctypes_CreateNamedPipe_CLAVE", t_ctypes_createnamedpipe),
    ("open_pipe_write_CLIENTE", t_open_pipe_write),
    ("socket", t_socket),
    ("os_mkfifo", t_mkfifo),
    ("os_rename", t_os_rename),
    ("glob", t_glob),
]


def run_probe():
    results = []
    for name, fn in TESTS:
        r = _t(name, fn)
        results.append(r)
        _log("%-34s ok=%s %s" % (name, r["ok"], r.get("value") or r.get("error")))
    payload = {
        "in_fl": _IN_FL,
        "python": sys.version,
        "idle_ticks": _idle[0],
        "results": results,
        "ts": time.time(),
    }
    with open(RESULT, "w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2, ensure_ascii=False)


# ======================================================================
# Callbacks de FL
# ======================================================================

_idle = [0]


def OnInit():
    print("[probe] === OnInit: arrancando sonda ===")
    _log("=== Sonda Heretic: OnInit ===")
    try:
        run_probe()
        _log("sonda escrita en %s" % RESULT)
    except Exception as e:
        _log("run_probe fallo: %s: %s" % (type(e).__name__, e))


def OnIdle():
    # Si OnIdle dispara, el contador sube. Esto es la comprobacion mas
    # importante de todas segun el informe del sandbox de FL 2025.
    _idle[0] += 1
    if _idle[0] in (1, 5, 30, 120):
        _log("OnIdle tick #%d" % _idle[0])
    if _idle[0] in (5, 120):
        # Re-escribe el resultado con el contador actualizado.
        try:
            with open(RESULT, "r", encoding="utf-8") as f:
                payload = json.load(f)
            payload["idle_ticks"] = _idle[0]
            with open(RESULT, "w", encoding="utf-8") as f:
                json.dump(payload, f, indent=2, ensure_ascii=False)
        except Exception:
            pass


def OnDeInit():
    _log("=== Sonda Heretic: OnDeInit, idle_ticks=%d ===" % _idle[0])
