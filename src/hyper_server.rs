//! Typed WebSocket connections for Hyper 1 using `hyper-tungstenite`.
//!
//! Check [`is_upgrade_request`], call [`upgrade`], and return its response.
//! Spawn a task that awaits the returned `HyperWebsocket`, then wraps the socket
//! with [`into_connection`]. Hyper HTTP/1 connections must enable `.with_upgrades()`.
//!
//! # Example
//!
//! ```no_run
//! # use ws_bridge::WsEndpoint;
//! # struct Echo;
//! # impl WsEndpoint for Echo {
//! #     const PATH: &'static str = "/ws/echo";
//! #     type ServerMsg = String;
//! #     type ClientMsg = String;
//! # }
//! fn handle(mut req: hyper_tungstenite::hyper::Request<hyper_tungstenite::hyper::body::Incoming>)
//!     -> Result<hyper_tungstenite::hyper::Response<impl hyper_tungstenite::hyper::body::Body>, hyper_tungstenite::tungstenite::error::ProtocolError>
//! {
//!     // Check request path and is_upgrade_request before calling this handler.
//!     let (response, socket) = ws_bridge::hyper_server::upgrade(&mut req, None)?;
//!     tokio::spawn(async move {
//!         match socket.await {
//!             Ok(socket) => {
//!                 let mut conn = ws_bridge::hyper_server::into_connection::<Echo>(socket);
//!                 while let Some(Ok(msg)) = conn.recv().await {
//!                     if conn.send(msg).await.is_err() { break; }
//!                 }
//!                 let (mut tx, _) = conn.split();
//!                 let _ = tx.close().await;
//!             }
//!             Err(error) => eprintln!("upgrade failed: {error}"),
//!         }
//!     });
//!     Ok(response)
//! }
//! ```
use crate::{
    transport::{self, Frame},
    WsConnection, WsEndpoint, WsMessage,
};
use hyper_tungstenite::{tungstenite::Message, HyperWebsocketStream};

pub use hyper_tungstenite::{is_upgrade_request, upgrade};

/// A server-side typed connection.
pub type Connection<E> = WsConnection<<E as WsEndpoint>::ServerMsg, <E as WsEndpoint>::ClientMsg>;

/// Wraps an already upgraded Hyper socket.
pub fn into_connection<E: WsEndpoint>(socket: HyperWebsocketStream) -> Connection<E> {
    transport::into_connection::<E, _, _, _>(socket, encode, decode)
}
fn encode(msg: WsMessage) -> Message {
    match msg {
        WsMessage::Text(t) => Message::Text(t.into()),
        WsMessage::Binary(b) => Message::Binary(b.into()),
    }
}
fn decode(msg: Message) -> Frame {
    match msg {
        Message::Text(t) => Frame::Message(WsMessage::Text(t.to_string())),
        Message::Binary(b) => Frame::Message(WsMessage::Binary(b.to_vec())),
        Message::Close(_) => Frame::Close,
        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => Frame::Control,
    }
}
