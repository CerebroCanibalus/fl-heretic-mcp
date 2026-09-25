// Plugin class header — VST3 FL Heretic Bridge.

#pragma once

#include "public.sdk/source/vst/vstaudioeffect.h"

#include <atomic>
#include <memory>
#include <string>

namespace flheretic {

// Plugin class — extiende AudioEffect para tener audio I/O + TCP server.
class Plugin : public Steinberg::Vst::AudioEffect {
public:
    Plugin();
    ~Plugin() override;

    // IPluginBase
    Steinberg::tresult PLUGIN_API initialize(Steinberg::FUnknown* context) override;
    Steinberg::tresult PLUGIN_API terminate() override;

    // IAudioProcessor
    Steinberg::tresult PLUGIN_API setActive(Steinberg::TBool state) override;
    Steinberg::tresult PLUGIN_API process(Steinberg::Vst::ProcessData& data) override;

    // IComponent (state save/load — guardamos solo el puerto TCP)
    Steinberg::tresult PLUGIN_API getState(Steinberg::IBStream* state) override;
    Steinberg::tresult PLUGIN_API setState(Steinberg::IBStream* state) override;

    // Acceso al config
    Steinberg::uint16 getTcpPort() const { return tcp_port_.load(); }

private:
    // Puerto TCP donde escucha el server (configurable).
    // Default 9790 para evitar conflicto con fLMCP Bridge (9876).
    std::atomic<Steinberg::uint16> tcp_port_{9790};

    // El server TCP se arranca en setActive(true) y se para en setActive(false).
    // Usamos un unique_ptr opaco (forward-declared) para no exponer detalles aquí.
    struct TcpServerHolder;
    std::unique_ptr<TcpServerHolder> tcp_server_;

    // Versión del protocolo.
    static constexpr const char* kProtocolVersion = "1.0.0";
    static constexpr const char* kPluginVersion  = "0.1.0";
};

}  // namespace flheretic