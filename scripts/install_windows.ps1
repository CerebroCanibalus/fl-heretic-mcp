<#
.SYNOPSIS
    Instalador one-shot de FL Heretic MCP.
.DESCRIPTION
    Deja el sistema listo para que un agente IA controle FL Studio:

      1. Detecta FL Studio (y su carpeta de Hardware).
      2. Compila el workspace Rust en release (sin VST3, sin MSVC, sin CMake).
      3. Copia el controller script a "FL Heretic Bridge" (NO a "fLMCP Bridge",
         que es el de un tercero y provocaria colision de ficheros).
      4. Registra el bridge en el registro de FL, para que aparezca ya elegido
         en Options > MIDI Settings.
      5. Genera el token Bearer si no existe.
      6. Registra el MCP server en la config de opencode.
      7. Verifica el entorno y escribe un log.

    Tras ejecutarlo, el usuario solo tiene que abrir FL Studio.

    El registro se escribe al CERRAR FL Studio, asi que si FL esta abierto el
    paso 4 se aplaza y se avisa. Forzar el registro con FL abierto puede
    corromper la config de hardware: el script no lo hace por ti.

.EXAMPLE
    .\install_windows.ps1
    .\install_windows.ps1 -SkipBuild
#>

[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$SkipRegistry
)

$ErrorActionPreference = "Continue"   # NUNCA "Stop": una comprobacion fallida
$ProgressPreference    = "SilentlyContinue"

$RepoRoot  = Split-Path -Parent $PSScriptRoot
$ReleaseDir = Join-Path $RepoRoot "target\release"
$FlHeretic  = Join-Path $ReleaseDir "fl-heretic.exe"
$McpExe     = Join-Path $ReleaseDir "fl-heretic-mcp.exe"
$BridgeSrc  = Join-Path $RepoRoot "fl-heretic-bridge\device_FLHereticBridge.py"
$BridgeName = "FL Heretic Bridge"
$DataDir    = Join-Path $env:LOCALAPPDATA "fl-heretic"
$LogFile    = Join-Path $DataDir "install.log"
$RegKey     = "HKCU:\SOFTWARE\Image-Line\FL Studio 25\Devices\MIDI input"

# --- registro de pasos: cada uno dice OK / FALLO / SALTADO ---------------
$script:Steps = @()
function Step  ($name) { $script:Cur = $name; Write-Host ""; Write-Host "==> $name" -ForegroundColor Cyan }
function Ok     ($m) { $script:Steps += ,@($script:Cur,"OK",$m);  Write-Host "  [OK] $m" -ForegroundColor Green }
function Warn   ($m) { $script:Steps += ,@($script:Cur,"SALTADO",$m); Write-Host "  [--] $m" -ForegroundColor Yellow }
function Fail   ($m) { $script:Steps += ,@($script:Cur,"FALLO",$m); Write-Host "  [XX] $m" -ForegroundColor Red }

New-Item -ItemType Directory -Path $DataDir -Force | Out-Null
$Log = @()

function Note($m) {
    $line = "  $m"
    Write-Host $line
    $Log += $line
}

Write-Host ""
Write-Host "============================================================" -ForegroundColor Magenta
Write-Host "  FL Heretic MCP  -  instalador" -ForegroundColor Magenta
Write-Host "============================================================" -ForegroundColor Magenta
Write-Host "  log: $LogFile"

# =========================================================================
# 1. FL Studio
# =========================================================================
Step "1/6  Detectar FL Studio"

$Hardware = Join-Path $env:USERPROFILE "Documents\Image-Line\FL Studio\Settings\Hardware"
if (Test-Path $Hardware) {
    Ok "carpeta de Hardware: $Hardware"
} else {
    Fail "no existe $Hardware - FL Studio no parece instalado, o se cambio el perfil"
    Write-Host "      Se sigue: puede que se cree al abrir FL Studio por primera vez."
    $Log += "      Se sigue: puede que se cree al abrir FL Studio por primera vez."
}

$flExe = @(
    "D:\Program Files\Image-Line\FL Studio 2025\FL64.exe",
    "$env:ProgramFiles\Image-Line\FL Studio 2025\FL64.exe",
    "${env:ProgramFiles(x86)}\Image-Line\FL Studio 2025\FL64.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if ($flExe) { Ok "FL Studio: $flExe" }
else { Warn "no se localizo FL64.exe por las rutas habituales" }

# =========================================================================
# 2. Build
# =========================================================================
Step "2/6  Compilar el workspace Rust"

$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if ($SkipBuild) {
    Warn "-SkipBuild: se usan los binarios que ya haya"
} elseif (-not $cargo) {
    Fail "cargo no esta en el PATH. Instala Rust desde https://rustup.rs/"
    Write-Host "      Sin build no hay nada que instalar. Aborta aqui."
    $Log += "      Sin build no hay nada que instalar."
} else {
    Note "cargo build --release (tarda el primer uso)"
    Push-Location $RepoRoot
    $buildOut = & cargo build --release --workspace 2>&1
    $code = $LASTEXITCODE
    Pop-Location
    if ($code -eq 0) { Ok "build correcto" }
    else {
        Fail "cargo build fallo (exit $code)"
        $buildOut | Select-Object -Last 15 | ForEach-Object {
            Write-Host "      $_" -ForegroundColor DarkGray
            $Log += "      $_"
        }
    }
}

if (-not (Test-Path $FlHeretic)) { Fail "falta $FlHeretic" }
else { Ok "daemon:  $FlHeretic" }

if (-not (Test-Path $McpExe)) { Fail "falta $McpExe - el MCP server no se podra arrancar" }
else { Ok "mcp:     $McpExe" }

# =========================================================================
# 3. Bridge
# =========================================================================
Step "3/6  Instalar el controller script"

$BridgeDest = Join-Path $Hardware $BridgeName
New-Item -ItemType Directory -Path $BridgeDest -Force | Out-Null

if (-not (Test-Path $BridgeSrc)) {
    Fail "no encuentro el fuente del bridge: $BridgeSrc"
} else {
    Copy-Item $BridgeSrc (Join-Path $BridgeDest "device_FLHereticBridge.py") -Force
    Ok "bridge en: $BridgeDest\device_FLHereticBridge.py"
}

# El bridge ajeno comparte ficheros con otro MCP. Si esta, fuera: dos
# clientes escribiendo en el mismo directorio.
$Ajeno = Join-Path $Hardware "fLMCP Bridge"
if (Test-Path $Ajeno) {
    Warn "existe tambien '$BridgeName' de otro MCP en $Ajeno"
    Note "  los dos bridges usarían ficheros hr_*.json distintos, pero"
    Note "  FL solo puede ejecutar un controller script por puerto MIDI."
    Note "  Si da problemas, renombra o borra esa carpeta."
    $Log += "  AVISO: hay otro bridge en $Ajeno"
}

# =========================================================================
# 4. Registro de FL
# =========================================================================
Step "4/6  Registrar el bridge en FL Studio"

$flRunning = Get-Process FL64 -ErrorAction SilentlyContinue

if ($SkipRegistry) {
    Warn "-SkipRegistry"
} elseif ($flRunning) {
    Warn "FL Studio esta abierto (pid $($flRunning[0].Id))"
    Note "  FL escribe su config al CERRAR. El registro se aplica al salir."
    Note " asi que se hace ahora y en la proxima vez que abras FL."
    # Se escribe igual: HKCU es del usuario y FL solo lo lee al arrancar.
    $applied = $false
    try {
        if (-not (Test-Path $RegKey)) { New-Item -Path $RegKey -Force | Out-Null }
        Get-ChildItem $RegKey -ErrorAction SilentlyContinue | ForEach-Object {
            Set-ItemProperty -Path $_.PSPath -Name "ScriptFolder" -Value $BridgeName -ErrorAction SilentlyContinue
            Set-ItemProperty -Path $_.PSPath -Name "Enabled" -Value 1 -ErrorAction SilentlyContinue
            $applied = $true
        }
        if ($applied) { Ok "ScriptFolder='$BridgeName' en los puertos MIDI existentes" }
        else { Warn "no hay puertos MIDI en el registro todavia (se crean al usar FL)" }
    } catch {
        Warn "no se pudo escribir el registro: $($_.Exception.Message)"
    }
    Note " Recuerda: en FL, Options > MIDI Settings, elige '$BridgeName'."
} else {
    $applied = $false
    try {
        if (-not (Test-Path $RegKey)) { New-Item -Path $RegKey -Force | Out-Null }
        Get-ChildItem $RegKey -ErrorAction SilentlyContinue | ForEach-Object {
            Set-ItemProperty -Path $_.PSPath -Name "ScriptFolder" -Value $BridgeName -ErrorAction SilentlyContinue
            Set-ItemProperty -Path $_.PSPath -Name "Enabled" -Value 1 -ErrorAction SilentlyContinue
            $applied = $true
        }
        if ($applied) { Ok "ScriptFolder='$BridgeName' aplicado" }
        else { Warn "no hay puertos MIDI registrados; seleccionalo en FL a mano" }
    } catch {
        Warn "no se pudo escribir el registro: $($_.Exception.Message)"
    }
}

# =========================================================================
# 5. Token
# =========================================================================
Step "5/6  Token de autenticacion"

$tokenPath = Join-Path $DataDir "token"
if (Test-Path $tokenPath) {
    Ok "ya existe: $tokenPath"
} elseif (Test-Path $FlHeretic) {
    & $FlHeretic token generate 2>&1 | Out-Null
    if (Test-Path $tokenPath) { Ok "generado: $tokenPath" }
    else { Fail "no se genero el token" }
} else {
    Warn "sin binario, sin token"
}

# =========================================================================
# 6. Config de opencode
# =========================================================================
Step "6/6  Registrar el MCP server en opencode"

$ocConfig = Join-Path $env:USERPROFILE ".config\opencode\opencode.jsonc"
if (-not (Test-Path $McpExe)) {
    Warn "sin fl-heretic-mcp.exe, se omite la config"
} else {
    if (Test-Path $ocConfig) {
        Copy-Item $ocConfig "$ocConfig.flheretic.bak" -Force
        Ok "copia de seguridad: $ocConfig.flheretic.bak"
        Note "  anade este bloque dentro del objeto 'mcp' de $ocConfig :"
        Note ""
        # Las llaves dobles son para -f, pero aqui solo se imprime texto:
        # no hace falta escapar, y escaparlas se nota en la salida.
        Note '      "fl-heretic": {'
        Note ('        "command": "{0}",' -f ($McpExe -replace '\\', '\\'))
        Note '        "env": { "RUST_LOG": "warn" }'
        Note '      }'
        Note ""
        Note "  se deja a mano a proposito: es un .jsonc con comentarios y"
        Note "  reescribirlo a ciega se los comeria. El .bak esta por si acaso."
        Note "  recuerda tener el daemon corriendo antes de usar las tools:"
        Note "    $FlHeretic daemon"
    } else {
        Warn "no existe $ocConfig; crealo a mano con el bloque de arriba"
    }
}

# =========================================================================
# Resumen
# =========================================================================
Write-Host ""
Write-Host "============================================================" -ForegroundColor White
Write-Host "  Resumen" -ForegroundColor White
Write-Host "============================================================" -ForegroundColor White

# Un paso puede emitir varias lineas (OK + avisos). Para el resumen solo
# interesa el PEOR estado de cada paso, no cada linea suelta.
$worst = @{}
$order = @()
foreach ($s in $script:Steps) {
    if (-not $worst.ContainsKey($s[0])) { $order += $s[0]; $worst[$s[0]] = $s[1] }
    elseif ($s[1] -eq "FALLO") { $worst[$s[0]] = "FALLO" }
}
foreach ($n in $order) {
    $color = switch ($worst[$n]) { "OK" {"Green"} "FALLO" {"Red"} default {"Yellow"} }
    Write-Host ("  {0,-42} {1}" -f $n, $worst[$n]) -ForegroundColor $color
}

$failed = @($order | Where-Object { $worst[$_] -eq "FALLO" }).Count
Write-Host ""
if ($failed -eq 0) {
    Write-Host "  Sin fallos. Para usarlo:" -ForegroundColor Green
    Write-Host "    1. $FlHeretic daemon   (dejarlo en una terminal)" -ForegroundColor Gray
    Write-Host "    2. abrir FL Studio" -ForegroundColor Gray
    Write-Host "    3. abrir opencode y probar fl_ping" -ForegroundColor Gray
} else {
    Write-Host "  $failed paso(s) con fallo. Revisa el log:" -ForegroundColor Red
    Write-Host "    $LogFile" -ForegroundColor Gray
}

$Log | Set-Content $LogFile -Encoding UTF8
Write-Host ""
Write-Host "  log escrito en $LogFile"
Write-Host ""
Read-Host "  Pulsa Enter para cerrar"
exit $failed
