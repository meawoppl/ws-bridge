# Migrating to 1.0

Update dependencies to `ws-bridge = { version = "1.0.0", features = [...] }`.
Rust 1.94 or later is supported. Existing endpoint definitions and adapter entry
points retain their signatures. Defaults remain empty; choose only your adapters.

`SendError` and `RecvError` are now non-exhaustive. Add a fallback arm to exhaustive
matches. Native, Actix and generic adapters report `Transport(error)` with the original
error available through `std::error::Error::source`; do not treat only `Closed` as
a transport failure. Browser adapters still report `Closed` for their transport failures because
browser errors cannot satisfy the shared error type’s thread-safety requirements. Decode errors remain recoverable and do not close connections.
Custom `transport::into_connection` errors must implement `Error + Send + Sync`.

`close()` is available on shared, browser and Actix connections and senders.
Continue polling the receiver until it ends, with an application timeout, to
complete a close handshake. Shared receivers return `None` permanently after
closure or after reporting a transport failure once.

Reconnecting sends encode once before opening a connection. Invalid messages
return immediately even while offline. Retries are indefinite and may duplicate
messages when acknowledgement is uncertain. Use IDs and application acknowledgements
for reliable delivery, and timeouts or task cancellation to bound operations.
A receive cancelled after consuming a frame may lose that frame; this API does not
promise cancellation-safe delivery. Concurrent sends require split connections.

Backoff multipliers below one or non-finite values are treated as one; the initial
delay is capped by the maximum. Zero delay remains an explicit option and can cause
a busy retry loop. Retry backoff resets after 30 seconds of connection stability.

The 1.x compatibility promise covers public API and wire encoding. Serde messages
remain JSON text and custom codecs own their format. New optional adapters and error
variants may be added. Dependency patch releases and supported Rust minimum changes
will be called out in the changelog; raising MSRV requires a minor release.
