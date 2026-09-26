-- Guardián: ejecuta un payload Lua y devuelve el resultado como JSON.
--
-- Existe por una razón medida, no por gusto. Un error de ReaScript en Reaper
-- abre un diálogo MODAL: Reaper deja de procesar `command.json`, el bridge no
-- contesta y el agente recibe un único síntoma, "el bridge no respondió en
-- 10s", sin ninguna pista de la causa. Pasó de verdad dos veces en una sesión.
--
-- Con `pcall`, el error es un valor de retorno y el DAW no se congela.
--
-- Contrato:
--   entrada  : `<ResourcePath>Scripts/daw_payload.lua` (lo escribe el MCP)
--   salida   : SetExtState("reaper_mcp_script_result", "last_result", JSON)
--   consuming: `daw_do script_read_result`
--
-- El payload se BORRA tras leerlo: así un reintento del bridge no vuelve a
-- ejecutar lo mismo por accidente.

local function set(v)
  reaper.SetExtState("reaper_mcp_script_result", "last_result", v, false)
end

local ESCAPES = {
  ['"'] = '\\"', ['\\'] = '\\\\', ['\n'] = '\\n',
  ['\r'] = '\\r', ['\t'] = '\\t',
}

local function esc(s)
  local t = tostring(s)
  t = t:gsub('[%c"\\]', function(c)
    return ESCAPES[c] or string.format('\\u%04x', c:byte())
  end)
  return '"' .. t .. '"'
end

local function enc(v, depth)
  depth = depth or 0
  if depth > 8 then return '"<demasiado anidado>"' end
  if v == nil then return "null" end
  local t = type(v)
  if t == "boolean" then return tostring(v) end
  if t == "number" then
    if v ~= v or v == math.huge or v == -math.huge then return "null" end
    if v == math.floor(v) and v >= -2147483648 and v <= 2147483647 then
      return string.format("%d", v)
    end
    return string.format("%.14g", v)
  end
  if t == "string" then return esc(v) end
  if t ~= "table" then return esc(t) end

  -- ¿array (1..n) u objeto?
  local n, es_array = 0, true
  for k in pairs(v) do
    if type(k) ~= "number" then
      es_array = false
      break
    end
    if k > n then n = math.floor(k) end
  end
  if es_array then
    for i = 1, n do
      if v[i] == nil then es_array = false break end
    end
  end

  if es_array then
    if n == 0 then return "[]" end
    local partes = {}
    for i = 1, n do partes[i] = enc(v[i], depth + 1) end
    return "[" .. table.concat(partes, ",") .. "]"
  end

  local claves = {}
  for k in pairs(v) do claves[#claves + 1] = tostring(k) end
  table.sort(claves) -- salida estable: dos llamadas iguales dan el mismo JSON
  local partes = {}
  for _, k in ipairs(claves) do
    partes[#partes + 1] = esc(k) .. ":" .. enc(v[k], depth + 1)
  end
  return "{" .. table.concat(partes, ",") .. "}"
end

local path = reaper.GetResourcePath() .. "Scripts\\daw_payload.lua"
local f = io.open(path, "rb")
if not f then
  set('{"ok":false,"error":"no hay payload en ' .. esc(path) .. '"}')
  return
end
local code = f:read("a")
f:close()
os.remove(path)

local chunk, err_sintaxis = load(code, "daw_payload")
if not chunk then
  set('{"ok":false,"error":"error de sintaxis: ' .. enc(err_sintaxis) .. '"}')
  return
end

-- Aquí está todo el valor de este fichero: `pcall` convierte un diálogo
-- modal que congela el DAW en una cadena de texto.
local ok, res = pcall(chunk)
if not ok then
  set('{"ok":false,"error":' .. enc(tostring(res)) .. '}')
  return
end

set('{"ok":true,"value":' .. enc(res) .. '}')
