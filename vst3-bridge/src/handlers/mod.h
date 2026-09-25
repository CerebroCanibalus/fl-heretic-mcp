// Handlers module — namespace + helpers comunes.

#pragma once

#include "../protocol.h"

#include <string>

namespace flheretic::handlers {

/// Extrae un valor double de los params JSON (string raw).
/// Retorna default si no está presente o no parsea.
double get_param_double(const std::string& params_json,
                        const std::string& key,
                        double default_value);

/// Extrae un valor int64 de los params JSON.
int64_t get_param_int(const std::string& params_json,
                     const std::string& key,
                     int64_t default_value);

/// Extrae un valor string de los params JSON.
std::string get_param_string(const std::string& params_json,
                             const std::string& key,
                             const std::string& default_value);

/// Construye una response de error.
inline std::string err(int64_t id, const std::string& msg) {
    return protocol::build_error_response(id, msg);
}

/// Construye una response OK con un objeto JSON como result.
inline std::string ok(int64_t id, const std::string& result_json) {
    return protocol::build_ok_response(id, result_json);
}

}  // namespace flheretic::handlers