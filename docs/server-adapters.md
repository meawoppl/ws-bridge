# Additional HTTP servers

The `WsEndpoint` and `WsCodec` definitions work with all supported server
frameworks. The original `server` feature is the axum adapter; enable one or
more of these optional features for other servers:

| Framework | Feature | Module | Runnable example |
|---|---|---|---|
| Actix Web 4 / actix-ws 0.4 | `actix-server` | `actix_server` | `actix_echo_server` |
| Warp 0.4 | `warp-server` | `warp_server` | `warp_echo_server` |
| Rocket 0.5 / rocket_ws 0.1 | `rocket-server` | `rocket_server` | `rocket_echo_server` |
| Poem 3 | `poem-server` | `poem_server` | `poem_echo_server` |
| Salvo 1 | `salvo-server` | `salvo_server` | `salvo_echo_server` |
| Hyper 1 / hyper-tungstenite 0.30 | `hyper-server` | `hyper_server` | `hyper_echo_server` |

These adapters are included in the 1.0.0 release:

```toml
ws-bridge = { version = "1.0.0", features = ["actix-server"] }
```

## Try an adapter

Each server example listens on `127.0.0.1:3000` and uses the endpoint and
message types from `examples/common/mod.rs`:

```sh
cargo run --example actix_echo_server --features actix-server
# In another terminal:
cargo run --example echo_client --features native-client
```

Substitute any example and feature from the table. All six work with the
same native and browser clients and support custom text and binary codecs.

## Upgrade and connection APIs

For Actix Web, Warp, Rocket, Poem, and Salvo, call the module's
`upgrade::<Endpoint, _, _>(...)` from a normal framework handler. Its callback
receives `Connection<Endpoint>`, with `send`, `recv`, and `split` methods.
The callback sends `Endpoint::ServerMsg` and receives `Endpoint::ClientMsg`.

Use framework extractors and middleware for authentication and shared state
before calling `upgrade`. Configure frame sizes or subprotocols on the framework
upgrade object first. Actix callers needing subprotocol negotiation can call
`actix_ws::handle_with_protocols`, then `actix_server::into_connection`.
`into_aggregated_connection` accepts a configured Actix aggregated stream for
custom continuation limits.

Actix request streams are not `Send`. Its adapter therefore uses a local
connection type; spawn receive tasks with `actix_web::rt::spawn`. The other
adapters use the existing `WsConnection` type and work with `tokio::spawn`.
Actix aggregates continuation frames and replies to ping/close frames while
`recv()` is polled. Other adapters let the underlying WebSocket implementation
handle control frames; control frames never reach `WsCodec`.

Call `close().await` on the sending half before ending a handler to flush the
close handshake. The Actix `upgrade` helper also closes its session when its
callback completes.

Rocket's route attribute requires a literal path. Its example verifies that
this literal matches `EchoEndpoint::PATH` when the server starts. Other
frameworks can register routes directly from `Endpoint::PATH`.

## Hyper

`hyper_server` re-exports `hyper-tungstenite`'s `is_upgrade_request` and
`upgrade`. Check the request path and upgrade headers, call `upgrade`, return
the response with its headers intact, and spawn a task that awaits the returned
WebSocket future. Convert that socket with
`hyper_server::into_connection::<Endpoint>(socket)`.

The HTTP/1 connection must use `.with_upgrades()`. Awaiting the WebSocket
future before returning the HTTP response prevents the upgrade from completing.
See `examples/hyper_echo_server.rs` for the complete server.

## Other transports

Enable `transport` and call `transport::into_connection::<Endpoint, _, _, _>`
with a `Send` WebSocket sink/stream and two conversion functions:

- Encode a `WsMessage` as a transport-specific message.
- Decode transport messages into `transport::Frame::Message`, `Control`, or `Close`.

The underlying transport must handle ping replies and close handshakes.
This extension point allows additional HTTP server/WebSocket stacks without
adding a framework dependency to ws-bridge.

## Validation

`cargo test --all-features --test server_adapters` starts each server on an
OS-assigned loopback port and checks text and binary round trips, split
connections, recovery after malformed data, fragmented messages, ping/pong, and graceful close.
CI also checks each adapter feature independently and compiles the API examples.
