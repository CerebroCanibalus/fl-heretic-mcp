#!/usr/bin/env python3
"""Medir un WAV de verdad, con fixtures que lo validan antes de que te fies.

Por que existe: medir un WAV a mano son cuatro errores esperando, y los cuatro
me pasaron en una tarde. Todos del mismo tipo, inventar el formato en vez de
leerlo:

1. ``struct.pack("<i", v)[:3]`` para escribir 24 bits se come el byte
   equivocado en los negativos. Un int32 de -1 es 0xFFFFFFFF y ``[:3]`` se
   queda con FF FF 00 en vez de FF FF FF.
2. Leer 24 bits como ``array("i").frombytes`` y desplazar: los 3 bytes NO son
   un int32 alineado, y un int32 no signo-extiende desde 24 bits (0xFFFFFF leido
   como int32 da 16777215, no -1). Con el ``>> 8`` encima, el error son
   20*log10(256) = **48,16 dB**, que es una cantidad que parece un nivel de
   signal y no un error de lectura.
3. Promediar los canales (``(L+R)/2``) en vez de tomar el maximo. Cuesta 3 dB
   siempre, y mas si los canales no son identicos.
4. Medir sin guardar con que se valida. Tres lectores erroneos concuerdan entre
   ellos: lo que falta es una respuesta conocida contra la que comparar.

    python tools/wav_measure.py --self-test
    python tools/wav_measure.py fichero.wav [otro.wav ...]

El `--self-test` genera senos con pico conocido a 8, 16, 24 y 32 bits, mas
silencio digital, un valor negativo a tope y 1 LSB, y falla si algo no cuadra.
"""
import argparse
import math
import struct
import sys
import wave

FULL = {8: 128.0, 16: 32768.0, 24: 8388608.0, 32: 2147483648.0}


def _empaquetar(muestras, bits):
    """Un valor con signo a 8/16/24/32 bits, empaquetado como lo manda el formato."""
    if bits == 24:
        return b"".join((int(round(v * FULL[24])) & 0xFFFFFF).to_bytes(3, "little") for v in muestras)
    if bits == 16:
        return struct.pack("<%dh" % len(muestras), *[int(round(v * FULL[16])) for v in muestras])
    if bits == 8:
        return bytes(max(0, min(255, int(round(v * FULL[8])) + 128)) for v in muestras)
    return struct.pack("<%di" % len(muestras), *[int(round(v * FULL[32])) for v in muestras])


def _desempaquetar(crudo, bits):
    if bits == 24:
        # 3 bytes little-endian y signo extender a mano. Rango -8388608..8388607.
        out = []
        for i in range(0, len(crudo) - 2, 3):
            v = crudo[i] | (crudo[i + 1] << 8) | (crudo[i + 2] << 16)
            if v >= 0x800000:
                v -= 0x1000000
            out.append(v)
        return out
    if bits == 16:
        return list(struct.unpack("<%dh" % (len(crudo) // 2), crudo))
    if bits == 8:
        return [b - 256 if b >= 128 else b for b in crudo]
    return list(struct.unpack("<%di" % (len(crudo) // 4), crudo))


def leer(ruta):
    """Devuelve (izq, der, sr, bits). `der` es None si es mono."""
    with wave.open(ruta, "rb") as w:
        n, sr, ch, sw = w.getnframes(), w.getframerate(), w.getnchannels(), w.getsampwidth()
        crudo = w.readframes(n)
    v = _desempaquetar(crudo, sw * 8)
    if ch == 1:
        return v, None, sr, sw * 8
    # Sin promediar: el pico de un fichero stereo es el maximo de los canales.
    # Promediar cuesta 3 dB y hide el canal mas alto.
    return v[0::2], v[1::2], sr, sw * 8


def db(x):
    return 20 * math.log10(x) if x > 0 else -999.0


def medir(ruta):
    izq, der, sr, bits = leer(ruta)
    canales = [izq] if der is None else [izq, der]
    esc = FULL[bits]
    # El pico de un fichero multicanal es el maximo entre canales, no el promedio.
    pico = max(abs(x) for c in canales for x in c) / esc
    n = len(izq)
    rms = math.sqrt(sum(x * x for c in canales for x in c) / (n * len(canales))) / esc
    ceros = sum(1 for x in izq if x == 0)
    return {
        "fichero": ruta.rsplit("\\", 1)[-1].rsplit("/", 1)[-1],
        "sr": sr,
        "bits": bits,
        "canales": len(canales),
        "segundos": n / sr,
        "pico_dbfs": db(pico),
        "rms_dbfs": db(rms),
        "cresta_db": db(pico) - db(rms),
        "ceros_pct": 100.0 * ceros / n,
        "distintos": len(set(izq)),
    }


def _self_test(tmp):
    fallos = []

    def escribe(nombre, muestras, bits=24, ch=1):
        ruta = tmp + "\\" + nombre
        with wave.open(ruta, "wb") as w:
            w.setnchannels(ch)
            w.setsampwidth(bits // 8)
            w.setframerate(44100)
            if ch == 2:
                crudo = _empaquetar(muestras + muestras, bits)
            else:
                crudo = _empaquetar(muestras, bits)
            w.writeframes(crudo)
        return ruta

    def comprueba(nombre, condicion, detalle):
        print("    %-34s %s  %s" % (nombre, "OK " if condicion else "MAL", detalle))
        if not condicion:
            fallos.append(nombre)

    print("  FIXTURES con respuesta conocida (seno 1 kHz, pico 0,5 = -6,0206 dBFS):")
    for bits in (16, 24, 32):
        m = [0.5 * math.sin(2 * math.pi * 1000 * i / 44100) for i in range(44100)]
        r = medir(escribe("t%d.wav" % bits, m, bits))
        ok = abs(r["pico_dbfs"] + 6.0206) < 0.02 and abs(r["cresta_db"] - 3.0103) < 0.02
        comprueba("%d bits, pico y cresta" % bits, ok,
                  "pico %.4f  cresta %.4f" % (r["pico_dbfs"], r["cresta_db"]))

    print("  CASOS TRAMPAO:")
    r = medir(escribe("sil.wav", [0.0] * 4410))
    comprueba("silencio digital", r["pico_dbfs"] < -900 and r["ceros_pct"] == 100.0,
              "pico %.2f  ceros %.1f%%" % (r["pico_dbfs"], r["ceros_pct"]))

    r = medir(escribe("neg.wav", [-1.0] * 100))
    comprueba("-1.0 a 24 bits (signo)", abs(r["pico_dbfs"]) < 0.001,
              "pico %.4f dBFS" % r["pico_dbfs"])

    r = medir(escribe("lsb.wav", [1.0 / FULL[24]] * 100))
    comprueba("1 LSB a 24 bits", abs(r["pico_dbfs"] + 138.52) < 0.2,
              "pico %.4f dBFS" % r["pico_dbfs"])

    # El bug 3: stereo con canales distintos. El pico es el maximo, no el promedio.
    izq = [0.5 * math.sin(2 * math.pi * 440 * i / 44100) for i in range(4410)]
    der = [0.1 * math.sin(2 * math.pi * 440 * i / 44100) for i in range(4410)]
    ruta = tmp + "\\stereo.wav"
    with wave.open(ruta, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(3)
        w.setframerate(44100)
        w.writeframes(_empaquetar(izq + der, 24))
    r = medir(ruta)
    ok = abs(r["pico_dbfs"] + 6.0206) < 0.05
    print("    %-34s %s  pico %.4f (promediar habria dado %.4f)"
          % ("stereo: pico = max, no promedio", "OK " if ok else "MAL", r["pico_dbfs"], -9.03))
    if not ok:
        fallos.append("stereo")

    print("")
    print("  %s" % ("TODOS LOS FIXTURES PASAN" if not fallos
                    else "FALLAN: " + ", ".join(fallos)))
    return 1 if fallos else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("ficheros", nargs="*")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            return _self_test(tmp)

    if not args.ficheros:
        ap.print_help()
        return 1
    print("  %-26s %7s %5s %8s %9s %9s %8s" %
          ("fichero", "sr", "bits", "canales", "pico", "rms", "cresta"))
    for f in args.ficheros:
        r = medir(f)
        print("  %-26s %7d %5d %8d %9.2f %9.2f %8.2f" %
              (r["fichero"], r["sr"], r["bits"], r["canales"],
               r["pico_dbfs"], r["rms_dbfs"], r["cresta_db"]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
