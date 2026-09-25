// file-RPC client implementation.

#include "file_rpc_client.h"

#include <chrono>
#include <cstdio>
#include <fstream>
#include <sstream>
#include <thread>

#ifdef _WIN32
    #include <shlobj.h>     // SHGetFolderPathW
    #include <windows.h>
#endif

namespace flheretic {

FileRpcClient::FileRpcClient() {
#ifdef _WIN32
    // Default: %USERPROFILE%\Documents\Image-Line\FL Studio\Settings\Hardware\fLMCP Bridge
    wchar_t* userprofile = nullptr;
    if (SUCCEEDED(SHGetKnownFolderPath(FOLDERID_Profile, 0, nullptr, &userprofile))) {
        char buf[MAX_PATH];
        WideCharToMultiByte(CP_UTF8, 0, userprofile, -1, buf, MAX_PATH, nullptr, nullptr);
        script_dir_ = std::string(buf) + "\\Documents\\Image-Line\\FL Studio\\Settings\\Hardware\\fLMCP Bridge";
        CoTaskMemFree(userprofile);
    }
#endif
}

FileRpcClient::~FileRpcClient() {}

void FileRpcClient::set_script_dir(const std::string& dir) {
    std::lock_guard<std::mutex> lock(mutex_);
    script_dir_ = dir;
}

void FileRpcClient::clear_response() {
    std::lock_guard<std::mutex> lock(mutex_);
    std::string resp_path = script_dir_ + "\\rpc_response.json";
    // Empty response file (best-effort)
    std::ofstream f(resp_path, std::ios::binary | std::ios::trunc);
    // Just open + close to truncate
}

bool FileRpcClient::call(const std::string& action,
                        const std::string& params_json,
                        int64_t request_id,
                        std::string& response_body,
                        int timeout_ms) {
    std::lock_guard<std::mutex> lock(mutex_);
    std::string req_path = script_dir_ + "\\rpc_request.json";
    std::string resp_path = script_dir_ + "\\rpc_response.json";

    // 1. Build request
    std::ostringstream os;
    os << "{\"id\":" << request_id
       << ",\"action\":\"" << action << "\""
       << ",\"params\":" << (params_json.empty() ? "{}" : params_json)
       << "}";
    std::string req_body = os.str();

    // 2. Clear response file (best-effort)
    {
        std::ofstream f(resp_path, std::ios::binary | std::ios::trunc);
    }

    // 3. Write request atomically (write to .tmp + rename)
    std::string tmp_path = req_path + ".tmp";
    {
        std::ofstream f(tmp_path, std::ios::binary | std::ios::trunc);
        if (!f) {
            std::fprintf(stderr, "[FL Heretic] file-RPC: no se pudo escribir %s\n", tmp_path.c_str());
            return false;
        }
        f.write(req_body.data(), static_cast<std::streamsize>(req_body.size()));
        f.flush();
    }
#ifdef _WIN32
    // Windows rename replacement (MoveFileEx)
    MoveFileExA(tmp_path.c_str(), req_path.c_str(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH);
#else
    rename(tmp_path.c_str(), req_path.c_str());
#endif

    // 4. Poll response
    auto deadline = std::chrono::steady_clock::now()
                  + std::chrono::milliseconds(timeout_ms);
    std::string id_str = std::to_string(request_id);
    while (std::chrono::steady_clock::now() < deadline) {
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
        std::ifstream f(resp_path, std::ios::binary);
        if (!f) continue;
        std::stringstream ss;
        ss << f.rdbuf();
        std::string text = ss.str();
        if (text.empty()) continue;
        if (text.find("\"" + id_str + "\"") == std::string::npos) {
            continue;  // respuesta de un request previo
        }
        response_body = std::move(text);
        return true;
    }
    std::fprintf(stderr, "[FL Heretic] file-RPC timeout esperando '%s' (id=%lld)\n",
                  action.c_str(), static_cast<long long>(request_id));
    return false;
}

}  // namespace flheretic