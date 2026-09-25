// TCP server thread — acepta conexiones en 127.0.0.1:<port> y procesa JSON-RPC.

#include "tcp_server.h"
#include "protocol.h"
#include "handlers/transport.h"
#include "handlers/meta.h"
#include "handlers/mixer.h"

#include <atomic>
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
// Estado global del server (sockets activos + dispatch)
// ============================================================================

namespace {

struct ServerState {
    SOCKET listen_sock = INVALID_SOCKET;
    std::mutex clients_mutex;
    std::set<SOCKET> clients;
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

// Lee exactamente N bytes (bloqueante).
// Retorna true si leyó todos los bytes, false si EOF/error.
bool read_exact(SOCKET sock, char* buf, size_t n) {
    size_t total = 0;
    while (total < n) {
        int r = ::recv(sock, buf + total, static_cast<int>(n - total), 0);
        if (r <= 0) return false;
        total += static_cast<size_t>(r);
    }
    return true;
}

// Escribe exactamente N bytes (bloqueante).
bool write_exact(SOCKET sock, const char* buf, size_t n) {
    size_t total = 0;
    while (total < n) {
        int r = ::send(sock, buf + total, static_cast<int>(n - total), 0);
        if (r <= 0) return false;
        total += static_cast<size_t>(r);
    }
    return true;
}

// Dispatch del request al handler correcto y devuelve el JSON del resultado.
std::string dispatch_request(const protocol::Request& req) {
    using namespace flheretic::handlers;
    if (req.action == "meta.ping") {
        return meta::ping(req);
    } else if (req.action == "meta.info") {
        return meta::info(req);
    } else if (req.action == "transport.start") {
        return transport::start(req);
    } else if (req.action == "transport.stop") {
        return transport::stop(req);
    } else if (req.action == "transport.status") {
        return transport::status(req);
    } else if (req.action == "transport.set_tempo") {
        return transport::set_tempo(req);
    } else if (req.action == "transport.set_position") {
        return transport::set_position(req);
    } else if (req.action == "mixer.set_volume") {
        return mixer::set_volume(req);
    } else if (req.action == "mixer.get_peaks") {
        return mixer::get_peaks(req);
    }
    return std::string("{\"error\":\"unknown action: ") + req.action + "\"}";
}

// Maneja una conexión cliente.
void handle_client(SOCKET sock) {
    std::fprintf(stderr, "[FL Heretic] cliente conectado (sock=%d)\n", static_cast<int>(sock));

    // Registrar el socket
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
        std::string result_json;
        if (!protocol::parse_request(body_str, req)) {
            result_json = protocol::build_error_response(0, "invalid request JSON");
        } else {
            std::fprintf(stderr, "[FL Heretic] dispatch: id=%lld action=%s\n",
                          static_cast<long long>(req.id), req.action.c_str());
            try {
                result_json = dispatch_request(req);
            } catch (const std::exception& e) {
                result_json = protocol::build_error_response(req.id, e.what());
            } catch (...) {
                result_json = protocol::build_error_response(req.id, "unknown exception");
            }
        }

        // Enviar response (siempre)
        std::string response;
        if (req.id == 0 && result_json.find("\"error\"") != std::string::npos) {
            response = protocol::build_error_response(0, result_json);
        } else if (req.id == 0) {
            response = protocol::build_ok_response(0, result_json);
        } else if (result_json.find("\"error\"") != std::string::npos) {
            // El handler devolvió JSON con error
            response = protocol::build_error_response(req.id,
                result_json.substr(result_json.find("\"error\":") + 9));
        } else {
            response = protocol::build_ok_response(req.id, result_json);
        }
        std::string frame;
        protocol::encode_frame(response, frame);
        if (!write_exact(sock, frame.data(), frame.size())) break;
    }

    // Cleanup
    {
        std::lock_guard<std::mutex> lock(g_state.clients_mutex);
        g_state.clients.erase(sock);
    }
    closesocket(sock);
    std::fprintf(stderr, "[FL Heretic] cliente desconectado (sock=%d)\n", static_cast<int>(sock));
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
    std::fprintf(stderr, "[FL Heretic] TCP server escuchando en 127.0.0.1:%u\n", port);

    // Set non-blocking para que accept() no bloquee indefinidamente
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
            // No hay conexiones pendientes — verificar running y reintentar
#ifdef _WIN32
            Sleep(50);
#else
            usleep(50 * 1000);
#endif
            continue;
        }

        // Set blocking para I/O del cliente (necesario para read_exact bloqueante)
#ifdef _WIN32
        u_long mode_blocking = 0;
        ioctlsocket(client_sock, FIONBIO, &mode_blocking);
#else
        int flags = fcntl(client_sock, F_GETFL, 0);
        fcntl(client_sock, F_SETFL, flags & ~O_NONBLOCK);
#endif

        char ip_str[INET_ADDRSTRLEN] = {0};
        inet_ntop(AF_INET, &client_addr.sin_addr, ip_str, sizeof(ip_str));
        std::fprintf(stderr, "[FL Heretic] conexión aceptada desde %s:%u\n",
                      ip_str, ntohs(client_addr.sin_port));

        // Manejar cliente en el mismo thread (serializado, simple).
        // Si necesitamos concurrencia, spawneamos thread por cliente.
        handle_client(client_sock);
    }

    // Cerrar conexiones restantes
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

// ============================================================================
// API pública
// ============================================================================

bool start_tcp_server(TcpServerHolder& holder, uint16_t port) {
    holder.port = port;
    holder.running.store(true);
    try {
        holder.thread = std::thread(server_thread_main, port, std::ref(holder.running));
    } catch (...) {
        holder.running.store(false);
        return false;
    }
    // Detach para que el thread se limpie solo al terminar
    holder.thread.detach();
    return true;
}

void stop_tcp_server(TcpServerHolder& holder) {
    holder.running.store(false);
    // Cerrar el listen socket para desbloquear accept()
    if (g_state.listen_sock != INVALID_SOCKET) {
        shutdown(g_state.listen_sock, 2 /* SHUT_RDWR */);
    }
    // El thread sale solo cuando running es false. Como está detached,
    // no podemos join(), pero el destructor de TcpServerHolder no se llamará
    // hasta que el thread salga (en realidad nunca, pero está OK para un plugin).
    // Para esperar limpiamente, podríamos usar std::jthread o un future.
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
}

}  // namespace flheretic