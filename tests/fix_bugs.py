#!/usr/bin/env python3
"""Arregla los 3 bugs de routing/params encontrados al auditar las tools.

B1. `fl_create_project` llama al metodo `create_project`, que NO esta en
    LIFECYCLE_METHODS, asi que cae en Transport y responde
    "metodo transport desconocido: create_project". La tool que mas trabajo
    costo es justo la que no funcionaba.

B2. `fl_set_song_position` envia `{"ms": ms}` pero el daemon lee
    `params["position"]` + `params["unit"]`. Siempre falla con
    "set_song_position: missing position". Ademas la tool documenta
    "in milliseconds" mientras el daemon por defecto asume "bars": un
    desajuste de 4x en la posicion si se arreglara solo el nombre del param.

B3. `Transport::dispatch` tiene el brazo `"call"` duplicado (lineas 44 y 46).
    Rust no avisa de arms duplicados en un match con guardia implicita; el
    segundo es inalcanzable.

Ademas se limpia `"save_as"` de LIFECYCLE_METHODS: no existe el handler, asi
que enrutarlo a Lifecycle solo produce "metodo de lifecycle desconocido".
"""
import io

# ---------------------------------------------------------------------------
# B1 + limpieza: pipe.rs
# ---------------------------------------------------------------------------
P = r"crates\heretic-daemon\src\pipe.rs"
s = io.open(P, encoding="utf-8").read()

old = '''const LIFECYCLE_METHODS: &[&str] = &[
    "open", "launch", "save", "save_as", "close", "fl_status", "wait_ready",
];'''
new = '''const LIFECYCLE_METHODS: &[&str] = &[
    "open",
    "launch",
    "save",
    "create_project",
    "close",
    "fl_status",
    "wait_ready",
];'''
assert old in s, "LIFECYCLE_METHODS no encontrado"
s = s.replace(old, new, 1)

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("B1 ok: 'create_project' enruta a Lifecycle")

# ---------------------------------------------------------------------------
# B3 + doc: transport.rs
# ---------------------------------------------------------------------------
P = r"crates\heretic-daemon\src\handlers\transport.rs"
s = io.open(P, encoding="utf-8").read()

old = '''            "call" => self.call(params).await,
            "actions" => self.actions().await,
            "call" => self.call(params).await,
'''
new = '''            "call" => self.call(params).await,
            "actions" => self.actions().await,
'''
assert old in s, "brazo duplicado no encontrado"
s = s.replace(old, new, 1)

# El doc-comment de la cabecera dice "133 actions del fLMCP Bridge v0.2.0",
# que es informacion vieja de dos generaciones atras.
s = s.replace(
    "//! Handlers transport — map a las 133 actions del fLMCP Bridge v0.2.0.",
    "//! Handlers transport — API fina sobre las 67 actions del FL Heretic Bridge.\n"
    "//!\n"
    "//! Estas son las unicas actions con semantica propia (validacion de rango,\n"
    "//! normalizacion de unidades, releer el estado real en vez de devolver el\n"
    "//! eco del bridge). Para todo lo demas esta `call`, que es el escape hatch.",
    1,
)

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("B3 ok: brazo duplicado eliminado")

# ---------------------------------------------------------------------------
# B2: tools.rs
# ---------------------------------------------------------------------------
P = r"crates\heretic-mcp\src\tools.rs"
s = io.open(P, encoding="utf-8").read()

old = '''#[tool(description = "Set FL Studio song position in milliseconds (>= 0). Returns the new position (ms/ticks/beats).")]
pub async fn fl_set_song_position(
    ms: f64,
) -> std::result::Result<Value, ToolError> {
    if ms < 0.0 {
        return Err(ToolError::invalid_params(format!("ms debe ser >= 0: {ms}")));
    }
    let client = daemon().await?;
    let data = client
        .call("set_song_position", json!({ "ms": ms }))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}'''

new = '''#[tool(description = "Move the FL Studio song position. Unit defaults to milliseconds; pass unit=bars|ms|seconds|ticks|steps for the others. Returns the REAL resulting position in ticks/bars/seconds, read back from FL after the move (not the requested value).")]
pub async fn fl_set_song_position(
    position: f64,
    unit: String,
) -> std::result::Result<Value, ToolError> {
    if position < 0.0 {
        return Err(ToolError::invalid_params(format!(
            "position debe ser >= 0: {position}"
        )));
    }
    let client = daemon().await?;
    // El param se llama `position`, no `ms`: el daemon lee `params["position"]`
    // y `params["unit"]`. Ademas hay que pasar SIEMPRE la unidad, porque el
    // default del daemon es "bars" y el llamante cree que son milisegundos.
    let data = client
        .call(
            "set_song_position",
            json!({ "position": position, "unit": unit }),
        )
        .await
        .map_err(daemon_err)?;
    Ok(data)
}'''
assert old in s, "fl_set_song_position no encontrado"
s = s.replace(old, new, 1)

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("B2 ok: fl_set_song_position usa position+unit")
