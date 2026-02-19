# Native Client (`native-client` feature)

The `native-client` feature provides a typed WebSocket client using [tokio-tungstenite](https://docs.rs/tokio-tungstenite). Use it for CLI tools, daemons, proxies, or any non-browser Rust application.

## Setup

```toml
[dependencies]
ws-bridge = { version = "0.1", features = ["native-client"] }
tokio = { version = "1", features = ["full"] }
```

## Connecting

### With endpoint path

`connect()` appends the endpoint's `PATH` to the base URL:

```rust
use ws_bridge::native_client;

// Connects to ws://localhost:3000/ws/echo
let mut conn = native_client::connect::<EchoEndpoint>("ws://localhost:3000").await?;
```

### With explicit URL

`connect_to_url()` connects to an exact URL, bypassing the endpoint's `PATH`. Useful for parameterized paths:

```rust
// For endpoints like /ws/voice/:session_id
let url = format!("ws://localhost:3000/ws/voice/{session_id}");
let mut conn = native_client::connect_to_url::<VoiceEndpoint>(&url).await?;
```

## Sending and receiving

The connection sends `E::ClientMsg` and receives `E::ServerMsg` (flipped from the server side):

```rust
let mut conn = native_client::connect::<EchoEndpoint>("ws://localhost:3000").await?;

// Send
conn.send(ClientMsg::Say { text: "hello".into() }).await?;

// Receive
while let Some(result) = conn.recv().await {
    match result {
        Ok(ServerMsg::Echo { payload }) => println!("Got: {payload}"),
        Ok(ServerMsg::Welcome { message }) => println!("Server: {message}"),
        Err(ws_bridge::RecvError::Decode(e)) => eprintln!("Bad message: {e}"),
        Err(ws_bridge::RecvError::Closed) => break,
        _ => {}
    }
}
```

## Splitting the connection

Use `split()` for concurrent send/receive:

```rust
let conn = native_client::connect::<EchoEndpoint>("ws://localhost:3000").await?;
let (mut sender, mut receiver) = conn.split();

// Send from one task
let send_handle = tokio::spawn(async move {
    sender.send(ClientMsg::Say { text: "ping".into() }).await.unwrap();
    sender.close().await.unwrap();
});

// Receive in another
while let Some(Ok(msg)) = receiver.recv().await {
    println!("{msg:?}");
}

send_handle.await.unwrap();
```

## Error handling

| Error | When |
|---|---|
| `ConnectError` | TCP connection or WebSocket handshake fails |
| `SendError::Encode(...)` | Message serialization fails |
| `SendError::Closed` | Connection closed when sending |
| `RecvError::Decode(...)` | Received frame can't be deserialized |
| `RecvError::Closed` | Connection closed by server or network |

## Reconnection

For automatic reconnection with exponential backoff, see [Reconnect](reconnect.md). The `reconnect` feature provides `connect_native()` which wraps the native client with retry logic:

```rust
use ws_bridge::reconnect::{self, BackoffConfig};

let mut ws = reconnect::connect_native::<EchoEndpoint>(
    "ws://localhost:3000".into(),
    BackoffConfig::default(),
);
```

## TLS

tokio-tungstenite handles `wss://` URLs automatically using the system's TLS stack. Just use a `wss://` URL:

```rust
let mut conn = native_client::connect::<MyEndpoint>("wss://example.com").await?;
```
