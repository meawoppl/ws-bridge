# Changelog

## Unreleased

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
