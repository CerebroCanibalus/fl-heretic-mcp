// TCP server + file-RPC proxy — el VST3 plugin actúa como router entre
// el daemon Rust (TCP en 127.0.0.1:9790) y el fLMCP Bridge (file-RPC en
// %USERPROFILE%\Documents\...\fLMCP Bridge\rpc_*.json).
//
// Cada cliente TCP que se conecta:
// 1. Lee un frame [BE u32 len][body JSON]
// 2. Parsea el request {id, action, params}
// 3. Lo re-empaqueta como file-RPC al Bridge
// 4. Espera la respuesta (con timeout)
// 5. Devuelve la respuesta al cliente TCP como frame JSON
//
// El plugin es esencialmente un PROXY stateless.

#include "tcp_server.h"
#include "protocol.h"
#include "file_rpc_client.h"
#include "handlers/meta.h"
#include "handlers/transport.h"
#include "handlers/mixer.h"

#include <atomic>
#include <chrono>
#include <cstring>
#include <cstdio>
#include <mutex>
#include <set>
#include <string>
#include <thread>
#include <vector>

#ifdef _WIN32
    #define WIN32_LEAN_AND_MEAN
    #include <winsock2.h>
    #include <ws2tcpip.h>
    typedef int socklen_t;
    #pragma comment(lib, "Ws2_32.lib")
#else
    #include <sys/socket.h>
    #include <netinet/in.h>
    #include <arpa/inet.h>
    #include <unistd.h>
    #include <fcntl.h>
    #include <errno.h>
    typedef int SOCKET;
    #define INVALID_SOCKET (-1)
    #define SOCKET_ERROR (-1)
    #define closesocket close
#endif

namespace flheretic {

// ============================================================================
// Estado global del server
// ============================================================================

namespace {

struct ServerState {
    SOCKET listen_sock = INVALID_SOCKET;
    std::mutex clients_mutex;
    std::set<SOCKET> clients;
    /// Cliente file-RPC compartido entre todos los handlers.
    /// Thread-safe (mutex interno).
    FileRpcClient rpc;
    /// Próximo id (atómico para evitar colisiones).
    std::atomic<uint64_t> next_id{1};
};

ServerState g_state;

bool init_sockets_once() {
#ifdef _WIN32
    static std::once_flag flag;
    static bool ok = false;
    std::call_once(flag, []() {
        WSADATA wsa;
        ok = (WSAStartup(MAKEWORD(2, 2), &wsa) == 0);
    });
    return ok;
#else
    return true;
#endif
}

bool read_exact(SOCKET sock, char* buf, size_t n) {
    size_t total = 0;
    while (total < n) {
        int r = ::recv(sock, buf + total, static_cast<int>(n - total), 0);
        if (r <= 0) return false;
        total += static_cast<size_t>(r);
    }
    return true;
}

bool write_exact(SOCKET sock, const char* buf, size_t n) {
    size_t total = 0;
    while (total < n) {
        int r = ::send(sock, buf + total, static_cast<int>(n - total), 0);
        if (r <= 0) return false;
        total += static_cast<size_t>(r);
    }
    return true;
}

/// Maneja una conexión cliente. Cada cliente = una request (stateless).
void handle_client(SOCKET sock) {
    std::fprintf(stderr, "[FL Heretic] cliente TCP conectado (sock=%d)\n", static_cast<int>(sock));

    {
        std::lock_guard<std::mutex> lock(g_state.clients_mutex);
        g_state.clients.insert(sock);
    }

    char header_buf[4];
    while (read_exact(sock, header_buf, 4)) {
        uint32_t len = 0;
        len  = (static_cast<uint32_t>(static_cast<uint8_t>(header_buf[0]))) << 24;
        len |= (static_cast<uint32_t>(static_cast<uint8_t>(header_buf[1]))) << 16;
        len |= (static_cast<uint32_t>(static_cast<uint8_t>(header_buf[2]))) <<  8;
        len |=  static_cast<uint32_t>(static_cast<uint8_t>(header_buf[3]));
        if (len == 0 || len > protocol::kMaxFrame) break;

        std::vector<char> body(static_cast<size_t>(len));
        if (!read_exact(sock, body.data(), len)) break;
        std::string body_str(body.data(), len);

        // Parsear request
        protocol::Request req;
        std::string response;
        if (!protocol::parse_request(body_str, req)) {
            response = protocol::build_error_response(0, "invalid request JSON");
        } else {
            std::fprintf(stderr, "[FL Heretic] proxy: id=%lld action=%s\n",
                          static_cast<long long>(req.id), req.action.c_str());
            // Delegar TODO al file-RPC (FL API access viene del Bridge).
            // El plugin VST3 es solo un proxy stateless.
            std::string rpc_response;
            bool ok = g_state.rpc.call(req.action, req.params_json, req.id, rpc_response, 5000);
            if (ok) {
                response = std::move(rpc_response);
            } else {
                response = protocol::build_error_response(req.id,
                    "file-RPC timeout — ¿FL Heretic Bridge cargado en FL Studio?");
            }
        }

        // Enviar response (siempre)
        std::string frame;
        protocol::encode_frame(response, frame);
        if (!write_exact(sock, frame.data(), frame.size())) break;
    }

    {
        std::lock_guard<std::mutex> lock(g_state.clients_mutex);
        g_state.clients.erase(sock);
    }
    closesocket(sock);
    std::fprintf(stderr, "[FL Heretic] cliente TCP desconectado (sock=%d)\n",
                  static_cast<int>(sock));
}

void server_thread_main(uint16_t port, std::atomic<bool>& running) {
    if (!init_sockets_once()) {
        std::fprintf(stderr, "[FL Heretic] ERROR: WSAStartup falló\n");
        return;
    }

    SOCKET listen_sock = ::socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (listen_sock == INVALID_SOCKET) {
        std::fprintf(stderr, "[FL Heretic] ERROR: socket() falló\n");
        return;
    }

    int opt = 1;
    ::setsockopt(listen_sock, SOL_SOCKET, SO_REUSEADDR,
                 reinterpret_cast<const char*>(&opt), sizeof(opt));

    sockaddr_in addr{};
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);  // 127.0.0.1
    addr.sin_port = htons(port);

    if (::bind(listen_sock, reinterpret_cast<sockaddr*>(&addr), sizeof(addr)) == SOCKET_ERROR) {
        std::fprintf(stderr, "[FL Heretic] ERROR: bind 127.0.0.1:%u falló\n", port);
        closesocket(listen_sock);
        return;
    }
    if (::listen(listen_sock, 4) == SOCKET_ERROR) {
        std::fprintf(stderr, "[FL Heretic] ERROR: listen() falló\n");
        closesocket(listen_sock);
        return;
    }
    g_state.listen_sock = listen_sock;
    std::fprintf(stderr, "[FL Heretic] TCP server (proxy) escuchando en 127.0.0.1:%u\n", port);
    std::fprintf(stderr, "[FL Heretic] delegando a fLMCP Bridge via file-RPC en %s\n",
                  g_state.rpc.script_dir().c_str());

#ifdef _WIN32
    u_long mode = 1;
    ioctlsocket(listen_sock, FIONBIO, &mode);
#else
    int flags = fcntl(listen_sock, F_GETFL, 0);
    fcntl(listen_sock, F_SETFL, flags | O_NONBLOCK);
#endif

    while (running.load()) {
        sockaddr_in client_addr{};
        socklen_t addr_len = sizeof(client_addr);
        SOCKET client_sock = ::accept(listen_sock,
                                      reinterpret_cast<sockaddr*>(&client_addr),
                                      &addr_len);
        if (client_sock == INVALID_SOCKET) {
#ifdef _WIN32
            Sleep(50);
#else
            usleep(50 * 1000);
#endif
            continue;
        }

#ifdef _WIN32
        u_long mode_blocking = 0;
        ioctlsocket(client_sock, FIONBIO, &mode_blocking);
#else
        int flags = fcntl(client_sock, F_GETFL, 0);
        fcntl(client_sock, F_SETFL, flags & ~O_NONBLOCK);
#endif

        char ip_str[INET_ADDRSTRLEN] = {0};
        inet_ntop(AF_INET, &client_addr.sin_addr, ip_str, sizeof(ip_str));
        std::fprintf(stderr, "[FL Heretic] TCP conexión desde %s:%u\n",
                      ip_str, ntohs(client_addr.sin_port));

        handle_client(client_sock);
    }

    {
        std::lock_guard<std::mutex> lock(g_state.clients_mutex);
        for (SOCKET c : g_state.clients) closesocket(c);
        g_state.clients.clear();
    }
    closesocket(listen_sock);
    g_state.listen_sock = INVALID_SOCKET;
#ifdef _WIN32
    WSACleanup();
#endif
    std::fprintf(stderr, "[FL Heretic] TCP server detenido\n");
}

}  // namespace

bool start_tcp_server(TcpServerHolder& holder, uint16_t port) {
    holder.port = port;
    holder.running.store(true);
    try {
        holder.thread = std::thread(server_thread_main, port, std::ref(holder.running));
    } catch (...) {
        holder.running.store(false);
        return false;
    }
    holder.thread.detach();
    return true;
}

void stop_tcp_server(TcpServerHolder& holder) {
    holder.running.store(false);
    if (g_state.listen_sock != INVALID_SOCKET) {
        shutdown(g_state.listen_sock, 2);
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
}

}  // namespace flheretic