<#
    Reinicia REAPER de forma ordenada: WM_CLOSE, responde al dialogo de guardar
    solo con un boton inequivoco, y relanza. El supervisor vuelve a levantar el
    bridge solo al arrancar.

    Por que no `Stop-Process`: matar Reaper dispara su recuperacion de
    proyecto, que abre un dialogo al siguiente arranque y se queda ahi.

    Por que no pulsa ningun boton "a pelo": el boton de un dialogo de Reaper
    cambia de texto entre versiones, y en una de ellas el busca-saltos de la
    licencia era "Buy Me [4]". Aqui SOLO se pulsa algo que case explicitamente
    con "no guardar"; si no aparece, se para y se dice.

    .\tools\restart_reaper.ps1
#>
[CmdletBinding()]
param([switch]$SinRelanzar)

$ErrorActionPreference = 'Continue'
$exe = 'C:\Program Files\REAPER (x64)\reaper.exe'
$log = "$env:TEMP\reaper_mcp\supervisor.log"

function Get-PidReaper {
    $p = Get-Process -Name 'reaper' -ErrorAction SilentlyContinue |
         Sort-Object StartTime | Select-Object -Last 1
    if ($p) { return $p.Id } else { return 0 }
}

$pid0 = Get-PidReaper
if ($pid0 -eq 0) {
    Write-Host '  REAPER no esta corriendo.'
} else {
    Write-Host ("  REAPER pid {0}, cerrando con WM_CLOSE" -f $pid0)
    $p = Get-Process -Id $pid0
    $p.CloseMainWindow() | Out-Null

    $cerrado = $false
    for ($i = 0; $i -lt 20; $i++) {
        Start-Sleep -Milliseconds 500
        if (-not (Get-Process -Id $pid0 -ErrorAction SilentlyContinue)) { $cerrado = $true; break }
    }
    if (-not $cerrado) {
        # Sigue vivo: hay un dialogo. El que responde es daw_debug op=unblock,
        # que solo pulsa botones cuyo texto case con "no guardar" y se niega a
        # tocar cualquier otro. No se reimplementa aqui: reimplementarlo es
        # exactamente como se pulso "Buy Me [4]" en una sesion anterior.
        Write-Host '  sigue vivo, hay un dialogo: lo resuelve daw_debug op=unblock'
        $raiz = Split-Path -Parent $PSScriptRoot
        python (Join-Path $raiz 'tools\llamar.py') daw_debug unblock |
            Select-String 'boton|por_que_no' | ForEach-Object { Write-Host ("    " + $_.Line.Trim()) }
        for ($i = 0; $i -lt 10; $i++) {
            Start-Sleep -Milliseconds 500
            if (-not (Get-Process -Id $pid0 -ErrorAction SilentlyContinue)) { $cerrado = $true; break }
        }
    }
    if ($cerrado) {
        Write-Host '  cerrado.'
    } else {
        Write-Host '  NO SE HA CERRADO. No lo mato a la fuerza: dejalo abierto y cierra tu el dialogo.' -ForegroundColor Yellow
        exit 1
    }
}

if ($SinRelanzar) { Write-Host '  no se relanza (pedido)'; exit 0 }
if (-not (Test-Path $exe)) { Write-Host ("  no encuentro {0}" -f $exe) -ForegroundColor Red; exit 1 }

Write-Host '  arrancando...'
Start-Process -FilePath $exe
Start-Sleep -Seconds 12
$pid1 = Get-PidReaper
if ($pid1 -eq 0) {
    Write-Host '  REAPER no arranco.' -ForegroundColor Red
    exit 1
}
Write-Host ("  arrancado: pid {0}" -f $pid1) -ForegroundColor Green

# El supervisor deberia haber lanzado el bridge. Se lee el log, no se supone.
if (Test-Path $log) {
    Write-Host ''
    Write-Host '  ultimas lineas del supervisor:'
    Get-Content $log -Tail 4 | ForEach-Object { Write-Host ("    " + $_) }
}
