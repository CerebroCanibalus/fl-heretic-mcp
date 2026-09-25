#!/usr/bin/env python3
"""Conecta save_as al handler de lifecycle y anade la tool MCP."""
import io

# ---- 1. lifecycle.rs: metodo save_as_to ----
P = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-daemon\src\handlers\lifecycle.rs"
s = io.open(P, encoding="utf-8").read()

old = '''    /// Guardar como. Abre un dialogo de FL: la ruta la pone el usuario.
    pub async fn save_as(&self) -> Result<Value> {
        let r = self.bridge.call("project.saveAs", json!({})).await?;
        Ok(json!({
            "saved": false,
            "warning": r
                .get("warning")
                .and_then(Value::as_str)
                .unwrap_or("FL abre un dialogo 'Save as'; la ruta la escribe el usuario"),
        }))
    }'''
new = '''    /// Guardar como CON ruta, rellenando el dialogo de FL.
    ///
    /// La FL Python API no expone guardar con ruta (`dir(general)` no tiene
    /// `saveProject` ni `getProjectFilePath`, comprobado sobre los 909
    /// simbolos), y `midi.FPT_SaveNew` abre el dialogo sin aceptar ruta. Asi
    /// que hay que pasar por la interfaz: el modulo `save_as` de heretic-fl
    /// enfoca FL, manda Ctrl+Shift+S y escribe la ruta con `WM_SETTEXT`.
    ///
    /// Parametros: `{ "path": "C:/.../proyecto.flp", "timeout": 8000 }`.
    ///
    /// CUIDADO: roba el foco. Si FL pide confirmar (directorio inexistente,
    /// ¿sobrescribir?), el dialogo se queda abierto y hay que cerrarlo a mano.
    pub async fn save_as(&self, params: Value) -> Result<Value> {
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HereticError::InvalidRequest("save_as: falta 'path'".into()))?;
        let timeout = params.get("timeout").and_then(|v| v.as_u64()).unwrap_or(8000);

        let p = PathBuf::from(path);
        // Crea el directorio si falta: si no, FL pide confirmación y el
        // diálogo no se cierra solo.
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    HereticError::Other(format!("creando {}: {e}", parent.display()))
                })?;
            }
        }

        let report = heretic_fl::save_as(&p, timeout)?;
        let out = serde_json::to_value(&report)
            .map_err(|e| HereticError::Other(format!("serializando informe: {e}")))?;

        if !report.ok {
            return Err(HereticError::Other(format!(
                "save_as no completó: {}",
                report.note
            )));
        }

        // Verificacion: el .flp tiene que existir y pesa mas de 0.
        let exists = p.exists();
        let size = p.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(json!({
            "report": out,
            "file_exists": exists,
            "file_size": size,
            "title_after": self
                .bridge
                .call("project.metadata", json!({}))
                .await
                .ok()
                .and_then(|v| v.get("title").cloned()),
        }))
    }'''
assert old in s, "save_as no encontrado en lifecycle"
s = s.replace(old, new)

# registrar en el dispatcher
s = s.replace('            "save_as" => self.save_as().await,',
              '            "save_as" => self.save_as(params).await,')
io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("lifecycle.rs: save_as(path) conectado")

# ---- 2. tools.rs: tool MCP ----
T = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-mcp\src\tools.rs"
t = io.open(T, encoding="utf-8").read()

addition = '''
#[tool(description = "Save the FL Studio project to a specific path, by driving the Save As dialog automatically (focus FL, Ctrl+Shift+S, type the path, confirm). This is the only way to choose the save path, because FL's Python API does not expose saving at all. Returns the file size to confirm it worked. CAUTION: it steals keyboard focus, and if FL asks to confirm (missing folder, overwrite) the dialog stays open for you to handle.")]
pub async fn fl_save_as(
    path: String,
) -> std::result::Result<Value, ToolError> {
    if !path.to_lowercase().ends_with(".flp") {
        return Err(ToolError::invalid_params(format!(
            "la ruta debe terminar en .flp: {path}"
        )));
    }
    let client = daemon().await?;
    let data = client
        .call("save_as", json!({ "path": path }))
        .await
        .map_err(daemon_err)?;
    Ok(data)
}
'''
t = t.rstrip() + "\n" + addition
io.open(T, "w", encoding="utf-8", newline="\n").write(t)
print("tools.rs: fl_save_as anadida")
