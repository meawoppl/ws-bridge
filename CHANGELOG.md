# Changelog

## 1.0.0 - 2026-10-10

- Stabilize the feature-gated API and declare Rust 1.94 as the supported minimum.
- Preserve original native and generic transport failures through error sources.
- Make send/receive errors non-exhaustive to allow compatible additions.
- Add native handshake headers, response inspection, and message/frame configuration.
- Return browser URL discovery errors instead of panicking; add close methods.
- Flush native/axum close frames and make shared receivers terminal after closure.
- Encode reconnecting messages once before connecting; clamp extreme backoff settings.
- Update vulnerable TLS dependencies and add security, MSRV, docs and packaging CI.
- Document migration, delivery semantics, security responsibilities and releases.


- Add optional server adapters for Actix Web, Warp, Rocket, Poem, Salvo, and Hyper
- Add a generic `transport` adapter for other `Send` WebSocket sink/stream types
- Add live adapter interoperability tests, runnable examples, and compiled API examples
- Return reconnection encoding errors without dropping the connection
- Retain retry backoff until a connection lasts at least 30 seconds
- Fix default-feature testing and browser-client lint coverage
- Update usage guides and compile all rustdoc examples

## 0.2.1

- Bump patch version for updated dependencies (axum 0.8, tokio-tungstenite 0.28)

## 0.2.0

- Update to axum 0.8 and tokio-tungstenite 0.28
- Add rustls TLS support for native-client (wss:// support)
- Add documentation: README and per-feature usage guides
- Add crates.io metadata and MIT license
- Add GitHub Actions CI
