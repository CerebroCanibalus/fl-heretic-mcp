#!/usr/bin/env python3
"""Anade el subcomando 'fl-heretic open <ruta.flp>'.

Hace falta porque new-project no sirve para un .flp que ya existe, y porque
PowerShell's Start-Process parte las rutas con espacios. Command::arg() de
Rust entrecomilla solo en Windows, asi que abrir por aqui es fiable.
"""
import io

P = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-daemon\src\main.rs"
s = io.open(P, encoding="utf-8").read()

# 1. variante del enum
anchor = "    /// Crea un proyecto nuevo en la carpeta de FL y lo abre."
nueva = '''    /// Abre un .flp existente en FL Studio.
    ///
    /// Lanza FL con el fichero como argumento. `Command::arg()` entrecomilla
    /// solo en Windows, asi que las rutas con espacios van bien (a diferencia
    /// de `Start-Process -ArgumentList` de PowerShell, que las parte).
    Open {
        /// Ruta del .flp.
        path: String,
        /// No esperar a que el bridge responda.
        #[arg(long)]
        no_wait: bool,
    },
''' + anchor
assert anchor in s
s = s.replace(anchor, nueva, 1)

# 2. arm del match
a = s.index("        Cmd::NewProject { name, dir, template, no_open } => {")
arm = '''        Cmd::Open { path, no_wait } => {
            let p = std::path::PathBuf::from(&path);
            if !p.is_file() {
                eprintln!("open error: no existe {}", p.display());
                return ExitCode::from(1);
            }
            match heretic_fl::launch(Some(&p), 25) {
                Ok(proc) => {
                    println!("abierto: {}", p.display());
                    println!("FL Studio pid {}", proc.pid);
                    if !no_wait {
                        println!("esperando al bridge...");
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("open error: {e}");
                    ExitCode::from(1)
                }
            }
        }
'''
s = s[:a] + arm + s[a:]

s = s.replace(
    '//! fl-heretic config template <ruta.flp>    # fija la plantilla de los nuevos',
    '//! fl-heretic config template <ruta.flp>    # fija la plantilla de los nuevos\n'
    '//! fl-heretic open "C:\\\\ruta\\\\p.flp"      # abre un proyecto existente',
)

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("main.rs: subcomando open anadido")
