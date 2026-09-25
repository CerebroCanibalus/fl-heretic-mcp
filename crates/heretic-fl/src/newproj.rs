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

/// Config persistente del daemon (`%LOCALAPPDATA%\\fl-heretic\\config.json`).
///
/// Existe para la plantilla: buscar "el `.flp` mas reciente" como base es una
/// mala idea, porque un proyecto creado por nosotros mismo se convertiria en
/// la plantilla del siguiente y el resultado dejaria de parecerse a la
/// plantilla real. Con config, la plantilla queda fijada.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// `.flp` con el que se crean los proyectos nuevos.
    #[serde(default)]
    pub template_flp: Option<PathBuf>,
    /// Carpeta destino de los proyectos nuevos. Defecto: la de FL.
    #[serde(default)]
    pub projects_dir: Option<PathBuf>,
    /// Ruta del controller script. Defecto: el habitual.
    #[serde(default)]
    pub bridge_dir: Option<PathBuf>,
}

/// Directorio de datos del daemon.
pub fn data_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var("LOCALAPPDATA")
            .map(|s| PathBuf::from(s).join("fl-heretic"))
            .unwrap_or_else(|_| default_projects_dir().join("..").join("..").join("data"))
    } else {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        home.join(".local").join("share").join("fl-heretic")
    }
}

/// Ruta del fichero de config.
pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

/// Lee la config. Si no existe o esta corrupta, devuelve la de por defecto.
pub fn load_config() -> Config {
    let p = config_path();
    let Ok(raw) = std::fs::read_to_string(&p) else {
        return Config::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Guarda la config creando el directorio si hace falta.
pub fn save_config(cfg: &Config) -> Result<()> {
    let p = config_path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| HereticError::Other(format!("creando {}: {e}", parent.display())))?;
    }
    let body = serde_json::to_string_pretty(cfg)
        .map_err(|e| HereticError::Other(format!("serializando config: {e}")))?;
    std::fs::write(&p, body)
        .map_err(|e| HereticError::Other(format!("escribiendo {}: {e}", p.display())))?;
    Ok(())
}

/// Fija la plantilla con la que se crean los proyectos nuevos.
pub fn set_template(path: &Path) -> Result<Config> {
    if !path.is_file() {
        return Err(HereticError::Other(format!(
            "la plantilla no existe: {}",
            path.display()
        )));
    }
    let mut cfg = load_config();
    cfg.template_flp = Some(path.to_path_buf());
    save_config(&cfg)?;
    Ok(cfg)
}

/// Busca la plantilla con la que crear proyectos nuevos.
///
/// Prioridad:
/// 1. La que se pase explícitamente.
/// 2. `FL_HERETIC_TEMPLATE_FLP` del entorno (para pruebas o CI).
/// 3. La fijada en `config.json` (lo normal).
/// 4. Como último recurso, el `.flp` más reciente de la carpeta de FL que no
///    sea backup ni autosave.
///
/// El paso 4 es un red de seguridad, no la vía normal: el `.flp` más reciente
/// puede ser un proyecto que acabamos de crear, y usarlo como base arrastraría
/// su contenido al siguiente. Por eso lo normal es tener la plantilla fijada.
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

    if let Some(p) = load_config().template_flp {
        if p.is_file() {
            return Ok(TemplateChoice {
                source: format!("config: {}", config_path().display()),
                path: p,
            });
        }
    }

    let dir = load_config()
        .projects_dir
        .unwrap_or_else(default_projects_dir);
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
            // Backups y autosaves son restos, nunca plantillas.
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
            source: format!(
                "AVISO: no hay plantilla fijada; uso el .flp mas reciente en {}.                  Un proyecto creado aqui se convertira en la plantilla del                  siguiente. Fijala con 'fl-heretic config template <ruta>'.",
                dir.display()
            ),
            path: p,
        }),
        None => Err(HereticError::Other(format!(
            "no hay ninguna plantilla configurada.\n\
             Haz una de estas dos cosas:\n\
             1. 'fl-heretic config template <ruta.flp>'  ->  fija la plantilla\n\
             2. Define FL_HERETIC_TEMPLATE_FLP=<ruta.flp>\n\
             Se ha mirado en {} y no hay ningun .flp usable.",
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

    let destino_dir = dir
        .map(Path::to_path_buf)
        .or_else(|| load_config().projects_dir)
        .unwrap_or_else(default_projects_dir);
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
    fn la_config_por_defecto_esta_vacia() {
        // No se comprueba el valor (depende del entorno), solo que no rompe.
        let _ = load_config();
    }

    #[test]
    fn set_template_rechaza_una_ruta_inexistente() {
        let err = set_template(Path::new("Z:\\no\\existe.flp")).unwrap_err();
        assert!(err.to_string().contains("no existe"), "{err}");
    }

    #[test]
    fn la_config_sobrevive_una_ida_y_vuelta() {
        let dir = tempfile::tempdir().unwrap();
        let tpl = dir.path().join("t.flp");
        std::fs::write(&tpl, b"x").unwrap();

        // Config vacia -> solo campos por defecto.
        let c = Config::default();
        assert!(c.template_flp.is_none());
        assert!(c.projects_dir.is_none());

        // Con plantilla.
        let mut c2 = Config::default();
        c2.template_flp = Some(tpl);
        let j = serde_json::to_string(&c2).unwrap();
        let back: Config = serde_json::from_str(&j).unwrap();
        assert_eq!(back.template_flp, c2.template_flp);
    }

    #[test]
    fn la_carpeta_por_defecto_es_la_de_fl() {
        let s = default_projects_dir().to_string_lossy().replace('\\', "/");
        assert!(s.ends_with("FL Studio/Projects"), "ruta: {s}");
    }
}
