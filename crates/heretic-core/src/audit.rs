//! Audit log SQLite append-only para el daemon blindado.
//!
//! - WAL mode para concurrencia (reads concurrent con writes).
//! - Tabla `events(id, ts, tool, params_json, result_status, duration_ms, error)`.
//! - Triggers que bloquean UPDATE/DELETE (append-only real).
//! - Retention por días (default 30, configurable).
//!
//! ## Path por defecto
//!
//! - Windows: `%LOCALAPPDATA%\fl-heretic\audit.db`
//! - Unix: `$HOME/.fl-heretic/audit.db`

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params as sqlite_params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{HereticError, Result};

/// Estado del resultado de un evento.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditStatus {
    Ok,
    Error,
    Denied,
    Timeout,
}

impl AuditStatus {
    fn as_str(&self) -> &'static str {
        match self {
            AuditStatus::Ok => "ok",
            AuditStatus::Error => "error",
            AuditStatus::Denied => "denied",
            AuditStatus::Timeout => "timeout",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "ok" => AuditStatus::Ok,
            "error" => AuditStatus::Error,
            "denied" => AuditStatus::Denied,
            "timeout" => AuditStatus::Timeout,
            _ => AuditStatus::Error,
        }
    }
}

/// Un evento del audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: Option<i64>,
    pub ts: u64,
    pub tool: String,
    /// Params serializados (JSON string).
    pub params_json: Option<String>,
    pub status: AuditStatus,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
}

impl AuditEvent {
    pub fn builder(tool: impl Into<String>) -> AuditEventBuilder {
        AuditEventBuilder {
            tool: tool.into(),
            params_json: None,
            status: AuditStatus::Ok,
            duration_ms: None,
            error: None,
        }
    }
}

#[derive(Debug)]
pub struct AuditEventBuilder {
    tool: String,
    params_json: Option<String>,
    status: AuditStatus,
    duration_ms: Option<u64>,
    error: Option<String>,
}

impl AuditEventBuilder {
    pub fn params(mut self, params: &serde_json::Value) -> Self {
        self.params_json = Some(serde_json::to_string(params).unwrap_or_default());
        self
    }
    pub fn status(mut self, status: AuditStatus) -> Self {
        self.status = status;
        self
    }
    pub fn duration_ms(mut self, ms: u64) -> Self {
        self.duration_ms = Some(ms);
        self
    }
    pub fn error(mut self, msg: impl Into<String>) -> Self {
        self.error = Some(msg.into());
        self.status = AuditStatus::Error;
        self
    }
    pub fn build(self) -> AuditEvent {
        AuditEvent {
            id: None,
            ts: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            tool: self.tool,
            params_json: self.params_json,
            status: self.status,
            duration_ms: self.duration_ms,
            error: self.error,
        }
    }
}

/// Audit log SQLite append-only.
///
/// La conexión se comparte vía `Arc<Mutex<Connection>>` (rusqlite no es Send
/// directamente, pero la conexión envuelta sí). El daemon mantiene UNA instancia
/// global.
#[derive(Debug)]
pub struct AuditLog {
    conn: Arc<std::sync::Mutex<Connection>>,
    path: PathBuf,
    retention_days: u32,
}

impl AuditLog {
    /// Abre (o crea) el audit log en el path dado.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        // WAL mode = reads concurrent con un writer
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // Foreign keys + busy timeout
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let log = Self {
            conn: Arc::new(std::sync::Mutex::new(conn)),
            path: path.clone(),
            retention_days: 30,
        };
        log.init_schema()?;
        Ok(log)
    }

    /// Path por defecto del audit log según OS.
    pub fn default_path() -> PathBuf {
        if cfg!(windows) {
            std::env::var("LOCALAPPDATA")
                .map(|p| PathBuf::from(p).join("fl-heretic").join("audit.db"))
                .unwrap_or_else(|_| PathBuf::from("fl-heretic-audit.db"))
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".fl-heretic").join("audit.db")
        }
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| HereticError::Poisoned(e.to_string()))?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS events (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                ts           INTEGER NOT NULL,
                tool         TEXT    NOT NULL,
                params_json  TEXT,
                status       TEXT    NOT NULL,
                duration_ms  INTEGER,
                error        TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_events_ts    ON events(ts);
            CREATE INDEX IF NOT EXISTS idx_events_tool  ON events(tool);
            CREATE INDEX IF NOT EXISTS idx_events_st    ON events(status);

            -- Triggers append-only: bloquear UPDATE/DELETE
            CREATE TRIGGER IF NOT EXISTS events_no_update
            BEFORE UPDATE ON events
            BEGIN
                SELECT RAISE(ABORT, 'audit log es append-only');
            END;
            CREATE TRIGGER IF NOT EXISTS events_no_delete
            BEFORE DELETE ON events
            BEGIN
                SELECT RAISE(ABORT, 'audit log es append-only');
            END;
            "#,
        )?;
        Ok(())
    }

    /// Append un evento. Append-only (los triggers bloquean UPDATE/DELETE).
    pub fn append(&self, event: AuditEvent) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| HereticError::Poisoned(e.to_string()))?;
        conn.execute(
            "INSERT INTO events (ts, tool, params_json, status, duration_ms, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            sqlite_params![
                event.ts as i64,
                event.tool,
                event.params_json,
                event.status.as_str(),
                event.duration_ms.map(|v| v as i64),
                event.error,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Cuenta eventos totales.
    pub fn len(&self) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| HereticError::Poisoned(e.to_string()))?;
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
        Ok(n)
    }

    /// ¿Está vacío?
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Eventos recientes (últimos N).
    pub fn recent(&self, n: u32) -> Result<Vec<AuditEvent>> {
        let conn = self.conn.lock().map_err(|e| HereticError::Poisoned(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, ts, tool, params_json, status, duration_ms, error
             FROM events ORDER BY id DESC LIMIT ?1",
        )?;
        let events = stmt
            .query_map([n as i64], Self::row_to_event)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(events)
    }

    /// Eventos por tool.
    pub fn by_tool(&self, tool: &str, limit: u32) -> Result<Vec<AuditEvent>> {
        let conn = self.conn.lock().map_err(|e| HereticError::Poisoned(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, ts, tool, params_json, status, duration_ms, error
             FROM events WHERE tool = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let events = stmt
            .query_map(sqlite_params![tool, limit as i64], Self::row_to_event)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(events)
    }

    fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuditEvent> {
        let status_str: String = row.get(4)?;
        Ok(AuditEvent {
            id: Some(row.get(0)?),
            ts: row.get::<_, i64>(1)? as u64,
            tool: row.get(2)?,
            params_json: row.get(3)?,
            status: AuditStatus::from_str(&status_str),
            duration_ms: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
            error: row.get(6)?,
        })
    }

    /// Rotation: borra eventos más viejos que `retention_days`. Append-only trigger
    /// no afecta este DELETE directo (los triggers son BEFORE UPDATE/DELETE de la
    /// tabla events, y este DELETE está dentro del método admin_rotation).
    ///
    /// **Implementación correcta**: usamos DROP TRIGGER temporal, ejecutamos DELETE,
    /// restauramos TRIGGER. Más simple que un admin-only mode.
    pub fn rotate(&self) -> Result<usize> {
        let cutoff = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            .saturating_sub(self.retention_days as u64 * 86400);
        let conn = self.conn.lock().map_err(|e| HereticError::Poisoned(e.to_string()))?;
        // Disable append-only enforcement temporarily para rotación
        conn.execute("DROP TRIGGER IF EXISTS events_no_delete", [])?;
        let deleted = conn.execute(
            "DELETE FROM events WHERE ts < ?1",
            sqlite_params![cutoff as i64],
        )?;
        conn.execute_batch(
            r#"
            CREATE TRIGGER IF NOT EXISTS events_no_delete
            BEFORE DELETE ON events
            BEGIN
                SELECT RAISE(ABORT, 'audit log es append-only');
            END;
            "#,
        )?;
        Ok(deleted)
    }

    /// Path del archivo SQLite.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn append_and_query() {
        let dir = tempdir().unwrap();
        let log = AuditLog::open(dir.path().join("audit.db")).unwrap();
        let id = log.append(
            AuditEvent::builder("fl_ping").status(AuditStatus::Ok).build(),
        ).unwrap();
        assert!(id > 0);
        assert_eq!(log.len().unwrap(), 1);
        assert!(!log.is_empty().unwrap());
        let events = log.recent(10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tool, "fl_ping");
    }

    #[test]
    fn append_blocks_update() {
        let dir = tempdir().unwrap();
        let log = AuditLog::open(dir.path().join("audit.db")).unwrap();
        log.append(AuditEvent::builder("fl_ping").build()).unwrap();
        let conn = log.conn.lock().unwrap();
        let result = conn.execute(
            "UPDATE events SET tool = 'evil' WHERE id = 1",
            [],
        );
        assert!(result.is_err(), "UPDATE debe estar bloqueado por el trigger");
    }

    #[test]
    fn append_blocks_delete_except_rotation() {
        let dir = tempdir().unwrap();
        let log = AuditLog::open(dir.path().join("audit.db")).unwrap();
        log.append(AuditEvent::builder("fl_ping").build()).unwrap();
        // DELETE directo debe fallar
        {
            let conn = log.conn.lock().unwrap();
            let result = conn.execute("DELETE FROM events WHERE id = 1", []);
            assert!(result.is_err(), "DELETE directo debe estar bloqueado");
        }
        // rotate() debe poder borrar (es la única vía permitida)
        let deleted = log.rotate().unwrap();
        // No hay nada viejo (ts es now, retention 30 días) → 0 deleted
        assert_eq!(deleted, 0);
        assert_eq!(log.len().unwrap(), 1);
    }

    #[test]
    fn query_by_tool() {
        let dir = tempdir().unwrap();
        let log = AuditLog::open(dir.path().join("audit.db")).unwrap();
        log.append(AuditEvent::builder("fl_ping").build()).unwrap();
        log.append(AuditEvent::builder("fl_set_mixer_volume").build()).unwrap();
        log.append(AuditEvent::builder("fl_ping").build()).unwrap();
        let events = log.by_tool("fl_ping", 10).unwrap();
        assert_eq!(events.len(), 2);
        let events = log.by_tool("nonexistent", 10).unwrap();
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn event_builder_chain() {
        let event = AuditEvent::builder("fl_set_volume")
            .params(&serde_json::json!({"track": 0, "value": -3.0}))
            .duration_ms(42)
            .build();
        assert_eq!(event.tool, "fl_set_volume");
        assert_eq!(event.status, AuditStatus::Ok);
        assert_eq!(event.duration_ms, Some(42));
        assert!(event.params_json.is_some());
    }
}