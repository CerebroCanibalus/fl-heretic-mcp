//! Tools MCP. Andamiaje vacio: se reconstruyen sobre `heretic-daw`.
//!
//! La lesson de la version de FL: 17 wrappers finos sobre acciones que ya
//! cubria un escape hatch solo anadian mantenimiento, y cada wrapper
//! reimplementaba la construccion de params (que es donde se colaron 3 bugs).
//! La version de Reaper mantiene 4 tools con superficies explicitas.

use flojo_mcp::prelude::*;
use serde_json::{json, Value};

/// Ping de diagnostico. Andamiaje: verifica que el server arranca.
#[tool(description = "Check the DAW MCP server is alive. Returns the bridge version and the DAW version.")]
pub async fn fl_diagnose() -> std::result::Result<Value, ToolError> {
    Ok(json!({ "status": "scaffolding", "daw": "reaper" }))
}
