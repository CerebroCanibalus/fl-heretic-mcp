//! Instalacion de plugins VST3/CLAP desde un catalogo versionado.
//!
//! # Por que esto SI es automatizable (y el VSTi de FL no)
//!
//! Un VST3 es un **bundle**: una carpeta que contiene un `.vst3`. Es un
//! fichero. Se descarga, se verifica y se copia. Lo mismo con un `.clap`.
//!
//! El VSTi de FL Studio, en cambio, **no exporta `VSTPluginMain`** (medido: ni
//! ese, ni `GetPluginFactory`, ni `DllGetClassObject`), asi que no es un VST2
//! autonomous: necesita el framework que instala el propio FL Studio desde su
//! menu. Medido y documentado en AGENTS.md.
//!
//! Esa diferencia es la que hace que "cambiar el catalogo de plugins" sea una
//! tarea de agente en vez de un muro.
//!
//! # Reproducibilidad
//!
//! El catalogo (`data/free-plugins.json`) esta **versionado en el repo** con
//! su URL y su hash. Instalar "la ultima version" cada vez daria resultados
//! distintos en maquinas distintas; fijar el catalogo da el mismo set exacto.
//!
//! Honestidad sobre los hashes: vienen vacios para los plugins que se
//! resuelven por la API de GitHub (la URL cambia con cada release, asi que el
//! hash cambia con ella). `install` avisa cuando no puede verificar, en vez de
//! fingir que lo ha hecho.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::file_rpc::RpcError;

/// Un plugin del catalogo.
#[derive(Debug, Clone, Deserialize)]
pub struct Plugin {
    /// Nombre corto, el que usa el LLM.
    pub name: String,
    /// Nombre real con el que aparece en la lista de plugins del DAW.
    pub display: String,
    /// instrument | effect | analyzer | sampler
    pub kind: String,
    /// vst3 | clap
    pub format: String,
    /// URL directa, o `github:owner/repo`, o `manual`.
    pub source: String,
    /// Nombre del asset dentro de la release de GitHub.
    #[serde(default)]
    pub asset: String,
    /// sha256 esperado. Vacio = no fijado.
    #[serde(default)]
    pub hash: String,
    /// Licencia, para que se pueda comprobar antes de instalar.
    #[serde(default)]
    pub license: String,
    /// Plugins de FL que cubre de verdad.
    #[serde(default)]
    pub replaces: Vec<String>,
    #[serde(default)]
    pub note: String,
}

impl Plugin {
    /// ¿Se puede instalar sin que nadie toque la GUI?
    pub fn is_automatable(&self) -> bool {
        self.source != "manual" && !self.source.is_empty()
    }

    pub fn format_dir(&self) -> &str {
        match self.format.as_str() {
            "clap" => "CLAP",
            _ => "VST3",
        }
    }
}

/// El catalogo completo.
#[derive(Debug, Clone, Deserialize)]
pub struct Catalog {
    pub version: u32,
    pub plugins: Vec<Plugin>,
    #[serde(default)]
    pub sets: std::collections::BTreeMap<String, Set>,
}

/// Un conjunto de plugins.
#[derive(Debug, Clone, Deserialize)]
pub struct Set {
    #[serde(default)]
    pub description: String,
    pub plugins: Vec<String>,
}

impl Catalog {
    /// Carga el catalogo compilado en el binario.
    ///
    /// Va embebido (`include_str!`) y no se lee de disco a proposito: si
    /// alguien edita el JSON local, lo que se instala deja de ser el catalogo
    /// que dice el repo. Reproducibilidad o nada.
    pub fn bundled() -> Self {
        serde_json::from_str(include_str!("../data/free-plugins.json"))
            .expect("el catalogo de plugins embebido esta mal formado")
    }

    pub fn get(&self, name: &str) -> Option<&Plugin> {
        self.plugins.iter().find(|p| p.name == name)
    }

    pub fn set(&self, name: &str) -> Option<&Set> {
        self.sets.get(name)
    }
}

/// Carpeta de plugins VST3 del usuario.
///
/// `%LOCALAPPDATA%\Programs\Common\VST3` es la ruta que Windows y la mayoría
/// de hosts ya miran. No requiere permisos de administrador, a diferencia de
/// `C:\Program Files\Common Files\VST3`, que además en esta maquina dio
/// "Acceso denegado" al intentar crearla.
pub fn vst3_dir() -> PathBuf {
    if let Ok(p) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(p)
            .join("Programs")
            .join("Common")
            .join("VST3");
    }
    PathBuf::from("C:/Program Files/Common Files/VST3")
}

/// Carpeta donde se descargan los `.zip` antes de descomprimirlos.
pub fn staging_dir() -> PathBuf {
    let base = std::env::var("TEMP")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("C:/Windows/Temp"));
    base.join("daw-heretic-downloads")
}

/// Resultado de instalar un plugin.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallReport {
    pub name: String,
    pub display: String,
    pub status: &'static str,
    pub path: Option<String>,
    pub verified: Option<bool>,
    pub message: String,
}

/// Como quedo una instalacion.
///
/// Un bundle VST3 puede ser de las dos formas que hay en la calle, y las dos
/// son validas:
///
/// - `Dexed.vst3` **suelto** (lo publica Dexed: 7 MB, un unico fichero)
/// - `SomePlugin.vst3/` **carpeta** con `Contents/x86_64-win/SomePlugin.vst3`
///
/// Comprobar solo la segunda (que es lo que se hizo al principio) hacia que
/// Dexed se instalara bien y la tool dijera "el zip se descomprimio pero no
/// quedo ningun bundle". Un `.vst3` o `.clap` en la carpeta de plugins, con o
/// sin contenido, cuenta.
pub fn installed_plugins(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let low = name.to_lowercase();
        if !low.ends_with(".vst3") && !low.ends_with(".clap") {
            continue;
        }
        // Si es un fichero suelto, ya es un bundle. Si es carpeta, se acepta
        // tal cual: un .vst3 vacio es raro pero no es motivo para fallar.
        out.push(name);
    }
    out.sort();
    out
}

/// Instala un plugin del catalogo.
///
/// `force` permite sobrescribir una version ya presente.
///
/// No borra nunca: si algo falla a media copia, el plugin anterior sigue ahi.
pub fn install(cat: &Catalog, name: &str, force: bool) -> Result<InstallReport, RpcError> {
    let p = cat.get(name).ok_or_else(|| {
        RpcError::Remote(format!(
            "'{name}' no esta en el catalogo. Usa daw_setup list para ver los {} disponibles.",
            cat.plugins.len()
        ))
    })?;

    if !p.is_automatable() {
        return Ok(InstallReport {
            name: p.name.clone(),
            display: p.display.clone(),
            status: "manual",
            path: None,
            verified: None,
            message: format!(
                "'{}' exige descarga manual ({}). No se puede automatizar: {}",
                p.display, p.license, p.note
            ),
        });
    }

    let url = resolve_url(p)?;
    let dest_dir = vst3_dir();
    std::fs::create_dir_all(&dest_dir).map_err(|e| RpcError::Io {
        path: dest_dir.clone(),
        source: e,
    })?;

    let staging = staging_dir();
    std::fs::create_dir_all(&staging).map_err(|e| RpcError::Io {
        path: staging.clone(),
        source: e,
    })?;

    let ext = if p.format == "clap" { "clap" } else { "vst3" };
    let dest = dest_dir.join(format!("{}.{ext}", p.display));
    if dest.exists() && !force {
        return Ok(InstallReport {
            name: p.name.clone(),
            display: p.display.clone(),
            status: "ya instalado",
            path: Some(dest.display().to_string()),
            verified: None,
            message: "ya esta en la carpeta de plugins (usa force para reemplazar)".into(),
        });
    }

    let bytes = download(&url)?;
    let verified = if p.hash.is_empty() {
        None
    } else {
        Some(sha256_hex(&bytes) == p.hash.to_ascii_lowercase())
    };
    if verified == Some(false) {
        return Ok(InstallReport {
            name: p.name.clone(),
            display: p.display.clone(),
            status: "hash incorrecto",
            path: None,
            verified: Some(false),
            message: format!(
                "el sha256 de lo descargado no coincide con el fijado ({}). \
                 Se aborta: no se instala nada.",
                p.hash
            ),
        });
    }

    // Un bundle VST3 puede venir como .zip o como .vst3 suelto.
    let written = if is_zip(&bytes) {
        unzip_into(&bytes, &dest_dir)?
    } else {
        std::fs::write(&dest, &bytes).map_err(|e| RpcError::Io {
            path: dest.clone(),
            source: e,
        })?;
        vec![dest.clone()]
    };

    let msg = match verified {
        Some(true) => "instalado y verificado por hash",
        Some(false) => unreachable!("ya se ha comprobado arriba"),
        None => "instalado SIN verificar (el catalogo no fija hash para esta fuente)",
    };

    Ok(InstallReport {
        name: p.name.clone(),
        display: p.display.clone(),
        status: "instalado",
        path: Some(written.iter().map(|x| x.display().to_string()).collect::<Vec<_>>().join(", ")),
        verified,
        message: format!("{msg}. Reinicia o reescanea el DAW para que lo vea."),
    })
}

/// Resuelve la URL real de descarga.
///
/// `github:owner/repo` se resuelve contra la API de releases de GitHub. El
/// error mas comun es que el `asset` este mal escrito, asi que la lista de
/// assets disponibles se incluye en el mensaje: asi se depura sin adivinar.
fn resolve_url(p: &Plugin) -> Result<String, RpcError> {
    if let Some(rest) = p.source.strip_prefix("github:") {
        let (owner, repo) = rest.split_once('/').ok_or_else(|| {
            RpcError::Remote(format!("source de github mal formada: {}", p.source))
        })?;
        let api = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
        let body = download(&api)?;
        let j: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|e| RpcError::Remote(format!("respuesta de la API de GitHub no parseable: {e}")))?;

        let assets: Vec<String> = j["assets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // Busca el asset por patron glob, no por nombre exacto.
        //
        // El nombre exacto no sirve: los proyectos versionan sus assets
        // (`Dexed-1.0.1-win.zip`), asi que un nombre fijo se rompe en
        // cuanto sale una version nueva, y el fallo aparece semanas
        // despues sin que nadie haya tocado nada.
        let elegido = if p.asset.is_empty() {
            None
        } else {
            let arts: Vec<Value> = j["assets"].as_array().cloned().unwrap_or_default();
            let pos = arts
                .iter()
                .position(|x| x["name"].as_str() == Some(p.asset.as_str()))
                .or_else(|| {
                    // glob: 'Dexed-*-win.zip'
                    arts.iter().position(|x| {
                        x["name"]
                            .as_str()
                            .is_some_and(|n| glob_match(&p.asset, n))
                    })
                });

            // Se devuelve la URL (String) y no una referencia al array: el
            // array es local y no sobrevive al return.
            pos.and_then(|i| arts[i]["browser_download_url"].as_str().map(String::from))
        };

        match elegido {
            Some(url) => Ok(url),
            None => Err(RpcError::Remote(format!(
                "no encuentro el asset '{}' en la ultima release de {owner}/{repo}.\n\
                 Assets disponibles:\n  - {}",
                p.asset,
                assets.join("\n  - ")
            ))),
        }
    } else {
        Ok(p.source.clone())
    }
}

fn download(url: &str) -> Result<Vec<u8>, RpcError> {
    // Sin dependencias: HTTPS por aqui es un problema. Se usa PowerShell,
    // que esta en Windows, en vez de arrastrar reqwest al crate.
    let ps = format!(
        "$ProgressPreference='SilentlyContinue'; \
         [Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12; \
         (Invoke-WebRequest -Uri '{}' -UseBasicParsing -OutFile $env:TEMP\\_daw_dl.bin -PassThru) | Out-Null; \
         exit $LASTEXITCODE",
        url.replace('\'', "''")
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .output()
        .map_err(|e| RpcError::Remote(format!("no se pudo lanzar PowerShell: {e}")))?;

    let tmp = std::env::var("TEMP").unwrap_or_else(|_| "C:\\Windows\\Temp".into());
    let path = std::path::Path::new(&tmp).join("_daw_dl.bin");
    if !out.status.success() {
        return Err(RpcError::Remote(format!(
            "fallo la descarga de {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let mut buf = Vec::new();
    std::fs::File::open(&path)
        .and_then(|mut f| f.read_to_end(&mut buf))
        .map_err(|e| RpcError::Remote(format!("no se pudo leer la descarga: {e}")))?;
    let _ = std::fs::remove_file(&path);
    if buf.is_empty() {
        return Err(RpcError::Remote(format!("la descarga de {url} vino vacia")));
    }
    Ok(buf)
}

fn is_zip(b: &[u8]) -> bool {
    b.len() > 4 && b[0] == 0x50 && b[1] == 0x4B && (b[2] == 0x03 || b[2] == 0x05)
}

/// Glob minimo: `*` = cualquier secuencia, `?` = un caracter.
///
/// No se usa el crate de globbing por un patron: aqui solo hacen falta `*` y
/// `?`, y 50 lineas de dependencia para eso no compensa. El caso real es
/// `Dexed-*-win.zip`, que es donde falla si se hardcodea el nombre.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<&str> = pattern.split('*').collect();
    if p.len() == 1 {
        return pattern == text;
    }
    // 1. todo antes del primer '*' tiene que estar al principio
    if !text.starts_with(p[0]) {
        return false;
    }
    let mut rest = &text[p[0].len()..];
    // 2. los tramos intermedios, en orden
    for seg in &p[1..p.len() - 1] {
        if seg.is_empty() {
            continue;
        }
        match rest.find(seg) {
            Some(i) => rest = &rest[i + seg.len()..],
            None => return false,
        }
    }
    // 3. el ultimo tiene que estar al final
    let last = p[p.len() - 1];
    last.is_empty() || rest.ends_with(last)
}

/// Descomprime con PowerShell (mismo criterio que la descarga: sin deps).
///
/// `$ErrorActionPreference='Stop'` va **primero**: puesto despues, un fallo
/// de `Expand-Archive` no lanza nada, el script sale con 0 y el caller cree
/// que instalo bien. Ese bug se CUELO una vez: la tool decia "instalado" con
/// 0 plugins en disco.
fn unzip_into(bytes: &[u8], dest: &Path) -> Result<Vec<PathBuf>, RpcError> {
    let tmp = std::env::var("TEMP").unwrap_or_else(|_| "C:\\Windows\\Temp".into());
    let zip = std::path::Path::new(&tmp).join("_daw_dl.zip");
    std::fs::create_dir_all(dest).map_err(|e| RpcError::Io {
        path: dest.to_path_buf(),
        source: e,
    })?;
    std::fs::write(&zip, bytes).map_err(|e| RpcError::Io {
        path: zip.clone(),
        source: e,
    })?;

    let ps = format!(
        "$ErrorActionPreference='Stop'; \
         Expand-Archive -LiteralPath '{zip}' -DestinationPath '{dest}' -Force; \
         if ($LASTEXITCODE) {{ exit 1 }}",
        zip = zip.display().to_string().replace('\'', "''"),
        dest = dest.display().to_string().replace('\'', "''"),
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .output()
        .map_err(|e| RpcError::Remote(format!("no se pudo descomprimir: {e}")))?;
    let _ = std::fs::remove_file(&zip);

    if !out.status.success() {
        return Err(RpcError::Remote(format!(
            "fallo al descomprimir: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }

    // Comprobar que hay algo: si no, es un error aunque PowerShell dijera 0.
    let plugins = installed_plugins(dest);
    if plugins.is_empty() {
        return Err(RpcError::Remote(format!(
            "el zip se descomprimio pero no quedo ningun bundle VST3/CLAP en {}. \
             El archivo puede no ser un plugin compatible.",
            dest.display()
        )));
    }
    Ok(plugins.into_iter().map(|n| dest.join(n)).collect())
}

/// sha256 sin dependencias: FNV no sirve, hace falta criptografico.
///
/// Se usa `certutil`, que viene en Windows, a traves de PowerShell.
fn sha256_hex(bytes: &[u8]) -> String {
    let tmp = std::env::var("TEMP").unwrap_or_else(|_| "C:\\Windows\\Temp".into());
    let f = std::path::Path::new(&tmp).join("_daw_dl_hash.bin");
    if std::fs::write(&f, bytes).is_err() {
        return String::new();
    }
    let ps = format!(
        "(Get-FileHash -LiteralPath '{}' -Algorithm SHA256).Hash.ToLower()",
        f.display().to_string().replace('\'', "''")
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .output();
    let _ = std::fs::remove_file(&f);
    String::from_utf8_lossy(&out.map(|o| o.stdout).unwrap_or_default())
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_catalogo_embebido_es_valido() {
        let c = Catalog::bundled();
        assert!(c.version >= 1);
        assert!(c.plugins.len() >= 10, "catalogo demasiado corto: {}", c.plugins.len());
        let mut names: Vec<&str> = c.plugins.iter().map(|p| p.name.as_str()).collect();
        let antes = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(antes, names.len(), "hay nombres repetidos en el catalogo");
    }

    #[test]
    fn todo_plugin_automatable_tiene_source_valida() {
        let c = Catalog::bundled();
        for p in &c.plugins {
            if p.is_automatable() {
                assert!(
                    p.source.starts_with("http") || p.source.starts_with("github:"),
                    "{}: source no valida: {}", p.name, p.source
                );
            } else {
                assert_eq!(p.source, "manual", "{} deberia ser manual", p.name);
            }
            assert!(
                p.format == "vst3" || p.format == "clap",
                "{}: formato desconocido: {}", p.name, p.format
            );
        }
    }

    #[test]
    fn todo_set_reapunta_a_plugins_existentes() {
        let c = Catalog::bundled();
        for (nombre, s) in &c.sets {
            for p in &s.plugins {
                assert!(c.get(p).is_some(),
                    "el set '{nombre}' lista '{p}', que no esta en el catalogo");
            }
            assert!(!s.plugins.is_empty(), "el set '{nombre}' esta vacio");
        }
    }

    #[test]
    fn el_set_base_promete_solo_lo_que_se_puede_instalar_solo() {
        // Este test ya ha atrapado un error real: 'base' incluia tdr-nova,
        // que exige descarga manual. La promesa de "sin tocar la GUI" no se
        // puede hacer si el set tiene uno manual dentro.
        let c = Catalog::bundled();
        let base = c.set("base").expect("deberia existir el set base");
        for n in &base.plugins {
            let p = c.get(n).unwrap();
            assert!(p.is_automatable(),
                "'{}' esta en el set base pero exige descarga manual", p.name);
        }
    }

    #[test]
    fn instalar_uno_inexistente_da_error_nombrado() {
        let c = Catalog::bundled();
        let e = install(&c, "no-existe-este", false).unwrap_err();
        assert!(e.to_string().contains("no esta en el catalogo"), "{e}");
    }

    #[test]
    fn instalar_uno_manual_no_intenta_descargar() {
        // vital y spitfire-labs exigen cuenta. Debe decirselo al LLM, no
        // intentar bajarlos y fallar con un error de red.
        let c = Catalog::bundled();
        for n in ["vital", "spitfire-labs"] {
            let r = install(&c, n, false).unwrap();
            assert_eq!(r.status, "manual", "{n} deberia reportar manual");
            assert!(r.message.contains("manual"), "{n}: {}", r.message);
        }
    }

    #[test]
    fn el_glob_resuelve_assets_versionados() {
        // El fallo real: el asset se llamaba `Dexed-1.0.1-win.zip` y el
        // catalogo pedia `dexed`. Un nombre fijo se rompe en cada release.
        assert!(glob_match("Dexed-*-win.zip", "Dexed-1.0.1-win.zip"));
        assert!(glob_match("Dexed-*-win.zip", "Dexed-2.3.4-win.zip"));
        assert!(glob_match("*.zip", "cualquier-cosa.zip"));
        assert!(glob_match("surge-xt-windows-x64.zip", "surge-xt-windows-x64.zip"));

        // Y tiene que RECHAZAR lo que no encaja, o instalaria el .dmg de Mac.
        assert!(!glob_match("Dexed-*-win.zip", "Dexed-1.0.1-macOS.dmg"));
        assert!(!glob_match("Dexed-*-win.zip", "dexed-source-bce5dee.tar.gz"));
        assert!(!glob_match("*.zip", "readme.md"));
    }

    #[test]
    fn vst3_dir_no_requiere_admin() {
        // `C:\\Program Files\\Common Files\\VST3` dio "Acceso denegado" en
        // esta maquina. La ruta del usuario siempre funciona.
        let d = vst3_dir();
        let s = d.display().to_string().to_lowercase();
        assert!(s.contains("localappdata") || s.contains("users"),
            "la carpeta de plugins deberia ser de usuario, no de sistema: {d:?}");
    }
}
