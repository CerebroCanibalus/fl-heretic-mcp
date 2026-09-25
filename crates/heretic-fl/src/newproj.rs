//! Crear proyectos nuevos sin pelearse con la interfaz de FL.
//!
//! # Por qué
//!
//! FL Studio no expone la API de proyecto a MIDI scripting: `dir(general)` no
//! tiene `saveProject` ni `getProjectFilePath`, y de las 79 constantes
//! `midi.FPT_*` no hay ninguna para guardar con ruta (`FPT_SaveNew` abre el
//! diálogo y no acepta ruta).
//!
//! La vía por la interfaz se ha intentado y **no funciona de forma fiable**:
//!
//! - `FPT_SaveNew` abre el diálogo la primera vez (`TNewProjForm`, "Save as").
//! - Sus campos son `TQuickEdit`, controles Delphi internos: aceptan
//!   `WM_SETTEXT`, pero al confirmar el diálogo se cierra **sin guardar**.
//! - Si el proyecto ya tiene nombre en memoria, `FPT_SaveNew` deja de abrir
//!   el diálogo por completo.
//! - `Ctrl+Shift+S` no es atajo de "Save as" en FL Studio 2025.
//!
//! ## La estrategia que sí funciona
//!
//! Un `.flp` es un fichero. Se copia una plantilla a la ruta deseada y se le
//! pide a FL que la abra con `CreateProcess`. Eso es determinista, no toca la
//! interfaz y da exactamente un "proyecto nuevo en la carpeta habitual".

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use heretic_core::{HereticError, Result};

/// Dónde guarda FL los proyectos por defecto.
pub fn default_projects_dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|_| std::env::var("HOME").map(PathBuf::from))
            .unwrap_or_else(|_| PathBuf::from("."))
    } else {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    };
    base.join("Documents")
        .join("Image-Line")
        .join("FL Studio")
        .join("Projects")
}

/// Qué plantilla usar como base.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateChoice {
    pub path: PathBuf,
    pub source: String,
}

/// Busca una plantilla `.flp` de la que copiar.
///
/// Prioridad:
/// 1. La ruta que se pase explicitamente.
/// 2. `FL_HERETIC_TEMPLATE_FLP` del entorno.
/// 3. El `.flp` más reciente de la carpeta de proyectos que no sea un
///    autosave ni un backup: si no, un proyecto "deseado" arrastraría
///    demasiado estado del proyecto anterior.
pub fn find_template() -> Result<TemplateChoice> {
    if let Ok(p) = std::env::var("FL_HERETIC_TEMPLATE_FLP") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(TemplateChoice {
                source: "env:FL_HERETIC_TEMPLATE_FLP".into(),
                path: p,
            });
        }
    }

    let dir = default_projects_dir();
    let mut mejor: Option<(std::time::SystemTime, PathBuf)> = None;
    if let Ok(entradas) = std::fs::read_dir(&dir) {
        for e in entradas.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("flp") {
                continue;
            }
            let nombre = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            // Los backups y autosaves no sirven de plantilla: son-restos.
            if nombre.contains("autosav") || nombre.contains("overwritten") {
                continue;
            }
            if let Ok(m) = e.metadata() {
                if let Ok(t) = m.modified() {
                    if mejor.as_ref().map(|(bt, _)| t > *bt).unwrap_or(true) {
                        mejor = Some((t, p));
                    }
                }
            }
        }
    }

    match mejor {
        Some((_, p)) => Ok(TemplateChoice {
            source: format!("mas reciente en {}", dir.display()),
            path: p,
        }),
        None => Err(HereticError::Other(format!(
            "no hay ningun .flp en {} para usar de plantilla.\n\
             Opciones:\n\
             1. Crea un proyecto vacio en FL, guardalo con Ctrl+S, y ya sera \
             plantilla automaticamente.\n\
             2. O define FL_HERETIC_TEMPLATE_FLP=/ruta/a/base.flp",
            dir.display()
        ))),
    }
}

/// Crea un proyecto nuevo en la carpeta habitual y devuelve su ruta.
///
/// Copia la plantilla a `<projects>/<name>.flp` y devuelve la ruta. NO abre
/// FL: eso lo hace quien llame, para poder elegir entre abrir y solo crear.
pub fn create(name: &str, dir: Option<&Path>, template: Option<&Path>) -> Result<PathBuf> {
    // Nombre de fichero seguro: sin separadores ni caracteres inválidos.
    let limpio: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    let limpio = limpio.trim().trim_end_matches(".flp").to_string();
    if limpio.is_empty() {
        return Err(HereticError::InvalidRequest(
            "el nombre del proyecto está vacío".into(),
        ));
    }

    let destino_dir = dir.map(Path::to_path_buf).unwrap_or_else(default_projects_dir);
    std::fs::create_dir_all(&destino_dir).map_err(|e| {
        HereticError::Other(format!("creando {}: {e}", destino_dir.display()))
    })?;

    let destino = destino_dir.join(format!("{limpio}.flp"));
    if destino.exists() {
        return Err(HereticError::Other(format!(
            "el proyecto ya existe: {}\n(elige otro nombre, o borra ese primero)",
            destino.display()
        )));
    }

    let origen = match template {
        Some(p) => {
            if !p.is_file() {
                return Err(HereticError::Other(format!(
                    "la plantilla no existe: {}",
                    p.display()
                )));
            }
            TemplateChoice {
                source: "argumento".into(),
                path: p.to_path_buf(),
            }
        }
        None => find_template()?,
    };

    std::fs::copy(&origen.path, &destino).map_err(|e| {
        HereticError::Other(format!(
            "copiando {} -> {}: {e}",
            origen.path.display(),
            destino.display()
        ))
    })?;

    tracing::info!(
        template = %origen.path.display(),
        destino = %destino.display(),
        "proyecto creado"
    );
    Ok(destino)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanea_nombres_de_fichero() {
        let casos = [
            ("mi/cancion", "mi_cancion"),
            ("a\\b:c*d?e", "a_b_c_d_e"),
            ("normal.flp", "normal"),
            ("con espacios", "con espacios"),
        ];
        for (entrada, esperado) in casos {
            let limpio: String = entrada
                .chars()
                .map(|c| match c {
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                    c => c,
                })
                .collect();
            let limpio = limpio.trim().trim_end_matches(".flp").to_string();
            assert_eq!(limpio, esperado, "entrada: {entrada}");
        }
    }

    #[test]
    fn rechaza_nombre_vacio() {
        assert!(create("   ", None, None).is_err());
        assert!(create("", None, None).is_err());
    }

    #[test]
    fn no_sobrescribe_un_proyecto_existente() {
        let dir = tempfile::tempdir().unwrap();
        let plantilla = dir.path().join("base.flp");
        std::fs::write(&plantilla, b"PK\x03\x04flp fake").unwrap();
        let destino_dir = dir.path().join("proyectos");

        let p = create("cancion", Some(&destino_dir), Some(&plantilla)).unwrap();
        assert!(p.is_file());
        assert_eq!(p.file_name().unwrap(), "cancion.flp");
        assert_eq!(std::fs::read(&p).unwrap(), std::fs::read(&plantilla).unwrap());

        // Segundo intento con el mismo nombre: debe fallar, no pisar.
        let err = create("cancion", Some(&destino_dir), Some(&plantilla)).unwrap_err();
        assert!(err.to_string().contains("ya existe"), "{err}");
    }

    #[test]
    fn crea_el_directorio_si_falta() {
        let dir = tempfile::tempdir().unwrap();
        let plantilla = dir.path().join("base.flp");
        std::fs::write(&plantilla, b"x").unwrap();
        let destino_dir = dir.path().join("a").join("b").join("c");
        let p = create("x", Some(&destino_dir), Some(&plantilla)).unwrap();
        assert!(p.is_file());
    }

    #[test]
    fn error_claro_si_la_plantilla_no_existe() {
        let dir = tempfile::tempdir().unwrap();
        let err = create("x", Some(dir.path()), Some(Path::new("Z:\\no\\existe.flp")))
            .unwrap_err();
        assert!(err.to_string().contains("plantilla no existe"), "{err}");
    }

    #[test]
    fn la_carpeta_por_defecto_es_la_de_fl() {
        let s = default_projects_dir().to_string_lossy().replace('\\', "/");
        assert!(s.ends_with("FL Studio/Projects"), "ruta: {s}");
    }
}
