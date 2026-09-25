//! Auth HMAC-SHA256 Bearer para el daemon blindado.
//!
//! - `Token` — token opaco generado al `init` (32 bytes random, base64).
//! - `TokenStore` — persiste el token en disco con permisos usuario-only.
//! - `AuthChallenge` — nonce que el servidor manda al cliente.
//! - `AuthVerifier` — verifica que un request firmado por el cliente sea legítimo.
//!
//! ## Protocolo de auth
//!
//! 1. Cliente conecta al Named Pipe.
//! 2. Servidor envía `AuthChallenge { nonce, timestamp }`.
//! 3. Cliente responde con `AuthResponse { token, signature }` donde
//!    `signature = HMAC-SHA256(token, nonce || timestamp || method || params_json)`.
//! 4. Servidor verifica firma, marca la conexión como autenticada.
//! 5. Cada request posterior lleva `Authorization: Bearer <token>` + firma del payload.
//!
//! La comparación de firmas usa `subtle::ConstantTimeEq` para evitar timing attacks.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::error::{HereticError, Result};

type HmacSha256 = Hmac<Sha256>;

/// Token opaco (32 bytes random → base64 url-safe).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    /// Genera un token nuevo de 32 bytes random.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        Self(encoded)
    }

    /// Parsea un token existente (valida formato).
    pub fn from_string(s: impl Into<String>) -> Result<Self> {
        let s: String = s.into();
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&s)
            .map_err(|_| HereticError::Token("token no es base64 url-safe válido".into()))?;
        if bytes.len() != 32 {
            return Err(HereticError::Token(format!(
                "token debe ser 32 bytes, tiene {}",
                bytes.len()
            )));
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Almacén persistente del token en disco.
///
/// Path por defecto: `%LOCALAPPDATA%\fl-heretic\token` (Windows).
/// Permisos: solo el usuario actual puede leer/escribir (0600 en Unix).
pub struct TokenStore {
    path: PathBuf,
}

impl TokenStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Path por defecto del token según OS.
    pub fn default_path() -> PathBuf {
        if cfg!(windows) {
            std::env::var("LOCALAPPDATA")
                .map(|p| PathBuf::from(p).join("fl-heretic").join("token"))
                .unwrap_or_else(|_| PathBuf::from("fl-heretic-token"))
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".fl-heretic").join("token")
        }
    }

    /// Lee el token del disco. Si no existe, genera uno nuevo y lo persiste.
    pub fn load_or_create(&self) -> Result<Token> {
        if self.path.exists() {
            self.load()
        } else {
            let token = Token::generate();
            self.save(&token)?;
            Ok(token)
        }
    }

    /// Lee el token existente (no crea uno nuevo).
    pub fn load(&self) -> Result<Token> {
        let s = std::fs::read_to_string(&self.path).map_err(|e| {
            HereticError::Token(format!("leyendo token en {}: {}", self.path.display(), e))
        })?;
        let s = s.trim();
        Token::from_string(s)
    }

    /// Persiste el token en disco con permisos restrictivos.
    pub fn save(&self, token: &Token) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                HereticError::Token(format!("creando dir {}: {}", parent.display(), e))
            })?;
        }
        std::fs::write(&self.path, token.as_str()).map_err(|e| {
            HereticError::Token(format!("escribiendo token en {}: {}", self.path.display(), e))
        })?;
        self.set_user_only_permissions(&self.path)?;
        Ok(())
    }

    /// En Unix: chmod 0600. En Windows: ACL solo usuario actual.
    #[cfg_attr(windows, allow(unused_variables))]
    fn set_user_only_permissions(&self, path: &Path) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(path, perms)?;
        }
        #[cfg(windows)]
        {
            // En Windows el ACL default ya es solo usuario actual
            // si el dir está bajo %LOCALAPPDATA%. No hacemos nada.
        }
        Ok(())
    }
}

/// Challenge que el servidor envía al cliente al conectar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthChallenge {
    /// Nonce random (32 bytes hex) que el cliente debe firmar.
    pub nonce: String,
    /// Timestamp unix (segundos) — el cliente debe responder dentro de una ventana.
    pub timestamp: u64,
    /// Versión del protocolo.
    pub protocol_version: u32,
}

impl AuthChallenge {
    pub fn new() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let nonce = hex::encode(bytes);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            nonce,
            timestamp,
            protocol_version: crate::PROTOCOL_VERSION,
        }
    }
}

/// Response del cliente al challenge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    /// Firma HMAC-SHA256(token, nonce || timestamp || "AUTH_HANDSHAKE").
    pub signature: String,
}

/// Verificador de firmas HMAC.
#[derive(Debug, Clone)]
pub struct AuthVerifier {
    token: Token,
    /// Ventana de tolerancia para timestamps (segundos).
    pub timestamp_skew_seconds: u64,
}

impl AuthVerifier {
    pub fn new(token: Token) -> Self {
        Self {
            token,
            timestamp_skew_seconds: 60,
        }
    }

    /// Firma un mensaje (challenge → response).
    pub fn sign(&self, challenge: &AuthChallenge) -> AuthResponse {
        let msg = format!(
            "{}|{}|AUTH_HANDSHAKE",
            challenge.nonce, challenge.timestamp
        );
        let signature = self.sign_bytes(msg.as_bytes());
        AuthResponse {
            token: self.token.as_str().to_string(),
            signature,
        }
    }

    /// Verifica un response contra el challenge. Constant-time comparison.
    pub fn verify_handshake(&self, challenge: &AuthChallenge, response: &AuthResponse) -> Result<()> {
        // 1. Token matchea
        if response.token.as_bytes().ct_eq(self.token.as_str().as_bytes()).unwrap_u8() == 0 {
            return Err(HereticError::AuthFailed("token inválido".into()));
        }
        // 2. Timestamp dentro de la ventana
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let drift = now.abs_diff(challenge.timestamp);
        if drift > self.timestamp_skew_seconds {
            return Err(HereticError::AuthFailed(format!(
                "timestamp drift {}s > {}s",
                drift, self.timestamp_skew_seconds
            )));
        }
        // 3. Firma verifica
        let expected = self.sign(challenge);
        let a = expected.signature.as_bytes();
        let b = response.signature.as_bytes();
        if a.ct_eq(b).unwrap_u8() == 0 {
            return Err(HereticError::AuthFailed("firma HMAC inválida".into()));
        }
        Ok(())
    }

    /// Firma un payload arbitrario (usado para autenticar cada request post-handshake).
    pub fn sign_payload(&self, method: &str, params_json: &str, timestamp: u64) -> String {
        let msg = format!("{}|{}|{}", method, timestamp, params_json);
        self.sign_bytes(msg.as_bytes())
    }

    /// Verifica un payload firmado.
    pub fn verify_payload(
        &self,
        method: &str,
        params_json: &str,
        timestamp: u64,
        signature_hex: &str,
    ) -> Result<()> {
        let drift = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            .abs_diff(timestamp);
        if drift > self.timestamp_skew_seconds {
            return Err(HereticError::AuthFailed(format!(
                "timestamp drift {}s > {}s",
                drift, self.timestamp_skew_seconds
            )));
        }
        let expected = self.sign_payload(method, params_json, timestamp);
        if expected.as_bytes().ct_eq(signature_hex.as_bytes()).unwrap_u8() == 0 {
            return Err(HereticError::AuthFailed("firma de payload inválida".into()));
        }
        Ok(())
    }

    fn sign_bytes(&self, msg: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(self.token.as_str().as_bytes())
            .expect("HMAC accepts any key length");
        mac.update(msg);
        let bytes = mac.finalize().into_bytes();
        hex::encode(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_roundtrip() {
        let t = Token::generate();
        let s = t.as_str().to_string();
        let parsed = Token::from_string(s).unwrap();
        assert_eq!(t, parsed);
    }

    #[test]
    fn token_rejects_garbage() {
        assert!(Token::from_string("not-base64").is_err());
        assert!(Token::from_string("aGVsbG8=").is_err()); // "hello" en base64 = 5 bytes, no 32
    }

    #[test]
    fn handshake_roundtrip() {
        let token = Token::generate();
        let verifier = AuthVerifier::new(token.clone());
        let challenge = AuthChallenge::new();
        let response = verifier.sign(&challenge);
        assert!(verifier.verify_handshake(&challenge, &response).is_ok());
    }

    #[test]
    fn handshake_rejects_wrong_token() {
        let token_a = Token::generate();
        let token_b = Token::generate();
        let verifier_a = AuthVerifier::new(token_a);
        let verifier_b = AuthVerifier::new(token_b);
        let challenge = AuthChallenge::new();
        let response = verifier_b.sign(&challenge); // firmado con B
        assert!(verifier_a.verify_handshake(&challenge, &response).is_err());
    }

    #[test]
    fn handshake_rejects_tampered_signature() {
        let token = Token::generate();
        let verifier = AuthVerifier::new(token);
        let challenge = AuthChallenge::new();
        let mut response = verifier.sign(&challenge);
        // Mutar un byte de la firma
        let mut bytes = hex::decode(&response.signature).unwrap();
        bytes[0] ^= 0xff;
        response.signature = hex::encode(bytes);
        assert!(verifier.verify_handshake(&challenge, &response).is_err());
    }

    #[test]
    fn payload_sign_verify() {
        let token = Token::generate();
        let verifier = AuthVerifier::new(token);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let sig = verifier.sign_payload("fl_ping", "{}", now);
        assert!(verifier.verify_payload("fl_ping", "{}", now, &sig).is_ok());
    }

    #[test]
    fn payload_rejects_tampered_params() {
        let token = Token::generate();
        let verifier = AuthVerifier::new(token);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let sig = verifier.sign_payload("fl_ping", "{\"a\":1}", now);
        // Cambiar params pero no la firma
        assert!(verifier.verify_payload("fl_ping", "{\"a\":2}", now, &sig).is_err());
    }

    #[test]
    fn token_store_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TokenStore::new(tmp.path().join("token"));
        let token = Token::generate();
        store.save(&token).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(token, loaded);
    }
}