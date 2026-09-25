// Transport handlers — play, stop, tempo, position.

#pragma once

#include "../protocol.h"
#include <string>

namespace flheretic::handlers::transport {

/// `transport.start` — play.
std::string start(const protocol::Request& req);

/// `transport.stop` — stop.
std::string stop(const protocol::Request& req);

/// `transport.status` — playing, recording, tempo, position.
std::string status(const protocol::Request& req);

/// `transport.set_tempo` — bpm.
std::string set_tempo(const protocol::Request& req);

/// `transport.set_position` — bar/bar|ms|ticks.
std::string set_position(const protocol::Request& req);

}  // namespace flheretic::handlers::transport