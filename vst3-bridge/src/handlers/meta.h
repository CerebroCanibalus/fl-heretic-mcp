// Meta handlers — ping, info, exec, etc.

#pragma once

#include "../protocol.h"
#include <string>

namespace flheretic::handlers::meta {

/// `meta.ping` — health check, devuelve version info.
std::string ping(const protocol::Request& req);

/// `meta.info` — información del plugin y FL.
std::string info(const protocol::Request& req);

}  // namespace flheretic::handlers::meta