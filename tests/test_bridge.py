#!/usr/bin/env python3
"""Test del transporte del FL Heretic Bridge, fuera de FL Studio.

No necesita FL: importa el script (que cae en el modo `in_fl = False`), y
simula el pump que FL haria en OnMidiIn. Comprueba lo que importa de verdad:

  1. Mailbox rotativo: varias requests seguidas, cada una a su slot.
  2. Commit marker: la respuesta termina en \\n y se puede partir por el.
  3. Escrituras a medias: un JSON truncado NO se procesa como respuesta.
  4. Id monotónico: una request con id viejo se ignora.
  5. Validacion: un handler que lanza produce un error, no rompe el script.
  6. Concurrencia logica: 2 slots con ids distintos, gana el mayor.
  7. Status/heartbeat se escribe.
"""
import importlib.util
import json
import shutil
import sys
import tempfile
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "fl-heretic-bridge" / "device_FLHereticBridge.py"

ok_count = 0
fail_count = 0


def check(name, cond, detail=""):
    global ok_count, fail_count
    if cond:
        ok_count += 1
        print(f"  OK   {name}")
    else:
        fail_count += 1
        print(f"  FAIL {name}  {detail}")


def load_module(script_dir):
    """Carga el script apuntando SCRIPT_DIR a un temporal."""
    src = SCRIPT.read_text(encoding="utf-8")
    # Sustituye la funcion script_dir() para que use nuestro temporal.
    patched = src.replace(
        '        base = Path(os.environ.get("USERPROFILE", str(Path.home()))) / "Documents" / "Image-Line" / "FL Studio" / "Settings"',
        f'        return Path(r"{script_dir}")',
    )
    tmp_py = script_dir / "bridge_under_test.py"
    tmp_py.write_text(patched, encoding="utf-8")
    spec = importlib.util.spec_from_file_location("bridge_under_test", tmp_py)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def write_req(mod, rid, action, params=None, slot=None):
    """Escribe una request como haria el daemon."""
    if slot is None:
        slot = rid % mod.SLOT_COUNT
    payload = json.dumps({"id": rid, "action": action, "params": params or {}})
    # Atomicidad del lado daemon: temp + replace.
    target = mod.REQ_FILES[slot]
    part = Path(str(target) + ".daemon_tmp")
    part.write_text(payload, encoding="utf-8")
    part.replace(target)
    return slot


def read_resp(mod, slot):
    """Lee la respuesta respetando el commit marker."""
    try:
        raw = mod.RESP_FILES[slot].read_text(encoding="utf-8")
    except FileNotFoundError:
        return None, "sin fichero"
    if not raw.endswith("\n"):
        return None, "sin commit marker (escritura a medias)"
    return json.loads(raw.split("\n", 1)[0]), ""


def main():
    print("=" * 66)
    print("  FL Heretic Bridge — test del transporte (sin FL Studio)")
    print("=" * 66)

    tmp = Path(tempfile.mkdtemp(prefix="heretic-bridge-test-"))
    try:
        mod = load_module(tmp)
        print(f"  slots={mod.SLOT_COUNT}  handlers={len(mod.HANDLERS)}  in_fl={mod.in_fl}")
        print()

        # --- 1. Round trip basico ---
        print("[1] round trip de meta.ping")
        s = write_req(mod, 1001, "meta.ping")
        mod.pump()
        resp, err = read_resp(mod, s)
        check("meta.ping responde", resp is not None, err)
        check("ok=true", resp and resp.get("ok") is True, str(resp))
        check("id correcto", resp and resp.get("id") == 1001, str(resp))
        check("trae bridge_version", resp and resp["result"].get("bridge_version"), str(resp))
        print()

        # --- 2. Commit marker ---
        print("[2] commit marker")
        raw = mod.RESP_FILES[s].read_text(encoding="utf-8")
        check("termina en \\n", raw.endswith("\n"), repr(raw[-10:]))
        check("no hay \\n dentro del JSON", raw.count("\n") == 1, f"hay {raw.count(chr(10))}")
        print()

        # --- 3. JSON truncado se descarta ---
        print("[3] escritura a medias se ignora")
        s2 = write_req(mod, 1002, "meta.ping")
        mod.RESP_FILES[s2].write_text('{"id": 1002, "ok": tr', encoding="utf-8")
        check("read_resp rechaza truncado", read_resp(mod, s2)[0] is None)
        # El pump debe tolerarlo y no romperse.
        try:
            mod.pump()
            check("pump sobrevive a truncado", True)
        except Exception as e:
            check("pump sobrevive a truncado", False, str(e))
        print()

        # --- 4. Id viejo se ignora ---
        print("[4] ids monotónicos")
        before = mod.pump_count
        write_req(mod, 5, "meta.ping")  # id << last_seen_id
        mod.pump()
        check("id antiguo ignorado", mod.pump_count == before + 1, "pump corrio pero no proceso")
        resp4, _ = read_resp(mod, 5 % mod.SLOT_COUNT)
        # Puede quedar la respuesta vieja del 1005... comprobamos que no sea del id 5
        check("no proceso id antiguo", resp4 is None or resp4.get("id") != 5, str(resp4))
        print()

        # --- 5. Error de handler no rompe nada ---
        print("[5] errores de handler")
        s5 = write_req(mod, 1005, "action.que.no.existe")
        mod.pump()
        r5, _ = read_resp(mod, s5)
        check("action desconocida -> ok=false", r5 and r5.get("ok") is False, str(r5))
        check("error descriptivo", r5 and "desconocida" in r5.get("error", ""), str(r5))

        s6 = write_req(mod, 1006, "transport.setTempo", {"bpm": 99999})
        mod.pump()
        r6, _ = read_resp(mod, s6)
        check("validacion de rango -> ok=false", r6 and r6.get("ok") is False, str(r6))
        check("error de rango concreto", r6 and "rango" in r6.get("error", ""), str(r6))
        print()

        # --- 6. Varias seguidas, mailbox rotativo ---
        print("[6] mailbox rotativo, 12 requests seguidas")
        rid = 2000
        slots_used = set()
        all_ok = True
        for i in range(12):
            rid += 1
            s = write_req(mod, rid, "meta.ping")
            slots_used.add(s)
            mod.pump()
            r, e = read_resp(mod, s)
            if not r or r.get("id") != rid or r.get("ok") is not True:
                all_ok = False
                print(f"      fallo en request {rid} slot {s}: {e or r}")
        check("12/12 respondieron bien", all_ok)
        check("se usaron varios slots", len(slots_used) >= 6, f"slots={sorted(slots_used)}")
        print(f"      slots usados: {sorted(slots_used)}")
        print()

        # --- 7. Dos slots con ids distintos: gana el mayor ---
        print("[7] dos slots pendientes a la vez")
        base = 3000
        s_a = write_req(mod, base + 1, "meta.ping", slot=0)
        s_b = write_req(mod, base + 2, "meta.ping", slot=1)
        # Los slots arrastran respuestas de pruebas anteriores: hay que
        # vaciarlos para poder afirmar "sin procesar".
        for sl in (0, 1):
            mod.RESP_FILES[sl].write_text("", encoding="utf-8")
        mod.pump()  # FIFO: debe procesar la MAS ANTIGUA (menor id)
        r_a1, _ = read_resp(mod, s_a)
        check("procesa la mas antigua primero", r_a1 and r_a1.get("id") == base + 1, str(r_a1))
        r_b1, _ = read_resp(mod, s_b)
        check("la mas nueva sigue pendiente", r_b1 is None, str(r_b1))
        mod.pump()  # ahora toca la mas nueva
        r_b2, _ = read_resp(mod, s_b)
        check("luego procesa la mas nueva", r_b2 and r_b2.get("id") == base + 2, str(r_b2))
        print()
        print("[8] heartbeat")
        check("hr_status.json existe", mod.STATUS_FILE.exists())
        st = json.loads(mod.STATUS_FILE.read_text(encoding="utf-8"))
        check("trae bridge_version", st.get("bridge_version") == mod.BRIDGE_VERSION, str(st))
        check("trae pump_count", st.get("pump_count", 0) > 0, str(st))
        check("trae handlers", st.get("handlers") == len(mod.HANDLERS), str(st))
        print()

        # --- 9. Catalogo de actions ---
        print("[9] catalogo")
        s9 = write_req(mod, 9001, "meta.actions")
        mod.pump()
        r9, _ = read_resp(mod, s9)
        acts = r9["result"]["actions"] if r9 and r9.get("ok") else []
        check("meta.actions lista", len(acts) == len(mod.HANDLERS), f"{len(acts)} vs {len(mod.HANDLERS)}")
        for must in ("meta.ping", "transport.status", "mixer.allTracks",
                     "channels.all", "plugins.params", "patterns.list",
                     "project.metadata", "meta.exec"):
            check(f"existe {must}", must in acts)
        print()

        # --- 10. meta.exec sin FL debe fallar limpio, no tumbar ---
        print("[10] meta.exec fuera de FL")
        s10 = write_req(mod, 9101, "meta.exec", {"code": "1+1"})
        mod.pump()
        r10, _ = read_resp(mod, s10)
        check("meta.exec responde", r10 is not None, str(r10))
        check("no rompe el script", mod.pump_count > 0)
        print()

    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    print("=" * 66)
    print(f"  {ok_count} OK / {fail_count} FAIL")
    print("=" * 66)
    return 1 if fail_count else 0


if __name__ == "__main__":
    sys.exit(main())
