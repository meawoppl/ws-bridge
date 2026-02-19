# Yew / Browser Client (`yew-client` feature)

The `yew-client` feature provides a typed WebSocket client for browser-based Rust applications using [gloo-net](https://docs.rs/gloo-net). It's designed for use with [Yew](https://yew.rs/) but works with any WASM frontend framework.

## Setup

Enable the feature in your `Cargo.toml`:

```toml
[dependencies]
ws-bridge = { version = "0.1", features = ["yew-client"] }
```

## Connecting

### Auto-derived URL

`connect()` derives `ws://` or `wss://` from the current page's protocol and host, then appends the endpoint's `PATH`:

```rust
use ws_bridge::yew_client;

// On https://example.com, this connects to wss://example.com/ws/echo
let conn = yew_client::connect::<EchoEndpoint>()?;
```

The derivation logic:
- `https:` page → `wss://` WebSocket
- `http:` page → `ws://` WebSocket

### Explicit URL

Use `connect_to()` when you need a custom URL (different host, port, or parameterized path):

```rust
let conn = yew_client::connect_to::<EchoEndpoint>("wss://other-host.com/ws/echo")?;
```

## The `!Send` constraint

Browser WebSocket types from gloo-net are **not `Send`**. WASM runs on a single thread, so Rust's `Send` bound doesn't apply. This means `yew_client::Connection<E>` is a **separate type** from the transport-agnostic `WsConnection<S, R>` used by the server and native client.

In practice, this means:
- You cannot pass a `yew_client::Connection` across thread boundaries (there's only one thread anyway).
- You must use `wasm_bindgen_futures::spawn_local()` instead of `tokio::spawn()` to run async WebSocket work.
- The `Sender` and `Receiver` halves from `split()` are also `!Send`.

## Sending and receiving

```rust
use ws_bridge::yew_client;

let mut conn = yew_client::connect::<EchoEndpoint>()?;

// Send a typed message (E::ClientMsg)
conn.send(ClientMsg::Say { text: "hello".into() }).await?;

// Receive typed messages (E::ServerMsg)
while let Some(result) = conn.recv().await {
    match result {
        Ok(msg) => { /* handle ServerMsg */ }
        Err(ws_bridge::RecvError::Decode(e)) => { /* bad message */ }
        Err(ws_bridge::RecvError::Closed) => break,
    }
}
```

## Splitting the connection

Use `split()` when you need to send and receive concurrently (common in Yew components):

```rust
let conn = yew_client::connect::<EchoEndpoint>()?;
let (mut sender, mut receiver) = conn.split();

// Sender: yew_client::Sender<E> — has .send(E::ClientMsg)
// Receiver: yew_client::Receiver<E> — has .recv() -> Option<Result<E::ServerMsg, RecvError>>
```

## Usage in a Yew component

A typical pattern is to open the WebSocket in a `use_effect_with` hook and communicate with the component via callbacks or state handles:

```rust
use yew::prelude::*;
use wasm_bindgen_futures::spawn_local;
use ws_bridge::yew_client;

#[function_component(Chat)]
fn chat() -> Html {
    let messages = use_state(Vec::new);

    {
        let messages = messages.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let mut conn = yew_client::connect::<ChatEndpoint>()
                    .expect("failed to connect");

                while let Some(Ok(msg)) = conn.recv().await {
                    messages.set({
                        let mut msgs = (*messages).clone();
                        msgs.push(msg);
                        msgs
                    });
                }
            });
            || () // cleanup
        });
    }

    html! {
        <div>
            { for messages.iter().map(|m| html! { <p>{ format!("{:?}", m) }</p> }) }
        </div>
    }
}
```

For bidirectional communication, split the connection and store the sender in a ref:

```rust
use yew::prelude::*;
use wasm_bindgen_futures::spawn_local;
use std::rc::Rc;
use std::cell::RefCell;

#[function_component(Chat)]
fn chat() -> Html {
    let sender = use_mut_ref(|| None::<ws_bridge::yew_client::Sender<ChatEndpoint>>);
    let messages = use_state(Vec::new);

    // Set up connection on mount
    {
        let sender = sender.clone();
        let messages = messages.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let conn = ws_bridge::yew_client::connect::<ChatEndpoint>()
                    .expect("failed to connect");
                let (tx, mut rx) = conn.split();
                *sender.borrow_mut() = Some(tx);

                while let Some(Ok(msg)) = rx.recv().await {
                    messages.set({
                        let mut msgs = (*messages).clone();
                        msgs.push(msg);
                        msgs
                    });
                }
            });
            || ()
        });
    }

    let on_send = {
        let sender = sender.clone();
        Callback::from(move |_| {
            let sender = sender.clone();
            spawn_local(async move {
                if let Some(ref mut tx) = *sender.borrow_mut() {
                    let _ = tx.send(ClientMsg::Say { text: "hello".into() }).await;
                }
            });
        })
    };

    html! {
        <div>
            <button onclick={on_send}>{"Send"}</button>
            { for messages.iter().map(|m| html! { <p>{ format!("{:?}", m) }</p> }) }
        </div>
    }
}
```

## Binary messages

If your endpoint uses a custom `WsCodec` implementation that produces `WsMessage::Binary(...)`, the yew client handles it transparently. Binary frames arrive as `gloo_net::websocket::Message::Bytes` and are converted to `WsMessage::Binary` before decoding.

See [Custom Codecs](custom-codecs.md) for how to implement binary protocols.

## Error types

| Error | When |
|---|---|
| `ConnectError` | `connect()` / `connect_to()` fails to open the WebSocket |
| `SendError::Encode(...)` | Message serialization fails |
| `SendError::Closed` | Connection is closed when sending |
| `RecvError::Decode(...)` | Received message can't be deserialized |
| `RecvError::Closed` | Connection closed by server or network |

## Build target

The `yew-client` feature only compiles for `wasm32-unknown-unknown`. Add it to your frontend crate, not your server:

```toml
# frontend/Cargo.toml
[dependencies]
ws-bridge = { version = "0.1", features = ["yew-client"] }

# server/Cargo.toml — don't include yew-client here
ws-bridge = { version = "0.1", features = ["server"] }
```
