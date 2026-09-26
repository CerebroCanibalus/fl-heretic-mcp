<#
    Compila el release y lo deja en el sitio que usa opencode.

    El MCP server se ejecuta desde target\release\daw-heretic-mcp.exe, y
    opencode lo mantiene abierto mientras corre. En Windows no se puede
    reemplazar un fichero abierto, asi que `cargo build --release` falla con un
    "failed to remove ... daw-heretic-mcp.exe" y parece un problema de
    compilacion cuando lo que esta bloqueado es el propio servidor.

    Hay que ejecutarlo con opencode CERRADO. Ahi es un paso, y al volver a
    abrirlo entra el binario nuevo con las tools nuevas.

    -Verify   Compila a target\verify en vez de target\release. Sirve para
              comprobar que compila sin tocar el binario que usa opencode, y
              se puede hacer con la sesion abierta.

    Ejemplos:
        .\tools\install_release.ps1
        .\tools\install_release.ps1 -Verify
#>
[CmdletBinding()]
param([switch]$Verify)

$ErrorActionPreference = 'Stop'
$raiz = Split-Path -Parent $PSScriptRoot
Set-Location $raiz
$fase = if ($Verify) { 'verify' } else { 'release' }

Write-Host ''
Write-Host '  DAW Heretic MCP: instalando el release'
if ($Verify) {
    Write-Host '  destino: target\verify\release (no toca el binario de opencode)'
} else {
    Write-Host '  destino: target\release, el que usa opencode'
}
Write-Host ''

# --- 1. Avisar ANTES de compilar si el sitio esta ocupado ------------------
# Compilar tarda un minuto; fallar al final por un fichero abierto es peor que
# comprobarlo en el segundo uno.
if (-not $Verify) {
    $destino = Join-Path $raiz 'target\release\daw-heretic-mcp.exe'
    $ocupado = $false
    if (Test-Path $destino) {
        # Abrir en EXCLUSIVO. Medido: `Copy-Item` SI funciona con un fichero
        # abierto, asi que copiar no dice nada. Lo que falla es abrirlo para
        # escribir sin compartir, que es justo lo que hace cargo al reemplazar.
        try {
            $fs = [System.IO.File]::Open(
                $destino,
                [System.IO.FileMode]::Open,
                [System.IO.FileAccess]::ReadWrite,
                [System.IO.FileShare]::None)
            $fs.Close()
        } catch {
            $ocupado = $true
        }
    }
    if ($ocupado) {
        Write-Host '  ESTE SCRIPT TIENE QUE CORRER CON OPENCODE CERRADO.' -ForegroundColor Yellow
        Write-Host ''
        Write-Host '  El binario lo tiene abierto:' -ForegroundColor Yellow
        Get-Process -Name 'daw-heretic-mcp' -ErrorAction SilentlyContinue |
            ForEach-Object { Write-Host ("    pid {0} desde {1}" -f $_.Id, $_.StartTime) -ForegroundColor Yellow }
        Write-Host ''
        Write-Host '  Cierra opencode y ejecuta esto otra vez.' -ForegroundColor Yellow
        Write-Host '  Para solo comprobar que compila: .\tools\install_release.ps1 -Verify' -ForegroundColor Yellow
        Write-Host ''
        exit 1
    }
}

# --- 2. Compilar ------------------------------------------------------------
$cargoArgs = @('build', '--release', '--workspace')
if ($Verify) { $cargoArgs += @('--target-dir', 'target/verify') }
Write-Host ("  cargo {0}" -f ($cargoArgs -join ' '))
# `2>&1` mete stderr en el flujo, y con ErrorActionPreference=Stop PowerShell
# convierte cada warning de cargo en un error terminante. Cargo avisa por stderr
# sin que nada haya fallado.
$ErrorActionPreference = 'Continue'
$log = & cargo @cargoArgs 2>&1
$ErrorActionPreference = 'Stop' 
if ($LASTEXITCODE -ne 0) {
    $log | Select-Object -Last 25 | ForEach-Object { Write-Host ("    " + $_) -ForegroundColor Red }
    exit 1
}

$binario = Join-Path $raiz ("target\{0}\release\daw-heretic-mcp.exe" -f $fase)
if (-not (Test-Path $binario)) {
    Write-Host ("  no encuentro {0}" -f $binario) -ForegroundColor Red
    exit 1
}
Write-Host ("  compilado: {0:N0} bytes" -f (Get-Item $binario).Length)

# --- 3. Tests: son la red de seguridad de todo esto -------------------------
Write-Host ''
Write-Host '  tests'
$ErrorActionPreference = 'Continue'
$t = & cargo test --workspace 2>&1
$ErrorActionPreference = 'Stop' 
$total = 0
$fallos = 0
foreach ($line in ($t | Select-String 'test result:')) {
    if ($line -match 'ok\. (\d+) passed; (\d+) failed') {
        $total += [int]$Matches[1]
        $fallos += [int]$Matches[2]
    }
}
Write-Host ("    {0} tests, {1} fallos" -f $total, $fallos)
if ($fallos -gt 0) {
    Write-Host '  HAY FALLOS. No sigas.' -ForegroundColor Red
    exit 1
}

# --- 4. Cuantas tools expone DE VERDAD, preguntandoselo al binario ------------
# Compilar no dice si el registro de tools funciona. Preguntar si.
Write-Host ''
Write-Host '  tools que expone el binario recien compilado:'
$lote = @(
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"install","version":"1"}}}',
    '{"jsonrpc":"2.0","method":"notifications/initialized"}',
    '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
) -join "`n"
# El servidor escribe sus logs de tracing por stderr. Con ErrorActionPreference
# = Stop eso es un error, no un log.
$ErrorActionPreference = 'Continue'
$salida = $lote | & $binario 2>$null
$ErrorActionPreference = 'Stop'
$nombres = @()
foreach ($line in $salida) {
    if (-not $line.Trim()) { continue }
    try { $m = $line | ConvertFrom-Json } catch { continue }
    if ($m.id -eq 2) { $nombres = @($m.result.tools.name) }
}
if ($nombres.Count -eq 0) {
    Write-Host '  el binario no respondio a tools/list' -ForegroundColor Red
    exit 1
}
$nombres | Sort-Object | ForEach-Object { Write-Host ("    " + $_) }
Write-Host ''
Write-Host ("  {0} tools" -f $nombres.Count) -ForegroundColor Green
Write-Host ''
if ($Verify) {
    Write-Host '  Esto es solo la copia de verificacion. Para que opencode lo pille,' -ForegroundColor Yellow
    Write-Host '  cierra opencode y ejecuta .\tools\install_release.ps1 sin -Verify' -ForegroundColor Yellow
} else {
    Write-Host '  Listo. Abre opencode y las tools nuevas entran solas.' -ForegroundColor Green
}
Write-Host ''
