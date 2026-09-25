// JSON-RPC protocol (compatible con fLMCP Bridge).
//
// Wire format:
//   [4 bytes BE u32 length][body = utf-8 JSON]
//
// Request:  {"id": int, "action": str, "params": {...}}
// Response: {"id": int, "ok": bool, "result": ..., "error": str|None}

#pragma once

#include <cstdint>
#include <string>

namespace flheretic::protocol {

constexpr uint32_t kHeaderSize = 4;
constexpr uint32_t kMaxFrame  = 16 * 1024 * 1024;  // 16MB, mirror fLMCP Bridge

/// Codifica un objeto JSON a un frame: [BE u32 len][body bytes].
/// Devuelve true si el frame se construyó correctamente.
bool encode_frame(const std::string& json_body, std::string& out_frame);

/// Decodifica un frame: lee [BE u32 len], luego [len bytes].
/// Devuelve true si el frame se leyó correctamente.
/// `in_data` se consume a medida que se lee.
bool decode_frame(const char*& in_data, size_t& in_remaining, std::string& out_body);

/// Parser JSON-RPC simplificado — solo extrae id, action, params del request.
/// Usa nlohmann/json o implementación mínima (regex) según disponibilidad.
struct Request {
    int64_t id = 0;
    std::string action;
    std::string params_json;  // raw JSON para pasarlo al handler
};

/// Construye un Response JSON con ok=true y result.
std::string build_ok_response(int64_t id, const std::string& result_json);

/// Construye un Response JSON con ok=false y error.
std::string build_error_response(int64_t id, const std::string& error_msg);

/// Parsea el request de un body JSON. Retorna true si es válido.
/// Implementación simple: busca "id":N, "action":"X", "params":{...}
bool parse_request(const std::string& body, Request& out);

}  // namespace flheretic::protocol