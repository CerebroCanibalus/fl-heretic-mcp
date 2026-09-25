// Mixer handlers — set/get track volume, pan, sends.

#pragma once

#include "../protocol.h"
#include <string>

namespace flheretic::handlers::mixer {

/// `mixer.set_volume` — track, value (0..1).
std::string set_volume(const protocol::Request& req);

/// `mixer.get_peaks` — left, right peaks del master.
std::string get_peaks(const protocol::Request& req);

}  // namespace flheretic::handlers::mixer