import sys, json
sys.path.insert(0, "tests")
from introspect_api import call, log

log("=== cada canal: instrumento y sample ===")
code = ("out = []\n"
        "for i in range(channels.channelCount(True)):\n"
        "    d = {'index': i}\n"
        "    d['name'] = channels.getChannelName(i)\n"
        "    try: d['type'] = channels.getChannelType(i)\n"
        "    except Exception as e: d['type'] = 'ERR'\n"
        "    d['midi_in'] = None\n"
        "    try: d['midi_in'] = channels.getChannelMidiInPort(i)\n"
        "    except Exception: pass\n"
        "    tr = None\n"
        "    try: tr = channels.getTargetFxTrack(i)\n"
        "    except Exception: pass\n"
        "    d['target_track'] = tr\n"
        "    d['fx'] = []\n"
        "    if tr is not None and tr >= 0:\n"
        "        for s in range(10):\n"
        "            try:\n"
        "                if plugins.isValid(tr, s):\n"
        "                    d['fx'].append((s, plugins.getPluginName(tr, s)))\n"
        "            except Exception:\n"
        "                pass\n"
        "    out.append(d)\n"
        "out")
r = call("meta.exec", {"code": code})
if r and r.get("ok"):
    for d in (r["result"].get("result") or []):
        log("  [%d] %-14r type=%-4s target=%-4s fx=%s"
            % (d["index"], d["name"], d["type"], d["target_track"], d["fx"]))
else:
    log("  error: %s" % json.dumps(r)[:200])

log("")
log("=== patrones: hay alguno en todo el pool? ===")
r = call("patterns.list")
if r and r.get("ok"):
    log("  %s" % json.dumps(r["result"])[:300])

log("")
log("=== has_GRID: hay notas en los 808? ===")
code2 = ("out = []\n"
         "for i in range(channels.channelCount(True)):\n"
         "    bits = 0\n"
         "    try:\n"
         "        for k in range(16):\n"
         "            if channels.getGridBit(i, k): bits += 1\n"
         "    except Exception: pass\n"
         "    out.append((i, bits))\n"
         "out")
r = call("meta.exec", {"code": code2})
if r and r.get("ok"):
    log("  (canal, pasos activos en el step sequencer): %s" % (r["result"].get("result"),))
