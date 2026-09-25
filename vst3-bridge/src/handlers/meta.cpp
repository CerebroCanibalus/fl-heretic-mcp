// Meta handlers implementation.

#include "meta.h"
#include "mod.h"

#include <chrono>
#include <string>

namespace flheretic::handlers::meta {

// Inicio del proceso (para uptime)
static const auto kStartTime = std::chrono::steady_clock::now();

std::string ping(const protocol::Request& req) {
    auto uptime = std::chrono::duration_cast<std::chrono::seconds>(
        std::chrono::steady_clock::now() - kStartTime).count();
    char buf[512];
    std::snprintf(buf, sizeof(buf),
        R"({"ok":true,"bridge_version":"0.3.0","plugin_version":"0.1.0","fl_version":"unknown","uptime_sec":%lld})",
        static_cast<long long>(uptime));
    return ok(req.id, buf);
}

std::string info(const protocol::Request& req) {
    char buf[1024];
    std::snprintf(buf, sizeof(buf),
        R"({"bridge_version":"0.3.0","plugin_version":"0.1.0","fl_version":"unknown","api_modules":["transport","mixer","channels","patterns","playlist","plugins","arrangement","ui","general","device","midi","vst"],"tcp_port":9790,"note":"VST3 plugin — uses FL shell functions for FL API access"})");
    return ok(req.id, buf);
}

}  // namespace flheretic::handlers::meta