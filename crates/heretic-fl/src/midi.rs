//! MIDI port discovery + open/close usando `midir`.
//!
//! Detecta los puertos loopMIDI (Windows) o IAC Driver (macOS) por nombre,
//! los abre, y devuelve las conexiones listas para I/O.

use midir::{MidiInput, MidiInputConnection, MidiInputPort, MidiOutput, MidiOutputConnection, MidiOutputPort};

use heretic_core::{HereticError, Result};

use crate::{DEFAULT_PORT_FROM_FL, DEFAULT_PORT_TO_FL};

/// Lista los puertos MIDI disponibles (input + output) con sus nombres.
pub fn list_ports() -> Vec<String> {
    let midi_out = match MidiOutput::new("fl-heretic-list") {
        Ok(m) => m,
        Err(_) => return vec![],
    };
    let midi_in = match MidiInput::new("fl-heretic-list") {
        Ok(m) => m,
        Err(_) => return vec![],
    };
    let mut ports = Vec::new();
    ports.extend(midi_out.ports().iter().filter_map(|p| midi_out.port_name(p).ok()));
    ports.extend(midi_in.ports().iter().filter_map(|p| midi_in.port_name(p).ok()));
    ports
}

/// Snapshot de puertos MIDI disponibles (test helper).
#[derive(Debug, Clone, Default)]
pub struct MidiPorts {
    pub outputs: Vec<String>,
    pub inputs: Vec<String>,
}

impl MidiPorts {
    /// Enumera puertos actualmente visibles al proceso.
    pub fn snapshot() -> Self {
        let outputs = MidiOutput::new("fl-heretic-snap")
            .ok()
            .map(|m| {
                m.ports()
                    .iter()
                    .filter_map(|p| m.port_name(p).ok())
                    .collect()
            })
            .unwrap_or_default();
        let inputs = MidiInput::new("fl-heretic-snap")
            .ok()
            .map(|m| {
                m.ports()
                    .iter()
                    .filter_map(|p| m.port_name(p).ok())
                    .collect()
            })
            .unwrap_or_default();
        Self { outputs, inputs }
    }

    /// Busca un puerto output por patrón (case-insensitive substring).
    pub fn find_output(&self, pattern: &str) -> Option<String> {
        let needle = pattern.to_lowercase();
        self.outputs
            .iter()
            .find(|name| name.to_lowercase().contains(&needle))
            .cloned()
    }

    /// Busca un puerto input por patrón (case-insensitive substring).
    pub fn find_input(&self, pattern: &str) -> Option<String> {
        let needle = pattern.to_lowercase();
        self.inputs
            .iter()
            .find(|name| name.to_lowercase().contains(&needle))
            .cloned()
    }
}

/// Mantiene las dos conexiones MIDI activas (input + output).
///
/// Tras `connect()`, las conexiones quedan listas:
/// - `send_sysex()` para enviar requests al controller script
/// - `incoming_rx` para recibir responses + heartbeats desde FL
pub struct MidiConnection {
    /// Output connection (server → FL). Wrap en `Mutex` porque `send()` requiere `&mut self`.
    pub out: std::sync::Mutex<MidiOutputConnection>,
    /// Input connection (FL → server). Mientras esté vivo, el callback sigue activo.
    /// Se guarda aquí solo para que no se dropee (al dropear, el callback se desconecta).
    _in: MidiInputConnection<()>,
    /// Receiver de mensajes SysEx recibidos del FL (filtrados por magic).
    pub incoming_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    /// Nombre del puerto output (para logs).
    pub out_port_name: String,
    /// Nombre del puerto input (para logs).
    pub in_port_name: String,
}

impl std::fmt::Debug for MidiConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MidiConnection")
            .field("out_port_name", &self.out_port_name)
            .field("in_port_name", &self.in_port_name)
            .finish()
    }
}

/// Abre los dos puertos MIDI necesarios.
///
/// `to_fl_pattern` y `from_fl_pattern` son los nombres (o substrings) de los puertos
/// loopMIDI/IAC. Si no se pasan, usan los defaults (`FLStudioMCP RX` / `FLStudioMCP TX`).
///
/// `client_name` es el nombre que aparece en el daemon de MIDI del OS (suele ser
/// visible en FL Studio > Options > MIDI Settings).
pub fn open_midi_ports(
    to_fl_pattern: Option<&str>,
    from_fl_pattern: Option<&str>,
    client_name: &str,
) -> Result<MidiConnection> {
    let to_fl = to_fl_pattern.unwrap_or(DEFAULT_PORT_TO_FL);
    let from_fl = from_fl_pattern.unwrap_or(DEFAULT_PORT_FROM_FL);

    let midi_out = MidiOutput::new(client_name)
        .map_err(|e| HereticError::Other(format!("MidiOutput::new: {e}")))?;
    let midi_in = MidiInput::new(client_name)
        .map_err(|e| HereticError::Other(format!("MidiInput::new: {e}")))?;

    let out_port = find_output_port(&midi_out, &midi_out.ports(), to_fl).ok_or_else(|| {
        HereticError::Other(format!(
            "OUTPUT MIDI port matching {:?} no encontrado. Disponibles: {:?}. \
             Crear el puerto en loopMIDI (Windows) o IAC Driver (macOS), \
             o setear FL_HERETIC_PORT_TO_FL.",
            to_fl,
            midi_out.ports().iter().filter_map(|p| midi_out.port_name(p).ok()).collect::<Vec<_>>()
        ))
    })?;
    let in_port = find_input_port(&midi_in, &midi_in.ports(), from_fl).ok_or_else(|| {
        HereticError::Other(format!(
            "INPUT MIDI port matching {:?} no encontrado. Disponibles: {:?}.",
            from_fl,
            midi_in.ports().iter().filter_map(|p| midi_in.port_name(p).ok()).collect::<Vec<_>>()
        ))
    })?;

    let out_port_name = midi_out.port_name(&out_port).unwrap_or_else(|_| "<unnamed>".into());
    let in_port_name = midi_in.port_name(&in_port).unwrap_or_else(|_| "<unnamed>".into());

    // Channel para mensajes SysEx entrantes (filtrados por magic)
    let (in_tx, in_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

    // Input connection con callback que filtra por magic y forwarda al channel
    let _in = midi_in
        .connect(
            &in_port,
            "FLStudioMCP TX",
            move |_stamp, message, _| {
                // `message` puede incluir o no F0/F7. `strip_framing` lo maneja.
                let inner = crate::sysex::strip_framing(message);
                // Filtrar por magic — solo mensajes nuestros
                if crate::sysex::decode_message(inner).is_some() {
                    let _ = in_tx.send(inner.to_vec());
                }
            },
            (),
        )
        .map_err(|e| HereticError::Other(format!("MidiInput::connect: {e}")))?;

    let out = midi_out
        .connect(&out_port, "FLStudioMCP RX")
        .map_err(|e| HereticError::Other(format!("MidiOutput::connect: {e}")))?;

    let out = std::sync::Mutex::new(out);

    Ok(MidiConnection {
        out,
        _in,
        incoming_rx: in_rx,
        out_port_name,
        in_port_name,
    })
}

/// Helper: encuentra el primer puerto OUTPUT cuyo nombre contiene el patrón.
fn find_output_port(
    midi_out: &MidiOutput,
    ports: &[MidiOutputPort],
    pattern: &str,
) -> Option<MidiOutputPort> {
    let needle = pattern.to_lowercase();
    for p in ports {
        if let Ok(name) = midi_out.port_name(p) {
            if name.to_lowercase().contains(&needle) {
                return Some(p.clone());
            }
        }
    }
    None
}

/// Helper: encuentra el primer puerto INPUT cuyo nombre contiene el patrón.
fn find_input_port(
    midi_in: &MidiInput,
    ports: &[MidiInputPort],
    pattern: &str,
) -> Option<MidiInputPort> {
    let needle = pattern.to_lowercase();
    for p in ports {
        if let Ok(name) = midi_in.port_name(p) {
            if name.to_lowercase().contains(&needle) {
                return Some(p.clone());
            }
        }
    }
    None
}

/// Envía un mensaje SysEx a FL (añade framing F0/F7 que algunos drivers esperan).
pub fn send_sysex(out: &std::sync::Mutex<MidiOutputConnection>, payload: &[u8]) -> Result<()> {
    let mut framed = Vec::with_capacity(payload.len() + 2);
    framed.push(0xF0);
    framed.extend_from_slice(payload);
    framed.push(0xF7);
    let mut g = out.lock().expect("midi out mutex poisoned");
    g.send(&framed)
        .map_err(|e| HereticError::Other(format!("MidiOutputConnection::send: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_doesnt_panic() {
        // En Windows con loopMIDI hay puertos; en CI sin MIDI falla silenciosamente
        let snap = MidiPorts::snapshot();
        // No assertion — solo verificamos que no panice
        let _ = snap.find_output("nonexistent");
        let _ = snap.find_input("nonexistent");
    }

    #[test]
    fn find_pattern_is_case_insensitive() {
        let snap = MidiPorts {
            outputs: vec!["FLStudioMCP RX".to_string(), "Other".to_string()],
            inputs: vec!["FLStudioMCP TX".to_string()],
        };
        assert_eq!(snap.find_output("flstudiomcp").as_deref(), Some("FLStudioMCP RX"));
        assert_eq!(snap.find_output("RX").as_deref(), Some("FLStudioMCP RX"));
        assert_eq!(snap.find_input("tx").as_deref(), Some("FLStudioMCP TX"));
        assert_eq!(snap.find_output("nonexistent"), None);
    }

    #[test]
    #[ignore = "requiere loopMIDI/IAC + FL Studio corriendo"]
    fn open_midi_ports_real() {
        // Test de integración — solo correr con MIDI setup real
        let conn = open_midi_ports(None, None, "fl-heretic-test").unwrap();
        println!("Opened MIDI: out={}, in={}", conn.out_port_name, conn.in_port_name);
    }
}