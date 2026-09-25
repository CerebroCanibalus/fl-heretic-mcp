#!/usr/bin/env python3
"""Rompe la recursion mutua find_fl_exe <-> running_process."""
import io

P = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-fl\src\process.rs"
s = io.open(P, encoding="utf-8").read()

old = '''/// Localiza el ejecutable de FL Studio.
///
/// 1. `FL_HERETIC_FL_EXE` si esta definido.
/// 2. Si FL esta corriendo, la ruta de su propio ejecutable.
/// 3. Las rutas conocidas de `KNOWN_PATHS`.
pub fn find_fl_exe() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("FL_HERETIC_FL_EXE") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(p) = running_process().map(|p| p.exe_path) {
        if p.is_file() {
            return Some(p);
        }
    }
    for p in KNOWN_PATHS {
        let p = Path::new(p);
        if p.is_file() {
            return Some(p.to_path_buf());
        }
    }
    None
}

/// Devuelve el proceso de FL Studio en ejecucion, si lo hay.
pub fn running_process() -> Option<FlProcess> {
    #[cfg(windows)]
    {
        // Tasklist es lo mas simple y no necesita crate extra.
        let out = Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq FL64.exe", "/FO", "CSV", "/NH"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            // Formato: "FL64.exe","16612","Console","1","122,120 KB"
            // Al partir por '"' los campos caen en los indices impares:
            //   [1]=nombre [3]=pid [5]=session [7]=num [9]=memoria
            // (el separador coma se cuela en los pares, y el tamper de
            // memoria "122,120 KB" lleva una coma dentro)
            let cols: Vec<&str> = line.split('"').collect();
            if cols.len() < 4 {
                continue;
            }
            if let Ok(pid) = cols[3].trim().parse::<u32>() {
                return Some(FlProcess {
                    pid,
                    exe_path: find_fl_exe().unwrap_or_default(),
                    window_title: String::new(),
                    uptime_sec: 0,
                });
            }
        }
        None
    }'''

new = '''/// Localiza el ejecutable de FL Studio.
///
/// 1. `FL_HERETIC_FL_EXE` si esta definido.
/// 2. La ruta del proceso en ejecucion, via `fl_exe_from_process()`.
/// 3. Las rutas conocidas de `KNOWN_PATHS`.
///
/// OJO: aqui NO se llama a `running_process()`. Esa funcion necesita el exe
/// para rellenar el struct, y si se llamaran mutuamente habria recursion
/// infinita (stack overflow en runtime). Por eso el paso 2 usa una funcion
/// independiente que solo pregunta por el proceso.
pub fn find_fl_exe() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("FL_HERETIC_FL_EXE") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(exe) = fl_exe_from_process() {
        if exe.is_file() {
            return Some(exe);
        }
    }
    for p in KNOWN_PATHS {
        let p = Path::new(p);
        if p.is_file() {
            return Some(p.to_path_buf());
        }
    }
    None
}

/// Ruta del ejecutable de FL Studio a partir del proceso vivo.
///
/// Separada de `running_process()` precisamente para romper la recursion
/// mutua entre ambas.
#[cfg(windows)]
fn fl_exe_from_process() -> Option<PathBuf> {
    let pid = fl_pid()?;
    let out = Command::new("wmic")
        .args([
            "process",
            "where",
            &format!("ProcessId={pid}"),
            "get",
            "ExecutablePath",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines().skip(1) {
        let p = line.trim();
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    None
}

#[cfg(not(windows))]
fn fl_exe_from_process() -> Option<PathBuf> {
    None
}

/// PID de FL Studio, o `None` si no esta corriendo.
#[cfg(windows)]
pub fn fl_pid() -> Option<u32> {
    let out = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq FL64.exe", "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    parse_tasklist_pid(&String::from_utf8_lossy(&out.stdout))
}

/// Extrae el PID de la salida CSV de `tasklist`.
///
/// Formato: `"FL64.exe","16612","Console","1","122,120 KB"`. Al partir por
/// '"' los campos caen en los indices impares: `[1]` nombre, `[3]` pid,
/// `[5]` sesion, `[7]` num, `[9]` memoria. Se parte por comillas y no por
/// comas porque el separador se cuela en los indices pares y el tamper de
/// memoria ("122,120 KB") lleva su propia coma.
#[cfg(windows)]
fn parse_tasklist_pid(text: &str) -> Option<u32> {
    for line in text.lines() {
        let cols: Vec<&str> = line.split('"').collect();
        if cols.len() < 4 {
            continue;
        }
        if let Ok(pid) = cols[3].trim().parse::<u32>() {
            return Some(pid);
        }
    }
    None
}

/// Devuelve el proceso de FL Studio en ejecucion, si lo hay.
pub fn running_process() -> Option<FlProcess> {
    #[cfg(windows)]
    {
        fl_pid().map(|pid| FlProcess {
            pid,
            exe_path: fl_exe_from_process().unwrap_or_default(),
            window_title: String::new(),
            uptime_sec: 0,
        })
    }'''

assert old in s, "el bloque no coincide (¿encoding?)"
s = s.replace(old, new, 1)

# Arreglar el test de tasklist para usar el helper
s = s.replace(
    """        for (line, esperado) in casos {
            let cols: Vec<&str> = line.split('"').collect();
            let got = if cols.len() < 4 {
                None
            } else {
                cols[3].trim().parse::<u32>().ok()
            };
            assert_eq!(got, esperado, "linea: {line:?}");
        }""",
    """        for (line, esperado) in casos {
            assert_eq!(parse_tasklist_pid(line), esperado, "linea: {line:?}");
        }""",
)

io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("process.rs: recursion rota")
