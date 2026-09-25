// JSON-RPC protocol implementation.

#include "protocol.h"

#include <cstring>
#include <sstream>
#include <string>

namespace flheretic::protocol {

// ============================================================================
// Frame encoding/decoding
// ============================================================================

bool encode_frame(const std::string& json_body, std::string& out_frame) {
    if (json_body.size() > kMaxFrame) {
        return false;
    }
    uint32_t len = static_cast<uint32_t>(json_body.size());
    out_frame.clear();
    out_frame.resize(kHeaderSize + len);
    // BE u32 length
    out_frame[0] = static_cast<char>((len >> 24) & 0xFF);
    out_frame[1] = static_cast<char>((len >> 16) & 0xFF);
    out_frame[2] = static_cast<char>((len >>  8) & 0xFF);
    out_frame[3] = static_cast<char>( len        & 0xFF);
    std::memcpy(&out_frame[kHeaderSize], json_body.data(), len);
    return true;
}

bool decode_frame(const char*& in_data, size_t& in_remaining, std::string& out_body) {
    if (in_remaining < kHeaderSize) {
        return false;
    }
    uint32_t len = 0;
    len  = static_cast<uint32_t>(static_cast<uint8_t>(in_data[0])) << 24;
    len |= static_cast<uint32_t>(static_cast<uint8_t>(in_data[1])) << 16;
    len |= static_cast<uint32_t>(static_cast<uint8_t>(in_data[2])) <<  8;
    len |= static_cast<uint32_t>(static_cast<uint8_t>(in_data[3]));
    if (len > kMaxFrame || len > in_remaining - kHeaderSize) {
        return false;
    }
    out_body.assign(in_data + kHeaderSize, len);
    in_data += kHeaderSize + len;
    in_remaining -= kHeaderSize + len;
    return true;
}

// ============================================================================
// Response builders
// ============================================================================

std::string build_ok_response(int64_t id, const std::string& result_json) {
    std::ostringstream os;
    os << "{\"id\":" << id
       << ",\"ok\":true,\"result\":"
       << (result_json.empty() ? "null" : result_json)
       << "}";
    return os.str();
}

std::string build_error_response(int64_t id, const std::string& error_msg) {
    // Escapar comillas y backslash en el error
    std::string escaped;
    escaped.reserve(error_msg.size() + 2);
    for (char c : error_msg) {
        if (c == '"' || c == '\\') {
            escaped.push_back('\\');
        }
        if (c == '\n') {
            escaped += "\\n";
        } else if (c == '\r') {
            escaped += "\\r";
        } else {
            escaped.push_back(c);
        }
    }
    std::ostringstream os;
    os << "{\"id\":" << id
       << ",\"ok\":false,\"error\":\"" << escaped << "\"}";
    return os.str();
}

// ============================================================================
// Parser simple de JSON (sin librería externa)
// ============================================================================
//
// Implementación minimalista: busca "id":N, "action":"X", "params":{...}.
// NO es un parser JSON completo — solo lo necesario para nuestro protocolo.

namespace {

// Salta whitespace
const char* skip_ws(const char* p, const char* end) {
    while (p < end && (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r')) ++p;
    return p;
}

// Busca la clave "key": y retorna el puntero al inicio del valor
const char* find_key(const char* p, const char* end, const char* key) {
    size_t klen = std::strlen(key);
    while (p + klen + 2 < end) {
        // Buscar "key"
        if (*p == '"' && std::memcmp(p + 1, key, klen) == 0 && p[1 + klen] == '"') {
            p += 2 + klen;  // saltar "key"
            p = skip_ws(p, end);
            if (p < end && *p == ':') {
                ++p;
                return skip_ws(p, end);
            }
        }
        ++p;
    }
    return nullptr;
}

// Lee un número entero (signed long long)
bool parse_int(const char* p, const char* end, int64_t& out) {
    p = skip_ws(p, end);
    bool neg = false;
    if (p < end && (*p == '-' || *p == '+')) {
        neg = (*p == '-');
        ++p;
    }
    if (p >= end || !(*p >= '0' && *p <= '9')) return false;
    int64_t val = 0;
    while (p < end && *p >= '0' && *p <= '9') {
        val = val * 10 + (*p - '0');
        ++p;
    }
    out = neg ? -val : val;
    return true;
}

// Lee un string JSON (entre comillas, con escapes)
bool parse_string(const char* p, const char* end, std::string& out) {
    p = skip_ws(p, end);
    if (p >= end || *p != '"') return false;
    ++p;
    out.clear();
    while (p < end && *p != '"') {
        if (*p == '\\' && p + 1 < end) {
            char next = *(p + 1);
            switch (next) {
                case '"': out.push_back('"'); break;
                case '\\': out.push_back('\\'); break;
                case '/': out.push_back('/'); break;
                case 'n': out.push_back('\n'); break;
                case 'r': out.push_back('\r'); break;
                case 't': out.push_back('\t'); break;
                default: out.push_back(next); break;
            }
            p += 2;
        } else {
            out.push_back(*p);
            ++p;
        }
    }
    if (p >= end) return false;
    ++p;  // saltar " de cierre
    return true;
}

// Lee un objeto JSON completo (captura el bloque {...} completo)
bool parse_object_raw(const char* p, const char* end, const char*& obj_start, const char*& obj_end) {
    p = skip_ws(p, end);
    if (p >= end || *p != '{') return false;
    obj_start = p;
    int depth = 0;
    while (p < end) {
        if (*p == '{') ++depth;
        else if (*p == '}') {
            --depth;
            if (depth == 0) {
                obj_end = p + 1;
                return true;
            }
        } else if (*p == '"') {
            // Saltar string
            ++p;
            while (p < end && *p != '"') {
                if (*p == '\\' && p + 1 < end) p += 2;
                else ++p;
            }
            if (p < end) ++p;
            continue;
        }
        ++p;
    }
    return false;
}

}  // namespace

bool parse_request(const std::string& body, Request& out) {
    if (body.empty()) return false;
    const char* p = body.data();
    const char* end = p + body.size();

    // Buscar "id"
    const char* id_p = find_key(p, end, "id");
    if (!id_p) return false;
    if (!parse_int(id_p, end, out.id)) return false;

    // Buscar "action"
    const char* action_p = find_key(p, end, "action");
    if (!action_p) return false;
    if (!parse_string(action_p, end, out.action)) return false;

    // Buscar "params" (objeto completo)
    const char* params_p = find_key(p, end, "params");
    if (!params_p) return false;
    const char* obj_start = nullptr, * obj_end = nullptr;
    if (params_p < end && *params_p == '{') {
        if (!parse_object_raw(params_p, end, obj_start, obj_end)) return false;
        out.params_json.assign(obj_start, obj_end - obj_start);
    } else if (params_p < end && *params_p == 'n' && std::strncmp(params_p, "null", 4) == 0) {
        out.params_json = "null";
    } else {
        // params es un valor escalar o array; capturar hasta el final del campo
        const char* e = params_p;
        int depth = 0;
        while (e < end) {
            char c = *e;
            if (c == '{' || c == '[') ++depth;
            else if (c == '}' || c == ']') {
                if (depth == 0) break;
                --depth;
            } else if (c == ',') {
                if (depth == 0) break;
            }
            ++e;
        }
        out.params_json.assign(params_p, e - params_p);
    }

    return true;
}

}  // namespace flheretic::protocol