//! Typed WebSocket connections for Warp. Keep routing, state, authentication,
//! and upgrade configuration in ordinary Warp filters.
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
//! fn handle(ws: warp::ws::Ws) -> impl warp::Reply {
//!     ws_bridge::warp_server::upgrade::<Echo, _, _>(ws, |mut conn| async move {
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
use std::future::Future;
use warp::ws::{Message, WebSocket, Ws};

/// A server connection that sends server messages and receives client messages.
pub type Connection<E> = WsConnection<<E as WsEndpoint>::ServerMsg, <E as WsEndpoint>::ClientMsg>;

/// Completes a configured Warp upgrade with a typed callback.
pub fn upgrade<E, F, Fut>(ws: Ws, callback: F) -> impl warp::Reply
where
    E: WsEndpoint,
    F: FnOnce(Connection<E>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    ws.on_upgrade(move |socket| callback(into_connection::<E>(socket)))
}

/// Wraps an already upgraded Warp socket.
pub fn into_connection<E: WsEndpoint>(socket: WebSocket) -> Connection<E> {
    transport::into_connection::<E, _, _, _>(socket, encode, decode)
}
fn encode(msg: WsMessage) -> Message {
    match msg {
        WsMessage::Text(t) => Message::text(t),
        WsMessage::Binary(b) => Message::binary(b),
    }
}
fn decode(msg: Message) -> Frame {
    if msg.is_close() {
        Frame::Close
    } else if msg.is_text() {
        Frame::Message(WsMessage::Text(
            msg.to_str().expect("validated text frame").to_owned(),
        ))
    } else if msg.is_binary() {
        Frame::Message(WsMessage::Binary(msg.into_bytes().to_vec()))
    } else {
        Frame::Control
    }
}
