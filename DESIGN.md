# ws-bridge: Strongly-Typed WebSocket Endpoints for Rust

A small Rust library that lets you define a WebSocket endpoint **once** — path, message types, codec — and use that single definition to build axum server handlers, yew/gloo browser clients, and tokio-tungstenite native clients, all with full type safety and zero duplicated plumbing.

## Motivation

This library was born from recurring patterns observed across two real-world Rust projects:

- **cc-proxy** — A proxy/relay for Claude Code sessions. Uses axum on the backend, yew on the frontend, and tokio-tungstenite for a native CLI proxy client. All three sides communicate via a shared `ProxyMessage` enum serialized as tagged JSON over WebSocket.

- **meter-sim/test-bench** — A camera testbench system. Uses axum on the backend and yew on the frontend. WebSocket carries binary image frames (custom header + JPEG payload) from server to browser.

Both projects independently arrived at the same boilerplate scaffolding around WebSocket connections, and both suffer from the same categories of duplication and fragility.

## Observations from cc-proxy

### What works well

- A **shared crate** (`shared/`) defines `ProxyMessage` as a `#[serde(tag = "type")]` enum, used by server, frontend, and native client.
- All three sides serialize/deserialize the same Rust type, guaranteeing protocol compatibility at compile time.

### What's duplicated / fragile

| Problem | Details |
|---|---|
| **Path strings scattered everywhere** | `"/ws/session"` appears in axum route definition (`backend/src/main.rs:373`), in the native client connect call (`claude-session-lib/src/proxy_session.rs:355`), and conceptually in the frontend (`frontend/src/pages/dashboard/session_view/websocket.rs:34` uses `ws_url("/ws/client")`). A typo in any one breaks the connection silently. |
| **Serde plumbing at every send/recv** | Every handler has `serde_json::to_string(&msg)` on send and `serde_json::from_str::<ProxyMessage>(&text)` on receive. This is ~4 lines of ceremony around every message, repeated in `proxy_socket.rs:26-27`, `web_client_socket.rs:26-28`, `websocket.rs:51-52,64`, `proxy_session.rs:47-48`, etc. |
| **`ws://` / `wss://` URL derivation** | Both the frontend (`frontend/src/utils.rs:17-28`) and the meter-sim frontend (`test-bench-frontend/src/ws_image_stream.rs:221-235`) have nearly identical functions to derive WebSocket URLs from the page's current protocol and host. |
| **Socket split + send task pattern** | The server-side pattern of `socket.split()` → `mpsc::unbounded_channel()` → spawn a send task that drains the channel → receive loop that processes incoming messages appears identically in `proxy_socket.rs:15-47` and `web_client_socket.rs:14-50`. |
| **Monolithic message enum** | `ProxyMessage` has ~30 variants, but each endpoint only uses a subset. The `/ws/session` endpoint never receives `ClaudeInput` directly (it gets `SequencedInput`), the `/ws/client` endpoint never receives `SequencedOutput`, etc. The type system doesn't enforce these constraints — you can accidentally match on or send a variant that's invalid for the current endpoint. |
| **Reconnection logic** | Exponential backoff reconnection is implemented ad-hoc in `proxy_session.rs:235-293` (native client) and `frontend/src/hooks/use_client_websocket.rs` (browser client) with similar but not identical parameters. |

### Endpoint inventory

| Path | Server handler | Client | ServerMsg subset | ClientMsg subset |
|---|---|---|---|---|
| `/ws/session` | `handle_session_websocket` | tokio-tungstenite (proxy CLI) | `RegisterAck`, `OutputAck`, `SequencedInput`, `Heartbeat`, `PermissionResponse`, `ServerShutdown` | `Register`, `SequencedOutput`, `InputAck`, `PermissionRequest`, `SessionUpdate`, `Heartbeat` |
| `/ws/client` | `handle_web_client_websocket` | gloo-net (yew frontend) | `ClaudeOutput`, `HistoryBatch`, `PermissionRequest`, `Error`, `SessionUpdate`, `UserSpendUpdate`, `ServerShutdown` | `Register`, `ClaudeInput`, `PermissionResponse` |
| `/ws/voice/:session_id` | `handle_voice_websocket` | gloo-net (yew frontend) | `Transcription`, `VoiceError`, `VoiceEnded` | `StartVoice`, `StopVoice`, audio binary frames |
| `/ws/launcher` | `handle_launcher_websocket` | tokio-tungstenite (launcher daemon) | `LauncherRegisterAck`, `LaunchSession`, `StopSession`, `ListDirectories` | `LauncherRegister`, `LaunchSessionResult`, `LauncherHeartbeat`, `ProxyLog`, `SessionExited`, `ListDirectoriesResult` |

## Observations from meter-sim/test-bench

### Architecture

- **Shared crate** (`test-bench-shared/`) contains HTTP request/response types (`PatternConfigResponse`, `CameraStats`, `SchemaResponse`, `TrackingStatus`, etc.) but does **not** define WebSocket message types.
- WebSocket is used for a single purpose: streaming camera frames as binary data from server to browser.
- The binary protocol is a custom frame format: `width(u32 LE) + height(u32 LE) + frame_number(u64 LE) + jpeg_data`.

### Server side (`test-bench/src/ws_stream.rs`)

- `WsBroadcaster` wraps a `tokio::sync::broadcast::channel<WsFrame>` for one-to-many streaming.
- `ws_stream_handler` receives from the broadcast channel and sends binary frames. No incoming message handling beyond ping/pong/close.
- Route registered as `.route("/ws-stream", get(ws_stream_endpoint::<C>))` in `camera_server.rs:1150`.

### Client side (`test-bench-frontend/src/ws_image_stream.rs`)

- `WsImageStream` Yew component opens `WebSocket::open(url)` where URL is derived from page protocol (same pattern as cc-proxy).
- Receives `Message::Bytes(data)`, manually parses the 16-byte header, creates a blob URL from the JPEG payload.
- Has its own exponential backoff reconnection (500ms base, 10s max, `2^attempt` multiplier).
- The binary frame parsing (lines 258-265) mirrors the server's `WsFrame::to_binary()` (lines 50-57) — the protocol is defined implicitly by matching encode/decode implementations rather than by a shared type.

### What's duplicated / fragile

| Problem | Details |
|---|---|
| **Binary codec defined implicitly** | `WsFrame::to_binary()` on the server and the manual byte parsing on the client must agree on field order and sizes. No shared code enforces this. |
| **Path string duplication** | `"/ws-stream"` appears in `camera_server.rs:1150` (route) and `ws_image_stream.rs:223,233` (client connect). |
| **URL derivation** | Same `protocol → ws_protocol` mapping as cc-proxy, copy-pasted. |
| **Reconnection** | Another independent exponential backoff implementation. |

## Common patterns across both projects

1. **Shared message types** (enum or struct) with `Serialize + Deserialize`
2. **axum `WebSocketUpgrade` → `socket.split()` → send task + receive loop**
3. **gloo-net `WebSocket::open()` → `split()` → send/receive with serde**
4. **tokio-tungstenite `connect_async()` → same split pattern**
5. **URL construction**: derive `ws://`/`wss://` from page protocol in WASM, format string in native
6. **Endpoint path**: string literal duplicated between server route and client connect call
7. **Exponential backoff reconnection**: reimplemented in every project/client

## Proposed library design

### Core trait

```rust
/// Defines a WebSocket endpoint — its path, and the message types
/// that flow in each direction.
///
/// Implement this on a unit struct in your shared crate. That struct
/// becomes the single source of truth for the endpoint, usable by
/// server, browser client, and native client alike.
pub trait WsEndpoint {
    /// The URL path for this endpoint (e.g., "/ws/session").
    const PATH: &'static str;

    /// Messages sent FROM the server TO the client.
    type ServerMsg: WsCodec + Clone + Send + 'static;

    /// Messages sent FROM the client TO the server.
    type ClientMsg: WsCodec + Clone + Send + 'static;
}
```

### Codec trait

```rust
/// Encode/decode messages to/from WebSocket frames.
///
/// A blanket implementation covers anything that implements
/// `Serialize + DeserializeOwned`, encoding as JSON text frames.
/// Override for binary protocols.
pub trait WsCodec: Sized {
    fn encode(&self) -> Result<WsMessage, EncodeError>;
    fn decode(msg: WsMessage) -> Result<Self, DecodeError>;
}

/// Blanket impl: JSON text encoding for any serde type.
impl<T: Serialize + DeserializeOwned> WsCodec for T {
    fn encode(&self) -> Result<WsMessage, EncodeError> {
        Ok(WsMessage::Text(serde_json::to_string(self)?))
    }
    fn decode(msg: WsMessage) -> Result<Self, DecodeError> {
        match msg {
            WsMessage::Text(text) => Ok(serde_json::from_str(&text)?),
            _ => Err(DecodeError::UnexpectedBinary),
        }
    }
}
```

For binary protocols (like the camera frame stream), implement `WsCodec` manually:

```rust
impl WsCodec for WsFrame {
    fn encode(&self) -> Result<WsMessage, EncodeError> {
        Ok(WsMessage::Binary(self.to_binary()))
    }
    fn decode(msg: WsMessage) -> Result<Self, DecodeError> {
        match msg {
            WsMessage::Binary(data) if data.len() > 16 => {
                // parse header...
                Ok(WsFrame { width, height, frame_number, jpeg_data })
            }
            _ => Err(DecodeError::UnexpectedText),
        }
    }
}
```

### Typed connection

```rust
/// A typed WebSocket connection. Knows which message types
/// can be sent and received based on the endpoint.
pub struct WsConnection<E: WsEndpoint> {
    // ...internal transport details...
}

// On the server side, you send ServerMsg and receive ClientMsg:
impl<E: WsEndpoint> WsConnection<E> {
    pub async fn send(&self, msg: E::ServerMsg) -> Result<(), SendError>;
    pub async fn recv(&mut self) -> Option<Result<E::ClientMsg, RecvError>>;
    pub fn split(self) -> (WsSender<E::ServerMsg>, WsReceiver<E::ClientMsg>);
}

// On the client side (browser or native), the types are flipped:
// you send ClientMsg and receive ServerMsg. This is handled by
// having separate ClientConnection<E> / ServerConnection<E> wrappers,
// or by a generic WsConnection<Send, Recv> parameterized differently
// on each side.
```

### Feature-gated integrations

#### `server` feature (axum)

```rust
use ws_bridge::server;

// Register a typed handler — the closure receives a typed connection
let app = Router::new()
    .route(
        SessionSocket::PATH,
        server::handler(|conn: server::Connection<SessionSocket>, state| async move {
            // conn.send(msg) only accepts SessionSocket::ServerMsg
            // conn.recv() only yields SessionSocket::ClientMsg
        }),
    );
```

Eliminates the `socket.split()` → `mpsc` channel → spawn send task → receive loop boilerplate that currently appears in every handler.

#### `yew-client` feature (gloo-net, WASM)

```rust
use ws_bridge::yew_client;

// Derives ws:///wss:// from page protocol automatically
let conn = yew_client::connect::<SessionSocket>().await?;

// Or with explicit base URL
let conn = yew_client::connect_to::<SessionSocket>("wss://example.com").await?;

conn.send(ClientToServer::Register { ... }).await?;
while let Some(msg) = conn.recv().await {
    match msg? {
        ServerToClient::ClaudeOutput { content } => { /* ... */ }
        // Fully typed — only valid ServerMsg variants
    }
}
```

#### `native-client` feature (tokio-tungstenite)

```rust
use ws_bridge::native_client;

let conn = native_client::connect::<SessionSocket>("ws://localhost:3000").await?;
conn.send(ProxyToServer::Register { ... }).await?;
// Same typed API as the browser client
```

#### `reconnect` feature

```rust
use ws_bridge::reconnect::{ReconnectingWs, BackoffConfig};

let config = BackoffConfig {
    initial: Duration::from_millis(500),
    max: Duration::from_secs(30),
    multiplier: 2.0,
};

let ws = ReconnectingWs::<SessionSocket>::connect("ws://localhost:3000", config).await;
// Automatically reconnects on disconnect, calls on_reconnect callback
```

### Usage example: defining endpoints

In your shared crate (depended on by both server and client):

```rust
use ws_bridge::WsEndpoint;
use serde::{Serialize, Deserialize};

// ---- Endpoint definitions ----

pub struct SessionSocket;
impl WsEndpoint for SessionSocket {
    const PATH: &'static str = "/ws/session";
    type ServerMsg = ServerToProxy;
    type ClientMsg = ProxyToServer;
}

pub struct ClientSocket;
impl WsEndpoint for ClientSocket {
    const PATH: &'static str = "/ws/client";
    type ServerMsg = ServerToClient;
    type ClientMsg = ClientToServer;
}

pub struct CameraStream;
impl WsEndpoint for CameraStream {
    const PATH: &'static str = "/ws-stream";
    type ServerMsg = CameraFrame;  // implements WsCodec manually for binary
    type ClientMsg = ();           // server-only push, no client messages
}

// ---- Per-endpoint message types ----
// (instead of one monolithic ProxyMessage with 30 variants)

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerToProxy {
    RegisterAck { success: bool, session_id: Uuid, error: Option<String> },
    OutputAck { session_id: Uuid, ack_seq: u64 },
    SequencedInput { session_id: Uuid, seq: i64, content: Value, send_mode: Option<SendMode> },
    PermissionResponse { request_id: String, allow: bool, /* ... */ },
    Heartbeat,
    ServerShutdown { reason: String, reconnect_delay_ms: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProxyToServer {
    Register { session_id: Uuid, session_name: String, /* ... */ },
    SequencedOutput { seq: u64, content: Value },
    InputAck { session_id: Uuid, ack_seq: i64 },
    PermissionRequest { request_id: String, tool_name: String, input: Value },
    SessionUpdate { session_id: Uuid, git_branch: Option<String> },
    Heartbeat,
}

// Similar for ServerToClient / ClientToServer...
```

### What this eliminates

| Boilerplate | Before | After |
|---|---|---|
| Path duplication | String literal in server route + every client | `E::PATH` — one definition |
| Serde plumbing | `serde_json::to_string` / `from_str` at every send/recv | Internal to `WsConnection`, invisible |
| URL construction | Copy-pasted `ws://`/`wss://` derivation in every frontend | `yew_client::connect::<E>()` handles it |
| Split + send task | ~15 lines per axum handler | `server::handler(...)` does it internally |
| Direction safety | Both sides deserialize to same enum; can send invalid variants | Separate `ServerMsg`/`ClientMsg` per endpoint |
| Reconnection | Ad-hoc in every project | `ReconnectingWs<E>` with configurable backoff |
| Binary codec agreement | Implicit (matching encode/decode) | `WsCodec` trait with shared impl |

### Migration path

The library is designed for incremental adoption:

1. **Start with just the trait** — Define `WsEndpoint` impls using your existing monolithic message enum as both `ServerMsg` and `ClientMsg`. You immediately get path deduplication.

2. **Adopt server helpers** — Replace the split+channel+spawn boilerplate in axum handlers with `server::handler(...)`.

3. **Adopt client helpers** — Replace manual `WebSocket::open` + URL construction with `yew_client::connect::<E>()` or `native_client::connect::<E>()`.

4. **Split message enums** — When ready, replace the monolithic enum with per-endpoint enums. The compiler tells you every place that needs updating.

5. **Add reconnection** — Replace ad-hoc backoff with `ReconnectingWs<E>`.

## Crate structure

```
ws-bridge/
  Cargo.toml
  src/
    lib.rs              # WsEndpoint trait, WsMessage, re-exports
    codec.rs            # WsCodec trait, blanket JSON impl, error types
    connection.rs       # WsConnection / WsSender / WsReceiver (transport-agnostic)
    server.rs           # #[cfg(feature = "server")] — axum WebSocketUpgrade integration
    yew_client.rs       # #[cfg(feature = "yew-client")] — gloo-net WebSocket integration
    native_client.rs    # #[cfg(feature = "native-client")] — tokio-tungstenite integration
    reconnect.rs        # #[cfg(feature = "reconnect")] — exponential backoff wrapper
```

```toml
[package]
name = "ws-bridge"
version = "0.1.0"
edition = "2021"

[features]
default = []
server = ["dep:axum", "dep:futures-util", "dep:tokio"]
yew-client = ["dep:gloo-net", "dep:wasm-bindgen-futures", "dep:web-sys"]
native-client = ["dep:tokio-tungstenite", "dep:tokio", "dep:futures-util"]
reconnect = []

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# server
axum = { version = "0.7", features = ["ws"], optional = true }
tokio = { version = "1", features = ["sync", "macros", "rt"], optional = true }
futures-util = { version = "0.3", optional = true }

# yew-client
gloo-net = { version = "0.6", features = ["websocket"], optional = true }
wasm-bindgen-futures = { version = "0.4", optional = true }
web-sys = { version = "0.3", features = ["Window", "Location"], optional = true }

# native-client
tokio-tungstenite = { version = "0.24", optional = true }
```

## Open questions

- **`()` as ClientMsg** — For server-push-only endpoints (like camera stream), `ClientMsg = ()`. The `WsCodec` impl for `()` should probably just error on decode and be a no-op on encode. Or use a `Never`/`Infallible` type.

- **State injection on the server** — axum handlers need access to `State<Arc<AppState>>` and sometimes extractors like cookies. The `server::handler` wrapper needs to be flexible enough to pass these through without becoming its own framework.

- **Parameterized paths** — The voice WebSocket endpoint is `/ws/voice/:session_id`. `PATH` is `&'static str` which can't represent path parameters. Could use a `fn path(&self) -> String` method instead, or handle params separately.

- **Heartbeat as a library concern?** — Both projects implement heartbeat/keepalive. Could be a built-in feature of `WsConnection` with configurable intervals, or left to the user.
