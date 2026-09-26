-- Supervisor del bridge MCP.
--
-- ## Por qué existe
--
-- `__startup.lua` hacía esto:
--
--     dofile(".../reaper_mcp_server.lua")
--
-- Eso ejecuta el bridge **en la misma instancia** que el arranque. Si el
-- bridge peta, se lleva por delante la única cosa que lo puede relanzar, y no
-- hay vuelta atrás hasta reiniciar Reaper. Pasó: el bridge llevaba 12 minutos
-- muerto con Reaper abierto, sin ningún síntoma más que "no responde", y la
-- única salida era pulsar Reaper a mano o reiniciar el DAW.
--
-- Aquí el bridge se registra como acción y se lanza con `Main_OnCommand`, que
-- crea una **instancia nueva**. Este supervisor queda en la suya, y cada
-- comprobación ve si el bridge sigue respirando.
--
-- ## Cómo sabe si el bridge vive
--
-- El bridge escribe un timestamp Unix en `%TEMP%\reaper_mcp\server.lock` cada
-- 10 s. No hace falta `stat`: se lee el número de dentro y se compara con
-- `os.time()`. Es el mismo criterio que usa el cliente Rust.
--
-- ## Los dos errores que este fichero evita
--
-- 1. **Bucle de reinicios.** Si el bridge muere al arrancar (un bug), un
--    supervisor sin memoria lo relanzaría en bucle y Reaper se comería la CPU.
--    Aquí hace falta ver el latido muerto *DOS* veces seguidas (unos 6 s) y
--    además se respeta un mínimo de 20 s entre relanzamientos.
-- 2. **Doble instancia.** Si el bridge estuviera vivo pero tardase, un
--    reinicio abriría un segundo servidor peleándose por el mismo
--    `command.json`. Por eso se comprueba el latido, no el proceso: el latido
--    es lo único que el bridge escribe de verdad.

local SEP = "\\"
local dir_ipc = os.getenv("TEMP") or os.getenv("TMP") or "C:\\Temp"
dir_ipc = dir_ipc .. SEP .. "reaper_mcp"

local ruta_bridge = reaper.GetResourcePath() .. "Scripts\\reaper_mcp_server.lua"
local ruta_lock = dir_ipc .. SEP .. "server.lock"

-- El bridge refresca cada 10 s. 30 s de margen son tres refrescos perdidos: no es
-- un umbral que dispare por un tic de disco lento.
local MARGEN_S = 30
local INTERVALO_S = 2
local MIN_ENTRE_RELANZAMIENTOS_S = 20
local FALLOS_PARA_RELANZAR = 2

local function puente_vivo()
  local f = io.open(ruta_lock, "r")
  if not f then return false end
  local txt = f:read("*a")
  f:close()
  local t = tonumber(txt)
  if not t then return false end
  return (os.time() - t) < MARGEN_S
end

local function log(msg)
  reaper.ShowConsoleMsg("[daw-supervisor] " .. msg .. "\n")
end

-- Registrarlo UNA sola vez: `AddRemoveReaScript` con `true` anade, y llamarlo
-- en cada tick acabaria con cientos de entradas en el action list.
local id_accion = reaper.AddRemoveReaScript(true, 0, ruta_bridge, true)
if not id_accion or id_accion == 0 then
  log("no pude registrar " .. ruta_bridge .. " como accion; el bridge no tiene supervisor.")
  return
end
log("bridge registrado como accion " .. tostring(id_accion) .. " (seccion 0)")

local fallos = 0
local ultimo_lanzamiento = 0
local lanzados = 0

local function arrancar(motivo)
  local ahora = os.time()
  if (ahora - ultimo_lanzamiento) < MIN_ENTRE_RELANZAMIENTOS_S then
    return
  end
  ultimo_lanzamiento = ahora
  lanzados = lanzados + 1
  log("lanzando el bridge (" .. motivo .. "), intento " .. lanzados)
  reaper.Main_OnCommand(id_accion, 0)
end

local function tick()
  if puente_vivo() then
    fallos = 0
  else
    fallos = fallos + 1
    if fallos >= FALLOS_PARA_RELANZAR then
      fallos = 0
      arrancar("el latido lleva " .. tostring(MARGEN_S) .. "s sin Actualizar")
    end
  end
  reaper.defer(tick)
end

-- Primero el bridge, despues la vigilancia. Al revés habria un hueco sin
-- bridge en el primer arranque.
arrancar("arranque de Reaper")
tick()
