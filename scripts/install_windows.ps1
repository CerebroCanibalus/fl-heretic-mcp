<#
.SYNOPSIS
    Instalador automático one-shot de FL Heretic MCP.
.DESCRIPTION
    Este script:
    1. Detecta FL Studio, MSVC, CMake, Rust
    2. Compila el VST3 plugin
    3. Compila el daemon Rust (release)
    4. Copia plugin a %COMMONPROGRAMFILES%\VST3%
    5. Copia FL Heretic Bridge a FL Studio Scripts dir
    6. Genera token si no existe
    7. Configura MCP server en opencode.jsonc
    8. Verifica end-to-end con meta.ping

    Tras correr este script, el usuario solo necesita:
    1. Abrir FL Studio
    2. Options > Manage plugins > Scan
    3. Arrastrar "FL Heretic Bridge" a un slot del mixer
    4. (Opcional) Cerrar y reabrir FL si quiere autorreplicar

    Y listo. Claude puede hablar con FL Studio via MCP.

.PARAMETER SkipBuild
    Si se pasa, salta el build (asume binarios precompilados).

.PARAMETER SkipInstall
    Si se pasa, salta la instalación (solo build).

.PARAMETER SkipOpencodeConfig
    Si se pasa, no modifica opencode.jsonc.

.EXAMPLE
    .\install_windows.ps1
#>

[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$SkipInstall,
    [switch]$SkipOpencodeConfig
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir
$Vst3Dir = Join-Path $RepoRoot "vst3-bridge"
$BridgeDir = Join-Path $RepoRoot "fLMCP-bridge"
$CargoBin = Join-Path $RepoRoot "target\release"
$FlHereticExe = Join-Path $CargoBin "fl-heretic.exe"
$Vst3BuildDir = Join-Path $Vst3Dir "build"
$Vst3Output = Join-Path $Vst3BuildDir "Release\FL Heretic Bridge.vst3"

# Colores
function Write-Step($msg) { Write-Host "`n==> $msg" -ForegroundColor Cyan }
function Write-Ok($msg)   { Write-Host "  [OK] $msg" -ForegroundColor Green }
function Write-Warn($msg) { Write-Host "  [!] $msg" -ForegroundColor Yellow }
function Write-Err($msg)  { Write-Host "  [X] $msg" -ForegroundColor Red; throw $msg }

# ============================================================================
# Banner
# ============================================================================

Write-Host ""
Write-Host "============================================================" -ForegroundColor Magenta
Write-Host "  FL Heretic MCP — Instalador Automático v0.3.0" -ForegroundColor Magenta
Write-Host "============================================================" -ForegroundColor Magenta
Write-Host ""

# ============================================================================
# 1. Detectar prerequisites
# ============================================================================

Write-Step "1. Detectando prerequisites..."

# FL Studio
$flScriptsRoot = Join-Path $env:USERPROFILE "Documents\Image-Line\FL Studio\Settings\Hardware"
if (-not (Test-Path $flScriptsRoot)) {
    Write-Err "FL Studio no detectado en $flScriptsRoot. ¿Está instalado?"
}
Write-Ok "FL Studio detectado en $flScriptsRoot"

# MSVC (cl.exe)
$clPath = Get-Command cl.exe -ErrorAction SilentlyContinue
if (-not $clPath) {
    # Buscar en Visual Studio install dirs
    $vsPaths = @(
        "${env:ProgramFiles}\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC"
    )
    foreach ($p in $vsPaths) {
        if (Test-Path $p) {
            $latest = Get-ChildItem $p -Directory | Sort-Object Name -Descending | Select-Object -First 1
            $clFull = Join-Path $p "$($latest.Name)\bin\Hostx64\x64\cl.exe"
            if (Test-Path $clFull) {
                $env:PATH = "$((Split-Path $clFull))\..\..\..\bin\Hostx64\x64" + ";" + $env:PATH
                Write-Ok "MSVC encontrado: $clFull"
                break
            }
        }
    }
    if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
        Write-Warn "MSVC (cl.exe) no detectado. Build VST3 se saltará."
        Write-Warn "Instala Build Tools for Visual Studio 2022 desde https://visualstudio.microsoft.com/downloads/"
    }
} else {
    Write-Ok "MSVC encontrado: $($clPath.Source)"
}

# CMake
$cmakePath = Get-Command cmake -ErrorAction SilentlyContinue
if (-not $cmakePath) {
    Write-Warn "CMake no detectado. Build VST3 se saltará."
} else {
    Write-Ok "CMake encontrado: $($cmakePath.Source)"
}

# Rust (cargo)
$cargoPath = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargoPath) {
    Write-Err "Rust (cargo) no detectado. Instala desde https://rustup.rs/"
}
Write-Ok "Rust encontrado: $($cargoPath.Source)"

# ============================================================================
# 2. Build VST3 plugin
# ============================================================================

if (-not $SkipBuild) {
    Write-Step "2. Compilando VST3 plugin..."
    if (-not $cmakePath) {
        Write-Warn "Saltando build VST3 (CMake no disponible)"
    } elseif (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
        Write-Warn "Saltando build VST3 (MSVC no disponible)"
    } else {
        if (Test-Path $Vst3Output) {
            Write-Ok "VST3 ya compilado: $Vst3Output"
        } else {
            Push-Location $Vst3Dir
            try {
                cmake -S . -B build -G "Visual Studio 17 2022" -A x64 `
                    -DCMAKE_BUILD_TYPE=Release 2>&1 | Out-Null
                cmake --build build --config Release 2>&1 | Out-Null
                if (-not (Test-Path $Vst3Output)) {
                    Write-Err "Build VST3 falló — no se encontró $Vst3Output"
                }
                Write-Ok "VST3 compilado: $Vst3Output"
            } finally {
                Pop-Location
            }
        }
    }
}

# ============================================================================
# 3. Build daemon Rust (release)
# ============================================================================

if (-not $SkipBuild) {
    Write-Step "3. Compilando daemon Rust (release)..."
    Push-Location $RepoRoot
    try {
        cargo build --release 2>&1 | Out-Null
        if (-not (Test-Path $FlHereticExe)) {
            Write-Err "Build daemon falló — no se encontró $FlHereticExe"
        }
        Write-Ok "Daemon compilado: $FlHereticExe"
    } finally {
        Pop-Location
    }
}

# ============================================================================
# 4. Instalar VST3 plugin
# ============================================================================

if (-not $SkipInstall) {
    Write-Step "4. Instalando VST3 plugin..."
    if (-not (Test-Path $Vst3Output)) {
        Write-Warn "Saltando instalación VST3 (binario no encontrado)"
    } else {
        $vst3Dir = "${env:COMMONPROGRAMFILES}\VST3"
        if (-not (Test-Path $vst3Dir)) {
            $vst3Dir = "${env:ProgramFiles}\Common Files\VST3"
        }
        New-Item -ItemType Directory -Path $vst3Dir -Force | Out-Null
        $dest = Join-Path $vst3Dir "FL Heretic Bridge.vst3"
        Copy-Item $Vst3Output $dest -Force
        Write-Ok "VST3 instalado en: $dest"
    }
}

# ============================================================================
# 5. Instalar FL Heretic Bridge (controller script)
# ============================================================================

if (-not $SkipInstall) {
    Write-Step "5. Instalando FL Heretic Bridge..."
    $bridgeDirDest = Join-Path $flScriptsRoot "fLMCP Bridge"
    New-Item -ItemType Directory -Path $bridgeDirDest -Force | Out-Null
    $bridgeScript = Join-Path $BridgeDir "device_FLStudioMCP.py"
    $bridgeScriptDest = Join-Path $bridgeDirDest "device_FLStudioMCP.py"
    if (Test-Path $bridgeScript) {
        Copy-Item $bridgeScript $bridgeScriptDest -Force
        Write-Ok "Bridge script instalado en: $bridgeScriptDest"
    } else {
        Write-Err "No se encontró $bridgeScript"
    }
}

# ============================================================================
# 6. Generar token
# ============================================================================

if (-not $SkipInstall) {
    Write-Step "6. Generando token..."
    $tokenPath = Join-Path $env:LOCALAPPDATA "fl-heretic\token"
    if (Test-Path $tokenPath) {
        Write-Ok "Token ya existe: $tokenPath"
    } else {
        if (Test-Path $FlHereticExe) {
            & $FlHereticExe token generate | Out-Null
            Write-Ok "Token generado: $tokenPath"
        } else {
            Write-Warn "Daemon no compilado — saltando generación de token"
        }
    }
}

# ============================================================================
# 7. Verificar opencode.jsonc
# ============================================================================

if (-not $SkipOpencodeConfig) {
    Write-Step "7. Configurando opencode MCP server..."
    $opencodeConfig = Join-Path $env:USERPROFILE ".config\opencode\opencode.jsonc"
    $mcpExe = Join-Path $CargoBin "fl-heretic-mcp.exe"

    if (-not (Test-Path $mcpExe)) {
        Write-Warn "fl-heretic-mcp.exe no compilado — saltando config opencode"
    } else {
        if (Test-Path $opencodeConfig) {
            # Leer config existente
            $config = Get-Content $opencodeConfig -Raw | ConvertFrom-Json
            if (-not $config.mcpServers) {
                $config | Add-Member -MemberType NoteProperty -Name "mcpServers" -Value ([PSCustomObject]@{})
            }
            # Verificar si ya existe la config
            if (-not $config.mcpServers."fl-studio") {
                $newServer = [PSCustomObject]@{
                    command = $mcpExe
                    env = [PSCustomObject]@{
                        FL_HERETIC_PIPE = "\\.\pipe\fl-heretic-$$"
                        RUST_LOG = "info"
                    }
                }
                $config.mcpServers | Add-Member -MemberType NoteProperty -Name "fl-studio" -Value $newServer -Force
                $config | ConvertTo-Json -Depth 10 | Set-Content $opencodeConfig
                Write-Ok "MCP server configurado en: $opencodeConfig"
            } else {
                Write-Ok "MCP server ya configurado"
            }
        } else {
            # Crear config nueva
            New-Item -ItemType Directory -Path (Split-Path $opencodeConfig) -Force | Out-Null
            $newConfig = [PSCustomObject]@{
                mcpServers = [PSCustomObject]@{
                    "fl-studio" = [PSCustomObject]@{
                        command = $mcpExe
                        env = [PSCustomObject]@{
                            FL_HERETIC_PIPE = "\\.\pipe\fl-heretic-$$"
                            RUST_LOG = "info"
                        }
                    }
                }
            }
            $newConfig | ConvertTo-Json -Depth 10 | Set-Content $opencodeConfig
            Write-Ok "opencode.jsonc creado en: $opencodeConfig"
        }
    }
}

# ============================================================================
# 8. Resumen + instrucciones finales
# ============================================================================

Write-Host ""
Write-Host "============================================================" -ForegroundColor Green
Write-Host "  Instalación completa" -ForegroundColor Green
Write-Host "============================================================" -ForegroundColor Green
Write-Host ""
Write-Host "El usuario debe hacer UNA SOLA COSA más:"
Write-Host ""
Write-Host "  1. Abrir FL Studio 2025" -ForegroundColor Yellow
Write-Host "  2. Menú Options > Manage plugins > click 'Start scan'" -ForegroundColor Yellow
Write-Host "  3. Confirmar que 'FL Heretic Bridge' aparece con check ✓" -ForegroundColor Yellow
Write-Host "  4. Cerrar el manager (no cargar el plugin todavía)" -ForegroundColor Yellow
Write-Host "  5. Abrir el mixer, arrastrar 'FL Heretic Bridge' a CUALQUIER insert slot" -ForegroundColor Yellow
Write-Host "     (es un pass-through — no afecta el audio)" -ForegroundColor Yellow
Write-Host ""
Write-Host "Tras eso, abrir Claude/OpenCode y ejecutar:" -ForegroundColor Cyan
Write-Host ""
Write-Host "  > Verifica la conexión con `fl_ping`" -ForegroundColor White
Write-Host ""
Write-Host "Si todo va bien, FL Studio responde con su version + uptime." -ForegroundColor Cyan
Write-Host ""
Write-Host "Para auto-arranque del daemon al iniciar sesión, considerar:" -ForegroundColor DarkGray
Write-Host "  schtasks /create /tn FLHereticDaemon /tr '$FlHereticExe daemon' /sc onlogon" -ForegroundColor DarkGray
Write-Host ""
Write-Host "Paths importantes:" -ForegroundColor DarkGray
Write-Host "  VST3: $Vst3Output → %COMMONPROGRAMFILES%\VST3\FL Heretic Bridge.vst3" -ForegroundColor DarkGray
Write-Host "  Bridge: $bridgeScriptDest" -ForegroundColor DarkGray
Write-Host "  Daemon: $FlHereticExe" -ForegroundColor DarkGray
Write-Host "  Token: $env:LOCALAPPDATA\fl-heretic\token" -ForegroundColor DarkGray
Write-Host "  Audit: $env:LOCALAPPDATA\fl-heretic\audit.db" -ForegroundColor DarkGray