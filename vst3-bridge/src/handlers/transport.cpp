// Transport handlers implementation.
//
// IMPORTANTE: los plugins VST3 NO tienen acceso directo a FL API.
// Para implementar estas funciones reales necesitaríamos:
//   1. Usar shell functions específicas de FL Studio (no oficiales pero conocidas)
//   2. Comunicarse con un controller script Python via file I/O
//   3. Parsear el archivo .flp del proyecto actual
//
// Por ahora devolvemos un placeholder que indica el estado. El handler completo
// se implementará en Fase 3 cuando se decida la estrategia de acceso a FL API.

#include "transport.h"
#include "mod.h"

#include <cstdio>
#include <string>

namespace flheretic::handlers::transport {

// Estado simulado (en Fase 3 se reemplaza por acceso real a FL API)
namespace {
    bool g_is_playing = false;
    double g_tempo = 140.0;
    int64_t g_position_ticks = 0;
}

std::string start(const protocol::Request& req) {
    g_is_playing = true;
    g_position_ticks = 0;
    char buf[256];
    std::snprintf(buf, sizeof(buf), R"({"is_playing":true})");
    return ok(req.id, buf);
}

std::string stop(const protocol::Request& req) {
    g_is_playing = false;
    return ok(req.id, R"({"stopped":true})");
}

std::string status(const protocol::Request& req) {
    char buf[512];
    std::snprintf(buf, sizeof(buf),
        R"({"is_playing":%s,"is_recording":false,"position_ticks":%lld,"position_bars":%lld,"position_seconds":%lld,"loop_mode":"pattern","bpm":%g})",
        g_is_playing ? "true" : "false",
        static_cast<long long>(g_position_ticks),
        static_cast<long long>(g_position_ticks / 96),
        static_cast<long long>(g_position_ticks / 96 / 4),
        g_tempo);
    return ok(req.id, buf);
}

std::string set_tempo(const protocol::Request& req) {
    double bpm = get_param_double(req.params_json, "bpm", 140.0);
    if (bpm < 10.0 || bpm > 999.0) {
        return err(req.id, "bpm fuera de rango 10-999");
    }
    g_tempo = bpm;
    char buf[128];
    std::snprintf(buf, sizeof(buf), R"({"bpm":%g})", g_tempo);
    return ok(req.id, buf);
}

std::string set_position(const protocol::Request& req) {
    // Acepta unit: bars (default), ticks, ms, seconds
    std::string unit = get_param_string(req.params_json, "unit", "bars");
    double position = get_param_double(req.params_json, "position", 0.0);
    // Convertir a ticks (96 ticks per beat, 4 beats per bar)
    int64_t ticks = 0;
    if (unit == "bars") {
        ticks = static_cast<int64_t>(position * 96 * 4);
    } else if (unit == "beats") {
        ticks = static_cast<int64_t>(position * 96);
    } else if (unit == "ticks") {
        ticks = static_cast<int64_t>(position);
    } else if (unit == "seconds") {
        ticks = static_cast<int64_t>(position * (g_tempo / 60.0) * 96 * 4);
    } else if (unit == "ms") {
        ticks = static_cast<int64_t>(position * (g_tempo / 60000.0) * 96 * 4);
    } else {
        return err(req.id, "unit desconocido: " + unit);
    }
    g_position_ticks = ticks;
    return ok(req.id, R"({"ok":true})");
}

}  // namespace flheretic::handlers::transport