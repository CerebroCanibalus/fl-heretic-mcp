#!/usr/bin/env python3
"""Anade el subcomando `fl-heretic save-as <ruta>` al binario del daemon."""
import io

P = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-daemon\src\main.rs"
s = io.open(P, encoding="utf-8").read()

# 1. nueva variante del enum Cmd, justo antes de Doctor
old_doc = "    /// Diagn\u00f3stico del entorno (puertos MIDI, FL alive, token path, etc.).\n    Doctor,"
new_doc = """    /// Guarda el proyecto de FL Studio en una ruta concreta, rellenando el
    /// dialogo de "Save as" automaticamente. La FL Python API no expone
    /// guardar con ruta, asi que hay que pasar por la interfaz.
    ///
    /// ROBARÁ EL FOCO del teclado yabr\u00e1 la ventana de FL al frente.
    SaveAs {
        /// Ruta completa del .flp, entre comillas si lleva espacios.
        path: String,
        /// Milisegundos que se espera a que aparezca el dialogo.
        #[arg(long, default_value_t = 8000)]
        timeout: u64,
    },
    /// Diagn\u00f3stico del entorno (puertos MIDI, FL alive, token path, etc.).
    Doctor,"""
assert old_doc in s, "no encuentro la variante Doctor"
s = s.replace(old_doc, new_doc, 1)

# 2. el match arm
old_arm = "        Cmd::Doctor => match commands::doctor::run() {"
new_arm = """        Cmd::SaveAs { path, timeout } => {
            use std::path::Path;
            let p = Path::new(&path);
            match heretic_fl::save_as(p, timeout) {
                Ok(r) => {
                    println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
                    if r.ok {
                        let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                        println!("\\nOK: {} ({} bytes)", p.display(), size);
                    } else {
                        println!("\\nFALLO: {}", r.note);
                    }
                }
                Err(e) => {
                    eprintln!("save-as error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Cmd::Doctor => match commands::doctor::run() {"""
assert old_arm in s, "no encuentro el arm de Doctor"
s = s.replace(old_arm, new_arm, 1)

# 3. la cabecera de uso
s = s.replace(
    "//! fl-heretic doctor                 # chequea entorno (puertos MIDI, FL alive, etc.)",
    "//! fl-heretic doctor                 # chequea entorno (puertos MIDI, FL alive, etc.)\n"
    "//! fl-heretic save-as \"C:\\\\ruta\\\\p.flp\"  # guarda con ruta (roba el foco)",
)

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("main.rs: subcomando save-as anadido")
