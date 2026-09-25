#!/usr/bin/env python3
"""Anade un fichero de configuracion persistente para la plantilla.

El problema: buscar 'el .flp mas reciente' como plantilla es una mala idea.
Un proyecto creado por nosotros mismo se convertiria en la base del
siguiente, y el resultado ya no tendria nada de tu plantilla real.

La solucion: un config.json en %LOCALAPPDATA%\\fl-heretic\\ con la plantilla
elegida. Prioridad: argumento > env > config > mas reciente de FL.
"""
import io

P = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-fl\src\newproj.rs"
s = io.open(P, encoding="utf-8").read()

# ---- 1. sustituye la busqueda de plantilla ----
start = s.index("/// Busca una plantilla `.flp` de la que copiar.")
end = s.index("/// Crea un proyecto nuevo en la carpeta habitual y devuelve su ruta.")
nuevo = '''/// Config persistente del daemon (`%LOCALAPPDATA%\\\\fl-heretic\\\\config.json`).
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
                "AVISO: no hay plantilla fijada; uso el .flp mas reciente en {}. \
                 Un proyecto creado aqui se convertira en la plantilla del \
                 siguiente. Fijala con 'fl-heretic config template <ruta>'.",
                dir.display()
            ),
            path: p,
        }),
        None => Err(HereticError::Other(format!(
            "no hay ninguna plantilla configurada.\\n\\
             Haz una de estas dos cosas:\\n\\
             1. 'fl-heretic config template <ruta.flp>'  ->  fija la plantilla\\n\\
             2. Define FL_HERETIC_TEMPLATE_FLP=<ruta.flp>\\n\\
             Se ha mirado en {} y no hay ningun .flp usable.",
            dir.display()
        ))),
    }
}

'''
s = s[:start] + nuevo + s[end:]

# ---- 2. create() usa la projects_dir de la config ----
s = s.replace(
    "    let destino_dir = dir.map(Path::to_path_buf).unwrap_or_else(default_projects_dir);",
    "    let destino_dir = dir\n"
    "        .map(Path::to_path_buf)\n"
    "        .or_else(|| load_config().projects_dir)\n"
    "        .unwrap_or_else(default_projects_dir);",
)

# ---- 3. tests de la config ----
s = s.replace(
    "    #[test]\n    fn la_carpeta_por_defecto_es_la_de_fl() {",
    '''    #[test]
    fn la_config_por_defecto_esta_vacia() {
        // No se comprueba el valor (depende del entorno), solo que no rompe.
        let _ = load_config();
    }

    #[test]
    fn set_template_rechaza_una_ruta_inexistente() {
        let err = set_template(Path::new("Z:\\\\no\\\\existe.flp")).unwrap_err();
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
    fn la_carpeta_por_defecto_es_la_de_fl() {''',
)

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("newproj.rs: config persistente anadida")
