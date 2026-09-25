cmake_minimum_required(VERSION 3.20)
project(fl-heretic-bridge
    VERSION 0.1.0
    DESCRIPTION "FL Heretic MCP Bridge — VST3 plugin exposing FL Studio API"
    LANGUAGES CXX
)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

# ---------------------------------------------------------------------------
# VST3 SDK — descarga automática via FetchContent
# ---------------------------------------------------------------------------
include(FetchContent)
set(VST3_SDK_VERSION "3.7.7_build_19-patched")
set(VST3_SDK_REPO "https://github.com/steinbergmedia/vst3sdk.git")

FetchContent_Declare(
    vst3sdk
    GIT_REPOSITORY ${VST3_SDK_REPO}
    GIT_TAG        ${VST3_SDK_VERSION}
    GIT_SHALLOW    TRUE
)
set(SMTG_CREATE_PLUGIN_LINK  OFF  CACHE BOOL "")
set(SMTG_CREATE_VSTGUI       OFF  CACHE BOOL "")
set(SMTG_ENABLE_PCH          OFF  CACHE BOOL "")
set(SMTG_RUN_VST_VALIDATOR   OFF  CACHE BOOL "")
set(SMTG_ADD_VST3_PLUGINS_SAMPLES OFF  CACHE BOOL "")
set(SMTG_ADD_VST3_HOSTING_SAMPLES OFF  CACHE BOOL "")
FetchContent_MakeAvailable(vst3sdk)

# ---------------------------------------------------------------------------
# Plugin
# ---------------------------------------------------------------------------
add_library(${PROJECT_NAME} MODULE
    src/entry.cpp
    src/plugin.cpp
    src/protocol.cpp
    src/tcp_server.cpp
    src/handlers/transport.cpp
    src/handlers/meta.cpp
    src/handlers/mixer.cpp
)

target_include_directories(${PROJECT_NAME}
    PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/src
        ${vst3sdk_SOURCE_DIR}/public.sdk/source/vst
        ${vst3sdk_SOURCE_DIR}/pluginterfaces
        ${vst3sdk_SOURCE_DIR}/base
)

# En Windows, no usar main() CRT — VST3 define su propio entry
if(WIN32)
    target_compile_definitions(${PROJECT_NAME} PRIVATE
        NOMINMAX
        _CRT_SECURE_NO_WARNINGS
        WIN32_LEAN_AND_MEAN
        WIN32
        _WINDOWS
    )
endif()

target_compile_options(${PROJECT_NAME} PRIVATE
    -Wall
    -Wextra
    -Wno-unused-parameter
    -Wno-deprecated-declarations
)

# Link
target_link_libraries(${PROJECT_NAME} PRIVATE
    sdk
)

# Renombrar output a .vst3 (VST3 bundle en macOS, .vst3 en Windows/Linux)
set_target_properties(${PROJECT_NAME} PROPERTIES
    PREFIX ""
    OUTPUT_NAME "FL Heretic Bridge"
)

if(APPLE)
    set_target_properties(${PROJECT_NAME} PROPERTIES
        SUFFIX ".vst3"
    )
elseif(WIN32)
    set_target_properties(${PROJECT_NAME} PROPERTIES
        SUFFIX ".vst3"
    )
else()
    set_target_properties(${PROJECT_NAME} PROPERTIES
        SUFFIX ".vst3"
    )
endif()

# Copy moduleinfo.json
configure_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/resource/moduleinfo.json.in
    ${CMAKE_CURRENT_BINARY_DIR}/moduleinfo.json
)

# Install: copia el .vst3 a la carpeta estándar de VST3
install(TARGETS ${PROJECT_NAME}
    RUNTIME DESTINATION vst3
    BUNDLE DESTINATION vst3
)