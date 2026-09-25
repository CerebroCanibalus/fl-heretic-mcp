//! `heretic-daemon` — el daemon blindado.
//!
//! Re-exporta los módulos públicos para tests.
//! El binario (`fl-heretic`) está en `src/main.rs`.

pub mod pipe;
pub mod commands;
pub mod handlers;