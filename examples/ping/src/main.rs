//! `ping` — cliente mínimo que conecta al daemon FL Heretic.
//!
//! Lee el token de `%LOCALAPPDATA%\fl-heretic\token`, conecta al Named Pipe
//! `\\.\pipe\fl-heretic-<pid>`, hace el handshake HMAC, ejecuta `ping`,
//! e imprime la respuesta.
//!
//! ## Uso
//!
//! ```powershell
//! # Terminal 1: arrancar daemon
//! fl-heretic daemon
//!
//! # Terminal 2: ejecutar cliente
//! cargo run -p ping
//! ```
//!
//! Si el daemon está en otro PID, puedes override:
//! `cargo run -p ping -- --pid 12345`

use std::process::ExitCode;

use heretic_core::{
    AuthChallenge, AuthVerifier, Request, Response, Token, TokenStore,
};
use serde_json::json;

#[cfg(windows)]
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(windows)]
use tokio::net::windows::named_pipe::ClientOptions;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let pid_override = parse_pid_flag(&args);

    match run_client(pid_override).await {
        Ok(response_json) => {
            println!("\n[OK] daemon respondió:");
            println!("{}", serde_json::to_string_pretty(&response_json).unwrap());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("[ERR] cliente: {e}");
            ExitCode::from(1)
        }
    }
}

fn parse_pid_flag(args: &[String]) -> Option<u32> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--pid" && i + 1 < args.len() {
            return args[i + 1].parse().ok();
        }
        i += 1;
    }
    None
}

#[cfg(windows)]
async fn run_client(pid_override: Option<u32>) -> anyhow::Result<serde_json::Value> {
    // 1. Cargar token
    let token_path = TokenStore::default_path();
    let token = Token::load_or_create_static(&token_path)?;
    let verifier = AuthVerifier::new(token.clone());

    // 2. Construir pipe name
    let pipe_name = match pid_override {
        Some(pid) => format!(r"\\.\pipe\fl-heretic-{pid}"),
        None => {
            // Buscar cualquier daemon corriendo (enumerar pipes es costoso,
            // pero podemos probar el PID actual primero, y si falla probar otros)
            // Para simplicidad: probar PID actual, luego dejar al usuario usar --pid
            let my_pid = std::process::id();
            format!(r"\\.\pipe\fl-heretic-{my_pid}")
        }
    };
    println!("Conectando a Named Pipe: {pipe_name}");

    // 3. Conectar
    let client = ClientOptions::new()
        .open(&pipe_name)
        .map_err(|e| anyhow::anyhow!("No se pudo conectar al daemon. ¿Está corriendo? ({e})"))?;
    println!("Conectado.");

    let (read_half, mut write_half) = tokio::io::split(client);
    let mut reader = BufReader::new(read_half);

    // 4. Leer AuthChallenge
    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        anyhow::bail!("server cerró antes de mandar challenge");
    }
    let challenge_envelope: serde_json::Value = serde_json::from_str(line.trim())?;
    let challenge: AuthChallenge = serde_json::from_value(
        challenge_envelope.get("data")
            .ok_or_else(|| anyhow::anyhow!("missing challenge data"))?
            .clone(),
    )?;
    println!("Challenge recibido (nonce {}..., ts={})",
        &challenge.nonce[..8], challenge.timestamp);

    // 5. Firmar y enviar AuthResponse
    let response = verifier.sign(&challenge);
    let resp_envelope = json!({ "type": "auth_response", "data": &response });
    let resp_json = serde_json::to_string(&resp_envelope)? + "\n";
    write_half.write_all(resp_json.as_bytes()).await?;
    write_half.flush().await?;

    // 6. Leer AuthAck
    line.clear();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        anyhow::bail!("server cerró después de auth");
    }
    let ack: serde_json::Value = serde_json::from_str(line.trim())?;
    if ack.get("ok") != Some(&serde_json::Value::Bool(true)) {
        anyhow::bail!("auth rechazada: {ack}");
    }
    println!("Auth OK (protocol v{})", ack.get("protocol_version").unwrap_or(&json!(null)));

    // 7. Enviar `ping` request
    let req = Request::new(
        "req-1",
        "ping",
        json!({}),
    );
    let req_json = serde_json::to_string(&req)? + "\n";
    write_half.write_all(req_json.as_bytes()).await?;
    write_half.flush().await?;
    println!("Request enviado: ping");

    // 8. Leer response
    line.clear();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        anyhow::bail!("server cerró después del request");
    }
    let resp: Response = serde_json::from_str(line.trim())?;
    match &resp.outcome {
        heretic_core::protocol::Outcome::Success { result } => Ok(result.clone()),
        heretic_core::protocol::Outcome::Error { error } => {
            anyhow::bail!("server error [{}]: {}", error.code, error.message)
        }
    }
}

#[cfg(not(windows))]
async fn run_client(_pid_override: Option<u32>) -> anyhow::Result<serde_json::Value> {
    anyhow::bail!("cliente solo soporta Windows (Named Pipes)")
}

// helper: load_or_create sin pasar por TokenStore (mantiene la API mínima)
trait TokenExt {
    fn load_or_create_static(path: &std::path::Path) -> anyhow::Result<Token>;
}

impl TokenExt for Token {
    fn load_or_create_static(path: &std::path::Path) -> anyhow::Result<Token> {
        let store = TokenStore::new(path);
        Ok(store.load_or_create()?)
    }
}