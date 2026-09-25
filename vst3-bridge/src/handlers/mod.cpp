// Handlers module — implementación de helpers.

#include "mod.h"

#include <cstdlib>
#include <cstring>
#include <cstdio>

namespace flheretic::handlers {

// ============================================================================
// Parser de params (búsqueda naive de "key":NUMBER|STRING|...)
// ============================================================================

double get_param_double(const std::string& params_json,
                        const std::string& key,
                        double default_value) {
    if (params_json.empty()) return default_value;
    const char* p = params_json.data();
    const char* end = p + params_json.size();
    // Buscar "key"
    std::string quoted = "\"" + key + "\"";
    size_t klen = quoted.size();
    for (const char* q = p; q + klen + 2 < end; ++q) {
        if (std::memcmp(q, quoted.c_str(), klen) == 0 && q[klen] == ':') {
            q += klen + 1;
            while (q < end && (*q == ' ' || *q == '\t')) ++q;
            char* endp = nullptr;
            double v = std::strtod(q, &endp);
            if (endp != q) return v;
        }
    }
    return default_value;
}

int64_t get_param_int(const std::string& params_json,
                     const std::string& key,
                     int64_t default_value) {
    if (params_json.empty()) return default_value;
    const char* p = params_json.data();
    const char* end = p + params_json.size();
    std::string quoted = "\"" + key + "\"";
    size_t klen = quoted.size();
    for (const char* q = p; q + klen + 2 < end; ++q) {
        if (std::memcmp(q, quoted.c_str(), klen) == 0 && q[klen] == ':') {
            q += klen + 1;
            while (q < end && (*q == ' ' || *q == '\t')) ++q;
            char* endp = nullptr;
            long long v = std::strtoll(q, &endp, 10);
            if (endp != q) return static_cast<int64_t>(v);
        }
    }
    return default_value;
}

std::string get_param_string(const std::string& params_json,
                             const std::string& key,
                             const std::string& default_value) {
    if (params_json.empty()) return default_value;
    const char* p = params_json.data();
    const char* end = p + params_json.size();
    std::string quoted = "\"" + key + "\"";
    size_t klen = quoted.size();
    for (const char* q = p; q + klen + 3 < end; ++q) {
        if (std::memcmp(q, quoted.c_str(), klen) == 0 && q[klen] == ':') {
            q += klen + 1;
            while (q < end && (*q == ' ' || *q == '\t')) ++q;
            if (q >= end || *q != '"') return default_value;
            ++q;  // saltar "
            std::string out;
            while (q < end && *q != '"') {
                if (*q == '\\' && q + 1 < end) {
                    out.push_back(*(q + 1));
                    q += 2;
                } else {
                    out.push_back(*q);
                    ++q;
                }
            }
            return out;
        }
    }
    return default_value;
}

}  // namespace flheretic::handlers