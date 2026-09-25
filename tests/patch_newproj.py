#!/usr/bin/env python3
"""Conecta newproj al crate y reemplaza el save_as (que no funcionaba) por
create_project, que es la via que si es determinista.
"""
import io

# ---- 1. lib.rs: registrar el modulo ----
L = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-fl\src\lib.rs"
s = io.open(L, encoding="utf-8").read()
if "pub mod newproj;" not in s:
    s = s.replace("pub mod save_as;", "pub mod newproj;")
    s = s.replace(
        "pub use save_as::{save_as, SaveAsReport};",
        "pub use save_as::{save_as, SaveAsReport};\n"
        "pub use newproj::{create as create_project_file, default_projects_dir, find_template};",
    )
io.open(L, "w", encoding="utf-8", newline="\n").write(s)
print("lib.rs: newproj registrado")

# ---- 2. lifecycle.rs: sustituir save_as por create_project ----
P = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-daemon\src\handlers\lifecycle.rs"
s = io.open(P, encoding="utf-8").read()

start = s.index("    /// Guardar como CON ruta, rellenando el dialogo de FL.")
end = s.index("    /// Cierra FL Studio enviando `WM_CLOSE` a su ventana.")
nuevo = '''    /// Crea un proyecto NUEVO en la carpeta habitual y lo abre.
    ///
    /// Por que no se usa el dialogo de "Save as" de FL: la FL Python API no
    /// expone la API de proyecto (`dir(general)` no tiene `saveProject` ni
    /// `getProjectFilePath`) y de las 79 constantes `midi.FPT_*` no hay ninguna
    /// que guarde con ruta. `FPT_SaveNew` abre el dialogo la primera vez,
    /// pero sus campos son `TQuickEdit` (controles Delphi internos) y al
    /// confirmar el dialogo se cierra SIN guardar. Intentar inyectar la ruta
    /// por `WM_SETTEXT` no funciona de forma fiable.
    ///
    /// Un `.flp` es un fichero: la via determinista es copiar una plantilla a
    /// la ruta deseada y pedirle a FL que la abra con `CreateProcess`. Cero
    /// interfaz, cero teclas, cero dialogos.
    ///
    /// Parametros:
    /// - `name` (obligatorio): nombre del proyecto, sin extension.
    /// - `dir` (opcional): carpeta destino. Defecto: la de FL.
    /// - `template` (opcional): `.flp` base. Defecto: el mas reciente de la
    ///   carpeta de FL que no sea backup ni autosave.
    /// - `open` (opcional, def. true): abrirlo en FL despues de crearlo.
    pub async fn create_project(&self, params: Value) -> Result<Value> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                HereticError::InvalidRequest("create_project: falta 'name'".into())
            })?
            .to_string();
        let dir = params.get("dir").and_then(|v| v.as_str()).map(PathBuf::from);
        let template = params
            .get("template")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);
        let open = params.get("open").and_then(Value::as_bool).unwrap_or(true);

        let dir_ref = dir.as_deref();
        let tpl_ref = template.as_deref();
        let path = heretic_fl::create_project_file(&name, dir_ref, tpl_ref)?;
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

        let mut out = json!({
            "created": path.display().to_string(),
            "file_size": size,
            "opened": false,
        });

        if open {
            match heretic_fl::launch(Some(&path), 25) {
                Ok(proc) => {
                    out["opened"] = json!(true);
                    out["pid"] = json!(proc.pid);
                    // Da tiempo a que FL levante el bridge antes de devolver,
                    // o el primer comando del LLM fallara por timeout.
                    let _ = self.wait_ready(json!({ "timeout": 30 })).await;
                    out["bridge_ready"] = json!(true);
                }
                Err(e) => {
                    out["open_error"] = json!(e.to_string());
                }
            }
        }
        Ok(out)
    }

'''
s = s[:start] + nuevo + s[end:]

# dispatcher: quitar save_as, anadir create_project
s = s.replace('            "save_as" => self.save_as(params).await,',
              '            "create_project" => self.create_project(params).await,')
# documentacion del modulo
s = s.replace(
    "//! | `save`     | bridge `project.save`     | `FPT_Save`, el atajo Ctrl+S",
    "//! | `save`     | bridge `project.save`     | `FPT_Save`, el atajo Ctrl+S",
)
s = s.replace(
    "//! | `save_as`  | bridge `project.saveAs`   | `FPT_SaveNew`, abre dialogo",
    "//! | `create`   | copiar .flp + CreateProcess | ruta arbitraria sin pelear con la UI",
)
io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("lifecycle.rs: create_project en lugar de save_as")

# ---- 3. tools.rs: nueva tool ----
T = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-mcp\src\tools.rs"
t = io.open(T, encoding="utf-8").read()
old_start = t.index("#[tool(description = \"Save the FL Studio project to a specific path")
nuevo_tool = '''#[tool(description = "Create a NEW FL Studio project in FL's projects folder and open it. Copies a template .flp to the destination and launches FL with it, which is fully automatic and needs no keyboard or dialogs. This is the reliable way to make a new project: FL's own Save As dialog cannot be automated because its fields are internal Delphi controls that close without saving. Template defaults to the most recent .flp in FL's projects folder that is not a backup.")]
pub async fn fl_create_project(
    name: String,
    dir: String,
    template: String,
) -> std::result::Result<Value, ToolError> {
    let client = daemon().await?;
    let mut params = json!({ "name": name });
    if !dir.is_empty() {
        params["dir"] = json!(dir);
    }
    if !template.is_empty() {
        params["template"] = json!(template);
    }
    let data = client
        .call("create_project", params)
        .await
        .map_err(daemon_err)?;
    Ok(data)
}
'''
t = t[:old_start] + nuevo_tool
io.open(T, "w", encoding="utf-8", newline="\n").write(t)
print("tools.rs: fl_create_project anadida")
