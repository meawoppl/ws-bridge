//! Typed WebSocket endpoints shared by HTTP servers, native clients, and browsers.
//!
//! Default features are empty. Enable a server or client adapter explicitly;
//! see the README for the feature matrix and migration notes.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod codec;
mod connection;

pub use codec::{DecodeError, EncodeError, NoMessages, WsCodec, WsMessage};
pub use connection::{RecvError, SendError, TransportError, WsConnection, WsReceiver, WsSender};

#[cfg(feature = "server")]
/// Axum server upgrades and typed connections.
pub mod server;

#[cfg(feature = "yew-client")]
/// Browser WebSocket connections using gloo.
pub mod yew_client;

#[cfg(feature = "native-client")]
/// Native WebSocket connections with rustls TLS support.
pub mod native_client;

#[cfg(feature = "reconnect")]
/// Connection factories and automatic exponential backoff.
pub mod reconnect;

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

#[cfg(feature = "transport")]
pub mod transport;

#[cfg(feature = "actix-server")]
pub mod actix_server;

#[cfg(feature = "warp-server")]
pub mod warp_server;

#[cfg(feature = "rocket-server")]
pub mod rocket_server;

#[cfg(feature = "poem-server")]
pub mod poem_server;

#[cfg(feature = "salvo-server")]
pub mod salvo_server;

#[cfg(feature = "hyper-server")]
pub mod hyper_server;
