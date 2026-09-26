//! `heretic-daw` — puente generico a un DAW, empezando por REAPER.
//!
//! # Por que un crate nuevo y no reusar `heretic-fl`
//!
//! El crate de FL hacia dos cosas que aqui se separan:
//!
//! 1. **Transporte hacia el DAW** (file-RPC por slots). Eso es generico: el
//!    bridge de REAPER (`xDarkzx/Reaper-MCP`) usa el mismo patron de IPC por
//!    ficheros, con `command.json` -> `response.json`. Se reusa tal cual.
//! 2. **Ciclo de vida del proceso del DAW** (lanzar, cerrar, projectos).
//!    Eso era especifico de FL porque su API no expone proyectos; en REAPER
//!    si, asi que la gestion de proyecto pasa a ser una accion del bridge y
//!    no un modulo aparte.
//!
//! # El salto de fondo
//!
//! FL Studio no tiene API de proyecto: ni `saveProject`, ni `getProjectFilePath`,
//! ni una constante `FPT_New` de las 79 `FPT_*`. Su menu File es owner-draw y
//! su Python va en un sandbox sin file I/O ni red. Por eso el transporte
//! acaba siendo "ficheros + MIDI para despertarlo" y por que abrir un proyecto
//! era tan lento: un `TWelcomeWizard` modal o un 'Save changes?' congelan el
//! bridge entero.
//!
//! REAPER tiene 900+ funciones, control externo por TCP (`python-reapy`), y
//! ReaScript con file I/O y red libres. El problema de FL no existia.
//!
//! # Estado
//!
//! Andamiaje: el transporte file-RPC se hereda del crate de FL. Falta el
//! catalogo de acciones de REAPER y el cliente tipado.

#![deny(unsafe_code)]

#[cfg(test)]
mod live_tests;

pub mod actions;
pub mod file_rpc;
pub mod reaper;

pub use actions::{ACTION_GROUPS, ACTIONS};
pub use file_rpc::{FileRpc, RpcError};
pub use reaper::{ReaperBridge, ReaperConfig};
