#!/usr/bin/env python3
"""Anade `call` (escape hatch) y arregla set_song_position en transport.rs."""
import io

PATH = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-daemon\src\handlers\transport.rs"
src = io.open(PATH, encoding="utf-8").read()

# --- 1. set_song_position: releer el status tras el set ---
old = '''        let status = self.bridge.set_position(position, unit).await?;
        Ok(json!({
            "position_ticks": status.position_ticks,
            "position_bars": status.position_bars,
            "position_seconds": status.position_seconds,
            "bpm": status.bpm,
        }))
    }
}'''
new = '''        self.bridge.set_position(position, unit).await?;
        // El bridge devuelve solo un eco de la posicion; se relee el estado
        // real para no mentirle a quien llama.
        let status = self.bridge.transport_status().await?;
        Ok(json!({
            "position_ticks": status.position_ticks,
            "position_bars": status.position_bars,
            "position_seconds": status.position_seconds,
            "bpm": status.bpm,
        }))
    }

    /// `call` — escape hatch a CUALQUIER action del bridge.
    ///
    /// Da acceso a las 67 actions sin escribir un handler MCP por cada una.
    /// El catalogo esta en `heretic_fl::bridge::ACTIONS`.
    /// Parametros: `{ "action": "channels.setVolume", "params": { ... } }`.
    pub async fn call(&self, params: Value) -> Result<Value> {
        let action = params
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HereticError::InvalidRequest("call: falta 'action'".into()))?
            .to_string();
        if !FlBridge::is_known_action(&action) {
            tracing::warn!(
                action,
                "action fuera del catalogo compilado; se envia igualmente. \\
                 Usa fl_actions para ver el catalogo."
            );
        }
        let p = params.get("params").cloned().unwrap_or(json!({}));
        self.bridge.call(&action, p).await
    }

    /// `actions` — catalogo de actions que el bridge declara.
    pub async fn actions(&self) -> Result<Value> {
        let known: Vec<&str> = heretic_fl::ACTIONS.to_vec();
        // Se le pregunta al bridge de verdad, que puede tener mas.
        let live = self.bridge.call("meta.actions", json!({})).await.ok();
        let live_list: Vec<String> = live
            .and_then(|v| v.get("actions").cloned())
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Ok(json!({
            "compiled_catalog": known,
            "compiled_count": known.len(),
            "bridge_reported": live_list,
            "bridge_count": live_list.len(),
        }))
    }
}'''
assert old in src, "set_song_position no coincide"
src = src.replace(old, new)

# --- 2. registrar en el dispatcher ---
old_disp = '            "set_song_position" => self.set_song_position(params).await,'
new_disp = old_disp + '\n            "call" => self.call(params).await,\n            "actions" => self.actions().await,'
assert old_disp in src, "dispatcher no coincide"
src = src.replace(old_disp, new_disp)

io.open(PATH, "w", encoding="utf-8", newline="\n").write(src)
print("transport.rs parcheado: %d lineas" % len(src.splitlines()))
