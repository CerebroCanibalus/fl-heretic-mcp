//! Aisla el pool de named pipes: conecta N clientes a la vez y cuenta cuantos
//! aceptan handshake. Sirve para no atribuir al pool un fallo que sea del
//! cliente, o al reves.
use std::sync::Arc;
use std::time::Duration;

use heretic_core::{AuthChallenge, AuthVerifier, Request, Response, TokenStore};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[cfg(windows)]
use tokio::net::windows::named_pipe::ClientOptions;

#[cfg(windows)]
async fn one_call(pipe: String, token: heretic_core::Token, id: u32) -> Result<String, String> {
    let client = ClientOptions::new()
        .open(&pipe)
        .map_err(|e| format!("open: {e}"))?;
    let (r, mut w) = tokio::io::split(client);
    let mut rd = BufReader::new(r);
    let verifier = AuthVerifier::new(token);

    let mut line = String::new();
    rd.read_line(&mut line).await.map_err(|e| e.to_string())?;
    let ch: AuthChallenge = serde_json::from_value(
        serde_json::from_str::<Value>(line.trim())
            .map_err(|e| e.to_string())?
            .get("data")
            .cloned()
            .ok_or("sin data")?,
    )
    .map_err(|e| e.to_string())?;

    let resp = verifier.sign(&ch);
    let j = serde_json::to_string(&serde_json::json!({"type":"auth_response","data":&resp}))
        .map_err(|e| e.to_string())?
        + "\n";
    w.write_all(j.as_bytes()).await.map_err(|e| e.to_string())?;
    w.flush().await.map_err(|e| e.to_string())?;

    line.clear();
    rd.read_line(&mut line).await.map_err(|e| e.to_string())?;
    let ack: Value = serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
    if ack.get("ok") != Some(&Value::Bool(true)) {
        return Err(format!("auth rechazada: {ack}"));
    }

    let req = Request::new(format!("req-{id}"), "ping", serde_json::json!({}));
    let j = serde_json::to_string(&req).map_err(|e| e.to_string())? + "\n";
    w.write_all(j.as_bytes()).await.map_err(|e| e.to_string())?;
    w.flush().await.map_err(|e| e.to_string())?;

    line.clear();
    rd.read_line(&mut line).await.map_err(|e| e.to_string())?;
    let r: Response = serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
    Ok(match r.outcome {
        heretic_core::protocol::Outcome::Success { .. } => "ok".into(),
        heretic_core::protocol::Outcome::Error { error } => format!("error: {}", error.message),
    })
}

#[cfg(windows)]
#[tokio::main]
async fn main() {
    let pipe = std::env::args()
        .nth(1)
        .unwrap_or_else(|| r"\\.\pipe\fl-heretic-e2e".into());
    let n: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    let token = TokenStore::new(TokenStore::default_path())
        .load()
        .expect("token");

    println!("  {n} clientes CONCURRENTES contra {pipe}");
    let mut tasks = Vec::new();
    for i in 0..n {
        let p = pipe.clone();
        let t = token.clone();
        tasks.push(tokio::spawn(async move {
            match tokio::time::timeout(Duration::from_secs(20), one_call(p, t, i as u32)).await {
                Ok(r) => r,
                Err(_) => Err("TIMEOUT".to_string()),
            }
        }));
    }
    let mut ok = 0;
    for (i, t) in tasks.into_iter().enumerate() {
        match t.await {
            Ok(Ok(s)) => {
                println!("    cliente {i}: {s}");
                if s == "ok" {
                    ok += 1;
                }
            }
            Ok(Err(e)) => println!("    cliente {i}: FALLO {e}"),
            Err(e) => println!("    cliente {i}: panic {e}"),
        }
    }
    println!("\n  {ok}/{n}Dance succeeded");
    if ok != n {
        std::process::exit(1);
    }
    let _ = Arc::new(0);
}

#[cfg(not(windows))]
fn main() {
    println!("solo Windows");
}
