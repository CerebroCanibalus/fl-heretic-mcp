//! `fl-heretic token <action>` — gestión del token Bearer.

use heretic_core::{Result, Token, TokenStore};

use super::TokenCmd;

pub fn run(action: TokenCmd) -> Result<()> {
    let path = TokenStore::default_path();
    let store = TokenStore::new(path.clone());
    match action {
        TokenCmd::Generate => {
            let token = Token::generate();
            store.save(&token)?;
            println!("Token nuevo generado y guardado.");
            println!("Path: {}", path.display());
            println!("Valor: {}", token);
            println!("\nEste token es el único que puede hablar con el daemon.");
            println!("Guárdalo en lugar seguro (Keychain, 1Password, etc.).");
        }
        TokenCmd::Rotate => {
            if path.exists() {
                if let Ok(old) = store.load() {
                    let backup_path = backup_path(&path);
                    std::fs::write(&backup_path, old.as_str())?;
                    println!("Token anterior respaldado en: {}", backup_path.display());
                }
            }
            let token = Token::generate();
            store.save(&token)?;
            println!("Token rotado.");
            println!("Nuevo valor: {}", token);
        }
        TokenCmd::Show => {
            let token = store.load()?;
            println!("{}", token);
        }
        TokenCmd::Path => {
            println!("{}", path.display());
        }
    }
    Ok(())
}

fn backup_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut p = path.to_path_buf();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    p.set_extension(format!("backup.{}", ts));
    p
}