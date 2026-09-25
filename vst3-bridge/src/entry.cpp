// VST3 entry point.

#include "public.sdk/source/vst/vstentry.h"
#include "plugin.h"

#include <cstdio>

using namespace Steinberg::Vst;
using namespace flheretic;

//-----------------------------------------------------------------------------
// Plugin factory function — VST3 SDK la llama para crear instancias del plugin.
//-----------------------------------------------------------------------------
BEGIN_FACTORY_DEF("FL Heretic MCP",
                  "https://github.com/CerebroCanibalus/fl-heretic-mcp",
                  "mailto:fl-heretic@example.com")

    // Categoría del plugin (Fx en este caso)
    DEF_CLASS2(INLINE_UID_FROM_FUID(0xF1H3E71C, 0xBEE5D03F, 0x9A2F1234, 0xDEADBEEF),
               Steinberg::Vst::PlugType::kFx,
               "FL Heretic Bridge",
               "Bridge between FL Studio and FL Heretic MCP daemon",
               0,  // subcategory (0 = generic)
               "Faster",  // version string
               kVstVersionString,
               Plugin::createInstance)

END_FACTORY_DEF

//-----------------------------------------------------------------------------
// Module entry point — VST3 SDK exporta esto
//-----------------------------------------------------------------------------
bool InitModule() {
    std::fprintf(stderr, "[FL Heretic] VST3 module InitModule()\n");
    return true;
}

bool DeinitModule() {
    std::fprintf(stderr, "[FL Heretic] VST3 module DeinitModule()\n");
    return true;
}