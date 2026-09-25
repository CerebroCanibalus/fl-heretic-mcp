// TCP server thread — escucha conexiones del daemon FL Heretic y despacha JSON-RPC.

#pragma once

#include <atomic>
#include <cstdint>
#include <memory>
#include <thread>

namespace flheretic {

// Opaque holder para el server (definido en .cpp).
struct TcpServerHolder {
    std::thread thread;
    std::atomic<bool> running{false};
    uint16_t port = 0;
};

/// Arranca el server en un thread dedicado.
/// Retorna true si el server arrancó correctamente.
bool start_tcp_server(TcpServerHolder& holder, uint16_t port);

/// Para el server (signal running=false y join thread).
void stop_tcp_server(TcpServerHolder& holder);

}  // namespace flheretic