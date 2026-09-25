#!/usr/bin/env python3
"""Anade el subcomando 'fl-heretic config template <ruta>'."""
import io

P = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-daemon\src\main.rs"
s = io.open(P, encoding="utf-8").read()

# 1. variante del enum
anchor = "    /// Crea un proyecto nuevo en la carpeta de FL y lo abre."
nueva = '''    /// Config persistente del daemon (plantilla, carpetas).
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
''' + anchor
assert anchor in s
s = s.replace(anchor, nueva, 1)

# 2. el enum ConfigCmd, antes de `enum Cmd {`
old_enum = "enum Cmd {"
new_enum = '''/// Subcomandos de `fl-heretic config`.
#[derive(Subcommand, Debug)]
enum ConfigCmd {
    /// Fija el .flp con el que se crean los proyectos nuevos.
    Template {
        /// Ruta del .flp de plantilla.
        path: String,
    },
    /// Muestra la configuracion actual.
    Show,
    /// Ruta del fichero de configuracion.
    Path,
}

enum Cmd {'''
assert old_enum in s
s = s.replace(old_enum, new_enum, 1)

# 3. import de Subcommand
if "Subcommand" not in s.split("enum Cmd")[0]:
    s = s.replace("use clap::{Parser, Subcommand};", "use clap::{Parser, Subcommand};")
    if "use clap::{Parser, Subcommand};" not in s:
        s = s.replace("use clap::Parser;", "use clap::{Parser, Subcommand};")

# 4. el arm del match
a = s.index("        Cmd::NewProject { name, dir, template, no_open } => {")
arm = '''        Cmd::Config { action: ConfigCmd::Template { path } } => {
            match heretic_fl::set_template(std::path::Path::new(&path)) {
                Ok(cfg) => {
                    println!("plantilla fijada: {}", path);
                    println!("config guardada en: {}", heretic_fl::config_path().display());
                    let _ = cfg;
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("config error: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Cmd::Config { action: ConfigCmd::Show } => {
            let cfg = heretic_fl::load_config();
            let v = serde_json::to_string_pretty(&cfg).unwrap_or_default();
            println!("config: {}", heretic_fl::config_path().display());
            println!("{v}");
            ExitCode::SUCCESS
        }
        Cmd::Config { action: ConfigCmd::Path } => {
            println!("{}", heretic_fl::config_path().display());
            ExitCode::SUCCESS
        }
'''
s = s[:a] + arm + s[a:]

# 5. cabecera de uso
s = s.replace(
    '//! fl-heretic new-project "mi-cancion"       # crea proyecto nuevo y lo abre',
    '//! fl-heretic new-project "mi-cancion"       # crea proyecto nuevo y lo abre\n'
    '//! fl-heretic config template <ruta.flp>    # fija la plantilla de los nuevos',
)

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("main.rs: subcomando config anadido")
