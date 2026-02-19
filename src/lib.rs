mod codec;
mod connection;

pub use codec::{DecodeError, EncodeError, NoMessages, WsCodec, WsMessage};
pub use connection::{RecvError, SendError, WsConnection, WsReceiver, WsSender};

#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "yew-client")]
pub mod yew_client;

#[cfg(feature = "native-client")]
pub mod native_client;

#[cfg(feature = "reconnect")]
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
