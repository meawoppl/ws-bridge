# ws-bridge

Strongly-typed WebSocket endpoints for Rust — define once, use everywhere.

**Server** (axum) · **Browser client** (yew/gloo) · **Native client** (tokio-tungstenite)

[![Crates.io](https://img.shields.io/crates/v/ws-bridge.svg)](https://crates.io/crates/ws-bridge)
[![Docs.rs](https://docs.rs/ws-bridge/badge.svg)](https://docs.rs/ws-bridge)
[![MIT licensed](https://img.shields.io/crates/l/ws-bridge.svg)](LICENSE)

## The problem

WebSocket projects in Rust tend to accumulate duplicated boilerplate:

- **Path strings** scattered across server routes and client connect calls
- **Serde plumbing** (`serde_json::to_string` / `from_str`) at every send/recv
- **URL derivation** (`ws://` vs `wss://`) copy-pasted in every frontend
- **Split + send task** pattern repeated in every axum handler
- **Reconnection logic** reimplemented in every project

## The solution

Define your endpoint once:

```rust
use ws_bridge::WsEndpoint;
use serde::{Serialize, Deserialize};

pub struct EchoEndpoint;

impl WsEndpoint for EchoEndpoint {
    const PATH: &'static str = "/ws/echo";
    type ServerMsg = ServerMsg;
    type ClientMsg = ClientMsg;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    Welcome { message: String },
    Echo { payload: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    Say { text: String },
    Quit,
}
```

Use it everywhere with full type safety:

### Server (axum)

```rust
use axum::Router;
use ws_bridge::WsEndpoint;

let app = Router::new().route(
    EchoEndpoint::PATH,
    ws_bridge::server::handler::<EchoEndpoint, _, _>(|mut conn| async move {
        conn.send(ServerMsg::Welcome { message: "Hello!".into() }).await.unwrap();
        while let Some(Ok(msg)) = conn.recv().await {
            // msg is ClientMsg — fully typed
        }
    }),
);
```

### Native client (tokio-tungstenite)

```rust
let mut conn = ws_bridge::native_client::connect::<EchoEndpoint>("ws://localhost:3000").await?;
conn.send(ClientMsg::Say { text: "hello".into() }).await?;
while let Some(Ok(msg)) = conn.recv().await {
    // msg is ServerMsg — fully typed
}
```

### Browser client (yew/gloo)

```rust
let mut conn = ws_bridge::yew_client::connect::<EchoEndpoint>()?;
conn.send(ClientMsg::Say { text: "hello".into() }).await?;
while let Some(Ok(msg)) = conn.recv().await {
    // msg is ServerMsg — fully typed
}
```

## Features

Enable only what you need:

| Feature | What it provides | Dependencies |
|---|---|---|
| `server` | axum WebSocket handlers | axum, tokio, futures-util |
| `yew-client` | Browser WebSocket client (WASM) | gloo-net, wasm-bindgen-futures, web-sys |
| `native-client` | tokio-tungstenite client | tokio-tungstenite, tokio, futures-util |
| `reconnect` | Exponential backoff reconnection | tokio, futures-util |

```toml
[dependencies]
# Server crate
ws-bridge = { version = "0.1", features = ["server"] }

# Frontend crate (WASM)
ws-bridge = { version = "0.1", features = ["yew-client"] }

# CLI / daemon crate
ws-bridge = { version = "0.1", features = ["native-client", "reconnect"] }
```

## Key concepts

- **`WsEndpoint`** — A trait you implement on a unit struct. Defines `PATH`, `ServerMsg`, and `ClientMsg`. This is the single source of truth shared between server and clients.
- **`WsCodec`** — Encode/decode trait. Any `Serialize + DeserializeOwned` type automatically gets JSON encoding. Implement manually for binary protocols.
- **`WsConnection<S, R>`** — Transport-agnostic typed connection. `send()` only accepts `S`, `recv()` only yields `R`.
- **`NoMessages`** — Uninhabited type for unidirectional endpoints (e.g., server-push-only streams).

## Documentation

- **[Server Guide](docs/server.md)** — axum handlers, state extraction, pre-upgrade auth, manual upgrades
- **[Browser Client Guide](docs/yew-client.md)** — WASM/yew usage, `!Send` constraints, Yew component patterns
- **[Native Client Guide](docs/native-client.md)** — tokio-tungstenite connections, TLS
- **[Reconnection Guide](docs/reconnect.md)** — Exponential backoff, custom connect functions
- **[Custom Codecs Guide](docs/custom-codecs.md)** — Binary protocols, `NoMessages`, the `WsCodec` trait

## Examples

Run the examples with:

```bash
# Terminal 1: start the server
cargo run --example echo_server --features server

# Terminal 2: connect a client
cargo run --example echo_client --features native-client

# Server with shared state
cargo run --example stateful_server --features server

# Reconnecting client
cargo run --example reconnecting_client --features "native-client,reconnect"
```

## License

MIT
