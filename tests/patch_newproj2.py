#!/usr/bin/env python3
"""Sustituye el subcomando save-as por new-project."""
import io

P = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-daemon\src\main.rs"
s = io.open(P, encoding="utf-8").read()

# 1. la variante del enum
start = s.index("    /// Guarda el proyecto de FL Studio en una ruta concreta")
end = s.index("    /// Diagn\u00f3stico del entorno (puertos MIDI, FL alive, token path, etc.).\n    Doctor,")
nuevo = '''    /// Crea un proyecto nuevo en la carpeta de FL y lo abre.
    ///
    /// Copia una plantilla .flp al destino y lanza FL con ella. Es la via
    /// determinista: el dialogo de "Save as" de FL no se puede automatizar
    /// porque sus campos son controles Delphi internos que cierran sin
    /// guardar, y la FL Python API no expone la API de proyecto.
    NewProject {
        /// Nombre del proyecto (sin .flp).
        name: String,
        /// Carpeta destino. Por defecto, la de FL.
        #[arg(long)]
        dir: Option<String>,
        /// .flp de partida. Por defecto, el mas reciente de la carpeta de FL.
        #[arg(long)]
        template: Option<String>,
        /// No abrir en FL (solo crear el fichero).
        #[arg(long)]
        no_open: bool,
    },
'''
s = s[:start] + nuevo + s[end:]

# 2. el arm del match
a = s.index("        Cmd::SaveAs { path, timeout } => {")
b = s.index("        Cmd::Doctor => match commands::doctor::run() {")
arm = '''        Cmd::NewProject { name, dir, template, no_open } => {
            let dirp = dir.map(std::path::PathBuf::from);
            let tpl = template.map(std::path::PathBuf::from);
            let r = heretic_fl::create_project_file(
                &name,
                dirp.as_deref(),
                tpl.as_deref(),
            );
            match r {
                Ok(path) => {
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    println!("creado: {} ({} bytes)", path.display(), size);
                    if no_open {
                        return ExitCode::SUCCESS;
                    }
                    match heretic_fl::launch(Some(&path), 25) {
                        Ok(p) => {
                            println!("FL Studio lanzado, pid {}", p.pid);
                            println!("esperando al bridge...");
                            for _ in 0..60 {
                                if heretic_fl::running_process().is_some() {
                                    std::thread::sleep(std::time::Duration::from_millis(500));
                                }
                                break;
                            }
                            ExitCode::SUCCESS
                        }
                        Err(e) => {
                            eprintln!("no se pudo abrir en FL: {e}");
                            ExitCode::from(1)
                        }
                    }
                }
                Err(e) => {
                    eprintln!("new-project error: {e}");
                    ExitCode::from(1)
                }
            }
        }
'''
s = s[:a] + arm + s[b:]

# 3. cabecera de uso
s = s.replace(
    '//! fl-heretic save-as "C:\\\\ruta\\\\p.flp"  # guarda con ruta (roba el foco)',
    '//! fl-heretic new-project "mi-cancion"       # crea proyecto nuevo y lo abre',
)

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("main.rs: new-project en lugar de save-as")
