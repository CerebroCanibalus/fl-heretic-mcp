//! `fl-heretic doctor` — diagnóstico del entorno.

use heretic_core::{AuditLog, Result, TokenStore};

pub fn run() -> Result<()> {
    println!("FL Heretic MCP — doctor\n");

    // 1. Token
    let token_path = TokenStore::default_path();
    print_check("Token path", &format!("{}", token_path.display()));
    if token_path.exists() {
        match TokenStore::new(token_path.clone()).load() {
            Ok(t) => print_ok(&format!("cargado ({} chars)", t.as_str().len())),
            Err(e) => print_err(&e.to_string()),
        }
    } else {
        print_warn("no existe — ejecutar `fl-heretic token generate`");
    }

    // 2. Audit log
    let audit_path = AuditLog::default_path();
    print_check("Audit log path", &format!("{}", audit_path.display()));
    if audit_path.exists() {
        match AuditLog::open(audit_path) {
            Ok(log) => print_ok(&format!("abierto ({} eventos)", log.len().unwrap_or(0))),
            Err(e) => print_err(&e.to_string()),
        }
    } else {
        print_warn("no existe — se creará al primer arranque del daemon");
    }

    // 3. OS / platform
    print_check(
        "Platform",
        &format!("{} {} ({})", std::env::consts::OS, std::env::consts::ARCH, std::env::consts::FAMILY),
    );

    // 4. Working dir
    if let Ok(cwd) = std::env::current_dir() {
        print_check("Working dir", &format!("{}", cwd.display()));
    }

    // 5. Protocol version
    print_check("Protocol version", &format!("v{}", heretic_core::PROTOCOL_VERSION));

    println!("\nListo. Para arrancar el daemon: `fl-heretic daemon`");
    Ok(())
}

fn print_check(label: &str, value: &str) {
    println!("  [ ] {:<22} {}", label, value);
}

fn print_ok(msg: &str) {
    println!("        \u{2713} OK: {}", msg);
}

fn print_warn(msg: &str) {
    println!("        ! WARN: {}", msg);
}

fn print_err(msg: &str) {
    println!("        \u{2717} ERROR: {}", msg);
}