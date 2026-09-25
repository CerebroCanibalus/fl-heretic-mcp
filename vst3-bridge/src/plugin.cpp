// Plugin class implementation.

#include "plugin.h"
#include "tcp_server.h"
#include "protocol.h"

#include "public.sdk/source/vst/hosting/hostclasses.h"
#include "pluginterfaces/base/ftypes.h"

#include <chrono>
#include <thread>

#define FLHERETIC_LOG(fmt, ...) std::fprintf(stderr, "[FL Heretic] " fmt "\n", ##__VA_ARGS__)

using namespace flheretic;
using namespace Steinberg;
using namespace Steinberg::Vst;

// ============================================================================
// Plugin lifecycle
// ============================================================================

Plugin::Plugin() {
    // Registrar el socket TCP para el estado save/load
    setHostContext(nullptr);  // FL provee el context en initialize()
}

Plugin::~Plugin() {
    // El destructor de tcp_server_ (unique_ptr) para el thread automáticamente
}

tresult PLUGIN_API Plugin::initialize(FUnknown* context) {
    tresult result = AudioEffect::initialize(context);
    if (result != kResultOk) {
        return result;
    }

    // Añadir un input bus (stereo) y un output bus (stereo).
    // El plugin es un "Fx" (audio effect) — pass-through.
    addAudioInput(STR16("Stereo In"), SpeakerArr::kStereo);
    addAudioOutput(STR16("Stereo Out"), SpeakerArr::kStereo);

    FLHERETIC_LOG("Plugin inicializado (v%s, protocol v%s)", kPluginVersion, kProtocolVersion);
    return kResultOk;
}

tresult PLUGIN_API Plugin::terminate() {
    // El destructor se encarga del resto
    FLHERETIC_LOG("Plugin terminado");
    return AudioEffect::terminate();
}

// ============================================================================
// Active state — arranca/para el TCP server
// ============================================================================

tresult PLUGIN_API Plugin::setActive(TBool state) {
    tresult result = AudioEffect::setActive(state);
    if (result != kResultOk) {
        return result;
    }

    if (state) {
        // Arrancar TCP server en thread dedicado
        if (!tcp_server_) {
            tcp_server_ = std::make_unique<TcpServerHolder>();
            if (!start_tcp_server(*tcp_server_, tcp_port_.load())) {
                FLHERETIC_LOG("ERROR: no se pudo arrancar TCP server en puerto %u", tcp_port_.load());
                tcp_server_.reset();
                return kResultFalse;
            }
            FLHERETIC_LOG("TCP server arrancado en 127.0.0.1:%u (esperando daemon)", tcp_port_.load());
        }
    } else {
        // Parar TCP server
        if (tcp_server_) {
            stop_tcp_server(*tcp_server_);
            tcp_server_.reset();
            FLHERETIC_LOG("TCP server detenido");
        }
    }
    return kResultOk;
}

// ============================================================================
// Audio processing — pass-through + heartbeat tick
// ============================================================================

tresult PLUGIN_API Plugin::process(ProcessData& data) {
    // Pass-through de audio (copiar input → output)
    if (data.numInputs == 0 || data.numOutputs == 0) {
        return kResultOk;
    }

    if (data.inputs[0].numChannels >= 2 && data.outputs[0].numChannels >= 2) {
        // Stereo pass-through
        int32 num_samples = data.inputs[0].numSamples;
        if (num_samples > data.outputs[0].numSamples) {
            num_samples = data.outputs[0].numSamples;
        }
        for (int ch = 0; ch < 2; ++ch) {
            if (data.inputs[0].channelBuffers32[ch] && data.outputs[0].channelBuffers32[ch]) {
                std::memcpy(
                    data.outputs[0].channelBuffers32[ch],
                    data.inputs[0].channelBuffers32[ch],
                    num_samples * sizeof(float)
                );
            }
        }
    }

    // TODO: tick del TCP server dispatcher (cada N samples, procesar requests pendientes)
    // Esto lo hace el thread del TCP server independientemente.

    return kResultOk;
}

// ============================================================================
// State save/load — guardamos solo el puerto TCP
// ============================================================================

tresult PLUGIN_API Plugin::getState(IBStream* state) {
    // Guardar: [u16 port]
    uint16 port = tcp_port_.load();
    state->write(&port, sizeof(uint16));
    return kResultOk;
}

tresult PLUGIN_API Plugin::setState(IBStream* state) {
    // Cargar: [u16 port]
    uint16 port = 0;
    if (state->read(&port, sizeof(uint16)) == kResultOk) {
        if (port > 0 && port < 65535) {
            tcp_port_.store(port);
        }
    }
    return kResultOk;
}