#!/usr/bin/env python3
"""Anade tests de regresión del parser de tasklist."""
import io

P = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-fl\src\process.rs"
s = io.open(P, encoding="utf-8").read()

anchor = """    #[test]
    fn running_process_is_optional() {"""
new = '''    /// Regresión: el parser de `tasklist` leía el PID de la columna
    /// equivocada (tomaba el nombre del proceso) y devolvía None aunque FL
    /// estuviera corriendo. Con esto no vuelve a pasar en silencio.
    #[cfg(windows)]
    #[test]
    fn el_parser_de_tasklist_saca_el_pid_de_la_columna_buena() {
        let casos: [(&str, Option<u32>); 4] = [
            ("\\"FL64.exe\\",\\"16612\\",\\"Console\\",\\"1\\",\\"122,120 KB\\"", Some(16612)),
            ("\\"FL64.exe\\",\\"4\\",\\"Services\\",\\"0\\",\\"1,024 KB\\"", Some(4)),
            ("", None),
            ("no hay tareas", None),
        ];
        for (line, esperado) in casos {
            let cols: Vec<&str> = line.split('"').collect();
            let got = if cols.len() < 4 {
                None
            } else {
                cols[3].trim().parse::<u32>().ok()
            };
            assert_eq!(got, esperado, "linea: {line:?}");
        }
    }

    /// Si FL esta corriendo, running_process() tiene que encontrarlo. Solo
    /// aplica en una maquina con FL instalado y abierto.
    #[cfg(windows)]
    #[test]
    fn detecta_fl_si_esta_corriendo() {
        let out = Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq FL64.exe", "/FO", "CSV", "/NH"])
            .output()
            .expect("tasklist");
        if String::from_utf8_lossy(&out.stdout).contains("FL64.exe") {
            let p = running_process();
            assert!(
                p.is_some(),
                "tasklist dice que FL corre pero running_process() no lo ve"
            );
        }
    }

    #[test]
    fn running_process_is_optional() {'''
assert anchor in s
s = s.replace(anchor, new, 1)
io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("tests de tasklist anadidos")
