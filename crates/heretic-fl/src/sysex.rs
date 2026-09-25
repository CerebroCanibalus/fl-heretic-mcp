//! SysEx wire format: encoding/decoding de mensajes MIDI entre el daemon y el controller script.
//!
//! Compatible 1:1 con `legacy/src/fl_studio_mcp/protocol.py` y
//! `legacy/fl_controller/FLStudioMCP/device_FLStudioMCP.py`.
//!
//! ## Wire format (bytes entre F0 y F7)
//!
//! ```text
//! [0]    0x7D                     Manufacturer ID (MIDI-spec private use)
//! [1..3] "MCP" (0x4D 0x43 0x50)   Magic, lets us ignore unrelated SysEx
//! [4]    direction                0x01 request, 0x02 response, 0x03 heartbeat
//! [5..12]  8 ASCII chars [a-z0-9] Request id, correlates response to request
//! [13..]    base64-encoded UTF-8 JSON payload
//! ```
//!
//! Por qué base64: sus chars son `[A-Za-z0-9+/=]`, todos < 128, así que entran
//! en data bytes SysEx (que deben ser < 128) sin re-encoding adicional.
//! Overhead 33% — aceptable porque nuestros payloads son pequeños (10B-1KB).
//! Si alguna vez necesitamos blobs grandes (audio, presets), cambiaremos a
//! 7-to-8 packing y bumpearemos versión.

use base64::Engine;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Versión del protocolo (debe matchear `PROTOCOL_VERSION` del controller script).
pub const PROTOCOL_VERSION: u32 = 2;

/// Manufacturer ID (private use per MIDI spec).
pub const SYSEX_MANUFACTURER: u8 = 0x7D;

/// Magic "MCP" para distinguir nuestros mensajes de otros SysEx ajenos.
pub const SYSEX_MAGIC: [u8; 3] = [0x4D, 0x43, 0x50]; // "MCP"

/// Dirección del mensaje.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Direction {
    Request = 0x01,
    Response = 0x02,
    Heartbeat = 0x03,
}

impl Direction {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Direction::Request),
            0x02 => Some(Direction::Response),
            0x03 => Some(Direction::Heartbeat),
            _ => None,
        }
    }
}

/// Longitud del request id en chars.
pub const REQUEST_ID_LEN: usize = 8;

/// Alfabeto del request id (mirror del legacy).
const REQUEST_ID_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// Genera un request id nuevo (8 chars del alfabeto legacy).
pub fn new_request_id() -> String {
    let mut rng = rand::thread_rng();
    let id: String = (0..REQUEST_ID_LEN)
        .map(|_| *REQUEST_ID_ALPHABET.choose(&mut rng).unwrap() as char)
        .collect();
    id
}

/// Codifica un mensaje a SysEx payload (sin F0/F7 framing — el caller lo añade).
///
/// `payload` es el dict JSON (request, response, o heartbeat).
pub fn encode_message(direction: Direction, request_id: &str, payload: &Value) -> Vec<u8> {
    assert_eq!(request_id.len(), REQUEST_ID_LEN, "request id debe ser 8 chars");
    assert!(
        request_id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
        "request id solo permite [a-z0-9]"
    );

    let body_json = serde_json::to_string(payload).expect("payload siempre serializable");
    let body_b64 = base64::engine::general_purpose::STANDARD.encode(body_json.as_bytes());

    let mut out = Vec::with_capacity(5 + REQUEST_ID_LEN + body_b64.len());
    out.push(SYSEX_MANUFACTURER);
    out.extend_from_slice(&SYSEX_MAGIC);
    out.push(direction as u8);
    out.extend_from_slice(request_id.as_bytes());
    out.extend_from_slice(body_b64.as_bytes());
    out
}

/// Decodifica un SysEx payload (sin framing F0/F7).
///
/// Returns `None` si no es uno de nuestros mensajes (magic incorrecto, mal formado, etc.).
///
/// `data` acepta bytes, bytearray, o cualquier iterable de `u8` en [0, 127].
pub fn decode_message(data: &[u8]) -> Option<Decoded> {
    if data.len() < 5 + REQUEST_ID_LEN {
        return None;
    }
    if data[0] != SYSEX_MANUFACTURER {
        return None;
    }
    if data[1..4] != SYSEX_MAGIC {
        return None;
    }
    let direction = Direction::from_byte(data[4])?;
    let request_id = std::str::from_utf8(&data[5..5 + REQUEST_ID_LEN])
        .ok()?
        .to_string();
    let body = &data[5 + REQUEST_ID_LEN..];
    let body_bytes = base64::engine::general_purpose::STANDARD.decode(body).ok()?;
    let payload: Value = serde_json::from_slice(&body_bytes).ok()?;
    Some(Decoded {
        direction,
        request_id,
        payload,
    })
}

/// Mensaje decodificado.
#[derive(Debug, Clone)]
pub struct Decoded {
    pub direction: Direction,
    pub request_id: String,
    pub payload: Value,
}

/// Strips F0/F7 framing si está presente (algunos drivers añaden framing, otros no).
///
/// Esta función es tolerante: acepta datos con o sin F0/F7.
pub fn strip_framing(data: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = data.len();
    if data.first() == Some(&0xF0) {
        start = 1;
    }
    if data.last() == Some(&0xF7) {
        end -= 1;
    }
    &data[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encode_decode_roundtrip() {
        let id = new_request_id();
        let payload = json!({"cmd": "ping", "params": {}});
        let encoded = encode_message(Direction::Request, &id, &payload);
        let decoded = decode_message(&encoded).expect("decode");
        assert_eq!(decoded.direction, Direction::Request);
        assert_eq!(decoded.request_id, id);
        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn encode_rejects_bad_id_length() {
        let payload = json!({});
        // id de 7 chars debe panic (assert)
        let result = std::panic::catch_unwind(|| {
            encode_message(Direction::Request, "abcdefg", &payload);
        });
        assert!(result.is_err());
    }

    #[test]
    fn decode_rejects_bad_manufacturer() {
        let mut bad = encode_message(Direction::Request, &new_request_id(), &json!({}));
        bad[0] = 0x7E; // manufacturer incorrecto
        assert!(decode_message(&bad).is_none());
    }

    #[test]
    fn decode_rejects_bad_magic() {
        let mut bad = encode_message(Direction::Request, &new_request_id(), &json!({}));
        bad[1] = b'X';
        bad[2] = b'Y';
        bad[3] = b'Z';
        assert!(decode_message(&bad).is_none());
    }

    #[test]
    fn decode_rejects_bad_direction() {
        let mut bad = encode_message(Direction::Request, &new_request_id(), &json!({}));
        bad[4] = 0xFF;
        assert!(decode_message(&bad).is_none());
    }

    #[test]
    fn decode_rejects_truncated() {
        let encoded = encode_message(Direction::Request, &new_request_id(), &json!({}));
        for trunc_len in [0, 5, 10, 20, encoded.len() - 1] {
            let truncated = &encoded[..trunc_len.min(encoded.len())];
            // Solo debe aceptar longitudes suficientes; si es corto, None
            if trunc_len < 5 + REQUEST_ID_LEN {
                assert!(decode_message(truncated).is_none());
            }
        }
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode_message(&[]).is_none());
        assert!(decode_message(&[0x00]).is_none());
        assert!(decode_message(&[0xFF; 100]).is_none());
        assert!(decode_message(b"not sysex at all").is_none());
    }

    #[test]
    fn strip_framing_handles_three_cases() {
        let inner = encode_message(Direction::Request, &new_request_id(), &json!({}));
        // Caso 1: sin framing
        assert_eq!(strip_framing(&inner), inner.as_slice());
        // Caso 2: con F0 al inicio
        let mut with_start = vec![0xF0];
        with_start.extend_from_slice(&inner);
        assert_eq!(strip_framing(&with_start), inner.as_slice());
        // Caso 3: con F0 al inicio y F7 al final
        let mut with_both = vec![0xF0];
        with_both.extend_from_slice(&inner);
        with_both.push(0xF7);
        assert_eq!(strip_framing(&with_both), inner.as_slice());
    }

    #[test]
    fn all_directions_encode_decode() {
        for dir in [Direction::Request, Direction::Response, Direction::Heartbeat] {
            let id = new_request_id();
            let payload = json!({"k": "v"});
            let encoded = encode_message(dir, &id, &payload);
            let decoded = decode_message(&encoded).unwrap();
            assert_eq!(decoded.direction, dir);
        }
    }
}