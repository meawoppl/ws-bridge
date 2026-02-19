# Reconnection (`reconnect` feature)

The `reconnect` feature provides `ReconnectingWs`, a wrapper that automatically reconnects with exponential backoff when the WebSocket connection drops.

## Setup

```toml
[dependencies]
ws-bridge = { version = "0.1", features = ["reconnect", "native-client"] }
tokio = { version = "1", features = ["full"] }
```

## Quick start

For native clients, use the convenience constructor:

```rust
use ws_bridge::reconnect::{self, BackoffConfig};

let mut ws = reconnect::connect_native::<EchoEndpoint>(
    "ws://localhost:3000".into(),
    BackoffConfig::default(),
);

// recv() automatically reconnects if the connection drops
while let Some(result) = ws.recv().await {
    match result {
        Ok(msg) => println!("{msg:?}"),
        Err(e) => eprintln!("decode error: {e}"),
    }
}
```

## Backoff configuration

```rust
use std::time::Duration;
use ws_bridge::reconnect::BackoffConfig;

let config = BackoffConfig {
    initial: Duration::from_millis(500),  // First retry after 500ms
    max: Duration::from_secs(30),         // Cap at 30s between retries
    multiplier: 2.0,                      // Double the delay each attempt
};
```

The default config is:
- `initial`: 1 second
- `max`: 30 seconds
- `multiplier`: 2.0

With the default config, retry delays are: 1s, 2s, 4s, 8s, 16s, 30s, 30s, 30s, ...

## Custom connect functions

`ReconnectingWs::new()` accepts any async function that returns `Option<WsConnection<S, R>>`:

```rust
use ws_bridge::reconnect::{ReconnectingWs, BackoffConfig};

let mut ws = ReconnectingWs::new(
    BackoffConfig::default(),
    || async {
        // Custom connection logic — return Some(conn) on success, None on failure
        ws_bridge::native_client::connect::<MyEndpoint>("ws://localhost:3000")
            .await
            .ok()
    },
);
```

This is useful when you need custom connection setup (e.g., adding auth headers, using a specific TLS config).

## Behavior

### Sending

`send()` attempts to send the message. If the connection is down, it reconnects first, then sends. If the send fails, it drops the connection and retries on the next loop iteration.

```rust
ws.send(ClientMsg::Ping).await?;
```

The `S: Clone` bound on the send type is required because the message may need to be resent after a reconnection.

### Receiving

`recv()` returns the next message. If the connection drops (`None` or `RecvError::Closed`), it transparently reconnects and continues receiving. Decode errors (`RecvError::Decode`) are passed through to the caller.

```rust
while let Some(result) = ws.recv().await {
    match result {
        Ok(msg) => { /* handle message */ }
        Err(ws_bridge::RecvError::Decode(e)) => {
            // Bad message from server — connection is still alive
            eprintln!("decode error: {e}");
        }
        // RecvError::Closed is handled internally (triggers reconnect)
        _ => {}
    }
}
// recv() returns None only if reconnection permanently fails
// (currently retries indefinitely, so this loop runs forever)
```

### Forced reconnection

Call `reconnect()` to explicitly drop and re-establish the connection:

```rust
ws.reconnect().await;
```

## Retry behavior

- On successful connect or message exchange, the attempt counter resets to 0.
- On connection failure, the delay increases exponentially up to `max`.
- Reconnection retries indefinitely — there is no max attempt limit.
- `recv()` never returns `None` (it always reconnects), so loops using `while let Some(...)` run forever.

## Example: resilient client

```rust
use ws_bridge::reconnect::{self, BackoffConfig};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let config = BackoffConfig {
        initial: Duration::from_secs(1),
        max: Duration::from_secs(30),
        multiplier: 2.0,
    };

    let mut ws = reconnect::connect_native::<MyEndpoint>(
        "ws://localhost:3000".into(),
        config,
    );

    loop {
        // Send a message (reconnects if needed)
        if let Err(e) = ws.send(ClientMsg::Heartbeat).await {
            eprintln!("send failed: {e}");
            continue;
        }

        // Wait for response
        if let Some(Ok(msg)) = ws.recv().await {
            println!("received: {msg:?}");
        }
    }
}
```
