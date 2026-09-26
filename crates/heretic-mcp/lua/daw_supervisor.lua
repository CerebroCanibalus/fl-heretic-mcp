-- Supervisor del bridge MCP.
--
-- ## Por quÃ© existe
--
-- `__startup.lua` hacÃ­a esto:
--
--     dofile(".../reaper_mcp_server.lua")
--
-- Eso ejecuta el bridge **en la misma instancia** que el arranque. Si el
-- bridge peta, se lleva por delante la Ãºnica cosa que lo puede relanzar, y no
-- hay vuelta atrÃ¡s hasta reiniciar Reaper. PasÃ³: el bridge llevaba 12 minutos
-- muerto con Reaper abierto, sin ningÃºn sÃ­ntoma mÃ¡s que "no responde", y la
-- Ãºnica salida era pulsar Reaper a mano o reiniciar el DAW.
--
-- AquÃ­ el bridge se registra como acciÃ³n y se lanza con `Main_OnCommand`, que
-- crea una **instancia nueva**. Este supervisor queda en la suya, y cada
-- comprobaciÃ³n ve si el bridge sigue respirando.
--
-- ## CÃ³mo sabe si el bridge vive
--
-- El bridge escribe un timestamp Unix en `%TEMP%\reaper_mcp\server.lock` cada
-- 10 s. No hace falta `stat`: se lee el nÃºmero de dentro y se compara con
-- `os.time()`. Es el mismo criterio que usa el cliente Rust.
--
-- ## Por que NO relanza en caliente
--
-- Este fichero antes relanzaba el bridge cada vez que el latido llevaba
-- 30 s caducado. Se probó y **rompió el bridge**, que es peor que no hacer
-- nada:
--
--   1. se escribió un timestamp viejo en server.lock
--   2. el supervisor decidió relanzar y llamó a `Main_OnCommand`
--   3. `Main_OnCommand` sobre un ReaScript **en marcha** no es un no-op: se
--      lleva la instancia que estaba y la sustituye. Y la sustituta no
--      arrancó, porque el propio bridge no mira el lock al arrancar
--      (`setup_ipc` sobrescribe server.lock sin comprobar nada).
--   4. resultado: ni la instancia vieja ni la nueva. Latido muerto para
--      siempre, y sin forma de volver desde aqui.
--
-- Es decir: `AddRemoveReaScript` + `Main_OnCommand` solo sirven con el bridge
-- **sabidamente parado**, que es justo el unico momento en que hace falta:
-- el arranque de Reaper. En caliente no se puede distinguir "colgado" de
-- "vivo con un falso negativo del latido" desde aqui, y equivocarse destruye.
--
-- Decision: en caliente **no se relanza**. El supervisor vigila y lo escribe
-- al log. Quien puede hacer la distincion correcta es el cliente, que hace una
-- peticion real por el RPC; para eso esta `daw_debug op=restart_bridge`, que
-- lee el id de accion de este mismo fichero.
--
-- ## Los dos errores que este fichero evita
--
-- 1. **Bucle de reinicios.** Si el bridge muere al arrancar (un bug), un
--    supervisor sin memoria lo relanzarÃ­a en bucle y Reaper se comerÃ­a la CPU.
--    AquÃ­ hace falta ver el latido muerto *DOS* veces seguidas (unos 6 s) y
--    ademÃ¡s se respeta un mÃ­nimo de 20 s entre relanzamientos.
-- 2. **Doble instancia.** Si el bridge estuviera vivo pero tardase, un
--    reinicio abrirÃ­a un segundo servidor peleÃ¡ndose por el mismo
--    `command.json`. Por eso se comprueba el latido, no el proceso: el latido
--    es lo Ãºnico que el bridge escribe de verdad.

local SEP = "\\"
local dir_ipc = os.getenv("TEMP") or os.getenv("TMP") or "C:\\Temp"
dir_ipc = dir_ipc .. SEP .. "reaper_mcp"

--
-- ## `GetResourcePath()` NO lleva separador final
--
-- Esto costo una sesion entera. El è¿”å›ž es `C:\Users\...\REAPER` y a secas,
-- asi que `"Scripts\\algo"` produce `...REAPERScripts\algo` y
-- `AddRemoveReaScript` devuelve 0 sin decir por que. El supervisor salia por
-- el `return` de "no pude registrar" y se acababa su trabajo: Reaper arrancaba
-- sin bridge y nadie vigilaba.
--
-- El proprio bridge lo tiene bien: usa `GetResourcePath() .. "/Scripts"`, con
-- barra normal, que en Windows siempre vale. Eso es lo que hay que hacer.

local ruta_bridge = reaper.GetResourcePath() .. "/Scripts/reaper_mcp_server.lua"
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

local ruta_log = dir_ipc .. SEP .. "supervisor.log"

-- Log a FICHERO, no solo a la consola de ReaScript.
--
-- La consola no se puede leer desde fuera: es un RichEdit, y GetWindowText de
-- otro proceso sobre un control de edicion no devuelve nada. Es decir: si el
-- supervisor falla, el unico rastro vivia en una ventana que nadie â€”ni el
-- agente, ni un testâ€” puede inspeccionar. Con el log, "por que no arranco el
-- bridge" es una linea de texto.
local function log(msg)
  local linea = "[daw-supervisor] " .. msg
  reaper.ShowConsoleMsg(linea .. "\n")
  local f = io.open(ruta_log, "a")
  if f then
    f:write(os.date("%Y-%m-%d %H:%M:%S") .. " " .. linea .. "\n")
    f:close()
  end
end

-- Registrarlo UNA sola vez: `AddRemoveReaScript` con `true` anade, y llamarlo
-- en cada tick acabaria con cientos de entradas en el action list.
local id_accion = reaper.AddRemoveReaScript(true, 0, ruta_bridge, true)
if not id_accion or id_accion == 0 then
  log("no pude registrar " .. ruta_bridge .. " como accion; el bridge no tiene supervisor.")
  return
end
log("bridge registrado como accion " .. tostring(id_accion) .. " (seccion 0)")

-- El id de accion se escribe a fichero para que `daw_debug op=restart_bridge`
-- pueda relanzar sin volver a registrar (y duplicar) la entrada del action list.
local f_id = io.open(dir_ipc .. SEP .. "bridge_action.txt", "w")
if f_id then f_id:write(tostring(id_accion)); f_id:close() end

local fallos = 0
local ultimo_lanzamiento = 0
local lanzados = 0
local ultimo_estado = nil

local function registrar_estado(que)
  local f = io.open(ruta_lock, "r")
  local edad = -1
  if f then
    local t = tonumber(f:read("*a")) or 0
    f:close()
    edad = os.time() - t
  end
  log("estado: " .. que .. " (latido hace " .. tostring(edad) .. " s)")
end

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
    if fallos > 0 or ultimo_estado ~= "vivo" then
      ultimo_estado = "vivo"
      registrar_estado("bridge vivo, fallos reseteados")
    end
    fallos = 0
  else
    fallos = fallos + 1
    -- Avisar, no relanzar. Ver la nota de arriba: relanzar en caliente
    -- destruye la instancia que funciona.
    if fallos == FALLOS_PARA_RELANZAR then
      fallos = 0
      registrar_estado("latido sin actualizar; NO relanzo en caliente,                          usa daw_debug op=restart_bridge")
    end
  end
  reaper.defer(tick)
end

-- El `defer` doble NO es opcional, y lo aprendi quitandolo.
--
-- `__startup.lua` corre en pleno arranque de Reaper, cuando la lista de
-- acciones todavia no esta lista: llamar `AddRemoveReaScript` + `Main_OnCommand`
-- ahi no hace nada, y el arranque se queda sin bridge en silencio. El
-- `__startup.lua` original que este supervisor reemplaza ya llevaba dos
-- `defer` anidados por eso, y yo los quite al reescribirlo. Resultado
-- medido: Reaper arrancado, bridge ausente, y nada en la consola que lo
-- explicara.
--
-- El primer `defer` sale del arranque; el segundo, de un momento mas
-- adelante. Con los dos, el supervisor ya registra y lanza.
reaper.defer(function()
  reaper.defer(function()
    arrancar("arranque de Reaper")
    registrar_estado("supervisor arrancado, id_accion=" .. tostring(id_accion))
    tick()
  end)
end)

