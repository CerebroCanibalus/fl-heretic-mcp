#!/usr/bin/env python3
"""Conecta Lifecycle al servidor del pipe y registra los handlers nuevos."""
import io

PATH = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\crates\heretic-daemon\src\pipe.rs"
src = io.open(PATH, encoding="utf-8").read()

# 1. import de Lifecycle
src = src.replace(
    "use crate::handlers::Transport;",
    "use crate::handlers::{Lifecycle, Transport};",
)

# 2. crear el handler de lifecycle junto al de transport
old = "    let transport = Arc::new(Transport::new(bridge));"
new = (
    "    let transport = Arc::new(Transport::new(bridge.clone()));\n"
    "    let lifecycle = Arc::new(Lifecycle::new(bridge));"
)
assert old in src, "no encuentro Transport::new"
src = src.replace(old, new)

# 3. pasar lifecycle a handle_client
old = """        let transport = transport.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(client, verifier, audit, transport).await {"""
new = """        let transport = transport.clone();
        let lifecycle = lifecycle.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(client, verifier, audit, transport, lifecycle).await {"""
assert old in src, "no encuentro el spawn"
src = src.replace(old, new)

# 4. firma de handle_client
old = """async fn handle_client(
    client: NamedPipeServer,
    verifier: Arc<AuthVerifier>,
    audit: Arc<AuditLog>,
    transport: Arc<Transport>,
) -> Result<(), HereticError> {"""
new = """async fn handle_client(
    client: NamedPipeServer,
    verifier: Arc<AuthVerifier>,
    audit: Arc<AuditLog>,
    transport: Arc<Transport>,
    lifecycle: Arc<Lifecycle>,
) -> Result<(), HereticError> {"""
assert old in src, "no encuentro la firma de handle_client"
src = src.replace(old, new)

# 5. la llamada a dispatch pasa ambos
old = "        let response = dispatch(&request, &transport).await;"
new = "        let response = dispatch(&request, &transport, &lifecycle).await;"
assert old in src, "no encuentro dispatch()"
src = src.replace(old, new)

# 6. dispatch: probar lifecycle antes que transport
old = '''/// Dispatch un Request al handler apropiado. Primero intenta transport; si no
/// es un método transport, usa el fallback del daemon (`ping`/`health`).
async fn dispatch(req: &Request, transport: &Transport) -> Response {
    match transport.dispatch(req.tool_name(), req.params_or_empty()).await {
        Ok(data) => Response::ok(req.id.clone(), data),
        Err(e) => Response::from_heretic(&req.id, &e),
    }
}'''
new = '''/// Nombres de metodo que son de lifecycle (control del proceso de FL), no
/// de FL Studio. Se comprueban ANTES que los de transport para que `status`,
/// por ejemplo, no choque con el `status` del transporte.
const LIFECYCLE_METHODS: &[&str] = &[
    "open", "launch", "save", "save_as", "close", "fl_status", "wait_ready",
];

/// Dispatch un Request al handler apropiado.
async fn dispatch(req: &Request, transport: &Transport, lifecycle: &Lifecycle) -> Response {
    let name = req.tool_name();
    let result = if LIFECYCLE_METHODS.contains(&name) {
        let method = name.strip_prefix("fl_").unwrap_or(name);
        lifecycle.dispatch(method, req.params_or_empty()).await
    } else {
        transport.dispatch(name, req.params_or_empty()).await
    };
    match result {
        Ok(data) => Response::ok(req.id.clone(), data),
        Err(e) => Response::from_heretic(&req.id, &e),
    }
}'''
assert old in src, "no encuentro la definicion de dispatch"
src = src.replace(old, new)

io.open(PATH, "w", encoding="utf-8", newline="\n").write(src)
print("pipe.rs conectado: %d lineas" % len(src.splitlines()))
