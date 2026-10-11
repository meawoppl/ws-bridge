//! Typed WebSocket connections for Poem. Use standard Poem extractors for
//! routing, shared state, and pre-upgrade authentication.
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
//! #[poem::handler]
//! async fn handle(ws: poem::web::websocket::WebSocket) -> impl poem::IntoResponse {
//!     ws_bridge::poem_server::upgrade::<Echo, _, _>(ws, |mut conn| async move {
//!         while let Some(Ok(msg)) = conn.recv().await {
//!             if conn.send(msg).await.is_err() { break; }
//!         }
//!         let (mut tx, _) = conn.split();
//!         let _ = tx.close().await;
//!     })
//! }
//! ```
use crate::{
    transport::{self, Frame},
    WsConnection, WsEndpoint, WsMessage,
};
use poem::web::websocket::{BoxWebSocketUpgraded, Message, WebSocket, WebSocketStream};
use std::future::Future;

/// A server-side typed connection.
pub type Connection<E> = WsConnection<<E as WsEndpoint>::ServerMsg, <E as WsEndpoint>::ClientMsg>;

/// Completes a configured Poem upgrade with a typed callback.
pub fn upgrade<E, F, Fut>(ws: WebSocket, callback: F) -> BoxWebSocketUpgraded
where
    E: WsEndpoint,
    F: FnOnce(Connection<E>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    ws.on_upgrade(move |socket| callback(into_connection::<E>(socket)))
        .boxed()
}

/// Wraps an already upgraded Poem socket.
pub fn into_connection<E: WsEndpoint>(socket: WebSocketStream) -> Connection<E> {
    transport::into_connection::<E, _, _, _>(socket, encode, decode)
}
fn encode(msg: WsMessage) -> Message {
    match msg {
        WsMessage::Text(t) => Message::text(t),
        WsMessage::Binary(b) => Message::binary(b),
    }
}
fn decode(msg: Message) -> Frame {
    match msg {
        Message::Text(t) => Frame::Message(WsMessage::Text(t)),
        Message::Binary(b) => Frame::Message(WsMessage::Binary(b)),
        Message::Close(_) => Frame::Close,
        Message::Ping(_) | Message::Pong(_) => Frame::Control,
    }
}
