//! `heretic-mcp` — MCP server stdio (FlojoMCP) que reenvía comandos al daemon.
//!
//! ## Topología
//!
//! ```text
//! [OpenCode/Claude]
//!      │ stdio (JSON-RPC MCP)
//!      ▼
//! [fl-heretic-mcp (este crate)]  ←──── FlojoMCP server
//!      │ Named Pipe (JSON-RPC interno)
//!      ▼
//! [fl-heretic daemon]  ←──── heretic-fl bridge (MIDI SysEx)
//!      │
//!      ▼
//! [FL Studio]
//! ```
//!
//! No hay daemon intermedio: este proceso ES el dueño del transporte. Eso
//! importa por dos razones concreteas:
//!
//! 1. **Serializacion.** El bridge usa un unico `command.json`. Si dos tools
//!    se ejecutan a la vez (que es lo normal en un agente), las dos escriben
//!    ahi y colisionan. Un `Mutex` aqui lo resuelve; un proceso aparte
//!    tambien, pero seria un proceso mas para lo mismo.
//! 2. **Liveness.** Sin daemon no hay a quien preguntar si el bridge vive.
//!    Por eso `daw_health` comprueba el bridge de verdad antes de prometer nada.
//!
//! La capa de auth HMAC / audit / rate limit que vivia en el daemon se ha
//! eliminado a proposito: el agente ES el proceso del cliente MCP, ya puede
//! escribir en %TEMP% el mismo, asi que un HMAC entre dos procesos del mismo
//! usuario no protege nada. Era seguridad de teatro.

#![forbid(unsafe_code)]

pub mod tools;
#[cfg(test)]
mod tools_tests;