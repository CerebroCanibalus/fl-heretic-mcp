// Mixer handlers implementation.

#include "mixer.h"
#include "mod.h"

#include <cstdio>
#include <cmath>

namespace flheretic::handlers::mixer {

namespace {
    // Estado simulado
    float g_master_volume = 0.8f;
}

std::string set_volume(const protocol::Request& req) {
    int64_t track = get_param_int(req.params_json, "track", 0);
    double value = get_param_double(req.params_json, "value", 0.8);
    if (value < 0.0 || value > 1.5) {
        return err(req.id, "volume fuera de rango 0..1.5");
    }
    if (track == 0) {
        g_master_volume = static_cast<float>(value);
    }
    char buf[256];
    std::snprintf(buf, sizeof(buf),
        R"({"track":%lld,"vol_norm":%g})",
        static_cast<long long>(track), value);
    return ok(req.id, buf);
}

std::string get_peaks(const protocol::Request& req) {
    // En Fase 4: leer los buffers de audio del master y calcular peaks reales.
    // Por ahora devolvemos ceros (audio no fluye a través de process() en este MVP).
    return ok(req.id, R"({"peak_l":0.0,"peak_r":0.0,"peak_max":0.0})");
}

}  // namespace flheretic::handlers::mixer