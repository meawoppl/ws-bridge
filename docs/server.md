# Server (`server` feature)

The `server` feature provides typed WebSocket handlers for [axum](https://docs.rs/axum). It eliminates the manual `socket.split()` → channel → spawn send task boilerplate.

## Setup

```toml
[dependencies]
ws-bridge = { version = "0.1", features = ["server"] }
axum = { version = "0.7", features = ["ws"] }
tokio = { version = "1", features = ["full"] }
```

## Quick start: `handler()`

The highest-level API. Returns an axum `MethodRouter` you can pass directly to `Router::route()`:

```rust
use axum::Router;
use ws_bridge::WsEndpoint;

let app = Router::new().route(
    EchoEndpoint::PATH,
    ws_bridge::server::handler::<EchoEndpoint, _, _>(|mut conn| async move {
        // conn.send() accepts E::ServerMsg
        // conn.recv() yields E::ClientMsg
        while let Some(Ok(msg)) = conn.recv().await {
            conn.send(ServerMsg::Echo { payload: format!("{msg:?}") })
                .await
                .unwrap();
        }
    }),
);
```

The callback receives a `server::Connection<E>`, which is a `WsConnection<E::ServerMsg, E::ClientMsg>` — note the types are oriented from the server's perspective (you **send** `ServerMsg`, you **receive** `ClientMsg`).

## With shared state: `handler_with_state()`

Access axum's `State<S>` extractor alongside the typed connection:

```rust
use axum::Router;
use std::sync::Arc;

struct AppState {
    db: DatabasePool,
}

let app = Router::new()
    .route(
        ChatEndpoint::PATH,
        ws_bridge::server::handler_with_state::<ChatEndpoint, _, _, Arc<AppState>>(
            |mut conn, state| async move {
                // `state` is Arc<AppState>
                while let Some(Ok(msg)) = conn.recv().await {
                    // Use state.db, etc.
                }
            },
        ),
    )
    .with_state(Arc::new(AppState { db: pool }));
```

## Manual upgrade: `upgrade()`

When you need custom extractors (cookies, headers, query params) or want to perform pre-upgrade authentication, use `upgrade()` inside your own handler function:

```rust
use axum::extract::ws::WebSocketUpgrade;
use axum::response::Response;

async fn handle_session(
    ws: WebSocketUpgrade,
    cookies: tower_cookies::Cookies,
) -> Response {
    // Authenticate before upgrading
    let session = cookies.get("session_token")
        .expect("missing session cookie");

    ws_bridge::server::upgrade::<SessionEndpoint, _, _>(ws, |mut conn| async move {
        while let Some(Ok(msg)) = conn.recv().await {
            // Handle messages with pre-verified auth context
        }
    })
}
```

This pattern is how you implement **pre-upgrade authentication** — rejecting unauthorized requests before the WebSocket handshake completes.

## Low-level: `into_connection()`

For maximum control, configure the `WebSocketUpgrade` yourself and then wrap the raw `WebSocket`:

```rust
async fn handle(ws: WebSocketUpgrade) -> Response {
    ws.max_message_size(1024 * 1024)  // 1MB max message
        .on_upgrade(|socket| async move {
            let mut conn = ws_bridge::server::into_connection::<SessionEndpoint>(socket);
            while let Some(Ok(msg)) = conn.recv().await {
                // ...
            }
        })
}
```

## Splitting the connection

Use `split()` when you need concurrent send/receive (e.g., forwarding messages from a channel while also receiving):

```rust
ws_bridge::server::handler::<ChatEndpoint, _, _>(|conn| async move {
    let (mut sender, mut receiver) = conn.split();

    // Spawn a task to forward broadcast messages
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = broadcast_rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Receive loop in the current task
    while let Some(Ok(msg)) = receiver.recv().await {
        // Process incoming messages
    }

    send_task.abort();
})
```

## API summary

| Function | Use case |
|---|---|
| `handler::<E, _, _>(cb)` | Simple handler, no state |
| `handler_with_state::<E, _, _, S>(cb)` | Handler with axum `State<S>` |
| `upgrade::<E, _, _>(ws, cb)` | Manual handler with custom extractors |
| `into_connection::<E>(socket)` | Wrap a raw `WebSocket` after custom upgrade |

## Post-connect authentication

For endpoints that authenticate via the first WebSocket message (instead of pre-upgrade), handle it in the callback:

```rust
ws_bridge::server::handler::<LauncherEndpoint, _, _>(|mut conn| async move {
    // Wait for the first message to carry auth credentials
    let Some(Ok(ClientMsg::Register { auth_token, .. })) = conn.recv().await else {
        return; // No register message, drop connection
    };

    if !verify_token(&auth_token).await {
        conn.send(ServerMsg::RegisterAck { success: false }).await.ok();
        return;
    }

    conn.send(ServerMsg::RegisterAck { success: true }).await.ok();

    // Authenticated — proceed with normal message loop
    while let Some(Ok(msg)) = conn.recv().await {
        // ...
    }
})
```
