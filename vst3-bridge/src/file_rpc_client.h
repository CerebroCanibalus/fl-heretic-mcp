// file-RPC client — el VST3 plugin usa esto para delegar al fLMCP Bridge.

#pragma once

#include <atomic>
#include <chrono>
#include <cstdint>
#include <mutex>
#include <string>

namespace flheretic {

// Cliente que escribe/lee rpc_*.json (mismo formato que el Bridge).
class FileRpcClient {
public:
    FileRpcClient();
    ~FileRpcClient();

    /// Configura el directorio del script. Default: %USERPROFILE%\Documents\...\fLMCP Bridge
    void set_script_dir(const std::string& dir);
    const std::string& script_dir() const { return script_dir_; }

    /// Llama a una action via file-RPC. Bloqueante con timeout.
    /// Devuelve el body JSON del response (sin el frame).
    /// Retorna false si timeout o error.
    bool call(const std::string& action,
              const std::string& params_json,
              int64_t request_id,
              std::string& response_body,
              int timeout_ms = 5000);

    /// Limpia la respuesta anterior (best-effort).
    void clear_response();

private:
    std::string script_dir_;
    std::mutex mutex_;            // serializa writes
    std::atomic<uint64_t> next_id_{1};
};

}  // namespace flheretic